//! Text whose font weight and font size are animated.
//!
//! [`iced::Font`] carries a nine-variant `Weight`, so the stock text widget
//! can draw a label at 500 or at 600 and at nothing in between. The bundled
//! Inter is a variable face and the text stack below iced is continuous in
//! `wght` all the way down — an `Attrs` weight becomes a
//! `cosmic_text::CacheKey` and then a `wght` variation on the scaler — so the
//! only thing in the way is that enum. This widget goes around it: while the
//! weight moves it shapes its own `cosmic_text::Buffer` at an arbitrary `u16`
//! weight and hands the renderer the buffer itself, which is what makes 437
//! and 612 reachable.
//!
//! The size needs no such detour, since iced already takes any `f32`; what
//! it needs is a size read inside `layout` and `draw` rather than when the
//! view is built, a line height for the sizes between the steps of the scale
//! ([`TextStyle::resized`]), and rounding.
//!
//! ```no_run
//! use iced_luminate::animate::{curves::QUICK, key, Motion};
//! use iced_luminate::theme::typography::{TextSize, TextStyle};
//! use iced_luminate::widget::animated_text::animated_text;
//! use iced::font::Weight;
//!
//! # fn view(motion: &Motion, hovered: bool) -> iced::Element<'_, (), iced_luminate::Theme, iced_luminate::Renderer> {
//! let weight = motion.to(key!(), QUICK, if hovered { 600.0 } else { 500.0 });
//! let size = motion.to(key!(), QUICK, if hovered { 16.0 } else { 14.0 });
//!
//! animated_text("Save", TextStyle::text(TextSize::Sm, Weight::Medium))
//!     .weight(weight)
//!     .size(size)
//!     .into()
//! # }
//! ```
//!
//! # Animating a weight
//!
//! Five rules, learned from the axis rather than from the code.
//!
//! **Weight never animates alone.** It accompanies a change of colour or
//! background, and has to share that change's [`Curve`](iced_animate::Curve)
//! and duration. On two curves the label and the highlight it belongs to land
//! on different frames, and the text reads as lagging its own row.
//!
//! **A hundred units, give or take fifty.** Below +50 the change does not
//! read as a change of state, only as text that got a little sharper; above
//! +200 it reads as a different type style, and takes the line's width with
//! it. The kit's own step is 500 → 600, the weights
#![cfg_attr(
    feature = "bundled-font",
    doc = "[`DECLARED_WEIGHTS`](crate::theme::typography::DECLARED_WEIGHTS) declares."
)]
#![cfg_attr(not(feature = "bundled-font"), doc = "`DECLARED_WEIGHTS` declares.")]
//!
//! **Not below 14 px.** At [`TextSize::Xs`](crate::theme::typography::TextSize::Xs)
//! a hundred units moves the stem by a fraction of a pixel, so the animation
//! arrives as a shimmer in the antialiasing rather than as weight.
//!
//! **Short labels only.** A button, a tab, a row of navigation. Every frame
//! reshapes the buffer and gives every glyph in it a new entry in the
//! renderer's atlas; a paragraph animating its weight is a great deal of both.
//!
//! **A spring, not an ease.** [`QUICK`](iced_animate::curves::QUICK)
//! overshoots by a few units and settles, which does not read as a movement
//! of its own, and it keeps its velocity when the pointer crosses back — a
//! label under a wandering cursor never snaps.
//!
//! # Animating a size
//!
//! **A size moves the layout, unless it is scaled.** A larger size is a
//! wider and a taller line. Under [`SizeLayout::Live`] the line is set at
//! the size of the moment and everything after it moves with it, every
//! frame a relayout. Under [`SizeLayout::Scaled`] the line is laid out and
//! shaped once at the size it is heading for, and drawn scaled to the size
//! of the moment: a redraw per frame and nothing reshaped, at the price of
//! glyphs that reach outside the laid-out box while the size is above its
//! target.
//!
//! **The line height follows the scale.** A size between two steps gets the
//! line height [`TextStyle::resized`] interpolates for it, rounded to a
//! whole pixel, so a 14 → 18 px transition ends on exactly the line box
//! `TextSize::Lg` has, and a `Live` line box grows a pixel at a time.
//!
//! **Headings and labels, not paragraphs.** Every size the text is drawn at
//! is a fresh raster of every glyph in it, whichever layout is chosen: the
//! glyph cache is keyed by size. The size is rounded to a
//! [`size_step`](AnimatedText::size_step) for that reason.
//!
//! # What it costs
//!
//! Every distinct weight is a fresh `Font` inside cosmic-text: the face parsed
//! again and its shaper tables rebuilt, cached from then on under
//! `(face, weight)`. Every distinct weight also gives every glyph its own
//! entry in the renderer's atlas. Both are why the animated weight is rounded
//! to a [`weight_step`](AnimatedText::weight_step) before it is shaped, and
//! why this widget is for labels — a button, a tab, a row of navigation — and
//! not for running text.
//!
//! Measured on the bundled Inter in a release build, a weight seen for the
//! first time costs about 100 µs to build and about 20 µs to reshape on every
//! frame after that, and holds some 24 KB — the face's bytes are shared, its
//! shaper tables are not. A 500 → 600 transition at 14 px asks for six of
//! them. That is small enough that nothing here needs warming up in advance,
//! and large enough that a step of one would not be.
//!
//! Rounding is what makes that affordable frame by frame. At 14 px the weight
//! only crosses a step every third frame or so, so most frames of an
//! animation reshape nothing: measured over a 500 → 600 transition on eight
//! labels, the median frame's `layout` and `draw` cost about 2 µs and the six
//! or so frames that do reshape cost 100-170 µs, a little over a millisecond
//! for the whole transition. Asking for `weight_step(1.0)` reshapes every
//! frame instead, and takes the median frame to 144 µs.
//!
//! A size builds no font instance: the face and its shaper tables do not
//! depend on it. A moving size costs a reshape of the line under
//! [`SizeLayout::Live`] and the glyph rasters under either layout.
//!
//! Only a moving weight takes the text off the ordinary paragraph path. A
//! size moving on its own is laid out and drawn as the stock text widget
//! would draw it, and text that is not animating at all — a constant, or a
//! track that has settled on one of the nine named weights — measures as the
//! stock text widget does.

use std::sync::{Arc, Mutex, PoisonError};

use iced::advanced::graphics::text as raw_text;
use iced::advanced::text::{Alignment, Fragment, IntoFragment, Paragraph, Wrapping, paragraph};
use iced::advanced::widget::text as stock;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Widget, layout, mouse, renderer, text};
use iced::widget::text::{Catalog, Style, StyleFn};
use iced::{Color, Element, Font, Length, Point, Rectangle, Size, Transformation, alignment};

use iced_animate::{Anim, Tier};

use crate::theme::typography::TextStyle;

mod shaped;
mod step;

pub use step::{
    DEFAULT_SIZE_STEP, MAX_WEIGHT, MIN_SIZE, MIN_WEIGHT, default_weight_step, quantize_size,
    quantize_weight,
};

use shaped::Shaped;

/// How an animated weight is allowed to move the layout.
///
/// A heavier weight is a wider line — `tests/shaping.rs` asserts exactly that
/// — so an animated weight is a moving width, and something has to give.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WeightLayout {
    /// The line is measured at the weight of the moment, so it grows with it.
    ///
    /// Typographically honest, and the reason the track is
    /// [`Tier::Layout`]: every frame of the animation relays out the tree
    /// around it, and whatever sits after the text on its line moves.
    #[default]
    Live,
    /// The line is measured at the weight the animation is heading for, and
    /// keeps that width throughout.
    ///
    /// Nothing moves and the track is only [`Tier::Paint`], so the frames of
    /// the animation cost a redraw instead of a relayout. At the lighter end
    /// the reserved box is a little wider than the glyphs in it — about 2-3 %
    /// of the line for a 500 → 600 transition at 14 px.
    ///
    /// It does not make the animation free: the buffer is still reshaped
    /// every frame, because the glyphs really do change width. What it buys
    /// is stillness, and a relayout the rest of the tree does not pay for.
    Snapped,
}

impl WeightLayout {
    /// The tier a weight bound under this policy is read at.
    const fn tier(self) -> Tier {
        match self {
            Self::Live => Tier::Layout,
            Self::Snapped => Tier::Paint,
        }
    }
}

/// How an animated size is allowed to move the layout.
///
/// A size moves both dimensions of the line, and by far more than a weight
/// does: 14 → 18 px is close to a third of the line's width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SizeLayout {
    /// The line is set at the size of the moment, so it grows with it.
    ///
    /// The track is [`Tier::Layout`], and every frame of the animation
    /// reshapes the line and relays out the tree around it.
    #[default]
    Live,
    /// The line is laid out and shaped at the size the animation is heading
    /// for, and drawn scaled to the size of the moment about the corner or
    /// edge its alignment names.
    ///
    /// Nothing moves, nothing is reshaped, and the track is only
    /// [`Tier::Paint`]. The glyphs are still rasterised at the size they
    /// appear at, so they stay sharp. The box stays the target's, so while
    /// the size is above its target the glyphs reach outside it — over a
    /// neighbour, or under the clip of a scrollable.
    Scaled,
}

impl SizeLayout {
    /// The tier a size bound under this policy is read at.
    const fn tier(self) -> Tier {
        match self {
            Self::Live => Tier::Layout,
            Self::Scaled => Tier::Paint,
        }
    }
}

/// Text drawn at an animated font weight and size.
///
/// See the [module documentation](self) for what it is for and what it costs.
#[allow(missing_debug_implementations)]
pub struct AnimatedText<'a, Theme = iced::Theme, Renderer = crate::Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    content: Fragment<'a>,
    style: TextStyle,
    weight: Anim<f32>,
    weight_layout: WeightLayout,
    weight_step: Option<f32>,
    size: Anim<f32>,
    size_layout: SizeLayout,
    size_step: f32,
    width: Length,
    height: Length,
    align_x: Alignment,
    align_y: alignment::Vertical,
    wrapping: Wrapping,
    class: Theme::Class<'a>,
    renderer: std::marker::PhantomData<Renderer>,
}

/// Text in `style`, drawn at the weight and size `style` names until a
/// [`weight`](AnimatedText::weight) or a [`size`](AnimatedText::size) is
/// bound.
pub fn animated_text<'a, Theme, Renderer>(
    content: impl IntoFragment<'a>,
    style: TextStyle,
) -> AnimatedText<'a, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    AnimatedText::new(content, style)
}

impl<'a, Theme, Renderer> AnimatedText<'a, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    /// Text in `style`.
    ///
    /// Without a [`weight`](Self::weight) or a [`size`](Self::size) the
    /// widget draws `style` as it is, through the ordinary paragraph path.
    pub fn new(content: impl IntoFragment<'a>, style: TextStyle) -> Self {
        Self {
            content: content.into_fragment(),
            style,
            weight: Anim::constant(weight_of(style)),
            weight_layout: WeightLayout::default(),
            weight_step: None,
            size: Anim::constant(style.size),
            size_layout: SizeLayout::default(),
            size_step: DEFAULT_SIZE_STEP,
            width: Length::Shrink,
            height: Length::Shrink,
            align_x: Alignment::Default,
            align_y: alignment::Vertical::Top,
            wrapping: Wrapping::default(),
            class: Theme::default(),
            renderer: std::marker::PhantomData,
        }
    }

    /// The weight to draw at, in `wght` units (100 to 900).
    ///
    /// Bind the same [`Curve`](iced_animate::Curve) the colour under the text
    /// animates on: weight on its own curve lands on a different frame from
    /// the background it belongs to, and the label visibly lags.
    #[must_use]
    pub fn weight(mut self, weight: impl Into<Anim<f32>>) -> Self {
        self.weight = weight.into();
        self
    }

    /// Whether the line is measured at the weight of the moment or at the one
    /// it is heading for. [`Live`](WeightLayout::Live) by default.
    #[must_use]
    pub const fn weight_layout(mut self, layout: WeightLayout) -> Self {
        self.weight_layout = layout;
        self
    }

    /// Rounds the animated weight to a multiple of `step` before shaping.
    ///
    /// The default comes from the font size ([`default_weight_step`]). A
    /// larger step asks the text stack for fewer font instances and fewer
    /// atlas entries; too large and the animation shows its stairs. The grid
    /// is anchored at the target, so the weight the animation ends on is
    /// always exact.
    #[must_use]
    pub const fn weight_step(mut self, step: f32) -> Self {
        self.weight_step = Some(step);
        self
    }

    /// The size to draw at, in logical pixels.
    ///
    /// The line height follows it along the kit's scale
    /// ([`TextStyle::resized`]).
    #[must_use]
    pub fn size(mut self, size: impl Into<Anim<f32>>) -> Self {
        self.size = size.into();
        self
    }

    /// Whether the line is set at the size of the moment or laid out at the
    /// one it is heading for and scaled. [`Live`](SizeLayout::Live) by
    /// default.
    #[must_use]
    pub const fn size_layout(mut self, layout: SizeLayout) -> Self {
        self.size_layout = layout;
        self
    }

    /// Rounds the animated size to a multiple of `step` before it is shaped
    /// or drawn. [`DEFAULT_SIZE_STEP`] by default.
    ///
    /// Every distinct size is a fresh raster of every glyph in the line, so a
    /// larger step asks for fewer of them. The grid is anchored at the
    /// target, so the size the animation ends on is always exact.
    #[must_use]
    pub const fn size_step(mut self, step: f32) -> Self {
        self.size_step = step;
        self
    }

    /// Sets the width of the text boundaries.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height of the text boundaries.
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the horizontal alignment of the text.
    ///
    /// Under [`SizeLayout::Scaled`] it is also the edge the text grows from.
    #[must_use]
    pub fn align_x(mut self, align: impl Into<Alignment>) -> Self {
        self.align_x = align.into();
        self
    }

    /// Sets the vertical alignment of the text.
    ///
    /// Under [`SizeLayout::Scaled`] it is also the edge the text grows from.
    #[must_use]
    pub fn align_y(mut self, align: impl Into<alignment::Vertical>) -> Self {
        self.align_y = align.into();
        self
    }

    /// Sets how the text wraps inside its boundaries.
    #[must_use]
    pub const fn wrapping(mut self, wrapping: Wrapping) -> Self {
        self.wrapping = wrapping;
        self
    }

    /// Sets the colour of the text.
    #[must_use]
    pub fn color(self, color: impl Into<Color>) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        let color = color.into();

        self.style_fn(move |_theme| Style { color: Some(color) })
    }

    /// Sets the style of the text.
    #[must_use]
    pub fn style_fn(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    /// Sets the style class of the text.
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    /// The weight the stock paragraph path would draw this text at, when it
    /// can draw it exactly.
    ///
    /// A weight that is not moving and lands on one of the nine weights
    /// [`iced::Font`] can name needs none of this widget's machinery, and
    /// should not pay for it: `Raw` text compares unequal to itself
    /// (`iced_graphics::text::Raw`'s `PartialEq` always returns `false`), so
    /// a raw label reads as changed to the renderer's damage tracking on
    /// every frame it is on screen. Resting text goes back through
    /// paragraphs.
    ///
    /// Resting means settled, not constant: a handle from
    /// [`Motion::to`](iced_animate::Motion::to) stays bound to its track long
    /// after the track has arrived, and a label that is not moving must not
    /// be drawn as one that is.
    fn resting_weight(&self) -> Option<iced::font::Weight> {
        if self.weight.is_animating() {
            return None;
        }

        named_weight(self.weight.get())
    }

    /// The weight to shape at, rounded to the step.
    fn shaped_weight(&self, moment: Moment) -> u16 {
        let step = self
            .weight_step
            .unwrap_or_else(|| default_weight_step(self.size.target()));
        let target = self.weight.target();

        let weight = match (moment, self.weight_layout) {
            (Moment::Paint, _) | (Moment::Layout, WeightLayout::Live) => self.weight.get(),
            (Moment::Layout, WeightLayout::Snapped) => target,
        };

        quantize_weight(weight, target, step)
    }

    /// The size the text is drawn at right now, rounded to the step.
    fn drawn_size(&self) -> f32 {
        quantize_size(self.size.get(), self.size.target(), self.size_step)
    }

    /// The size the line is laid out and shaped at: the size of the moment,
    /// or under [`SizeLayout::Scaled`] the one it is heading for.
    fn shaped_size(&self) -> f32 {
        match self.size_layout {
            SizeLayout::Live => self.drawn_size(),
            SizeLayout::Scaled => {
                let target = self.size.target();

                quantize_size(target, target, self.size_step)
            }
        }
    }

    /// The style the line is laid out and shaped in.
    fn shaped_style(&self) -> TextStyle {
        self.style.resized(self.shaped_size())
    }

    /// The format the stock paragraph path lays this text out with.
    fn format(&self, style: TextStyle, weight: iced::font::Weight) -> stock::Format<Font> {
        stock::Format {
            width: self.width,
            height: self.height,
            size: Some(style.size.into()),
            font: Some(Font {
                weight,
                ..style.font()
            }),
            line_height: style.line_height(),
            align_x: self.align_x,
            align_y: self.align_y,
            // The same reasoning as `TextStyle::apply`: a variable face shaped
            // by the basic shaper is set on its default instance's advances.
            shaping: iced::advanced::text::Shaping::Advanced,
            wrapping: self.wrapping,
        }
    }

    /// The point a [`SizeLayout::Scaled`] line grows from: the corner or edge
    /// of `bounds` its alignment holds still.
    fn scale_origin(&self, bounds: Rectangle) -> Point {
        let x = match self.align_x {
            Alignment::Center => bounds.center_x(),
            Alignment::Right => bounds.x + bounds.width,
            Alignment::Default | Alignment::Left | Alignment::Justified => bounds.x,
        };
        let y = match self.align_y {
            alignment::Vertical::Top => bounds.y,
            alignment::Vertical::Center => bounds.center_y(),
            alignment::Vertical::Bottom => bounds.y + bounds.height,
        };

        Point::new(x, y)
    }

    /// Runs `draw` scaled from the size the line was shaped at to the size of
    /// the moment, handing it the viewport in the coordinates it draws in.
    fn draw_scaled(
        &self,
        renderer: &mut Renderer,
        bounds: Rectangle,
        viewport: &Rectangle,
        draw: impl FnOnce(&mut Renderer, &Rectangle),
    ) {
        let scale = self.drawn_size() / self.shaped_size();

        // Exactly `1.0` whenever the line is `Live` or has settled, which is
        // every frame but the moving ones of a `Scaled` line.
        #[allow(clippy::float_cmp)]
        if scale == 1.0 || !scale.is_finite() {
            draw(renderer, viewport);
            return;
        }

        let origin = self.scale_origin(bounds);
        let transformation = Transformation::translate(origin.x, origin.y)
            * Transformation::scale(scale)
            * Transformation::translate(-origin.x, -origin.y);
        let viewport = *viewport * transformation.inverse();

        renderer.with_transformation(transformation, |renderer| draw(renderer, &viewport));
    }
}

/// Which of a frame's two passes is asking for the weight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Moment {
    /// `layout`, which under [`WeightLayout::Snapped`] measures ahead.
    Layout,
    /// `draw`, which always paints the weight of the moment.
    Paint,
}

/// The [`iced::font::Weight`] that names `weight` exactly, if one does.
fn named_weight(weight: f32) -> Option<iced::font::Weight> {
    use iced::font::Weight;

    Some(match weight {
        100.0 => Weight::Thin,
        200.0 => Weight::ExtraLight,
        300.0 => Weight::Light,
        400.0 => Weight::Normal,
        500.0 => Weight::Medium,
        600.0 => Weight::Semibold,
        700.0 => Weight::Bold,
        800.0 => Weight::ExtraBold,
        900.0 => Weight::Black,
        _ => return None,
    })
}

/// The `wght` value of the weight `style` names.
fn weight_of(style: TextStyle) -> f32 {
    let weight: iced::font::Weight = style.weight;

    f32::from(match weight {
        iced::font::Weight::Thin => 100_u16,
        iced::font::Weight::ExtraLight => 200,
        iced::font::Weight::Light => 300,
        iced::font::Weight::Normal => 400,
        iced::font::Weight::Medium => 500,
        iced::font::Weight::Semibold => 600,
        iced::font::Weight::Bold => 700,
        iced::font::Weight::ExtraBold => 800,
        iced::font::Weight::Black => 900,
    })
}

/// Widget state: the stock paragraph, the shaped buffers, and which of the
/// two the last `layout` chose.
#[derive(Debug, Default)]
struct State<P: Paragraph> {
    inner: Mutex<Inner>,
    /// The stock paragraph, used while the weight is not moving.
    paragraph: paragraph::Plain<P>,
    /// The path the last `layout` took, which `draw` follows.
    ///
    /// Decided once per layout and not again in `draw`: a track can settle
    /// between the two, and a `draw` that switched to the paragraph then
    /// would draw one that was never laid out.
    path: Path,
}

/// Which way the text reaches the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Path {
    /// Through the stock paragraph: the weight rests on a named one.
    #[default]
    Paragraph,
    /// Through a buffer shaped here and drawn as `Raw`.
    Shaped,
}

#[derive(Debug, Default)]
struct Inner {
    /// Shaped at the weight of the moment; the buffer the renderer draws.
    paint: Shaped,
    /// Shaped at the target weight, to measure a `Snapped` line without
    /// disturbing `paint`.
    reserve: Shaped,
    /// The bounds the last `layout` shaped inside, so `draw` shapes inside
    /// the same ones and does not undo its work.
    bounds: Size,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for AnimatedText<'_, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer<Font = Font> + raw_text::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer::Paragraph>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer::Paragraph>::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State<Renderer::Paragraph>>();

        // Only the consumer knows where it reads each value. Marked on every
        // path, the paragraph's included: a settled track resting here now
        // is the one that moves next.
        self.weight.mark_tier(self.weight_layout.tier());
        self.size.mark_tier(self.size_layout.tier());

        let style = self.shaped_style();

        if let Some(weight) = self.resting_weight() {
            state.path = Path::Paragraph;

            return stock::layout(
                &mut state.paragraph,
                renderer,
                limits,
                &self.content,
                self.format(style, weight),
            );
        }

        state.path = Path::Shaped;

        let weight = self.shaped_weight(Moment::Layout);
        let inner = &mut *state.inner.lock().unwrap_or_else(PoisonError::into_inner);

        layout::sized(limits, self.width, self.height, |limits| {
            let bounds = limits.max();
            inner.bounds = bounds;

            let shaped = match self.weight_layout {
                WeightLayout::Live => &mut inner.paint,
                WeightLayout::Snapped => &mut inner.reserve,
            };

            shaped.update(
                &self.content,
                style,
                weight,
                bounds,
                self.align_x,
                self.wrapping,
            )
        })
    }

    fn operate(
        &mut self,
        _tree: &mut Tree,
        layout: Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        operation.text(None, layout.bounds(), &self.content);
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        defaults: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State<Renderer::Paragraph>>();
        let appearance = theme.style(&self.class);
        let bounds = layout.bounds();

        if state.path == Path::Paragraph {
            self.draw_scaled(renderer, bounds, viewport, |renderer, viewport| {
                stock::draw(
                    renderer,
                    defaults,
                    bounds,
                    state.paragraph.raw(),
                    appearance,
                    viewport,
                );
            });

            return;
        }

        let weight = self.shaped_weight(Moment::Paint);
        let inner = &mut *state.inner.lock().unwrap_or_else(PoisonError::into_inner);

        // The same bounds `layout` shaped inside, so that a `Live` line is
        // shaped once a frame rather than once here and once there.
        let shaped_bounds = inner.bounds;
        let min_bounds = inner.paint.update(
            &self.content,
            self.shaped_style(),
            weight,
            shaped_bounds,
            self.align_x,
            self.wrapping,
        );

        self.draw_scaled(renderer, bounds, viewport, |renderer, viewport| {
            let Some(clip_bounds) = bounds.intersection(viewport) else {
                return;
            };

            renderer.fill_raw(raw_text::Raw {
                buffer: Arc::downgrade(inner.paint.buffer()),
                position: bounds.anchor(min_bounds, self.align_x, self.align_y),
                color: appearance.color.unwrap_or(defaults.text_color),
                clip_bounds,
            });
        });
    }
}

impl<'a, Message, Theme, Renderer> From<AnimatedText<'a, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: Catalog + 'a,
    Renderer: text::Renderer<Font = Font> + raw_text::Renderer + 'a,
{
    fn from(text: AnimatedText<'a, Theme, Renderer>) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use iced::font::Weight;
    use iced_animate::{Motion, MotionKey, curves::QUICK};

    use super::*;
    use crate::theme::typography::TextSize;

    type Label = AnimatedText<'static, iced::Theme, crate::Renderer>;

    const STYLE: TextStyle = TextStyle::text(TextSize::Sm, Weight::Medium);

    /// The regression: a handle from `Motion::to` is bound to its track for
    /// as long as the view keeps asking for it, moving or not. Every hover
    /// label in an application is such a handle, and one that took the raw
    /// path at rest would read as changed to damage tracking on every frame.
    #[test]
    fn a_settled_track_rests_on_the_paragraph_path() {
        let motion = Motion::new();
        let weight = motion.to(MotionKey::unique(), QUICK, 600.0);

        assert!(weight.is_live(), "the handle is bound to a track");

        let label: Label = animated_text("Save", STYLE).weight(weight);

        assert_eq!(label.resting_weight(), Some(Weight::Semibold));
    }

    #[test]
    fn a_moving_weight_leaves_the_paragraph_path() {
        let motion = Motion::new();
        let key = MotionKey::unique();
        let _ = motion.to(key, QUICK, 500.0);
        let weight = motion.to(key, QUICK, 600.0);

        let label: Label = animated_text("Save", STYLE).weight(weight);

        assert_eq!(label.resting_weight(), None);
    }

    /// A size needs nothing the stock paragraph cannot do, so a size moving
    /// on its own keeps the text off the raw path.
    #[test]
    fn a_moving_size_stays_on_the_paragraph_path() {
        let motion = Motion::new();
        let key = MotionKey::unique();
        let _ = motion.to(key, QUICK, 14.0);
        let size = motion.to(key, QUICK, 18.0);

        let label: Label = animated_text("Save", STYLE).size(size);

        assert_eq!(label.resting_weight(), Some(Weight::Medium));
    }

    #[test]
    fn a_scaled_size_is_shaped_at_its_target_and_drawn_at_the_moment() {
        let motion = Motion::new();
        let key = MotionKey::unique();
        let _ = motion.to(key, QUICK, 28.0);
        let size = motion.to(key, QUICK, 14.0);

        let live: Label = animated_text("Save", STYLE).size(size.clone());
        let scaled: Label = animated_text("Save", STYLE)
            .size(size)
            .size_layout(SizeLayout::Scaled);

        assert_eq!((live.shaped_size(), live.drawn_size()), (28.0, 28.0));
        assert_eq!((scaled.shaped_size(), scaled.drawn_size()), (14.0, 28.0));
        assert_eq!(scaled.shaped_style().line_height, 20.0);
    }

    #[test]
    fn a_scaled_line_grows_from_the_edge_its_alignment_holds() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 40.0));

        let origin = |x: Alignment, y: alignment::Vertical| {
            let label: Label = animated_text("Save", STYLE).align_x(x).align_y(y);
            label.scale_origin(bounds)
        };

        assert_eq!(
            origin(Alignment::Default, alignment::Vertical::Top),
            Point::new(10.0, 20.0)
        );
        assert_eq!(
            origin(Alignment::Center, alignment::Vertical::Center),
            Point::new(60.0, 40.0)
        );
        assert_eq!(
            origin(Alignment::Right, alignment::Vertical::Bottom),
            Point::new(110.0, 60.0)
        );
    }
}
