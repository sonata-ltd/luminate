//! Text whose font weight is animated along Inter's `wght` axis.
//!
//! [`iced::Font`] carries a nine-variant `Weight`, so the stock text widget
//! can draw a label at 500 or at 600 and at nothing in between. The bundled
//! Inter is a variable face and the text stack below iced is continuous in
//! `wght` all the way down — an `Attrs` weight becomes a
//! `cosmic_text::CacheKey` and then a `wght` variation on the scaler — so the
//! only thing in the way is that enum. This widget goes around it: it shapes
//! its own `cosmic_text::Buffer` at an arbitrary `u16` weight and hands the
//! renderer the buffer itself, which is what makes 437 and 612 reachable.
//!
//! ```no_run
//! use iced_luminate::animate::{curves::QUICK, key, Motion};
//! use iced_luminate::theme::typography::{TextSize, TextStyle};
//! use iced_luminate::widget::weighted_text::weighted_text;
//! use iced::font::Weight;
//!
//! # fn view(motion: &Motion, hovered: bool) -> iced::Element<'_, (), iced_luminate::Theme, iced_luminate::Renderer> {
//! let weight = motion.to(key!(), QUICK, if hovered { 600.0 } else { 500.0 });
//!
//! weighted_text("Save", TextStyle::text(TextSize::Sm, Weight::Medium))
//!     .weight(weight)
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
//! # What it costs
//!
//! Every distinct weight is a fresh `Font` inside cosmic-text: the face parsed
//! again and its shaper tables rebuilt, cached from then on under
//! `(face, weight)`. Every distinct weight also gives every glyph its own
//! entry in the renderer's atlas. Both are why the animated weight is rounded
//! to a [`step`](WeightedText::step) before it is shaped, and why this widget
//! is for labels — a button, a tab, a row of navigation — and not for running
//! text.
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
//! for the whole transition. Asking for `step(1.0)` reshapes every frame
//! instead, and takes the median frame to 144 µs.
//!
//! A weight that is not animating costs none of this: a constant weight is
//! drawn through the ordinary paragraph path instead, and measures as the
//! stock text widget does.

use std::sync::{Arc, Mutex, PoisonError};

use iced::advanced::graphics::text as raw_text;
use iced::advanced::text::{Alignment, Fragment, IntoFragment, Paragraph, Wrapping, paragraph};
use iced::advanced::widget::text as stock;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Widget, layout, mouse, renderer, text};
use iced::widget::text::{Catalog, Style, StyleFn};
use iced::{Color, Element, Font, Length, Rectangle, Size, alignment};

use iced_animate::{Anim, Tier};

use crate::theme::typography::TextStyle;

mod shaped;

pub use shaped::{MAX, MIN, default_step, quantize};

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

/// Text drawn at an animated font weight.
///
/// See the [module documentation](self) for what it is for and what it costs.
#[allow(missing_debug_implementations)]
pub struct WeightedText<'a, Theme = iced::Theme, Renderer = crate::Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    content: Fragment<'a>,
    style: TextStyle,
    weight: Anim<f32>,
    layout: WeightLayout,
    step: Option<f32>,
    width: Length,
    height: Length,
    align_x: Alignment,
    align_y: alignment::Vertical,
    wrapping: Wrapping,
    class: Theme::Class<'a>,
    renderer: std::marker::PhantomData<Renderer>,
}

/// Text in `style`, drawn at the weight `style` names until a
/// [`weight`](WeightedText::weight) is bound.
pub fn weighted_text<'a, Theme, Renderer>(
    content: impl IntoFragment<'a>,
    style: TextStyle,
) -> WeightedText<'a, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    WeightedText::new(content, style)
}

impl<'a, Theme, Renderer> WeightedText<'a, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: text::Renderer,
{
    /// Text in `style`.
    ///
    /// Without a [`weight`](Self::weight) the widget draws the weight `style`
    /// names, through the ordinary paragraph path.
    pub fn new(content: impl IntoFragment<'a>, style: TextStyle) -> Self {
        Self {
            content: content.into_fragment(),
            style,
            weight: Anim::constant(weight_of(style)),
            layout: WeightLayout::default(),
            step: None,
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
        self.layout = layout;
        self
    }

    /// Rounds the animated weight to a multiple of `step` before shaping.
    ///
    /// The default comes from the font size ([`default_step`]). A larger step
    /// asks the text stack for fewer font instances and fewer atlas entries;
    /// too large and the animation shows its stairs. The grid is anchored at
    /// the target, so the weight the animation ends on is always exact.
    #[must_use]
    pub const fn step(mut self, step: f32) -> Self {
        self.step = Some(step);
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
    #[must_use]
    pub fn align_x(mut self, align: impl Into<Alignment>) -> Self {
        self.align_x = align.into();
        self
    }

    /// Sets the vertical alignment of the text.
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

    /// The font the stock paragraph path would draw this text with, when it
    /// can draw it exactly.
    ///
    /// A weight that is not moving and lands on one of the nine weights
    /// [`iced::Font`] can name needs none of this widget's machinery, and
    /// should not pay for it: `Raw` text compares unequal to itself
    /// (`iced_graphics::text::Raw`'s `PartialEq` always returns `false`), so
    /// a raw label reads as changed to the renderer's damage tracking on
    /// every frame it is on screen. Resting text goes back through
    /// paragraphs.
    fn resting_font(&self) -> Option<Font> {
        if self.weight.is_live() {
            return None;
        }

        let weight = named_weight(self.weight.get())?;

        Some(Font {
            weight,
            ..self.style.font()
        })
    }

    /// The weight to shape at, rounded to the step.
    fn shaped_weight(&self, moment: Moment) -> u16 {
        let step = self.step.unwrap_or_else(|| default_step(self.style.size));
        let target = self.weight.target();

        let weight = match (moment, self.layout) {
            (Moment::Paint, _) | (Moment::Layout, WeightLayout::Live) => self.weight.get(),
            (Moment::Layout, WeightLayout::Snapped) => target,
        };

        quantize(weight, target, step)
    }

    /// The format the stock paragraph path lays this text out with.
    fn format(&self, font: Font) -> stock::Format<Font> {
        stock::Format {
            width: self.width,
            height: self.height,
            size: Some(self.style.size.into()),
            font: Some(font),
            line_height: self.style.line_height(),
            align_x: self.align_x,
            align_y: self.align_y,
            // The same reasoning as `TextStyle::apply`: a variable face shaped
            // by the basic shaper is set on its default instance's advances.
            shaping: iced::advanced::text::Shaping::Advanced,
            wrapping: self.wrapping,
        }
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

/// Widget state: the buffer that is drawn, the one the line is measured from
/// when the layout is [`Snapped`](WeightLayout::Snapped), and the bounds the
/// last layout shaped inside.
#[derive(Debug, Default)]
struct State<P: Paragraph> {
    inner: Mutex<Inner>,
    /// The stock paragraph, used while the weight is not animating.
    paragraph: paragraph::Plain<P>,
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
    for WeightedText<'_, Theme, Renderer>
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

        if let Some(font) = self.resting_font() {
            return stock::layout(
                &mut state.paragraph,
                renderer,
                limits,
                &self.content,
                self.format(font),
            );
        }

        // Only the consumer knows where it reads the value: `Live` measures
        // the line every frame, `Snapped` measures it once and repaints.
        self.weight.mark_tier(self.layout.tier());

        let weight = self.shaped_weight(Moment::Layout);
        let inner = &mut *state.inner.lock().unwrap_or_else(PoisonError::into_inner);

        layout::sized(limits, self.width, self.height, |limits| {
            let bounds = limits.max();
            inner.bounds = bounds;

            let shaped = match self.layout {
                WeightLayout::Live => &mut inner.paint,
                WeightLayout::Snapped => &mut inner.reserve,
            };

            shaped.update(
                &self.content,
                self.style,
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

        if self.resting_font().is_some() {
            stock::draw(
                renderer,
                defaults,
                bounds,
                state.paragraph.raw(),
                appearance,
                viewport,
            );

            return;
        }

        let weight = self.shaped_weight(Moment::Paint);
        let inner = &mut *state.inner.lock().unwrap_or_else(PoisonError::into_inner);

        // The same bounds `layout` shaped inside, so that a `Live` line is
        // shaped once a frame rather than once here and once there.
        let shaped_bounds = inner.bounds;
        let min_bounds = inner.paint.update(
            &self.content,
            self.style,
            weight,
            shaped_bounds,
            self.align_x,
            self.wrapping,
        );

        let Some(clip_bounds) = bounds.intersection(viewport) else {
            return;
        };

        renderer.fill_raw(raw_text::Raw {
            buffer: Arc::downgrade(inner.paint.buffer()),
            position: bounds.anchor(min_bounds, self.align_x, self.align_y),
            color: appearance.color.unwrap_or(defaults.text_color),
            clip_bounds,
        });
    }
}

impl<'a, Message, Theme, Renderer> From<WeightedText<'a, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: Catalog + 'a,
    Renderer: text::Renderer<Font = Font> + raw_text::Renderer + 'a,
{
    fn from(text: WeightedText<'a, Theme, Renderer>) -> Self {
        Self::new(text)
    }
}
