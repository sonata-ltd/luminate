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
use crate::record::{Record, TextureRenderer, normalize_opacity};
use crate::texture_cache::TextureCache;
use crate::warp::Warp;

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
}

macro_rules! half {
    ($name:ident, $inner:ty, $store:ty) => {
        impl $name {
            pub(crate) fn new(inner: $inner, store: Arc<$store>, scale_factor: f32) -> Self {
                Self {
                    inner,
                    store,
                    scale_factor,
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
                self.inner.start_transformation(transformation);
            }

            fn end_transformation(&mut self) {
                self.inner.end_transformation();
            }

            fn fill_quad(&mut self, quad: renderer::Quad, background: impl Into<Background>) {
                self.inner.fill_quad(quad, background);
            }

            fn reset(&mut self, new_bounds: Rectangle) {
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
        opacity: f32,
        filter: FilterQuality,
        warp: Warp,
    ) {
        use iced_wgpu::primitive::Renderer as _;

        let Some(opacity) = normalize_opacity(opacity) else {
            return;
        };
        let Some(view) = self.store.view(cache.id()) else {
            return;
        };

        // The warp is defined on the content, which sits inside the padded
        // texture, and its corner mask rounds in real pixels.
        let frame = crate::composite::Frame::new(bounds, content, self.scale_factor());

        self.with_layer(clip, |renderer| {
            renderer.with_transformation(transform, |renderer| {
                renderer.inner.draw_primitive(
                    bounds,
                    crate::composite::CompositePrimitive::new(view, opacity, filter, warp, frame),
                );
            });
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
        opacity: f32,
        filter: FilterQuality,
        warp: Warp,
    ) {
        let Some(opacity) = normalize_opacity(opacity) else {
            return;
        };
        let Some(handle) = self.store.handle(cache.id()) else {
            return;
        };

        // No shaders here, so the genie's neck cannot be drawn. A scale
        // about the same anchor, at the same progress, keeps the motion and
        // its timing; see `Warp::affine_fallback`. The anchor is a corner
        // of the content, not of the padding around it.
        let Some(fallback) = warp.affine_transform(content) else {
            return;
        };
        let transform = transform * fallback;

        // There is no bicubic kernel in iced's raster path, so `CatmullRom`
        // degrades to the same bilinear tap as `Bilinear`. `Snap` composites
        // on the pixel grid, where nearest is exact and cheapest; the caller
        // has already snapped the transform.
        let snap = filter.snaps();
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
        opacity: f32,
        filter: FilterQuality,
        warp: Warp,
    ) {
        match self {
            Self::Primary(renderer) => {
                renderer.draw_cached(
                    cache, bounds, content, clip, transform, opacity, filter, warp,
                );
            }
            Self::Secondary(renderer) => {
                renderer.draw_cached(
                    cache, bounds, content, clip, transform, opacity, filter, warp,
                );
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
