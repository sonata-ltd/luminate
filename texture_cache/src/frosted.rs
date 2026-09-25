//! [`Frosted`]: composite somebody else's cached texture, blurred, as the
//! backdrop of a pane of glass.

use std::marker::PhantomData;

use iced_animate::{Anim, Tier};
use iced_core::layout::{self, Layout};
use iced_core::widget::{Tree, Widget};
use iced_core::{Element, Length, Rectangle, Size, Transformation, border, mouse, renderer};

use crate::record::TextureRenderer;
use crate::texture_cache::TextureCache;

/// The generic parameters `Frosted` is generic over but owns none of.
/// Factored out so the marker field itself does not read as a "complex
/// type" to clippy.
type Generics<Message, Theme, Renderer> = fn() -> (Message, Theme, Renderer);

/// A pane of frosted glass over the texture of another widget's
/// [`TextureCache`].
///
/// `Frosted` records nothing. It reads the texture some other
/// [`Cached`](crate::Cached) wrote and composites a blurred copy of the
/// part of it that lies **under the pane's own bounds on screen** — the
/// backdrop, not the source stretched across the pane. It has no content
/// and no children: it is a leaf that draws a backdrop.
///
/// # What costs what
///
/// The blur runs once per rasterisation of the source, at a reduced
/// resolution, and is kept until the source is re-recorded. Moving the
/// pane, resizing it, clipping it and fading it cost nothing but a
/// composite — which is what makes a sidebar over a page affordable.
/// Changing the radius or the downscale is a re-blur.
///
/// # Ordering within a frame
///
/// The source has to be drawn **before** the pane, which is the natural
/// order for glass over content. Drawn the other way round, the pane shows
/// the source's previous frame and catches up on the next one. This is
/// documented behaviour, not a bug: the alternative is a two-phase draw of
/// the whole widget tree.
///
/// # Degenerate cases
///
/// A source that was never recorded, one too large to cache, and one that
/// does not lie under the pane all draw nothing, create nothing and do not
/// panic.
///
/// # Examples
///
/// ```no_run
/// use iced::widget::{stack, text};
/// use iced_texture_cache::{TextureCache, cached, frosted};
///
/// let page = TextureCache::new();
/// let _: iced_texture_cache::Element<'_, ()> = stack![
///     cached(page.clone(), text("the page behind")),
///     frosted(page).radius(12.0),
/// ]
/// .into();
/// ```
pub struct Frosted<Message, Theme = iced_core::Theme, Renderer = crate::Renderer> {
    source: TextureCache,
    radius: f32,
    opacity: Anim<f32>,
    downscale: u32,
    corners: border::Radius,
    width: Length,
    height: Length,
    // `fn() -> (...)` rather than `(...)`: `Frosted` owns none of these
    // type parameters, so the marker must not claim ownership either — that
    // would affect auto-trait inference (`Send`/`Sync`) and drop-check for
    // types that do not need it.
    marker: PhantomData<Generics<Message, Theme, Renderer>>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Frosted<Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frosted")
            .field("source", &self.source)
            .field("radius", &self.radius)
            .field("opacity", &self.opacity)
            .field("downscale", &self.downscale)
            .field("corners", &self.corners)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// A pane of frosted glass over the texture written under `source`. See
/// [`Frosted`].
#[must_use]
pub fn frosted<Message, Theme, Renderer>(
    source: TextureCache,
) -> Frosted<Message, Theme, Renderer> {
    Frosted::new(source)
}

impl<Message, Theme, Renderer> Frosted<Message, Theme, Renderer> {
    /// A pane over the texture written under `source`.
    #[must_use]
    pub fn new(source: TextureCache) -> Self {
        Self {
            source,
            radius: 0.0,
            opacity: Anim::constant(1.0),
            downscale: 0,
            corners: border::Radius::default(),
            width: Length::Fill,
            height: Length::Fill,
            marker: PhantomData,
        }
    }

    /// The blur radius: the sigma of the equivalent Gaussian, in **logical**
    /// pixels.
    ///
    /// Stated as a sigma rather than as a number of passes so that the two
    /// backends can be calibrated to it — the GPU runs a separable Gaussian
    /// over a downscaled copy, the software backend three box passes, and
    /// "radius 12" would otherwise mean two different pictures. Zero,
    /// negative and non-finite values draw nothing. Changing it is a
    /// re-blur, so animate [`opacity`](Self::opacity) instead.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        self.radius = radius;
        self
    }

    /// Group opacity of the composited pane; the cheapest thing to animate,
    /// exactly as on [`Cached::opacity`](crate::Cached::opacity). It never
    /// re-blurs.
    #[must_use]
    pub fn opacity(mut self, opacity: impl Into<Anim<f32>>) -> Self {
        self.opacity = opacity.into();
        self.opacity.mark_tier(Tier::Composite);
        self
    }

    /// The factor the blur is computed and stored at, a power of two in
    /// `1..=8`; other values are rounded to the nearest (a tie rounds up)
    /// and clamped.
    ///
    /// A blur destroys high frequencies, so there is nowhere and no reason
    /// to keep them: the default (derived from the radius) leaves three to
    /// six pixels of residual blur in the reduced space, which cuts both
    /// the passes and the memory by the square of the factor. Changing it
    /// is a re-blur.
    ///
    /// Each step halves, so the residual lands anywhere in that range
    /// rather than on a fixed value. Past a sigma of 24 pixels of the
    /// *source* texture — radius times the scale the source was recorded
    /// at, which is the device's scale factor times
    /// [`Cached::supersample`](crate::Cached::supersample), not the
    /// window's own scale — the default stops at 8, the coarsest the chain
    /// supports, and a wider radius is paid for in kernel passes instead of
    /// in resolution.
    ///
    /// Like [`Cached::supersample`](crate::Cached::supersample), `0` is not
    /// a way to ask for the automatic choice from this builder: it is
    /// clamped to `1` (no downscale) the same as any other out-of-range
    /// value. The automatic choice is what you get by never calling
    /// `downscale` at all.
    #[must_use]
    pub fn downscale(mut self, factor: u32) -> Self {
        self.downscale = factor.max(1);
        self
    }

    /// Rounds the corners of the pane. Zero (the default) is a rectangle.
    ///
    /// This is the pane's own shape, not the backdrop's. It is needed even
    /// when the source is already rounded, because a blur spreads outwards:
    /// an opaque source under a rounded pane bleeds past the rounding, and
    /// without this there is nothing to cut it back. Nothing in iced does it
    /// for you — `iced_tiny_skia` never reads `image::Image::border_radius`,
    /// and this crate's wgpu composite draws an unmasked quad.
    ///
    /// Changing it never re-blurs; it is a property of the composite.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use iced_texture_cache::{TextureCache, frosted};
    ///
    /// let page = TextureCache::new();
    /// let _: iced_texture_cache::Element<'_, ()> =
    ///     frosted(page).radius(12.0).border_radius(24.0).into();
    /// ```
    #[must_use]
    pub fn border_radius(mut self, radius: impl Into<border::Radius>) -> Self {
        self.corners = radius.into();
        self
    }

    /// The width of the pane (default [`Length::Fill`]).
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// The height of the pane (default [`Length::Fill`]).
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Frosted<Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    fn size(&self) -> Size<Length> {
        Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn draw(
        &self,
        _tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let opacity = self.opacity.get();
        if opacity.is_nan() || opacity <= 0.0 {
            return;
        }

        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };

        renderer.draw_frosted(
            &self.source,
            bounds,
            clip,
            Transformation::IDENTITY,
            opacity.min(1.0),
            crate::Frost {
                radius: self.radius,
                downscale: self.downscale,
                corners: self.corners,
            },
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Frosted<Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + TextureRenderer + 'a,
{
    fn from(frosted: Frosted<Message, Theme, Renderer>) -> Self {
        Element::new(frosted)
    }
}

// The harness is software-only (see `test_support`).
#[cfg(all(test, feature = "tiny-skia"))]
mod tests {
    use super::*;
    use crate::cached;
    use crate::test_support::Harness;
    use iced::widget::stack;
    use iced_animate::widget::shape;
    use iced_core::time::Instant;
    use iced_core::{Color, Size};

    const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);

    #[test]
    fn a_pane_over_nothing_draws_nothing_and_does_not_panic() {
        // A source handle nobody ever recorded into.
        let orphan = TextureCache::new();
        let element: crate::Element<'_, ()> = frosted(orphan).radius(6.0).into();
        let mut harness = Harness::new(Size::new(50.0, 50.0), element);
        harness.redraw(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(
            shot.pixel(25, 25),
            [255, 255, 255, 255],
            "the white background"
        );
    }

    #[test]
    fn a_pane_lays_out_at_the_size_it_was_given() {
        use iced_core::Widget as _;
        use iced_core::layout;
        use iced_core::widget::Tree;

        let renderer = crate::testing::headless_tiny_skia();
        let limits = layout::Limits::new(Size::ZERO, Size::new(100.0, 100.0));

        let mut fixed: Frosted<()> = frosted(TextureCache::new()).width(20.0).height(10.0);
        assert_eq!(
            fixed.size(),
            Size::new(Length::Fixed(20.0), Length::Fixed(10.0))
        );
        let mut tree = Tree::empty();
        assert_eq!(
            fixed.layout(&mut tree, &renderer, &limits).size(),
            Size::new(20.0, 10.0)
        );

        // No size asked for: a pane fills whatever it is given, so that
        // stacking one over a layer covers that layer.
        let mut filling: Frosted<()> = frosted(TextureCache::new());
        assert_eq!(filling.size(), Size::new(Length::Fill, Length::Fill));
        assert_eq!(
            filling.layout(&mut tree, &renderer, &limits).size(),
            Size::new(100.0, 100.0)
        );
    }

    /// The glass is a rectangle unless told otherwise, and a blur spreads
    /// outwards — so a rounded source under a square pane bleeds past its
    /// own rounding with nothing to cut it back.
    #[test]
    fn a_border_radius_cuts_the_corners_off_the_pane() {
        use iced::widget::container;

        let source = TextureCache::new();
        let square = |color| shape().width(40.0).height(40.0).fill(color);
        // Same arrangement as the blur test above, and for the same reason:
        // the pane has to be able to overhang the square before anything it
        // does is visible at all.
        let pane = |corners: f32| -> crate::Element<'_, ()> {
            stack![
                container(cached(source.clone(), square(RED)))
                    .width(60.0)
                    .height(60.0),
                frosted(source.clone())
                    .radius(6.0)
                    .border_radius(corners)
                    .width(50.0)
                    .height(50.0),
            ]
            .into()
        };

        let mut rounded = Harness::new(Size::new(60.0, 60.0), pane(25.0));
        rounded.frame(Instant::now());
        let cut = rounded.screenshot(1.0);

        let mut square_pane = Harness::new(Size::new(60.0, 60.0), pane(0.0));
        square_pane.frame(Instant::now());
        let uncut = square_pane.screenshot(1.0);

        // (41, 1) is the one place the radius can be seen: outside the
        // square (so the glass is the only thing colouring it) and outside
        // the pane's top-right arc — centre (25, 25), radius 25, and the
        // point is 28.7 away. Anywhere over the square itself, a blurred red
        // copy composited on sharp red is the same red whatever its alpha,
        // which is the trap the blur test above documents.
        //
        // The margin matters: at a radius of 20 the point sits 1.8 px
        // outside the arc and comes out at 244 rather than 255, because the
        // software mask is computed in the derived texture's pixels (half
        // resolution here) and the bilinear upscale softens its edge across
        // about two device pixels. That is the documented cost of masking a
        // downscaled buffer, not a leak.
        assert_eq!(
            cut.pixel(41, 1),
            [255, 255, 255, 255],
            "the pane's corner should be cut away"
        );
        assert_ne!(
            uncut.pixel(41, 1),
            [255, 255, 255, 255],
            "without a radius the same point is covered"
        );

        // The middle of the pane's right edge is reached by no arc.
        assert_eq!(cut.pixel(41, 20), uncut.pixel(41, 20));
    }

    #[test]
    fn a_pane_blurs_the_texture_of_its_source() {
        use iced::widget::container;

        let source = TextureCache::new();
        let square = |color| shape().width(40.0).height(40.0).fill(color);
        // The square and the pane cannot simply be stacked at matching
        // sizes: a plain `stack![cached(..), frosted(..)]` shrinks to the
        // size of its first (base) layer, so a `frosted(..).width(40.0)`
        // alongside it is silently forced back to 40x40 too (`Limits`
        // clamps a nested `Fixed` both up and down once the enclosing box
        // is itself fixed). At that size the pane can never look any
        // different from the square: every pixel it covers is *also*
        // covered by the square's own opaque red, so blending a blurred
        // (but identically red) copy over identically red content is
        // invisible however the alpha comes out.
        //
        // `container` gives the base layer a bigger footprint (60x60)
        // without changing the square drawn inside it (still 40x40, at the
        // container's default top-left alignment), which lets the pane
        // below be genuinely larger than the square and spill over its
        // edge — into the transparent bleed `Cached` records around its
        // content (`BLEED` in `cached.rs`). That is the one place the blur
        // is visible: white bleeding into the red the blur pulled past the
        // square's own edge.
        let tree: crate::Element<'_, ()> = stack![
            container(cached(source.clone(), square(RED)))
                .width(60.0)
                .height(60.0),
            frosted(source.clone()).radius(6.0).width(50.0).height(50.0),
        ]
        .into();

        let mut harness = Harness::new(Size::new(60.0, 60.0), tree);
        harness.frame(Instant::now());
        let shot = harness.screenshot(1.0);

        // Just past the square's bottom-right corner (its own edge sits at
        // 40, 40): the blur carried its red beyond the object it covers,
        // and white bleeds into that red.
        let [r, g, b, _] = shot.pixel(41, 41);
        assert!(g > 40 && b > 40, "the corner is softened, got {r},{g},{b}");
        // The centre, far from any edge, is still solid red.
        let [r, g, b, _] = shot.pixel(20, 20);
        assert!(
            r > 200 && g < 60 && b < 60,
            "the centre stays red, got {r},{g},{b}"
        );
    }
}
