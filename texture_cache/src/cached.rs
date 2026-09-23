//! [`Cached`]: rasterize a subtree once, composite it every frame.

use iced_animate::{Anim, Tier};
use iced_core::layout::{self, Layout};
use iced_core::widget::{Operation, Tree, Widget, tree};
use iced_core::{
    Clipboard, Element, Event, Length, Point, Rectangle, Shell, Size, Transformation, Vector,
    mouse, overlay, renderer, window,
};

use crate::ancestors;
use crate::filter::FilterQuality;
use crate::geometry;
use crate::reaction::{Activity, observe};
use crate::record::{Record, TextureRenderer};
use crate::renderer::Backend;
use crate::texture_cache::{TextureCache, TextureCacheId};
use crate::warp::{Genie, GenieShape, Warp};

/// Logical pixels of content padding recorded around the layout bounds, so
/// bilinear filtering at the texture's edge does not clip anti-aliasing.
const BLEED: u32 = 2;

/// Whether the composited texture is snapped to the device-pixel grid.
///
/// Snapping the texel grid onto the device grid avoids the resampling that
/// softens a texture composited at a fractional position, but quantizes
/// motion to whole device pixels, which reads as jitter while moving.
///
/// # Examples
///
/// ```no_run
/// use iced::widget::text;
/// use iced_texture_cache::{PixelSnap, TextureCache, cached};
///
/// let cache = TextureCache::new();
/// let _: iced_texture_cache::Element<'_, ()> =
///     cached(cache, text("crisp when still")).pixel_snap(PixelSnap::Auto).into();
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PixelSnap {
    /// Snap only a pure translation that is **at rest**: crisp when stopped,
    /// smooth while moving. Never snaps while a live scale is bound (the
    /// snap→unsnap flip at the first frame of a scale would read as a jerk).
    ///
    /// The flip is the catch. A layer that glides sub-pixel and then lands on
    /// the grid changes its rendering in one frame, at the moment the eye has
    /// just been told the motion is over — which reads worse than the steady
    /// roughness of [`LayoutOnly`](PixelSnap::LayoutOnly), even though less
    /// total movement is quantised. Reach for `Auto` when smooth motion
    /// matters more than the frame it ends on.
    Auto,
    /// Always snap the composited origin to the device grid.
    Always,
    /// Never snap; rely on [`Cached::supersample`] or accept the softness of
    /// a fractional position.
    Never,
    /// Snap the *discrete* part of the origin and carry the smooth part on
    /// top of it untouched. The default, and the only policy whose rendering
    /// never switches modes: nothing is rounded that is moving, and nothing
    /// that has stopped is left off the grid, so there is no frame in which
    /// the layer changes how it is drawn.
    ///
    /// Which part is discrete depends on where the motion comes from, and
    /// this tier covers both:
    ///
    /// * With a running [`translate`](Cached::translate) or
    ///   [`scale`](Cached::scale), the layout box is the discrete part: it is
    ///   snapped every frame, so the resampling phase varies only with the
    ///   animation and the blur level cannot "breathe" when the surrounding
    ///   layout reshuffles mid-animation.
    /// * With no animation of its own, a widget can still be moving, because
    ///   an ancestor is animating the space it is laid out in — the way a card
    ///   re-centres its header as the page stack below it interpolates its
    ///   height. Then the *last resting position* is the discrete part; it
    ///   stays snapped while the travel since rides on top fractionally,
    ///   instead of the creep being rounded into a staircase of whole-pixel
    ///   jumps.
    ///
    /// The cost is up to ½ device pixel of displacement between the layout box
    /// and the image. The texture and its viewport still agree exactly: the
    /// record translates by the snapped origin too.
    #[default]
    LayoutOnly,
}

/// Caches the rasterization of `content` in a [`TextureCache`] and
/// composites that texture, optionally transformed, on every frame.
///
/// `Cached` has no size of its own: it reports and lays out exactly as its
/// content does.
///
/// # When the texture is recorded
///
/// * the cache is new or was [`invalidated`](TextureCache::invalidate);
/// * the layout size or the window's scale factor changed;
/// * the [`supersample`](Self::supersample) factor changed, including the
///   1.5× step of [`supersample_in_motion`](Self::supersample_in_motion);
/// * the content reacted to an event and
///   [`auto_invalidate`](Self::auto_invalidate) is on (the default);
/// * a nested `Cached` inside the content was invalidated (its image is
///   baked into this texture).
///
/// # Hit-testing and overlays
///
/// Only the `cursor` handed to the content is mapped through
/// [`translate`](Self::translate) and [`scale`](Self::scale); positions
/// carried *by events* (`CursorMoved`, touch) reach the content untransformed,
/// and so do the bounds seen by operations (`focus`, scroll-to). An overlay
/// (pick list menu, tooltip) of translated content opens where the image
/// is; iced's overlay API has no scale, so the overlay of *scaled* content
/// opens at the layout origin. A scale within `1e-4` of zero disables
/// hit-testing (the cursor becomes `Unavailable`).
///
/// # Z-order
///
/// Anything drawn *after* a `Cached` within the same parent layer renders
/// beneath the cached texture (iced's layer stack reopens the previous layer
/// after a clip; the same happens for `image`/`text` versus quads). Put
/// overlapping siblings in a `stack`, which gives every child its own layer.
///
/// # Limits
///
/// * The texture covers the **whole layout box**, not the visible part: a
///   `Cached` around a long list inside a `scrollable` records the entire
///   list.
/// * Content larger than the backend's texture limit (8192 px on wgpu's
///   default limits, 16 384 px on the software backend) is drawn directly
///   in place, with its clip and transform but **without** group opacity; a
///   warning is logged once per cache.
/// * Content outside the parent's clip, or with an opacity of zero, is
///   neither recorded nor composited.
/// * The scale factor used for recording lags one frame behind a DPI
///   change, so the first frames after one record twice.
/// * Paint-only animation *inside* the subtree is not detected (the engine
///   ticks outside it); call [`TextureCache::invalidate`] or animate on the
///   outside.
///
/// # Examples
///
/// ```no_run
/// use iced::widget::text;
/// use iced_texture_cache::{TextureCache, cached};
///
/// let cache = TextureCache::new();
/// let _: iced_texture_cache::Element<'_, ()> = cached(cache, text("expensive")).into();
/// ```
pub struct Cached<'a, Message, Theme = iced_core::Theme, Renderer = crate::Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    content: Element<'a, Message, Theme, Renderer>,
    cache: TextureCache,
    supersample: f32,
    translate: Anim<Vector>,
    scale: Anim<f32>,
    opacity: Anim<f32>,
    auto_invalidate: bool,
    /// See [`Cached::live_at_rest`].
    live_at_rest: bool,
    pixel_snap: PixelSnap,
    supersample_in_motion: bool,
    /// `None` inherits the renderer's tier; see [`Cached::filter_quality`].
    filter: Option<FilterQuality>,
    /// The shape a genie collapses with, or `None` for no warp at all.
    warp_shape: Option<GenieShape>,
    /// Genie progress. Meaningless without an anchor.
    warp_progress: Anim<f32>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Cached<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cached")
            .field("cache", &self.cache)
            .field("supersample", &self.supersample)
            .field("translate", &self.translate)
            .field("scale", &self.scale)
            .field("opacity", &self.opacity)
            .field("auto_invalidate", &self.auto_invalidate)
            .field("live_at_rest", &self.live_at_rest)
            .field("pixel_snap", &self.pixel_snap)
            .field("supersample_in_motion", &self.supersample_in_motion)
            .field("filter", &self.filter)
            .finish_non_exhaustive()
    }
}

/// Caches the rasterization of `content` under `cache`. See [`Cached`].
#[must_use]
pub fn cached<'a, Message, Theme, Renderer>(
    cache: TextureCache,
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Cached<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    Cached::new(cache, content)
}

impl<'a, Message, Theme, Renderer> Cached<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    /// Caches the rasterization of `content` under `cache`.
    #[must_use]
    pub fn new(
        cache: TextureCache,
        content: impl Into<Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            content: content.into(),
            cache,
            supersample: 1.0,
            translate: Anim::constant(Vector::ZERO),
            scale: Anim::constant(1.0),
            opacity: Anim::constant(1.0),
            auto_invalidate: true,
            live_at_rest: false,
            pixel_snap: PixelSnap::LayoutOnly,
            supersample_in_motion: false,
            filter: None,
            warp_shape: None,
            warp_progress: Anim::constant(1.0),
        }
    }

    /// Overrides the [`FilterQuality`] used to composite this texture.
    ///
    /// Without it the widget inherits the renderer's tier: the app-wide
    /// [`set_filter_quality`](crate::set_filter_quality) override if one is
    /// set, otherwise the tier chosen for the graphics adapter.
    ///
    /// [`FilterQuality::Snap`] also forces the composite onto the device-pixel
    /// grid, overriding [`pixel_snap`](Self::pixel_snap). Changing the tier
    /// never re-records the texture.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iced::widget::text;
    /// use iced_texture_cache::{FilterQuality, TextureCache, cached};
    ///
    /// let cache = TextureCache::new();
    /// let _: iced_texture_cache::Element<'_, ()> = cached(cache, text("sharp"))
    ///     .filter_quality(FilterQuality::CatmullRom)
    ///     .into();
    /// ```
    #[must_use]
    pub fn filter_quality(mut self, quality: FilterQuality) -> Self {
        self.filter = Some(quality);
        self
    }

    /// Sets the [`PixelSnap`] policy (default [`PixelSnap::LayoutOnly`]).
    ///
    /// [`FilterQuality::Snap`] overrides this: see
    /// [`filter_quality`](Self::filter_quality).
    #[must_use]
    pub fn pixel_snap(mut self, mode: PixelSnap) -> Self {
        self.pixel_snap = mode;
        self
    }

    /// Records the texture at `factor` times the device scale. `NaN` or
    /// values below 1 are treated as 1. Costs `factor²` memory; changing it
    /// re-records.
    #[must_use]
    pub fn supersample(mut self, factor: f32) -> Self {
        self.supersample = factor.max(1.0);
        self
    }

    /// Records at `max(supersample, 1.5)`× **only while the texture is
    /// moving**, dropping back to the plain factor at rest (where `Auto`
    /// snaps for a 1:1 blit). Off by default: it costs one extra record at
    /// each rest↔motion transition.
    ///
    /// Meant for `Anim`-driven motion. A constant offset supplied from
    /// `view()` that dwells on one value for a frame counts as a rest, so
    /// app-driven motion pays the transition cost at every dwell. An opacity
    /// fade is not motion.
    #[must_use]
    pub fn supersample_in_motion(mut self, on: bool) -> Self {
        self.supersample_in_motion = on;
        self
    }

    /// Re-record automatically when the content reacts to an event
    /// (captures it, publishes a message, invalidates layout/widgets,
    /// requests a redraw) or its cursor-dependent appearance changes.
    /// Enabled by default; disable it so that events no longer re-record.
    /// Explicit [`TextureCache::invalidate`], size and scale changes and
    /// nested caches still do.
    ///
    /// Engine tracks that change only *paint* inside the subtree are not
    /// detected (the engine ticks in [`Host`](iced_animate::widget::Host),
    /// outside the subtree, so the child's shell stays silent); size changes
    /// are, because the renderer compares the recorded size.
    #[must_use]
    pub fn auto_invalidate(mut self, enabled: bool) -> Self {
        self.auto_invalidate = enabled;
        self
    }

    /// Draw the content live, in place, whenever nothing needs a texture, and
    /// record only while the layer moves.
    ///
    /// Disabled by default. Enable it to keep an animation wrapper in the tree
    /// permanently: iced rebuilds a subtree whose wrapper appears or
    /// disappears, losing its scroll offsets and focus, so a wrapper added
    /// only for the length of an animation costs the content its state. With
    /// this on, the resting wrapper costs no record at all.
    ///
    /// "Nothing needs a texture" means the translation, scale and warp are
    /// not animating, the warp is not live and the opacity is `1.0`; a
    /// resting translation or scale is drawn live under the same transform.
    /// The first frame of any movement records, and that record is always
    /// fresh: the texture is invalidated whenever the layer leaves live
    /// drawing, so content that changed while live is never shown stale.
    #[must_use]
    pub fn live_at_rest(mut self, enabled: bool) -> Self {
        self.live_at_rest = enabled;
        self
    }

    /// Offsets the cached texture when compositing, without moving its
    /// layout box. Resolved every frame inside the widget, so an animated
    /// value keeps moving between view rebuilds. Costs neither a relayout
    /// nor a re-record ([`Tier::Composite`]); pointer input follows the
    /// moved image (see "Hit-testing and overlays" on [`Cached`]).
    #[must_use]
    pub fn translate(mut self, offset: impl Into<Anim<Vector>>) -> Self {
        self.translate = offset.into();
        self.translate.mark_tier(Tier::Composite);
        self
    }

    /// Scales the cached texture about its own centre when compositing.
    /// The layout box is unchanged, so siblings do not move. Pair a scale-up
    /// with [`supersample`](Self::supersample) to keep it sharp. Scale is
    /// expected to be positive; a scale within `1e-4` of zero draws nothing
    /// useful and disables hit-testing.
    #[must_use]
    pub fn scale(mut self, scale: impl Into<Anim<f32>>) -> Self {
        self.scale = scale.into();
        self.scale.mark_tier(Tier::Composite);
        self
    }

    /// Group opacity applied to the composited texture: the subtree fades as
    /// one image, not piece by piece. The cheapest thing to animate; it
    /// never affects snapping or supersampling.
    #[must_use]
    pub fn opacity(mut self, opacity: impl Into<Anim<f32>>) -> Self {
        self.opacity = opacity.into();
        self.opacity.mark_tier(Tier::Composite);
        self
    }

    /// Warps the cached texture when compositing: a non-affine collapse that
    /// [`scale`](Self::scale) cannot express. Resolved every frame inside the
    /// widget, and costs neither a relayout nor a re-record
    /// ([`Tier::Composite`]).
    ///
    /// A warp only ever shrinks the image inside its own rectangle, so
    /// siblings do not move and nothing has to make room. Drive it with a
    /// curve that does not overshoot — `curves::QUICK` and `curves::SMOOTH`
    /// are zero-bounce springs — since progress is clamped to `0..=1` and a
    /// bouncy curve would simply flatten against that ceiling.
    ///
    /// The pointer follows the picture: a press lands on the content it is
    /// drawn over, and where the collapsed shape does not reach — all of
    /// it, at progress `0` — the content sees no cursor at all.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iced::widget::text;
    /// use iced_texture_cache::{GenieShape, TextureCache, cached};
    ///
    /// let cache = TextureCache::new();
    /// let _: iced_texture_cache::Element<'_, ()> = cached(cache, text("collapsing"))
    ///     .genie(0.5, GenieShape::default())
    ///     .into();
    /// ```
    #[must_use]
    pub fn genie(mut self, progress: impl Into<Anim<f32>>, shape: GenieShape) -> Self {
        self.warp_shape = Some(shape);
        self.warp_progress = progress.into();
        self.warp_progress.mark_tier(Tier::Composite);
        self
    }

    /// The warp composited this frame, built from the anchor and whatever
    /// progress has reached.
    fn warp(&self) -> Warp {
        match self.warp_shape {
            None => Warp::None,
            Some(shape) => Warp::Genie(Genie::new(self.warp_progress.get(), shape)),
        }
    }

    /// Whether the transform is still moving. Opacity is deliberately not
    /// part of it: a fade changes no texel phase.
    fn is_moving(&self) -> bool {
        self.translate.is_animating()
            || self.scale.is_animating()
            || self.warp_progress.is_animating()
    }

    /// Whether this frame is drawn live rather than composited from a
    /// texture; see [`Cached::live_at_rest`].
    fn draws_live(&self) -> bool {
        self.live_at_rest
            && !self.is_moving()
            && !self.opacity.is_animating()
            && self.opacity.get() >= 1.0
            && !self.warp().is_live()
    }

    /// Draws the content in place for [`Cached::live_at_rest`]: like the
    /// uncacheable fallback, in its own layer, so the z-order is the same as
    /// when composited. The transform is not snapped, so at rest the content
    /// lands exactly where it would without this wrapper.
    #[allow(clippy::too_many_arguments)]
    fn draw_live(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let transform = self.transform(bounds);
        let Some(clip) = geometry::composite_clip(bounds.expand(BLEED as f32), transform, viewport)
        else {
            return;
        };
        let cursor = self.content_cursor(cursor, bounds, transform, renderer);
        let content_viewport = clip * transform.inverse();
        renderer.with_layer(clip, |r| {
            r.with_transformation(transform, |r| {
                self.content.as_widget().draw(
                    &tree.children[0],
                    r,
                    theme,
                    style,
                    layout,
                    cursor,
                    &content_viewport,
                );
            });
        });
    }

    /// Draws the content composited from its texture, recording it first if
    /// it is stale.
    #[allow(clippy::too_many_arguments)]
    fn draw_composited(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let scale = renderer.scale_factor();

        // Fully transparent (or NaN) content has nothing to show: not worth
        // a record, nor the geometry below.
        let opacity = self.opacity.get();
        if opacity.is_nan() || opacity <= 0.0 {
            return;
        }
        let opacity = opacity.min(1.0);

        let warp = self.warp();

        let user_transform = self.transform(bounds);

        let at_rest = state.at_rest;
        let filter = self.filter.unwrap_or_else(|| renderer.filter_quality());
        // The neck minifies hard, and a cache texture has no mip chain to
        // minify through. `CatmullRom`'s high-frequency boost makes that
        // worse rather than better, so a live warp drops to a single
        // bilinear tap and the configured tier returns at rest.
        let filter = if warp.is_live() && !filter.snaps() {
            FilterQuality::Bilinear
        } else {
            filter
        };

        let supersample =
            geometry::record_supersample(self.supersample, self.supersample_in_motion, at_rest);
        // A widget that has never been through a redraw has no anchor yet;
        // where it is now is the best available answer, and it is a resting
        // position by definition.
        let layout_anchor = (self.pixel_snap == PixelSnap::LayoutOnly)
            .then(|| state.snap_anchor.unwrap_or_else(|| bounds.position()));
        let composite =
            geometry::composite_geometry(BLEED, bounds, scale, supersample, layout_anchor);

        let snap = geometry::snap_decision(
            filter,
            self.pixel_snap,
            self.scale.is_live() || warp.is_live(),
            geometry::is_translation_only(&user_transform),
            at_rest,
        );
        let transform = if snap {
            geometry::snap_transform(user_transform, composite.cache_bounds.position(), scale)
        } else {
            user_transform
        };

        // Off-screen content has nowhere to show: not worth a record either.
        let Some(clip) = geometry::composite_clip(composite.cache_bounds, transform, viewport)
        else {
            return;
        };

        let cursor = self.content_cursor(cursor, bounds, user_transform, renderer);
        let content = &self.content;
        let content_tree = &tree.children[0];
        // The content's layout origin lands `BLEED` texels into the texture,
        // whatever the texture is composited at: with `LayoutOnly` that is
        // the snapped origin, which displaces the image by the snap delta
        // (up to ½ device pixel) instead of resampling it. The viewport is
        // the texture's extent in the content's own space.
        let origin = Vector::new(BLEED as f32 - bounds.x, BLEED as f32 - bounds.y);
        let record_viewport = Rectangle {
            x: bounds.x - BLEED as f32,
            y: bounds.y - BLEED as f32,
            ..composite.cache_bounds
        };

        let record = renderer.record(
            &self.cache,
            composite.physical,
            composite.texture_scale,
            |r| {
                // Texture space: `(0, 0)` is the (padded, possibly snapped) origin.
                r.with_translation(origin, |r| {
                    content.as_widget().draw(
                        content_tree,
                        r,
                        theme,
                        style,
                        layout,
                        cursor,
                        &record_viewport,
                    );
                });
            },
        );

        match record {
            Record::Fresh | Record::Reused => {
                renderer.draw_cached(
                    &self.cache,
                    composite.cache_bounds,
                    composite.content_bounds,
                    clip,
                    transform,
                    opacity,
                    filter,
                    warp,
                );
            }
            Record::Uncacheable => {
                // Too large for a texture: draw in place with the same clip
                // (opened in the enclosing layout space, outside the user
                // transform, because iced's clip layers do not intersect with
                // their parent) and the same transform. Group opacity cannot
                // be applied without a texture, and neither can the genie:
                // the software backend's affine approximation stands in, so
                // the content still collapses and the pointer mapping above
                // agrees with what is drawn. The viewport is that clip in
                // the content's space, so huge content still culls.
                let Some(fallback) = warp.affine_transform(bounds) else {
                    return;
                };
                let transform = transform * fallback;
                let content_viewport = clip * transform.inverse();
                renderer.with_layer(clip, |r| {
                    r.with_transformation(transform, |r| {
                        content.as_widget().draw(
                            content_tree,
                            r,
                            theme,
                            style,
                            layout,
                            cursor,
                            &content_viewport,
                        );
                    });
                });
            }
        }
    }

    /// Invalidates the texture when recording follows live drawing. `update`
    /// does this on each frame's `RedrawRequested`; a frame drawn before that
    /// reached `update` still has the flag from its live spell, and a texture
    /// that predates it.
    fn leave_live(&self, state: &State) {
        if self.live_at_rest && state.live {
            self.cache.invalidate();
        }
    }

    /// The transform actually composited this frame.
    fn transform(&self, bounds: Rectangle) -> Transformation {
        geometry::effective_transform(self.translate.get(), self.scale.get(), bounds)
    }

    /// The cursor as the content sees it: back through the affine
    /// transform, then through whatever warp is actually being drawn, so a
    /// press lands on the pixel it was aimed at and collapsed content is
    /// not clickable where it used to be. The shader draws the genie
    /// itself; the software backend, and content too large for a texture
    /// on either, draw the affine approximation.
    fn content_cursor(
        &self,
        cursor: mouse::Cursor,
        bounds: Rectangle,
        transform: Transformation,
        renderer: &Renderer,
    ) -> mouse::Cursor {
        let cursor = geometry::translate_cursor(cursor, transform);
        let exact = renderer.backend() == Backend::Wgpu && !self.cache.is_uncacheable();
        self.warp().map_cursor(exact, bounds, cursor)
    }
}

#[derive(Debug)]
struct State {
    cache_id: TextureCacheId,
    activity: Activity,
    /// The child's `mouse::Interaction` on the previous frame. A change means
    /// its cursor-dependent appearance changed, even when it was the layer
    /// that moved, not the cursor, so the cache must be re-recorded.
    last_interaction: mouse::Interaction,
    /// The effective transform seen on the previous `RedrawRequested`.
    last_transform: Option<Transformation>,
    /// The layout bounds seen on the previous `RedrawRequested`.
    ///
    /// A `Cached` with no animation of its own can still be moving: an
    /// ancestor may be animating the space it is laid out in, the way a card
    /// re-centres its header as the page stack below it interpolates its
    /// height. Comparing bounds between frames is how it finds out — the
    /// same question `Shape` asks, for the same reason.
    last_bounds: Option<Rectangle>,
    /// Where the layout put this widget the last time nothing was moving it.
    ///
    /// [`PixelSnap::LayoutOnly`] snaps *this* rather than the live bounds, and
    /// adds the travel since on top untouched. Holding it for the length of a
    /// move is what separates the discrete part of the origin from the smooth
    /// one when the motion arrives through layout — as it does for a card
    /// header re-centred by the page stack below it, which has no transform of
    /// its own to split the two.
    snap_anchor: Option<Point>,
    /// The transform is not animating and did not change since the previous
    /// frame: the texture may snap to the device grid.
    at_rest: bool,
    /// The cache generation last propagated to the ancestors.
    propagated_generation: u64,
    /// Whether the previous frame was drawn live ([`Cached::live_at_rest`]).
    /// Starts `true`, so a layer that is moving from its very first frame
    /// records afresh even with a cache that holds another tree's picture.
    live: bool,
}

impl State {
    fn new(cache_id: TextureCacheId) -> Self {
        Self {
            cache_id,
            activity: Activity::default(),
            last_interaction: mouse::Interaction::None,
            last_transform: None,
            last_bounds: None,
            snap_anchor: None,
            at_rest: false,
            propagated_generation: 0,
            live: true,
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Cached<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(self.cache.id()))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        if tree.state.downcast_ref::<State>().cache_id != self.cache.id() {
            tree.state = self.state();
        }
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let user_transform = self.transform(bounds);
        let cursor = self.content_cursor(cursor, bounds, user_transform, renderer);

        let redraw_now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => Some(*now),
            _ => None,
        };

        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_mut::<State>();
        let content_tree = &mut children[0];

        if redraw_now.is_some() {
            // Rest is decided once per frame, here, so a second `draw` of the
            // same frame cannot flip the snap decision.
            // The first observed frame counts as rest: nothing has moved yet,
            // so it must not pay a supersample-in-motion record.
            state.at_rest = !self.is_moving()
                && state
                    .last_transform
                    .is_none_or(|last| last == user_transform)
                && state.last_bounds.is_none_or(|last| last == bounds);
            state.last_transform = Some(user_transform);
            state.last_bounds = Some(bounds);
            // Which part of the origin is the discrete one depends on where
            // the motion comes from. A widget animating its own transform has
            // the layout as its discrete part, so the anchor follows the
            // bounds every frame and a mid-animation reshuffle is still
            // snapped away — the case this tier was written for. A widget
            // standing still that is nonetheless being carried has the
            // opposite split: the anchor holds at its last resting position
            // and the travel since rides on top unrounded.
            if state.at_rest || self.is_moving() {
                state.snap_anchor = Some(bounds.position());
            }
            // Leaving live drawing: whatever the texture holds predates what
            // was drawn live, so the first moving frame must record afresh.
            if self.live_at_rest {
                let live = self.draws_live();
                if state.live && !live {
                    self.cache.invalidate();
                }
                state.live = live;
            }
        }

        let mut local_messages = Vec::new();
        let mut local = Shell::new(&mut local_messages);

        ancestors::with_ancestor(&self.cache, || {
            self.content.as_widget_mut().update(
                content_tree,
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                &mut local,
                viewport,
            );
        });

        if self.auto_invalidate {
            // A cursor-driven appearance change the child applies silently on
            // `RedrawRequested` (a moving layer crossing the cursor over/off
            // an interactive child): `mouse_interaction` flips at the boundary.
            // The walk is skipped while the cursor is away and the previous
            // frame already showed no interaction: nothing can have changed.
            let worth_checking = redraw_now.is_some()
                && (cursor.is_over(bounds) || state.last_interaction != mouse::Interaction::None);
            let interaction_changed = worth_checking && {
                let interaction = self.content.as_widget().mouse_interaction(
                    content_tree,
                    layout,
                    cursor,
                    viewport,
                    renderer,
                );
                let changed = state.last_interaction != interaction;
                state.last_interaction = interaction;
                changed
            };

            if observe(&local, redraw_now, interaction_changed, &mut state.activity) {
                self.cache.invalidate();
            }
        }

        // Our texture is baked into every ancestor's: tell them once per
        // invalidation, not once per event while we are hidden.
        let _ = ancestors::propagate(&self.cache, &mut state.propagated_generation);

        shell.merge(local, |m| m);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let cursor = self.content_cursor(cursor, bounds, self.transform(bounds), renderer);

        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // An overlay of translated content opens where the image is; iced's
        // overlay API has no scale, so scaled content keeps the layout origin.
        let transform = self.transform(layout.bounds());
        let translation = if geometry::is_translation_only(&transform) {
            translation + transform.translation()
        } else {
            translation
        };

        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if self.draws_live() {
            self.draw_live(tree, renderer, theme, style, layout, cursor, viewport);
        } else {
            self.leave_live(tree.state.downcast_ref::<State>());
            self.draw_composited(tree, renderer, theme, style, layout, cursor, viewport);
        }
    }
}

impl<'a, Message, Theme, Renderer> From<Cached<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + TextureRenderer + 'a,
{
    fn from(cached: Cached<'a, Message, Theme, Renderer>) -> Self {
        Element::new(cached)
    }
}

// The harness is software-only (see `test_support`).
#[cfg(all(test, feature = "tiny-skia"))]
mod tests {
    use super::*;
    use crate::test_support::Harness;
    use iced::widget::{button, column, row, scrollable, space};
    use iced_animate::widget::shape;
    use iced_core::Color;
    use iced_core::time::Instant;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        Go,
    }

    fn square<'a, M: 'a>(color: Color) -> crate::Element<'a, M> {
        shape().width(40.0).height(20.0).fill(color).into()
    }

    #[test]
    fn a_translated_cache_reaches_the_compositor_tier_and_costs_no_relayout() {
        use iced_animate::curves::SMOOTH;
        use iced_animate::{Motion, key};
        use std::time::Duration;

        let motion = Motion::new();
        let key = key!();
        let _ = motion.to(key, SMOOTH, Vector::ZERO);
        let offset = motion.to(key, SMOOTH, Vector::new(64.0, 0.0));

        // The builder marks the tier: the value goes straight to the
        // compositor, so nothing below has to be measured or drawn again.
        let _: crate::Element<'_, ()> = cached(TextureCache::new(), square::<()>(Color::BLACK))
            .translate(offset.clone())
            .into();
        assert_eq!(offset.tier(), Some(Tier::Composite));

        let status = motion.tick(Instant::now() + Duration::from_millis(16));
        assert!(status.animating, "the offset is still moving");
        assert!(
            !status.layout_invalid,
            "a composited offset must not drag a relayout along"
        );
    }

    #[test]
    fn clicking_cached_content_invalidates_but_idle_redraws_do_not() {
        let cache = TextureCache::new();
        let mut harness = Harness::new(
            Size::new(300.0, 200.0),
            column![cached(cache.clone(), button("go").on_press(Message::Go))].width(Length::Fill),
        );
        let now = Instant::now();

        // A fresh cache starts invalidated; the first frame consumes it.
        harness.frame(now);
        assert_eq!(cache.record_count(), 1);
        harness.frame(now);
        assert!(!cache.is_invalidated(), "an idle frame must not re-record");
        assert_eq!(cache.record_count(), 1);

        // A click is captured and publishes a message: re-record.
        harness.click("go");
        assert!(cache.is_invalidated(), "a captured click must re-record");

        // Next redraw: the cursor now rests on the button, so its
        // `mouse_interaction` flipped `None -> Pointer` (hover appearance
        // changed): activity. Then one trailing re-record, then quiet.
        let mut sequence = Vec::new();
        for _ in 0..4 {
            harness.frame(now);
            sequence.push(cache.record_count());
        }
        assert_eq!(sequence, vec![2, 3, 3, 3]);
        assert_eq!(harness.into_messages(), vec![Message::Go]);
    }

    #[test]
    fn an_inner_cache_invalidation_reaches_its_ancestors() {
        let outer = TextureCache::new();
        let inner = TextureCache::new();
        let inner_el: crate::Element<'_, Message> =
            cached(inner.clone(), button("go").on_press(Message::Go)).into();
        let mut harness = Harness::new(
            Size::new(300.0, 200.0),
            column![cached(outer.clone(), column![inner_el])].width(Length::Fill),
        );
        let now = Instant::now();

        harness.frame(now);
        harness.frame(now);
        assert_eq!(
            (outer.record_count(), inner.record_count()),
            (1, 1),
            "idle: nothing"
        );

        harness.click("go");
        assert!(inner.is_invalidated(), "the click re-records the inner");
        assert!(outer.is_invalidated(), "…and therefore the outer");

        // Let the hover-change + trailing frames settle.
        for _ in 0..5 {
            harness.frame(now);
        }
        let settled = (outer.record_count(), inner.record_count());
        harness.frame(now);
        assert_eq!(
            (outer.record_count(), inner.record_count()),
            settled,
            "settled"
        );

        // An explicit inner invalidation (as an app would do in `update`)
        // reaches the outer on the next event.
        inner.invalidate();
        harness.redraw(now);
        assert!(
            outer.is_invalidated(),
            "explicit inner invalidate reaches the outer"
        );
    }

    #[test]
    fn a_hidden_inner_cache_does_not_keep_its_ancestors_invalidated() {
        let outer = TextureCache::new();
        let inner = TextureCache::new();
        let inner_el: crate::Element<'_, ()> = cached(inner.clone(), square(Color::BLACK))
            .opacity(0.0)
            .into();
        let mut harness = Harness::new(
            Size::new(100.0, 100.0),
            cached(outer.clone(), column![inner_el]),
        );
        let now = Instant::now();

        harness.frame(now);
        assert!(
            inner.is_invalidated(),
            "never drawn: its flag stays pending"
        );
        assert_eq!(inner.record_count(), 0);
        assert_eq!(outer.record_count(), 1);

        harness.frame(now);
        harness.frame(now);
        assert!(
            !outer.is_invalidated(),
            "a hidden inner must not re-invalidate the outer every frame"
        );
        assert_eq!(outer.record_count(), 1);
    }

    #[test]
    fn fully_transparent_content_is_neither_recorded_nor_drawn() {
        let cache = TextureCache::new();
        let red: crate::Element<'_, ()> = square(Color::from_rgb(1.0, 0.0, 0.0));
        let mut harness = Harness::new(
            Size::new(60.0, 40.0),
            cached(cache.clone(), red).opacity(0.0),
        );
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(cache.record_count(), 0);
        assert_eq!(&shot.pixel(10, 10)[..3], &[255, 255, 255]);
    }

    #[test]
    fn auto_invalidate_off_ignores_events_but_not_size_changes() {
        let cache = TextureCache::new();
        let build = |wide: bool| -> crate::Element<'_, Message> {
            let content = row![
                button("go").on_press(Message::Go),
                space().width(if wide { 80.0 } else { 0.0 })
            ];
            column![cached(cache.clone(), content).auto_invalidate(false)]
                .width(Length::Fill)
                .into()
        };
        let mut harness = Harness::new(Size::new(300.0, 200.0), build(false));
        let now = Instant::now();
        harness.frame(now);
        assert_eq!(cache.record_count(), 1);

        harness.click("go");
        for _ in 0..3 {
            harness.frame(now);
        }
        assert_eq!(cache.record_count(), 1, "events no longer re-record");

        let mut harness = harness.rebuild(build(true));
        harness.frame(now);
        assert_eq!(
            cache.record_count(),
            2,
            "a size change still does (renderer level)"
        );
        assert_eq!(harness.into_messages(), vec![Message::Go]);
    }

    #[test]
    fn collapsed_content_is_not_clickable_where_it_used_to_be() {
        use iced::widget::container;

        let cache = TextureCache::new();
        let build = |progress: f32| -> crate::Element<'_, Message> {
            container(
                cached(cache.clone(), button("go").on_press(Message::Go))
                    .genie(progress, GenieShape::default()),
            )
            .padding(20.0)
            .into()
        };

        // Fully collapsed into its top-left corner: the button is gone,
        // and so is its hit box.
        let mut harness = Harness::new(Size::new(200.0, 100.0), build(0.0));
        let now = Instant::now();
        harness.frame(now);
        harness.click("go");
        assert_eq!(harness.into_messages(), vec![]);

        // Half way, the software backend draws the button at half size
        // about its top-left corner: its old centre is outside the picture,
        // and the picture's centre is the button's.
        let mut harness = Harness::new(Size::new(200.0, 100.0), build(0.5));
        harness.frame(now);
        let mut find = iced_test::selector::Selector::find("go");
        harness.operate(&mut find);
        let iced_core::widget::operation::Outcome::Some(Some(target)) = find.finish() else {
            panic!("no button on screen")
        };
        let bounds = target.visible_bounds().expect("the text is visible");
        harness.click_at(bounds.center());
        harness.click_at(Point::new(
            bounds.x + bounds.width * 0.25,
            bounds.y + bounds.height * 0.25,
        ));
        assert_eq!(harness.into_messages(), vec![Message::Go]);

        // Open, everything is where it was.
        let mut harness = Harness::new(Size::new(200.0, 100.0), build(1.0));
        harness.frame(now);
        harness.click("go");
        assert_eq!(harness.into_messages(), vec![Message::Go]);
    }

    #[test]
    fn a_dpi_change_re_records_once() {
        let cache = TextureCache::new();
        let mut harness = Harness::new(
            Size::new(100.0, 50.0),
            cached(cache.clone(), square::<()>(Color::BLACK)),
        );
        let now = Instant::now();
        harness.frame(now);
        harness.frame(now);
        assert_eq!(cache.record_count(), 1, "same scale: reused");

        // Presenting at 2x moves the renderer's scale factor for the next frame.
        let _ = harness.screenshot(2.0);
        harness.frame(now);
        assert_eq!(cache.record_count(), 2, "the scale changed: one re-record");
        harness.frame(now);
        assert_eq!(cache.record_count(), 2);
    }

    #[test]
    fn layout_only_snap_records_once_and_draws() {
        use iced::widget::container;
        let cache = TextureCache::new();
        let red: crate::Element<'_, ()> = square(Color::from_rgb(1.0, 0.0, 0.0));
        let mut harness = Harness::new(
            Size::new(100.0, 60.0),
            container(cached(cache.clone(), red).pixel_snap(PixelSnap::LayoutOnly)).padding(10.3),
        );
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(cache.record_count(), 1);
        assert_eq!(&shot.pixel(30, 20)[..3], &[255, 0, 0]);
        assert_eq!(&shot.pixel(5, 5)[..3], &[255, 255, 255]);
        // The layout origin 10.3 is snapped to 10: the left edge column is
        // fully covered (a fractional composite would blend it) and nothing
        // spills onto column 9.
        assert_eq!(&shot.pixel(10, 20)[..3], &[255, 0, 0], "snapped edge");
        assert_eq!(&shot.pixel(9, 20)[..3], &[255, 255, 255]);
    }

    const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);
    const BLUE: Color = Color::from_rgb(0.0, 0.0, 1.0);

    /// Runs `harness` for `frames` frames of 16 ms from `*clock`.
    fn run<M>(harness: &mut Harness<'_, M>, clock: &mut Instant, frames: usize) {
        for _ in 0..frames {
            *clock += std::time::Duration::from_millis(16);
            harness.frame(*clock);
        }
    }

    #[test]
    fn live_at_rest_draws_in_place_without_recording() {
        let cache = TextureCache::new();
        let content = row![
            space().width(10.0),
            cached(cache.clone(), square::<()>(RED)).live_at_rest(true)
        ];
        let mut harness = Harness::new(Size::new(100.0, 40.0), content);
        harness.frame(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(cache.record_count(), 0, "nothing was recorded at rest");
        assert_eq!(&shot.pixel(20, 10)[..3], &[255, 0, 0], "drawn in place");
        assert_eq!(&shot.pixel(5, 10)[..3], &[255, 255, 255]);
    }

    #[test]
    fn without_live_at_rest_a_resting_layer_still_records() {
        let cache = TextureCache::new();
        let mut harness = Harness::new(
            Size::new(100.0, 40.0),
            cached(cache.clone(), square::<()>(RED)),
        );
        harness.frame(Instant::now());
        assert_eq!(cache.record_count(), 1, "the default is unchanged");
    }

    #[test]
    fn live_at_rest_records_only_while_moving() {
        use iced_animate::curves::SMOOTH;
        use iced_animate::{Motion, MotionKey};

        let motion = Motion::new();
        let cache = TextureCache::new();
        let offset = motion.enter(
            MotionKey::unique(),
            SMOOTH,
            Vector::ZERO,
            Vector::new(30.0, 0.0),
        );
        let root = motion.host(
            cached(cache.clone(), square::<()>(RED))
                .translate(offset.clone())
                .live_at_rest(true),
        );
        let mut harness = Harness::new(Size::new(100.0, 40.0), root);
        let mut clock = Instant::now();
        run(&mut harness, &mut clock, 3);
        assert!(cache.record_count() > 0, "a moving layer is a texture");

        run(&mut harness, &mut clock, 300);
        assert!(!offset.is_animating(), "the slide has arrived");
        let settled = cache.record_count();
        run(&mut harness, &mut clock, 3);
        assert_eq!(cache.record_count(), settled, "at rest it draws live");
        let shot = harness.screenshot(1.0);
        assert_eq!(&shot.pixel(35, 10)[..3], &[255, 0, 0], "at its rest offset");
        assert_eq!(&shot.pixel(5, 10)[..3], &[255, 255, 255]);
    }

    #[test]
    fn a_layer_that_starts_moving_records_what_it_shows_now() {
        // One cache across two slides, the content changed in between while
        // it drew live: the second slide must not bring back the first
        // slide's picture.
        use iced_animate::curves::SMOOTH;
        use iced_animate::{Motion, MotionKey};

        let motion = Motion::new();
        let cache = TextureCache::new();
        let first = motion.enter(
            MotionKey::unique(),
            SMOOTH,
            Vector::new(30.0, 0.0),
            Vector::ZERO,
        );
        let build = |color: Color, offset: Anim<Vector>| -> crate::Element<'_, ()> {
            motion
                .host(
                    cached(cache.clone(), square::<()>(color))
                        .translate(offset)
                        .live_at_rest(true),
                )
                .into()
        };
        let mut harness = Harness::new(Size::new(100.0, 40.0), build(RED, first.clone()));
        let mut clock = Instant::now();
        run(&mut harness, &mut clock, 300);
        assert!(!first.is_animating(), "the first slide has arrived");

        // At rest the content changes, then a second slide starts.
        let mut harness = harness.rebuild(build(BLUE, first));
        run(&mut harness, &mut clock, 2);
        let second = motion.enter(
            MotionKey::unique(),
            SMOOTH,
            Vector::ZERO,
            Vector::new(30.0, 0.0),
        );
        let mut harness = harness.rebuild(build(BLUE, second.clone()));
        run(&mut harness, &mut clock, 2);
        assert!(second.is_animating(), "the second slide is under way");
        let shot = harness.screenshot(1.0);
        assert_eq!(&shot.pixel(20, 10)[..3], &[0, 0, 255], "the new content");
    }

    #[test]
    fn the_first_frame_is_at_rest_so_supersample_in_motion_records_once() {
        use iced::widget::container;
        let cache = TextureCache::new();
        let mut harness = Harness::new(
            Size::new(100.0, 60.0),
            container(
                cached(cache.clone(), square::<()>(Color::BLACK)).supersample_in_motion(true),
            )
            .padding(10.3),
        );
        let now = Instant::now();
        harness.frame(now);
        harness.frame(now);
        assert_eq!(
            cache.record_count(),
            1,
            "no rest<->motion transition on the first frame"
        );
    }

    #[test]
    fn a_nested_pager_re_records_its_ancestor_while_sliding_and_stops_after() {
        use iced::widget::text;
        use iced_animate::Motion;
        use std::time::Duration;

        let motion = Motion::new();
        let outer = TextureCache::new();
        let build = |current: usize| -> crate::Element<'_, ()> {
            let pager = crate::pager([text("first"), text("second")])
                .current(current)
                .motion(motion.clone())
                .width(200.0);
            motion.host(cached(outer.clone(), pager)).into()
        };
        let mut harness = Harness::new(Size::new(300.0, 100.0), build(0));
        let start = Instant::now();
        harness.frame(start);
        harness.frame(start);
        assert_eq!(outer.record_count(), 1, "idle");

        let mut harness = harness.rebuild(build(1));
        let mut frame = 0;
        let mut previous = outer.record_count();
        for _ in 0..4 {
            frame += 1;
            harness.frame(start + Duration::from_millis(16 * frame));
            let count = outer.record_count();
            assert!(
                count > previous,
                "frame {frame}: the outer follows the slide"
            );
            previous = count;
        }
        // Let the slide arrive and the trailing frames settle.
        for _ in 0..300 {
            frame += 1;
            harness.frame(start + Duration::from_millis(16 * frame));
        }
        let settled = outer.record_count();
        for _ in 0..3 {
            frame += 1;
            harness.frame(start + Duration::from_millis(16 * frame));
        }
        assert_eq!(
            outer.record_count(),
            settled,
            "at rest the outer stops re-recording"
        );
    }

    #[test]
    fn uncacheable_content_is_drawn_under_its_translate() {
        // 17 000 logical px tall: uncacheable on the software backend. The
        // content is laid out at x = 100 and translated by -100, so it is
        // drawn from x = 0 to 100 and nothing to the right.
        let cache = TextureCache::new();
        let tall: crate::Element<'_, ()> = shape()
            .width(100.0)
            .height(17_000.0)
            .fill(Color::from_rgb(1.0, 0.0, 0.0))
            .into();
        let content = row![
            space().width(100.0),
            cached(cache.clone(), tall).translate(Vector::new(-100.0, 0.0))
        ];
        let root = scrollable(content).width(300.0).height(60.0);
        let mut harness = Harness::new(Size::new(300.0, 60.0), root);
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(cache.record_count(), 0, "nothing was recorded");
        assert_eq!(&shot.pixel(50, 20)[..3], &[255, 0, 0], "translated left");
        assert_eq!(
            &shot.pixel(150, 20)[..3],
            &[255, 255, 255],
            "nothing at the layout position"
        );
    }

    #[test]
    fn uncacheable_content_stays_inside_the_parent_clip() {
        let cache = TextureCache::new();
        let wide: crate::Element<'_, ()> = shape()
            .width(17_000.0)
            .height(40.0)
            .fill(Color::from_rgb(1.0, 0.0, 0.0))
            .into();
        let content = row![space().width(100.0), cached(cache.clone(), wide)];
        // A 150 px wide clipping parent (a horizontal scrollable lets the
        // content overflow it): the fallback must not paint past it.
        let root = scrollable(content)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .width(150.0)
            .height(60.0);
        let mut harness = Harness::new(Size::new(300.0, 60.0), root);
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(cache.record_count(), 0, "nothing was recorded");
        assert_eq!(
            &shot.pixel(120, 20)[..3],
            &[255, 0, 0],
            "inside the parent clip"
        );
        assert_eq!(
            &shot.pixel(200, 20)[..3],
            &[255, 255, 255],
            "outside the parent clip"
        );
    }

    #[test]
    fn uncacheable_content_is_drawn_in_place() {
        // 17 000 logical px exceeds the software backend's 16 384 px texture
        // limit, so the renderer refuses to cache it and the widget must draw
        // it itself, at its layout position.
        let cache = TextureCache::new();
        let wide: crate::Element<'_, ()> = shape()
            .width(17_000.0)
            .height(40.0)
            .fill(Color::from_rgb(1.0, 0.0, 0.0))
            .into();
        let content = row![space().width(100.0), cached(cache.clone(), wide)];
        let root = scrollable(content)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::default(),
            ))
            .width(300.0)
            .height(60.0);
        let mut harness = Harness::new(Size::new(300.0, 60.0), root);
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(cache.record_count(), 0, "nothing was recorded");
        assert_eq!(
            &shot.pixel(150, 20)[..3],
            &[255, 0, 0],
            "content starts at x = 100"
        );
        assert_eq!(
            &shot.pixel(50, 20)[..3],
            &[255, 255, 255],
            "nothing left of the layout origin"
        );
    }

    #[test]
    fn uncacheable_content_collapses_and_takes_clicks_where_it_is_drawn() {
        // Too large for a texture, the content is drawn in place and no
        // shader can warp it: the affine approximation stands in, and the
        // pointer follows it rather than the genie the shader would draw.
        let cache = TextureCache::new();
        let build = |progress: f32| -> crate::Element<'_, Message> {
            let wide = row![
                button("go").on_press(Message::Go),
                shape()
                    .width(17_000.0)
                    .height(40.0)
                    .fill(Color::from_rgb(1.0, 0.0, 0.0))
            ];
            let content = row![
                space().width(100.0),
                cached(cache.clone(), wide).genie(progress, GenieShape::default())
            ];
            scrollable(content)
                .direction(scrollable::Direction::Horizontal(
                    scrollable::Scrollbar::default(),
                ))
                .width(300.0)
                .height(60.0)
                .into()
        };

        let now = Instant::now();
        // Fully collapsed: nothing is drawn, and nothing is clickable.
        let mut harness = Harness::new(Size::new(300.0, 60.0), build(0.0));
        harness.frame(now);
        assert_eq!(cache.record_count(), 0, "nothing was recorded");
        let shot = harness.screenshot(1.0);
        assert_eq!(&shot.pixel(250, 20)[..3], &[255, 255, 255], "gone");
        harness.click("go");
        assert_eq!(harness.into_messages(), vec![]);

        // Half way, scaled about the content's top-left corner at x = 100:
        // the red covers half the height it did, and the button is drawn
        // at half size from that corner, so its old centre is past its
        // picture while the picture's centre is the button's own.
        let mut harness = Harness::new(Size::new(300.0, 60.0), build(0.5));
        harness.frame(now);
        let mut find = iced_test::selector::Selector::find("go");
        harness.operate(&mut find);
        let iced_core::widget::operation::Outcome::Some(Some(target)) = find.finish() else {
            panic!("no button on screen")
        };
        let bounds = target.visible_bounds().expect("the text is visible");
        let shot = harness.screenshot(1.0);
        assert_eq!(&shot.pixel(250, 10)[..3], &[255, 0, 0], "half height");
        assert_eq!(&shot.pixel(250, 45)[..3], &[255, 255, 255], "not full");
        harness.click_at(Point::new(
            bounds.x + bounds.width * 0.9,
            bounds.y + bounds.height * 0.9,
        ));
        assert_eq!(harness.into_messages(), vec![]);

        let mut harness = Harness::new(Size::new(300.0, 60.0), build(0.5));
        harness.frame(now);
        harness.click_at(Point::new(
            100.0 + (bounds.center_x() - 100.0) * 0.5,
            bounds.center_y() * 0.5,
        ));
        assert_eq!(harness.into_messages(), vec![Message::Go]);
    }

    #[test]
    fn a_cached_inside_a_scrollable_renders_without_panicking() {
        use iced::widget::text;
        let cache = TextureCache::new();
        let tall: crate::Element<'_, ()> =
            column((0..40).map(|i| text(format!("row {i}")).into())).into();
        let mut harness = Harness::new(
            Size::new(200.0, 100.0),
            column![scrollable(cached(cache, tall)).height(50.0), text("below")],
        );
        harness.frame(Instant::now());
    }
}
