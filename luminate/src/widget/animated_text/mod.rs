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
//! line height [`TextStyle::resized`] interpolates for it. While a `Live`
//! size moves, that line height is left unrounded
//! ([`TextStyle::resized_exact`]), so the line box, and whatever is laid out
//! after it, grows a fraction of a pixel per frame instead of standing still
//! and jumping a whole one. At rest it is rounded to a whole pixel, and a
//! 14 → 18 px transition ends on exactly the line box `TextSize::Lg` has.
//!
//! **Or composited, like a browser's `transform`.** Under
//! [`SizeLayout::Composited`] a transition — of the size, the weight or both
//! — shapes and records the line twice, as it was and as it will be, and
//! draws every frame in between from those two textures: scaled to the size
//! of the moment, stretched to a line box interpolated between the two, and
//! cross-faded. Nothing is reshaped or rasterised while it runs. Unlike a
//! browser's composited scale, it does not snap at the end: near either end
//! a texture is drawn 1:1 on the device grid with the live text's sub-pixel
//! phase, so the first and last frames are the resting text to the pixel.
//! What it saves is text work, which is CPU work either way; compositing is
//! nearly free on the GPU and is not on the software backend, where two
//! scaled textures per frame cost more than reshaping a short label.
//!
//! **Headings and labels, not paragraphs.** Every size the text is drawn at
//! is a fresh raster of every glyph in it, whichever layout is chosen: the
//! glyph cache is keyed by size. The size is rounded to a
//! [`size_step`](AnimatedText::size_step) for that reason. A size scales the
//! whole width of the line, so the default step follows its length
//! ([`default_size_step`]): fine enough that one step moves the far end of
//! the line by at most [`SIZE_STEP_REACH`], and never finer than
//! [`MIN_SIZE_STEP`]. The line is measured by each `layout` and the step
//! taken from the previous one, so the first frame of a label that animates
//! as it appears still uses [`DEFAULT_SIZE_STEP`].
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

use iced_animate::{Anim, Tier, scale_factor, snap_between};
use iced_texture_cache::TextureRenderer;

use crate::theme::typography::TextStyle;

mod composite;
mod shaped;
mod step;

pub use step::{
    DEFAULT_SIZE_STEP, MAX_WEIGHT, MIN_SIZE, MIN_SIZE_STEP, MIN_WEIGHT, SIZE_STEP_REACH,
    default_size_step, default_weight_step, fit_step, quantize_size, quantize_weight,
};

use composite::Transition;
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
    /// The line is shaped and recorded once as it was and once as it will
    /// be, and every frame in between is composited from those two
    /// textures.
    ///
    /// Covers the weight as well as the size: whichever of them moves, the
    /// line box is interpolated between the two measured boxes, so what is
    /// laid out after the text moves as smoothly as under `Live`, and the
    /// two textures are scaled to the size of the moment, stretched to the
    /// box and cross-faded. A transition costs two shapes and two records,
    /// then a relayout and a composite per frame: no line is reshaped and no
    /// glyph rasterised while it runs. Near either end a texture is drawn
    /// 1:1 on the device grid, with its glyphs at the live text's sub-pixel
    /// phase, so the first and the last frame are the resting text to the
    /// pixel. At rest the text is drawn as under `Live`.
    ///
    /// The price is the crossfade: halfway through a weight transition both
    /// weights are on screen at once, faintly, rather than one weight in
    /// between. Both tracks are [`Tier::Layout`] (the box moves), and
    /// [`WeightLayout`] does not apply. A composited frame draws in a layer
    /// of its own, so a sibling drawn after the text in the same layer
    /// renders beneath it; see `iced_texture_cache::Cached`.
    Composited,
}

impl SizeLayout {
    /// The tier a size bound under this policy is read at.
    const fn tier(self) -> Tier {
        match self {
            Self::Live | Self::Composited => Tier::Layout,
            Self::Scaled => Tier::Paint,
        }
    }
}

/// A renderer [`AnimatedText`] can draw with.
///
/// Any renderer that draws text qualifies. One that can also render into a
/// texture — `iced_texture_cache::Renderer`, the kit's own
/// [`Renderer`](crate::Renderer) — composites [`SizeLayout::Composited`]
/// transitions and lends [`pixel_snap`](AnimatedText::pixel_snap) the scale
/// factor it draws at. Any other draws a composited line as
/// [`SizeLayout::Live`] and snaps to the process-wide
/// `iced_animate::scale_factor`.
pub trait AnimatedTextRenderer: text::Renderer<Font = Font> + raw_text::Renderer {
    /// The texture-capable renderer this is, if it is one.
    #[doc(hidden)]
    fn textures(&mut self) -> Option<&mut iced_texture_cache::Renderer> {
        None
    }

    /// Whether [`textures`](Self::textures) has one to give.
    #[doc(hidden)]
    fn composites(&self) -> bool {
        false
    }

    /// The device pixels per logical pixel this renderer draws at, if it
    /// knows.
    #[doc(hidden)]
    fn device_scale(&self) -> Option<f32> {
        None
    }
}

impl AnimatedTextRenderer for iced_texture_cache::Renderer {
    fn textures(&mut self) -> Option<&mut iced_texture_cache::Renderer> {
        Some(self)
    }

    fn composites(&self) -> bool {
        true
    }

    fn device_scale(&self) -> Option<f32> {
        Some(TextureRenderer::scale_factor(self))
    }
}

impl AnimatedTextRenderer for iced::Renderer {}

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
    opacity: Anim<f32>,
    weight_layout: WeightLayout,
    weight_step: Option<f32>,
    size: Anim<f32>,
    size_layout: SizeLayout,
    size_step: Option<f32>,
    pixel_snap: bool,
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
            opacity: Anim::constant(1.0),
            weight_layout: WeightLayout::default(),
            weight_step: None,
            size: Anim::constant(style.size),
            size_layout: SizeLayout::default(),
            size_step: None,
            pixel_snap: false,
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
    /// or drawn.
    ///
    /// By default the step follows the length of the line
    /// ([`default_size_step`]): a size scales the whole width of the line, so
    /// a longer line needs a finer step for its far end not to move in
    /// visible stairs.
    ///
    /// Every distinct size is a fresh raster of every glyph in the line, so a
    /// larger step asks for fewer of them. The grid is anchored at the
    /// target, so the size the animation ends on is always exact.
    #[must_use]
    pub const fn size_step(mut self, step: f32) -> Self {
        self.size_step = Some(step);
        self
    }

    /// Grows or shrinks the line box in whole device pixels while the size
    /// moves.
    ///
    /// A size moves the line box, and with it everything laid out after the
    /// text: the rows below a heading that grows. Their text sits on whole
    /// device pixels vertically and their backgrounds do not, so a line box
    /// that grows by fractions of a pixel moves them in step with neither,
    /// and their text snaps against its own background. Snapped, the box
    /// changes by whole device pixels from frame to frame, and what follows
    /// it moves by the same steps as the text in it.
    ///
    /// Only the height is snapped: horizontally text is placed in
    /// quarter-pixel steps, so a width that glides leaves nothing behind.
    /// The box leaves exactly from the height it rested at and rests exactly
    /// on the one it is heading for, snapped on a grid through the first for
    /// half the way and through the second for the rest
    /// (`iced_animate::snap_between`); only a moving size is snapped. The
    /// size of a device pixel is the renderer's own where it knows it, the
    /// kit's does. It does nothing for
    /// a [`SizeLayout::Scaled`] line, whose box does not move, nor for a line
    /// given a height of its own. See `iced_animate::widget::Sized::pixel_snap`
    /// for the same on any box, and `iced_animate::scale_factor` for where
    /// the size of a device pixel comes from.
    #[must_use]
    pub const fn pixel_snap(mut self, snap: bool) -> Self {
        self.pixel_snap = snap;
        self
    }

    /// Whether this frame's line box is snapped: asked for, the size moving,
    /// and the height the line's own.
    fn snaps(&self) -> bool {
        self.pixel_snap
            && self.size.is_animating()
            && matches!(self.height, Length::Shrink)
            && !matches!(self.size_layout, SizeLayout::Scaled)
    }

    /// `node`, a `Live` line's, with its height snapped between the height
    /// the line rested at (`from`) and the one it rests at when the size
    /// arrives, at `scale` device pixels per logical one, if
    /// [`snaps`](Self::snaps).
    fn snap_live(
        &self,
        node: layout::Node,
        style: TextStyle,
        from: Option<f32>,
        scale: f32,
    ) -> layout::Node {
        if !self.snaps() || style.line_height <= 0.0 {
            return node;
        }

        let size = node.size();
        let lines = (size.height / style.line_height).round().max(1.0);
        let resting = lines * self.style.resized(self.size.target()).line_height;
        let from = from.unwrap_or(f32::NAN);

        layout::Node::new(Size::new(
            size.width,
            snap_between(size.height, from, resting, scale).max(0.0),
        ))
    }

    /// Finishes a `Live` layout: snaps `node` if it should, and at rest
    /// records its height as where the next snapped motion leaves from.
    fn settle<P: Paragraph>(
        &self,
        state: &mut State<P>,
        node: layout::Node,
        style: TextStyle,
        moving: bool,
        scale: f32,
    ) -> layout::Node {
        let node = self.snap_live(node, style, state.rest_height, scale);

        if !moving {
            state.rest_height = Some(node.size().height);
        }

        node
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

    /// Fades the text: `1.0` is its colour as it is, `0.0` draws nothing.
    /// May be an animated value.
    ///
    /// The fade is applied where the text is drawn, on every path — the
    /// alpha of the colour the paragraph and the shaped line are drawn in,
    /// and the opacity a composited transition's textures are drawn at — so
    /// it costs a redraw per frame and never a relayout ([`Tier::Paint`]),
    /// and a size or a weight animating underneath keeps animating.
    ///
    /// Wrapping the text in an `iced_texture_cache::Cached` to fade it does
    /// the same for less: the text is swapped for a texture for the length
    /// of the fade, recorded with its box snapped to the pixel grid and its
    /// glyphs off their sub-pixel phase, so it shifts by up to half a pixel
    /// as the fade starts and back as it ends; and an animation inside the
    /// cached text that leaves its box alone is not seen until the fade is
    /// over. Here the text is never anything but live.
    ///
    /// Every glyph is faded on its own rather than the line as one image,
    /// which differs only where two glyphs overlap: rare within a line of
    /// text.
    ///
    /// ```
    /// use iced_animate::{curves::FADE, key, Motion};
    /// use iced_luminate::theme::typography::{TextSize, TextStyle};
    /// use iced_luminate::widget::animated_text::animated_text;
    /// use iced::font::Weight;
    ///
    /// let m = Motion::new();
    /// let shown = false;
    /// let opacity = m.to(key!(), FADE, if shown { 1.0_f32 } else { 0.0 });
    ///
    /// let _: iced_luminate::Element<'_, ()> = animated_text(
    ///     "Downloading 12/40",
    ///     TextStyle::text(TextSize::Md, Weight::Medium),
    /// )
    /// .opacity(opacity)
    /// .into();
    /// ```
    #[must_use]
    pub fn opacity(mut self, opacity: impl Into<Anim<f32>>) -> Self {
        self.opacity = opacity.into();
        self.opacity.mark_tier(Tier::Paint);
        self
    }

    /// The opacity of this frame, within `0..=1`; a value that is not a
    /// number draws the text as it is.
    fn opacity_now(&self) -> f32 {
        let opacity = self.opacity.get();

        if opacity.is_nan() {
            1.0
        } else {
            opacity.clamp(0.0, 1.0)
        }
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

    /// The tier the weight is read at: its layout's, unless the line is
    /// composited, where the weight moves the box.
    const fn weight_tier(&self) -> Tier {
        match self.size_layout {
            SizeLayout::Composited => Tier::Layout,
            SizeLayout::Live | SizeLayout::Scaled => self.weight_layout.tier(),
        }
    }

    /// Lays out a composited transition: starts one if the targets changed,
    /// shapes its two ends (a no-op once they stand) and reports the line
    /// box of the moment, interpolated between theirs.
    fn layout_composited<P: Paragraph>(
        &self,
        state: &mut State<P>,
        limits: &layout::Limits,
        scale: f32,
    ) -> layout::Node {
        state.path = Path::Composited;

        let to = (
            self.size.target().max(MIN_SIZE),
            composite::whole_weight(self.weight.target()),
        );
        let size = self.size.get().max(MIN_SIZE);
        let weight = self.weight.get();
        let rest = state.rest;
        let rest_height = state.rest_height.unwrap_or(f32::NAN);
        let snaps = self.snaps();
        let inner = state
            .inner
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner);

        layout::sized(limits, self.width, self.height, |limits| {
            let bounds = limits.max();
            inner.bounds = bounds;

            // Out of rest, a transition starts from exactly where the text
            // stood; out of another transition, from wherever that one had
            // got to, which it is then replaced from.
            let from = match (&inner.transition, rest) {
                (None, Some(rest)) => rest,
                _ => (size, composite::whole_weight(weight)),
            };
            if !inner
                .transition
                .as_ref()
                .is_some_and(|transition| transition.heads_for(to.0, to.1))
            {
                inner.transition = None;
            }

            let transition = inner
                .transition
                .get_or_insert_with(|| Transition::new(from, to));
            transition.shape(
                &self.content,
                self.style,
                bounds,
                self.align_x,
                self.wrapping,
            );

            let progress = transition.progress(size, weight);
            let line = transition.line_box(self.style, size, progress);

            if snaps {
                let resting = transition.resting_height();

                Size::new(
                    line.width,
                    snap_between(line.height, rest_height, resting, scale).max(0.0),
                )
            } else {
                line
            }
        })
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
    fn shaped_weight(&self, moment: Moment, step: f32) -> u16 {
        let target = self.weight.target();

        let weight = match (moment, self.weight_layout) {
            (Moment::Paint, _) | (Moment::Layout, WeightLayout::Live) => self.weight.get(),
            (Moment::Layout, WeightLayout::Snapped) => target,
        };

        quantize_weight(weight, target, step)
    }

    /// The step the size is rounded to, for a line `em_width` ems wide: the
    /// one given, or the one that line needs.
    fn size_step_for(&self, em_width: f32) -> f32 {
        self.size_step
            .unwrap_or_else(|| default_size_step(em_width))
    }

    /// The steps the size and the weight are rounded to this frame, for a
    /// line `em_width` ems wide that last rested at `rest`.
    ///
    /// Each is fitted to the distance from where the text rested to where it
    /// is heading ([`fit_step`]), so the grid, anchored at the target, passes
    /// through the start too, and the first frame of a transition is the
    /// resting text exactly.
    fn steps(&self, em_width: f32, rest: Option<(f32, u16)>) -> (f32, f32) {
        let size = self.size_step_for(em_width);
        let weight = self
            .weight_step
            .unwrap_or_else(|| default_weight_step(self.size.target()));

        let Some((rest_size, rest_weight)) = rest else {
            return (size, weight);
        };

        (
            fit_step(size, rest_size, self.size.target()),
            fit_step(weight, f32::from(rest_weight), self.weight.target()),
        )
    }

    /// The size the text is drawn at right now, rounded to `step`.
    fn drawn_size(&self, step: f32) -> f32 {
        quantize_size(self.size.get(), self.size.target(), step)
    }

    /// The size the line is laid out and shaped at: the size of the moment,
    /// or under [`SizeLayout::Scaled`] the one it is heading for.
    fn shaped_size(&self, step: f32) -> f32 {
        match self.size_layout {
            // A composited line reaches here only at rest, at its target.
            SizeLayout::Live | SizeLayout::Composited => self.drawn_size(step),
            SizeLayout::Scaled => {
                let target = self.size.target();

                quantize_size(target, target, step)
            }
        }
    }

    /// The style the line is laid out and shaped in.
    ///
    /// A `Live` line whose size is moving takes the line height of the
    /// moment unrounded ([`TextStyle::resized_exact`]): rounded, the line box
    /// and everything after it would move a whole pixel at a time. At rest,
    /// and for a `Scaled` line, which is shaped at its target, the line box
    /// sits on a whole pixel as the scale authors it; on a step of the scale
    /// the two agree, so coming to rest does not move it.
    ///
    /// Off the scale the rounded and the exact line height part by up to
    /// half a pixel, so the exact one is corrected by a share of each end's
    /// rounding: all of the start's at the start, all of the target's at the
    /// target. A transition then begins and ends on the resting line box,
    /// and glides between them.
    fn shaped_style(&self, step: f32, rest_size: Option<f32>) -> TextStyle {
        let size = self.shaped_size(step);

        // A composited line reaches here at rest, or on a renderer that
        // cannot composite, where it is laid out as a live one.
        if self.size_layout == SizeLayout::Scaled || !self.size.is_animating() {
            return self.style.resized(size);
        }

        let exact = self.style.resized_exact(size);
        let Some(from) = rest_size else {
            return exact;
        };

        let target = self.size.target();
        let rounding = |size: f32| {
            self.style.resized(size).line_height - self.style.resized_exact(size).line_height
        };
        let p = composite::progress(size, from, target);

        TextStyle {
            line_height: (rounding(target) - rounding(from)).mul_add(p, rounding(from))
                + exact.line_height,
            ..exact
        }
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
        step: f32,
        renderer: &mut Renderer,
        bounds: Rectangle,
        viewport: &Rectangle,
        draw: impl FnOnce(&mut Renderer, &Rectangle),
    ) {
        let scale = self.drawn_size(step) / self.shaped_size(step);

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

/// The width of a line measured as `min_bounds` in `style`, in ems: `0.0`,
/// which reads as "not measured", when there is nothing to divide.
fn em_width(min_bounds: Size, style: TextStyle) -> f32 {
    if style.size > 0.0 && min_bounds.width.is_finite() {
        min_bounds.width / style.size
    } else {
        0.0
    }
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
    /// The size step the last `layout` rounded to, which `draw` rounds to as
    /// well: a step recomputed from a width measured in between would shape
    /// the line in `draw` at a size `layout` never measured.
    size_step: f32,
    /// The weight step the last `layout` rounded to, which `draw` rounds to
    /// as well, for the same reason.
    weight_step: f32,
    /// The width of the line in ems as the last `layout` measured it, which
    /// the next one derives its size step from.
    em_width: f32,
    /// The size and the weight the text last rested at: where a composited
    /// transition out of rest starts, so its first frame is the text as it
    /// stood.
    rest: Option<(f32, u16)>,
    /// The height of the line box the text last rested in: where a snapped
    /// line box leaves from.
    rest_height: Option<f32>,
}

/// Which way the text reaches the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Path {
    /// Through the stock paragraph: the weight rests on a named one.
    #[default]
    Paragraph,
    /// Through a buffer shaped here and drawn as `Raw`.
    Shaped,
    /// Composited from the two ends of a transition.
    Composited,
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
    /// The composited transition under way, if any; dropped, textures and
    /// all, once the text comes to rest.
    transition: Option<Transition>,
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for AnimatedText<'_, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: AnimatedTextRenderer,
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
        self.weight.mark_tier(self.weight_tier());
        self.size.mark_tier(self.size_layout.tier());

        let moving = self.size.is_animating() || self.weight.is_animating();
        // The renderer's own factor where it has one: the process-wide one
        // is whichever window presented last.
        let scale = renderer.device_scale().unwrap_or_else(scale_factor);

        if self.size_layout == SizeLayout::Composited && renderer.composites() {
            if moving {
                return self.layout_composited(state, limits, scale);
            }

            // At rest: the textures of the last transition are done with.
            state
                .inner
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner)
                .transition = None;
        }

        if !moving {
            state.rest = Some((
                self.size.get().max(MIN_SIZE),
                composite::whole_weight(self.weight.get()),
            ));
        }

        // The step comes from the line as it was measured at rest, so it
        // holds for the whole of a transition: re-measured every frame, a
        // moving weight would nudge it, and the grid it anchors would shift
        // under the size partway there.
        let (step, weight_step) = self.steps(state.em_width, state.rest);
        state.size_step = step;
        state.weight_step = weight_step;

        let style = self.shaped_style(step, state.rest.map(|(size, _)| size));

        if let Some(weight) = self.resting_weight() {
            state.path = Path::Paragraph;

            let node = stock::layout(
                &mut state.paragraph,
                renderer,
                limits,
                &self.content,
                self.format(style, weight),
            );
            if !moving {
                state.em_width = em_width(state.paragraph.min_bounds(), style);
            }

            return self.settle(state, node, style, moving, scale);
        }

        state.path = Path::Shaped;

        let weight = self.shaped_weight(Moment::Layout, weight_step);
        let mut measured = Size::ZERO;

        let node = {
            let inner = state
                .inner
                .get_mut()
                .unwrap_or_else(PoisonError::into_inner);

            layout::sized(limits, self.width, self.height, |limits| {
                let bounds = limits.max();
                inner.bounds = bounds;

                let shaped = match self.weight_layout {
                    WeightLayout::Live => &mut inner.paint,
                    WeightLayout::Snapped => &mut inner.reserve,
                };

                measured = shaped.update(
                    &self.content,
                    style,
                    weight,
                    bounds,
                    self.align_x,
                    self.wrapping,
                );

                measured
            })
        };
        if !moving {
            state.em_width = em_width(measured, style);
        }

        self.settle(state, node, style, moving, scale)
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
        let bounds = layout.bounds();
        let step = state.size_step;

        let opacity = self.opacity_now();
        if opacity <= 0.0 {
            return;
        }

        let base = theme.style(&self.class);
        let color = base.color.unwrap_or(defaults.text_color);
        // The paragraph and the shaped line take the fade in their colour;
        // a composited transition takes it in the opacity of its textures,
        // so the colour its textures are recorded in stays put.
        let faded = Color {
            a: color.a * opacity,
            ..color
        };
        let appearance = Style { color: Some(faded) };

        if state.path == Path::Composited {
            let mut inner = state.inner.lock().unwrap_or_else(PoisonError::into_inner);
            // `layout` takes this path only on a renderer that composites.
            let (Some(transition), Some(renderer)) =
                (inner.transition.as_mut(), renderer.textures())
            else {
                return;
            };

            let size = self.size.get().max(MIN_SIZE);
            let progress = transition.progress(size, self.weight.get());
            let line = transition.line_box(self.style, size, progress);
            let block = Rectangle::new(bounds.anchor(line, self.align_x, self.align_y), line);

            transition.draw(renderer, block, size, progress, color, opacity, viewport);

            return;
        }

        if state.path == Path::Paragraph {
            self.draw_scaled(step, renderer, bounds, viewport, |renderer, viewport| {
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

        let weight = self.shaped_weight(Moment::Paint, state.weight_step);
        let inner = &mut *state.inner.lock().unwrap_or_else(PoisonError::into_inner);

        // The same bounds `layout` shaped inside, so that a `Live` line is
        // shaped once a frame rather than once here and once there.
        let shaped_bounds = inner.bounds;
        let min_bounds = inner.paint.update(
            &self.content,
            self.shaped_style(step, state.rest.map(|(size, _)| size)),
            weight,
            shaped_bounds,
            self.align_x,
            self.wrapping,
        );

        self.draw_scaled(step, renderer, bounds, viewport, |renderer, viewport| {
            let Some(clip_bounds) = bounds.intersection(viewport) else {
                return;
            };

            renderer.fill_raw(raw_text::Raw {
                buffer: Arc::downgrade(inner.paint.buffer()),
                position: bounds.anchor(min_bounds, self.align_x, self.align_y),
                color: faded,
                clip_bounds,
            });
        });
    }
}

impl<'a, Message, Theme, Renderer> From<AnimatedText<'a, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Theme: Catalog + 'a,
    Renderer: AnimatedTextRenderer + 'a,
{
    fn from(text: AnimatedText<'a, Theme, Renderer>) -> Self {
        Self::new(text)
    }
}

#[cfg(test)]
mod tests {
    use iced::font::Weight;
    use iced_animate::{Motion, MotionKey, curves::QUICK, testing::FrameClock};

    use super::*;
    use crate::theme::typography::TextSize;

    type Label = AnimatedText<'static, iced::Theme, crate::Renderer>;

    const STYLE: TextStyle = TextStyle::text(TextSize::Sm, Weight::Medium);

    /// A software renderer that has presented a frame at `scale`, as a
    /// window's renderer has: the factor it then reports is the one a
    /// snapped line box snaps to.
    fn renderer_at(scale: f32) -> crate::Renderer {
        use iced::advanced::renderer::Headless;

        crate::Luminate::load_fonts();

        let mut renderer = iced_texture_cache::testing::headless_tiny_skia();
        let _ = renderer.screenshot(Size::new(1, 1), scale, Color::WHITE);
        renderer
    }

    /// Lays `label` out in `tree`, the tree an application keeps between
    /// frames, and returns its box.
    fn lay(label: &mut Label, tree: &mut Tree, renderer: &crate::Renderer) -> Size {
        let limits = layout::Limits::new(Size::ZERO, Size::new(600.0, 200.0));

        Widget::<(), iced::Theme, crate::Renderer>::layout(label, tree, renderer, &limits).size()
    }

    fn state_of(tree: &Tree) -> &State<<crate::Renderer as text::Renderer>::Paragraph> {
        tree.state.downcast_ref()
    }

    /// The step is taken from the line as measured at rest, and holds for
    /// the whole of a transition: re-measured every frame, a moving weight
    /// would nudge the width, the step with it, and the grid the size is
    /// rounded to would shift partway there.
    #[test]
    fn the_size_step_holds_for_the_whole_of_a_transition() {
        let renderer = renderer_at(1.0);
        let motion = Motion::new();
        let (size_key, weight_key) = (MotionKey::unique(), MotionKey::unique());

        let label = |size: &Anim<f32>, weight: &Anim<f32>| -> Label {
            animated_text("Downloading Libraries", STYLE)
                .size(size.clone())
                .weight(weight.clone())
        };

        let resting = (
            motion.to(size_key, QUICK, 14.0),
            motion.to(weight_key, QUICK, 400.0),
        );
        let mut first = label(&resting.0, &resting.1);
        let mut tree = Tree::new(&first as &dyn Widget<(), iced::Theme, crate::Renderer>);
        let _ = lay(&mut first, &mut tree, &renderer);
        let at_rest = state_of(&tree).em_width;
        assert!(at_rest > 0.0, "the line was measured at rest");

        // A heavier weight widens the line as it moves.
        let size = motion.to(size_key, QUICK, 18.0);
        let weight = motion.to(weight_key, QUICK, 800.0);
        let mut clock = FrameClock::new(&motion);

        let mut steps = Vec::new();
        for _ in 0..8 {
            let _ = clock.run(1);
            let _ = lay(&mut label(&size, &weight), &mut tree, &renderer);

            assert_eq!(
                state_of(&tree).em_width,
                at_rest,
                "re-measured mid-transition"
            );
            steps.push(state_of(&tree).size_step);
        }
        assert!(
            steps
                .windows(2)
                .all(|pair| pair[0].to_bits() == pair[1].to_bits()),
            "the step moved: {steps:?}"
        );
    }

    /// A snapped line box leaves exactly from the height it rested at and
    /// lands exactly on the one it rests at: the first half of the way is
    /// whole device pixels from the first, the second from the second.
    /// Anchored at the target alone, 30 → 24 px at 1.25x is 7.5 device pixels,
    /// and the first frame would jump by the half.
    #[test]
    fn a_snapped_line_box_leaves_its_rest_on_its_grid() {
        let scale = 1.25;
        let renderer = renderer_at(scale);

        for size_layout in [SizeLayout::Live, SizeLayout::Composited] {
            let motion = Motion::new();
            let key = MotionKey::unique();

            let label = |size: &Anim<f32>| -> Label {
                animated_text("Downloading Libraries", STYLE)
                    .size(size.clone())
                    .size_layout(size_layout)
                    .pixel_snap(true)
            };

            let resting = motion.to(key, QUICK, 20.0);
            let mut first = label(&resting);
            let mut tree = Tree::new(&first as &dyn Widget<(), iced::Theme, crate::Renderer>);
            let rest = lay(&mut first, &mut tree, &renderer).height;
            assert_eq!(rest, 30.0);

            let size = motion.to(key, QUICK, 16.0);
            let mut clock = FrameClock::new(&motion);
            let mut heights = Vec::new();
            let _ = clock.run(1);
            while size.is_animating() {
                heights.push(lay(&mut label(&size), &mut tree, &renderer).height);
                let _ = clock.run(1);
            }
            let end = lay(&mut label(&size), &mut tree, &renderer).height;
            assert_eq!(end, 24.0, "{size_layout:?}: it rests on its line box");

            for height in &heights {
                let anchor = if (height - rest).abs() < (height - end).abs() {
                    rest
                } else {
                    end
                };
                let steps = (height - anchor) * scale;

                assert!(
                    (steps - steps.round()).abs() < 1e-3,
                    "{size_layout:?}: {height} is {steps} device px from {anchor}"
                );
            }
            assert!(
                heights
                    .iter()
                    .any(|height| (height - rest).abs() < (height - end).abs()),
                "{size_layout:?}: no frame left from the rest's grid"
            );
        }
    }

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

        let step = DEFAULT_SIZE_STEP;
        assert_eq!(
            (live.shaped_size(step), live.drawn_size(step)),
            (28.0, 28.0)
        );
        assert_eq!(
            (scaled.shaped_size(step), scaled.drawn_size(step)),
            (14.0, 28.0)
        );
        assert_eq!(scaled.shaped_style(step, None).line_height, 20.0);
    }

    /// A 14 → 18 px line whose size has just started moving, and its clock.
    fn moving_size(layout: SizeLayout) -> (Label, FrameClock) {
        let motion = Motion::new();
        let key = MotionKey::unique();
        let _ = motion.to(key, QUICK, 14.0);
        let size = motion.to(key, QUICK, 18.0);

        let label: Label = animated_text("Save", STYLE).size(size).size_layout(layout);

        (label, FrameClock::new(&motion))
    }

    /// Rounded, the line box of a moving size stands still for several
    /// frames and then jumps a whole pixel, and so does everything after it.
    #[test]
    fn a_moving_live_size_sets_an_unrounded_line_box() {
        let step = MIN_SIZE_STEP;
        let (label, mut clock) = moving_size(SizeLayout::Live);

        let fractional = (0..30).any(|_| {
            let _ = clock.run(1);
            let style = label.shaped_style(step, None);

            assert_eq!(
                style,
                STYLE.resized_exact(label.shaped_size(step)),
                "the line box of the moment, unrounded"
            );
            style.line_height.fract() != 0.0
        });
        assert!(fractional, "no frame set a fractional line box");

        let _ = clock.run_until_settled();
        assert_eq!(
            label.shaped_style(step, None),
            TextStyle::text(TextSize::Lg, Weight::Medium),
            "at rest the line box is the scale's again"
        );
    }

    /// A scaled line is shaped at its target, where the rounded and the exact
    /// line box agree; rounding keeps it on the box `layout` reserved.
    #[test]
    fn a_moving_scaled_size_keeps_the_rounded_line_box() {
        let step = MIN_SIZE_STEP;
        let (label, mut clock) = moving_size(SizeLayout::Scaled);

        let _ = clock.run(3);
        assert_eq!(label.shaped_style(step, None).line_height, 28.0);
    }

    #[test]
    fn a_given_size_step_overrides_the_one_the_line_needs() {
        let derived: Label = animated_text("Save", STYLE);
        assert_eq!(derived.size_step_for(10.0), default_size_step(10.0));

        let given: Label = animated_text("Save", STYLE).size_step(0.5);
        assert_eq!(given.size_step_for(10.0), 0.5);
    }

    #[test]
    fn a_line_is_measured_in_ems() {
        let style = TextStyle::text(TextSize::Xl, Weight::Medium);

        assert_eq!(em_width(Size::new(200.0, 30.0), style), 10.0);
        assert_eq!(
            em_width(Size::new(f32::INFINITY, 30.0), style),
            0.0,
            "an unbounded measure is no measure"
        );
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
