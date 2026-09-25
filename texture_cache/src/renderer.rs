//! The renderer type apps use in place of `iced::Renderer`.
//!
//! [`Renderer`] is a thin newtype over `iced_wgpu::Renderer` and/or
//! `iced_tiny_skia::Renderer` that adds cache storage and a per-renderer
//! scale factor; with both backends enabled it is iced's own fallback
//! renderer over the two. Every renderer trait is delegated; the
//! render-to-texture operations are [`TextureRenderer`].

use std::fmt;
use std::sync::Arc;

use iced_core::Renderer as _;
use iced_core::renderer::{self, Headless};
use iced_core::{
    Background, Color, Font, Pixels, Point, Rectangle, Size, Transformation, image, text,
};
use iced_graphics::{compositor, mesh};

use crate::filter::FilterQuality;
use crate::record::{Composite, Frost, Record, TextureRenderer, normalize_opacity};
use crate::texture_cache::TextureCache;

#[cfg(feature = "tiny-skia")]
use crate::record::TinySkiaCacheStore;
#[cfg(feature = "wgpu")]
use crate::record::WgpuCacheStore;

/// Which backend a [`Renderer`] runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Backend {
    /// Hardware rendering through `iced_wgpu`.
    Wgpu,
    /// Software rendering through `iced_tiny_skia`.
    TinySkia,
}

impl fmt::Display for Backend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Backend::Wgpu => "wgpu",
            Backend::TinySkia => "tiny-skia",
        })
    }
}

/// A wgpu and/or `tiny_skia` renderer with render-to-texture support.
///
/// Every stock iced widget is generic over its renderer, so this alias slots
/// in wherever `iced::Element<'_, M, T, R>` is used with a generic `R`. Code
/// that names `iced::Renderer` concretely must be made generic. With both
/// backend features this is `iced_renderer::fallback::Renderer` over
/// [`WgpuRenderer`] and [`TinySkiaRenderer`]; with one, it is that half.
#[cfg(all(feature = "wgpu", feature = "tiny-skia"))]
pub type Renderer = iced_renderer::fallback::Renderer<WgpuRenderer, TinySkiaRenderer>;

/// A wgpu renderer with render-to-texture support.
///
/// Every stock iced widget is generic over its renderer, so this alias slots
/// in wherever `iced::Element<'_, M, T, R>` is used with a generic `R`. Code
/// that names `iced::Renderer` concretely must be made generic.
#[cfg(all(feature = "wgpu", not(feature = "tiny-skia")))]
pub type Renderer = WgpuRenderer;

/// A `tiny_skia` software renderer with render-to-texture support.
///
/// Every stock iced widget is generic over its renderer, so this alias slots
/// in wherever `iced::Element<'_, M, T, R>` is used with a generic `R`. Code
/// that names `iced::Renderer` concretely must be made generic.
#[cfg(all(not(feature = "wgpu"), feature = "tiny-skia"))]
pub type Renderer = TinySkiaRenderer;

/// The wgpu half of [`Renderer`]: `iced_wgpu::Renderer` plus cache storage.
///
/// Not constructible by user code; the compositor and `Headless::new` build
/// it. Use [`Renderer`].
#[cfg(feature = "wgpu")]
pub struct WgpuRenderer {
    inner: iced_wgpu::Renderer,
    store: Arc<WgpuCacheStore>,
    scale_factor: f32,
    /// The transformations inherited from ancestors, composed.
    transformations: Transformations,
}

/// The software half of [`Renderer`]: `iced_tiny_skia::Renderer` plus cache
/// storage.
///
/// Not constructible by user code; the compositor and `Headless::new` build
/// it. Use [`Renderer`].
#[cfg(feature = "tiny-skia")]
pub struct TinySkiaRenderer {
    inner: iced_tiny_skia::Renderer,
    store: Arc<TinySkiaCacheStore>,
    scale_factor: f32,
    /// The transformations inherited from ancestors, composed.
    transformations: Transformations,
}

/// The stack of transformations the renderer is inside, each entry composed
/// with the ones before it, as iced's own layer stack composes them.
///
/// `iced_wgpu` and `iced_tiny_skia` fold these into what they draw but never
/// hand them back, and a composite needs them: the placement a pane of glass
/// samples its source by must be where the source actually landed on screen,
/// scrolled and translated by every ancestor, not merely moved by the
/// transform passed with the composite.
#[derive(Debug, Default)]
struct Transformations(Vec<Transformation>);

impl Transformations {
    /// Every inherited transformation, composed; identity at the root.
    fn current(&self) -> Transformation {
        self.0.last().copied().unwrap_or(Transformation::IDENTITY)
    }

    fn push(&mut self, transformation: Transformation) {
        let composed = self.current() * transformation;
        self.0.push(composed);
    }

    fn pop(&mut self) {
        let _ = self.0.pop();
    }

    fn clear(&mut self) {
        self.0.clear();
    }
}

macro_rules! half {
    ($name:ident, $inner:ty, $store:ty) => {
        impl $name {
            pub(crate) fn new(inner: $inner, store: Arc<$store>, scale_factor: f32) -> Self {
                Self {
                    inner,
                    store,
                    scale_factor,
                    transformations: Transformations::default(),
                }
            }

            pub(crate) fn inner_mut(&mut self) -> &mut $inner {
                &mut self.inner
            }

            pub(crate) fn set_scale_factor(&mut self, scale_factor: f32) {
                self.scale_factor = scale_factor;
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.debug_struct(stringify!($name))
                    .field("scale_factor", &self.scale_factor)
                    .finish_non_exhaustive()
            }
        }

        impl renderer::Renderer for $name {
            fn start_layer(&mut self, bounds: Rectangle) {
                self.inner.start_layer(bounds);
            }

            fn end_layer(&mut self) {
                self.inner.end_layer();
            }

            fn start_transformation(&mut self, transformation: Transformation) {
                self.transformations.push(transformation);
                self.inner.start_transformation(transformation);
            }

            fn end_transformation(&mut self) {
                self.transformations.pop();
                self.inner.end_transformation();
            }

            fn fill_quad(&mut self, quad: renderer::Quad, background: impl Into<Background>) {
                self.inner.fill_quad(quad, background);
            }

            fn reset(&mut self, new_bounds: Rectangle) {
                self.transformations.clear();
                self.inner.reset(new_bounds);
            }

            fn allocate_image(
                &mut self,
                handle: &image::Handle,
                callback: impl FnOnce(Result<image::Allocation, image::Error>) + Send + 'static,
            ) {
                self.inner.allocate_image(handle, callback);
            }
        }

        impl text::Renderer for $name {
            type Font = <$inner as text::Renderer>::Font;
            type Paragraph = <$inner as text::Renderer>::Paragraph;
            type Editor = <$inner as text::Renderer>::Editor;

            const ICON_FONT: Font = <$inner as text::Renderer>::ICON_FONT;
            const CHECKMARK_ICON: char = <$inner as text::Renderer>::CHECKMARK_ICON;
            const ARROW_DOWN_ICON: char = <$inner as text::Renderer>::ARROW_DOWN_ICON;
            const SCROLL_UP_ICON: char = <$inner as text::Renderer>::SCROLL_UP_ICON;
            const SCROLL_DOWN_ICON: char = <$inner as text::Renderer>::SCROLL_DOWN_ICON;
            const SCROLL_LEFT_ICON: char = <$inner as text::Renderer>::SCROLL_LEFT_ICON;
            const SCROLL_RIGHT_ICON: char = <$inner as text::Renderer>::SCROLL_RIGHT_ICON;
            const ICED_LOGO: char = <$inner as text::Renderer>::ICED_LOGO;

            fn default_font(&self) -> Self::Font {
                self.inner.default_font()
            }

            fn default_size(&self) -> Pixels {
                self.inner.default_size()
            }

            fn fill_paragraph(
                &mut self,
                paragraph: &Self::Paragraph,
                position: Point,
                color: Color,
                clip_bounds: Rectangle,
            ) {
                self.inner
                    .fill_paragraph(paragraph, position, color, clip_bounds);
            }

            fn fill_editor(
                &mut self,
                editor: &Self::Editor,
                position: Point,
                color: Color,
                clip_bounds: Rectangle,
            ) {
                self.inner.fill_editor(editor, position, color, clip_bounds);
            }

            fn fill_text(
                &mut self,
                text: text::Text<String, Self::Font>,
                position: Point,
                color: Color,
                clip_bounds: Rectangle,
            ) {
                self.inner.fill_text(text, position, color, clip_bounds);
            }
        }

        impl iced_graphics::text::Renderer for $name {
            fn fill_raw(&mut self, raw: iced_graphics::text::Raw) {
                self.inner.fill_raw(raw);
            }
        }

        impl mesh::Renderer for $name {
            fn draw_mesh(&mut self, mesh: mesh::Mesh) {
                self.inner.draw_mesh(mesh);
            }

            fn draw_mesh_cache(&mut self, cache: mesh::Cache) {
                self.inner.draw_mesh_cache(cache);
            }
        }

        #[cfg(feature = "svg")]
        impl iced_core::svg::Renderer for $name {
            fn measure_svg(&self, handle: &iced_core::svg::Handle) -> Size<u32> {
                self.inner.measure_svg(handle)
            }

            fn draw_svg(&mut self, svg: iced_core::Svg, bounds: Rectangle, clip_bounds: Rectangle) {
                self.inner.draw_svg(svg, bounds, clip_bounds);
            }
        }

        #[cfg(feature = "canvas")]
        impl iced_graphics::geometry::Renderer for $name {
            type Geometry = <$inner as iced_graphics::geometry::Renderer>::Geometry;
            type Frame = <$inner as iced_graphics::geometry::Renderer>::Frame;

            fn new_frame(&self, bounds: Rectangle) -> Self::Frame {
                self.inner.new_frame(bounds)
            }

            fn draw_geometry(&mut self, geometry: Self::Geometry) {
                self.inner.draw_geometry(geometry);
            }
        }

        impl Headless for $name {
            async fn new(
                default_font: Font,
                default_text_size: Pixels,
                backend: Option<&str>,
            ) -> Option<Self> {
                Self::headless_new(default_font, default_text_size, backend).await
            }

            fn name(&self) -> String {
                Headless::name(&self.inner)
            }

            /// Also marks a frame boundary and adopts `scale_factor` as the
            /// recording scale, so caches drawn before the *next* screenshot
            /// match it (`iced_test::Simulator` draws, then screenshots, then
            /// draws again).
            fn screenshot(
                &mut self,
                size: Size<u32>,
                scale_factor: f32,
                background_color: Color,
            ) -> Vec<u8> {
                self.store.begin_frame();
                self.scale_factor = scale_factor;
                Headless::screenshot(&mut self.inner, size, scale_factor, background_color)
            }
        }
    };
}

/// `image::Renderer` is delegated separately from [`half!`]: the software
/// half always has it (this crate enables `iced_tiny_skia/image` itself and
/// composites through it), the wgpu half only under the `image` feature, as
/// `iced_wgpu` gates its own impl on it.
#[cfg(any(feature = "tiny-skia", feature = "image"))]
macro_rules! image_renderer {
    ($name:ident) => {
        impl image::Renderer for $name {
            type Handle = image::Handle;

            fn load_image(&self, handle: &Self::Handle) -> Result<image::Allocation, image::Error> {
                self.inner.load_image(handle)
            }

            fn measure_image(&self, handle: &Self::Handle) -> Option<Size<u32>> {
                self.inner.measure_image(handle)
            }

            fn draw_image(
                &mut self,
                image: image::Image<Self::Handle>,
                bounds: Rectangle,
                clip_bounds: Rectangle,
            ) {
                self.inner.draw_image(image, bounds, clip_bounds);
            }
        }
    };
}

#[cfg(feature = "wgpu")]
half!(WgpuRenderer, iced_wgpu::Renderer, WgpuCacheStore);
#[cfg(all(feature = "wgpu", feature = "image"))]
image_renderer!(WgpuRenderer);
#[cfg(feature = "tiny-skia")]
half!(
    TinySkiaRenderer,
    iced_tiny_skia::Renderer,
    TinySkiaCacheStore
);
#[cfg(feature = "tiny-skia")]
image_renderer!(TinySkiaRenderer);

#[cfg(feature = "wgpu")]
impl WgpuRenderer {
    /// Mirrors `iced_wgpu`'s headless renderer, but keeps the device so
    /// caches record and composite exactly as in a window.
    async fn headless_new(
        default_font: Font,
        default_text_size: Pixels,
        backend: Option<&str>,
    ) -> Option<Self> {
        use crate::compositor::{headless_format, instance_flags, request_gpu};

        if backend.is_some_and(|backend| backend != "wgpu") {
            return None;
        }

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::from_env().unwrap_or(wgpu::Backends::PRIMARY),
            flags: instance_flags(),
            ..wgpu::InstanceDescriptor::default()
        });

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .ok()?;

        let gpu = request_gpu(
            &adapter,
            headless_format(),
            Some(iced_graphics::Antialiasing::MSAAx4),
            iced_graphics::Shell::headless(),
            "iced_texture_cache [headless]",
        )
        .await
        .ok()?;

        let store = Arc::new(WgpuCacheStore::new(gpu, default_font, default_text_size));
        Some(Self::new(store.new_renderer(), store, 1.0))
    }
}

#[cfg(feature = "wgpu")]
impl compositor::Default for WgpuRenderer {
    type Compositor = crate::compositor::WgpuCompositor;
}

#[cfg(feature = "wgpu")]
impl TextureRenderer for WgpuRenderer {
    fn backend(&self) -> Backend {
        Backend::Wgpu
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    fn filter_quality(&self) -> FilterQuality {
        crate::filter::filter_quality().unwrap_or(self.store.gpu().filter_quality)
    }

    fn record(
        &mut self,
        cache: &TextureCache,
        size: Size<u32>,
        scale_factor: f32,
        f: impl FnOnce(&mut Self),
    ) -> Record {
        let store = Arc::clone(&self.store);

        store.record(cache, size, scale_factor, |inner, viewport| {
            let mut nested = Self::new(inner, Arc::clone(&store), scale_factor);
            nested.reset(Rectangle::with_size(viewport.logical_size()));
            f(&mut nested);
            nested.inner
        })
    }

    fn draw_cached(
        &mut self,
        cache: &TextureCache,
        bounds: Rectangle,
        content: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        composite: Composite,
    ) {
        use iced_wgpu::primitive::Renderer as _;

        let Some(opacity) = normalize_opacity(composite.opacity) else {
            return;
        };
        let Some(view) = self.store.view(cache.id()) else {
            return;
        };
        self.store.note_composited(
            cache.id(),
            bounds * (self.transformations.current() * transform),
        );

        // At rest the texture is cut to the corners grown out to its padded
        // edge. The warp is defined on the content, which sits inside that
        // padding, and its own mask rounds the content's corners in real
        // pixels.
        let mask = crate::composite::Mask::whole(
            bounds.size(),
            crate::geometry::padded_corners(composite.corners, bounds, content),
        );
        let frame =
            crate::composite::Frame::new(bounds, content, composite.corners, self.scale_factor());

        self.with_layer(clip, |renderer| {
            renderer.with_transformation(transform, |renderer| {
                renderer.inner.draw_primitive(
                    bounds,
                    crate::composite::CompositePrimitive::new(
                        view,
                        opacity,
                        composite.filter,
                        mask,
                        composite.warp,
                        frame,
                    ),
                );
            });
        });
    }

    fn draw_frosted(
        &mut self,
        source: &TextureCache,
        bounds: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        opacity: f32,
        frost: Frost,
    ) {
        use iced_wgpu::primitive::Renderer as _;

        let Some(opacity) = normalize_opacity(opacity) else {
            return;
        };
        // `frost` is handed over unresolved: the radius is in logical
        // pixels and the blur runs in the pixels of the *source* texture,
        // whose recorded scale (`scale` times that widget's supersample)
        // only the store knows. Resolving it here against this renderer's
        // own scale factor would halve the sigma of a supersampled source.
        // Both the pane and the source placement it is matched against are
        // on screen, through every inherited transformation.
        let inherited = self.transformations.current();
        let pane = bounds * (inherited * transform);
        let Some((view, uv, visible)) = self.store.frosted(source.id(), frost, pane) else {
            return;
        };

        // The corners belong to the whole pane, but only `visible` is drawn,
        // so the coverage is sampled in the pane's own coordinates — which
        // matters exactly when a pane overhangs its source.
        let shape = Rectangle {
            x: visible.x - pane.x,
            y: visible.y - pane.y,
            width: pane.width,
            height: pane.height,
        };
        // Glass does not warp: the frame is only there to fill the uniform.
        let frame = crate::composite::Frame::new(
            visible,
            visible,
            iced_core::border::Radius::default(),
            self.scale_factor(),
        );

        // `visible` is already in screen space: the store intersected the
        // transformed bounds with the source's placement, so the primitive
        // is drawn without the transform, and with the inherited ones the
        // backend is about to apply undone.
        let drawn = visible * inherited.inverse();

        self.with_layer(clip, |renderer| {
            renderer.inner.draw_primitive(
                drawn,
                crate::composite::CompositePrimitive::new(
                    view,
                    opacity,
                    FilterQuality::Bilinear,
                    crate::composite::Mask {
                        uv,
                        corners: frost.corners,
                        shape,
                        drawn: visible.size(),
                    },
                    crate::warp::Warp::None,
                    frame,
                ),
            );
        });
    }
}

#[cfg(feature = "tiny-skia")]
impl TinySkiaRenderer {
    async fn headless_new(
        default_font: Font,
        default_text_size: Pixels,
        backend: Option<&str>,
    ) -> Option<Self> {
        // Honours iced's backend-name check; the renderer itself is built
        // synchronously.
        let inner =
            <iced_tiny_skia::Renderer as Headless>::new(default_font, default_text_size, backend)
                .await?;
        let store = Arc::new(TinySkiaCacheStore::new(default_font, default_text_size));
        Some(Self::new(inner, store, 1.0))
    }

    /// A software renderer with cache storage, built without an executor.
    pub(crate) fn headless(default_font: Font, default_text_size: Pixels) -> Self {
        let store = Arc::new(TinySkiaCacheStore::new(default_font, default_text_size));
        Self::new(store.new_renderer(), store, 1.0)
    }
}

#[cfg(feature = "tiny-skia")]
impl compositor::Default for TinySkiaRenderer {
    type Compositor = crate::compositor::TinySkiaCompositor;
}

#[cfg(feature = "tiny-skia")]
impl TextureRenderer for TinySkiaRenderer {
    fn backend(&self) -> Backend {
        Backend::TinySkia
    }

    fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    // A rasterizer has no adapter to read a tier from, and `Bilinear` is what
    // this backend has always composited with; `Snap` is honoured when it is
    // asked for explicitly.
    fn filter_quality(&self) -> FilterQuality {
        crate::filter::filter_quality().unwrap_or(FilterQuality::Bilinear)
    }

    fn record(
        &mut self,
        cache: &TextureCache,
        size: Size<u32>,
        scale_factor: f32,
        f: impl FnOnce(&mut Self),
    ) -> Record {
        let store = Arc::clone(&self.store);

        store.record(cache, size, scale_factor, |inner, viewport| {
            let mut nested = Self::new(inner, Arc::clone(&store), scale_factor);
            nested.reset(Rectangle::with_size(viewport.logical_size()));
            f(&mut nested);
            nested.inner
        })
    }

    fn draw_cached(
        &mut self,
        cache: &TextureCache,
        bounds: Rectangle,
        content: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        composite: Composite,
    ) {
        let Some(opacity) = normalize_opacity(composite.opacity) else {
            return;
        };
        // Cut to shape here rather than through `image::Image::border_radius`:
        // `iced_tiny_skia` destructures that field away and never reads it.
        // The corners are grown out to the padded edge, so the arc lands on
        // the content's corner. The rounded texture is kept until the
        // corners or the recording change.
        let corners = crate::geometry::padded_corners(composite.corners, bounds, content);
        let Some(handle) = self.store.handle_rounded(cache.id(), corners) else {
            return;
        };

        // No shaders here, so the genie's neck cannot be drawn. A scale
        // about the same anchor, at the same progress, keeps the motion and
        // its timing; see `Warp::affine_fallback`. The anchor is a corner
        // of the content, not of the padding around it. The rounded corners
        // scale with it: keeping their radius needs the shader's mask.
        let Some(fallback) = composite.warp.affine_transform(content) else {
            return;
        };
        let transform = transform * fallback;
        self.store.note_composited(
            cache.id(),
            bounds * (self.transformations.current() * transform),
        );

        // There is no bicubic kernel in iced's raster path, so `CatmullRom`
        // degrades to the same bilinear tap as `Bilinear`. `Snap` composites
        // on the pixel grid, where nearest is exact and cheapest; the caller
        // has already snapped the transform.
        let snap = composite.filter.snaps();
        let image = image::Image {
            handle,
            filter_method: if snap {
                image::FilterMethod::Nearest
            } else {
                image::FilterMethod::Linear
            },
            rotation: iced_core::Radians(0.0),
            border_radius: iced_core::border::Radius::default(),
            opacity,
            snap,
        };

        self.with_layer(clip, |renderer| {
            renderer.with_transformation(transform, |renderer| {
                image::Renderer::draw_image(&mut renderer.inner, image, bounds, bounds);
            });
        });
    }

    fn draw_frosted(
        &mut self,
        source: &TextureCache,
        bounds: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        opacity: f32,
        frost: Frost,
    ) {
        let Some(opacity) = normalize_opacity(opacity) else {
            return;
        };
        // See the wgpu implementation: the store resolves `frost`, because
        // the scale that matters is the one the source was recorded at.
        // Matched on screen, through every inherited transformation, and
        // drawn back through them: see the wgpu implementation.
        let inherited = self.transformations.current();
        let Some((handle, visible)) =
            self.store
                .frosted(source.id(), frost, bounds * (inherited * transform))
        else {
            return;
        };
        let drawn = visible * inherited.inverse();

        // Always linear: the derived texture is upscaled back from
        // 1/downscale, and nearest would show its blocks.
        let image = image::Image {
            handle,
            filter_method: image::FilterMethod::Linear,
            rotation: iced_core::Radians(0.0),
            border_radius: iced_core::border::Radius::default(),
            opacity,
            snap: false,
        };

        self.with_layer(clip, |renderer| {
            // The handle covers a little more than `visible` — the crop was
            // grown outwards to whole derived texels — and is stretched
            // back into it rather than drawn at the texels' own screen
            // positions. That is not a choice: `iced_tiny_skia`'s raster
            // pipeline places a pixmap at an integer multiple of its own
            // texel size (`raster.rs`: `(bounds.x / width_scale) as i32`,
            // truncating towards zero, after any layer transformation has
            // been folded in), so the exact position is not expressible.
            // Stretching keeps some point inside the pane registered — the
            // centre only when the crop grew by the same amount on both
            // sides — and spreads the error to the edges, where it stays
            // under one derived texel; truncating to the whole texel below
            // instead would offset the whole backdrop by up to a full one,
            // which is what `the_same_sigma_survives_every_downscale`
            // measures. See `TinySkiaCacheStore::frosted` for the whole
            // account.
            image::Renderer::draw_image(&mut renderer.inner, image, drawn, drawn);
        });
    }
}

#[cfg(all(feature = "wgpu", feature = "tiny-skia"))]
impl TextureRenderer for Renderer {
    fn backend(&self) -> Backend {
        match self {
            Self::Primary(_) => Backend::Wgpu,
            Self::Secondary(_) => Backend::TinySkia,
        }
    }

    fn scale_factor(&self) -> f32 {
        match self {
            Self::Primary(renderer) => renderer.scale_factor,
            Self::Secondary(renderer) => renderer.scale_factor,
        }
    }

    fn filter_quality(&self) -> FilterQuality {
        match self {
            Self::Primary(renderer) => renderer.filter_quality(),
            Self::Secondary(renderer) => renderer.filter_quality(),
        }
    }

    fn record(
        &mut self,
        cache: &TextureCache,
        size: Size<u32>,
        scale_factor: f32,
        f: impl FnOnce(&mut Self),
    ) -> Record {
        match self {
            Self::Primary(renderer) => {
                let store = Arc::clone(&renderer.store);

                store.record(cache, size, scale_factor, |inner, viewport| {
                    let mut nested =
                        Self::Primary(WgpuRenderer::new(inner, Arc::clone(&store), scale_factor));
                    nested.reset(Rectangle::with_size(viewport.logical_size()));
                    f(&mut nested);

                    match nested {
                        Self::Primary(nested) => nested.inner,
                        // `f` sees `&mut Renderer` and the halves have no
                        // public constructor, so the variant cannot change.
                        Self::Secondary(_) => unreachable!("a nested renderer keeps its backend"),
                    }
                })
            }
            Self::Secondary(renderer) => {
                let store = Arc::clone(&renderer.store);

                store.record(cache, size, scale_factor, |inner, viewport| {
                    let mut nested = Self::Secondary(TinySkiaRenderer::new(
                        inner,
                        Arc::clone(&store),
                        scale_factor,
                    ));
                    nested.reset(Rectangle::with_size(viewport.logical_size()));
                    f(&mut nested);

                    match nested {
                        Self::Secondary(nested) => nested.inner,
                        Self::Primary(_) => unreachable!("a nested renderer keeps its backend"),
                    }
                })
            }
        }
    }

    fn draw_cached(
        &mut self,
        cache: &TextureCache,
        bounds: Rectangle,
        content: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        composite: Composite,
    ) {
        match self {
            Self::Primary(renderer) => {
                renderer.draw_cached(cache, bounds, content, clip, transform, composite);
            }
            Self::Secondary(renderer) => {
                renderer.draw_cached(cache, bounds, content, clip, transform, composite);
            }
        }
    }

    fn draw_frosted(
        &mut self,
        source: &TextureCache,
        bounds: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        opacity: f32,
        frost: Frost,
    ) {
        match self {
            Self::Primary(renderer) => {
                renderer.draw_frosted(source, bounds, clip, transform, opacity, frost);
            }
            Self::Secondary(renderer) => {
                renderer.draw_frosted(source, bounds, clip, transform, opacity, frost);
            }
        }
    }
}

/// A software [`Renderer`] with cache storage; needs neither a GPU nor an
/// executor. Backs `crate::testing::headless_tiny_skia`.
#[cfg(feature = "tiny-skia")]
pub(crate) fn headless_tiny_skia() -> Renderer {
    let half = TinySkiaRenderer::headless(Font::DEFAULT, Pixels(16.0));

    #[cfg(feature = "wgpu")]
    let renderer = Renderer::Secondary(half);
    #[cfg(not(feature = "wgpu"))]
    let renderer = half;

    renderer
}

/// Blurs run by this renderer's store. Diagnostics only; backs
/// `crate::testing::blur_count`.
pub(crate) fn blur_count(renderer: &Renderer) -> u64 {
    #[cfg(all(feature = "wgpu", feature = "tiny-skia"))]
    {
        match renderer {
            Renderer::Primary(renderer) => renderer.store.blur_count(),
            Renderer::Secondary(renderer) => renderer.store.blur_count(),
        }
    }
    #[cfg(not(all(feature = "wgpu", feature = "tiny-skia")))]
    {
        renderer.store.blur_count()
    }
}

/// Live derived blur textures. Diagnostics only.
pub(crate) fn derived_len(renderer: &Renderer) -> usize {
    #[cfg(all(feature = "wgpu", feature = "tiny-skia"))]
    {
        match renderer {
            Renderer::Primary(renderer) => renderer.store.derived_len(),
            Renderer::Secondary(renderer) => renderer.store.derived_len(),
        }
    }
    #[cfg(not(all(feature = "wgpu", feature = "tiny-skia")))]
    {
        renderer.store.derived_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headless(backend: Option<&str>) -> Option<Renderer> {
        iced_test::futures::futures::executor::block_on(<Renderer as Headless>::new(
            Font::DEFAULT,
            Pixels(16.0),
            backend,
        ))
    }

    #[cfg(feature = "tiny-skia")]
    #[test]
    fn a_requested_tiny_skia_backend_is_honoured() {
        let renderer = headless(Some("tiny-skia")).expect("tiny_skia needs no GPU");
        assert_eq!(renderer.backend(), Backend::TinySkia);
        assert_eq!(renderer.scale_factor(), 1.0);
    }

    #[cfg(feature = "tiny-skia")]
    #[test]
    fn the_testing_helper_is_a_software_renderer() {
        assert_eq!(headless_tiny_skia().backend(), Backend::TinySkia);
    }

    #[cfg(feature = "wgpu")]
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn a_headless_wgpu_renderer_records_on_wgpu() {
        let renderer = headless(Some("wgpu")).expect("an adapter is available");
        assert_eq!(renderer.backend(), Backend::Wgpu);
    }

    #[test]
    fn an_unknown_backend_yields_no_renderer() {
        assert!(headless(Some("nonsense")).is_none());
    }
}
