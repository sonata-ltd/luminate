//! Render-to-texture: the [`TextureRenderer`] trait and the per-backend
//! cache stores behind it.
//!
//! On wgpu a cache is a GPU texture recorded by a pooled nested
//! `iced_wgpu::Renderer` and composited as a textured quad (see
//! `composite.rs`); on `tiny_skia` it is a pixmap recorded by a pooled nested
//! `iced_tiny_skia::Renderer` and composited through iced's image pipeline.
//! Both are implemented entirely with iced's public API; iced itself is
//! unmodified.
//!
//! A store is owned by the compositor (or by a headless renderer) and shared
//! by `Arc` with every renderer it creates. A record pops a nested renderer
//! from the store's pool, rasterizes the closure into the cache's texture and
//! pushes the renderer back; a nested record (a `Cached` inside a `Cached`)
//! pops a second one, so the pool holds one renderer per nesting depth and
//! nothing per cache except the texture itself.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError, Weak};

use iced_core::{Rectangle, Size, Transformation};

use crate::filter::FilterQuality;
use crate::renderer::Backend;
use crate::texture_cache::{Inner, TextureCache, TextureCacheId as Id};
use crate::warp::Warp;

/// Outcome of [`TextureRenderer::record`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use = "Uncacheable content must be drawn in place"]
pub enum Record {
    /// The closure ran and the texture now holds its output.
    Fresh,
    /// The existing texture is still valid; the closure did not run.
    Reused,
    /// The request exceeds the backend's limits (see
    /// [`TextureRenderer::record`]): nothing was recorded and the closure
    /// did not run. The caller draws its content in place.
    Uncacheable,
}

/// Render-to-texture operations of a renderer.
///
/// Implemented by [`Renderer`](crate::Renderer). The trait is public so
/// widget code can be generic over it. A third-party renderer can implement
/// it too, each method documents what it must guarantee, and
/// [`TextureCache::take_invalidated`] / [`TextureCache::note_record`] are
/// public so it can keep the same invalidation semantics, but it must
/// report one of this crate's [`Backend`] variants: `Backend` names only
/// wgpu and `tiny_skia` today and is `#[non_exhaustive]` so a variant for
/// other backends can be added later.
pub trait TextureRenderer: iced_core::Renderer {
    /// The active backend.
    #[must_use]
    fn backend(&self) -> Backend;

    /// The scale factor this renderer records at.
    ///
    /// In a window it is the window's scale factor as of the last presented
    /// frame (1.0 before that): the first frame after creation or after a
    /// DPI change records at the previous scale and is re-recorded on the
    /// next frame. Headless, it is the `scale_factor` of the last
    /// `Headless::screenshot` call (1.0 before that). Inside a record it is
    /// the texture's own scale.
    #[must_use]
    fn scale_factor(&self) -> f32;

    /// The reconstruction tier in force on this renderer: the app-wide
    /// [`set_filter_quality`](crate::set_filter_quality) override if one is
    /// set, otherwise the tier chosen for the backend (the graphics adapter
    /// on wgpu, [`FilterQuality::Bilinear`] on the software backend).
    ///
    /// A widget that sets its own tier passes that to
    /// [`draw_cached`](Self::draw_cached) instead of this one; it still reads
    /// this value to keep the snap decision and the composite in agreement.
    #[must_use]
    fn filter_quality(&self) -> FilterQuality;

    /// Rasterizes `f` into the texture of `cache` at `size` physical pixels
    /// if the cache is new, invalidated, or its size or scale changed;
    /// otherwise the existing texture is kept and `f` does not run.
    ///
    /// Inside `f`, coordinates are logical pixels (`size / scale_factor`)
    /// with `(0, 0)` at the texture's top-left. Zero dimensions are clamped
    /// to one pixel. This never draws into `self`: when `size` exceeds the
    /// backend's limits (the device's `max_texture_dimension_2d` on wgpu;
    /// 16 384 px per side and 256 MiB on `tiny_skia`) the result is
    /// [`Record::Uncacheable`], `f` does not run, any stale texture is
    /// dropped and a warning is logged once per cache. Never panics.
    ///
    /// Consumes the cache's invalidation flag in every case.
    fn record(
        &mut self,
        cache: &TextureCache,
        size: Size<u32>,
        scale_factor: f32,
        f: impl FnOnce(&mut Self),
    ) -> Record;

    /// Composites the texture of `cache` into `bounds` under `transform`,
    /// clipped to `clip`.
    ///
    /// `bounds` is in the transformed space; `clip` is in the current
    /// (untransformed) space and is applied first, as a clip layer, because
    /// iced's clip layers do not intersect with their parent: a composite
    /// that overhangs an enclosing clip (a `scrollable`, say) would
    /// otherwise escape it. `opacity` is clamped to `0.0..=1.0`; `NaN` or
    /// `<= 0` draws nothing. No-op if the cache was never recorded or is
    /// uncacheable.
    ///
    /// `filter` selects the reconstruction kernel. It only affects the
    /// composite, never the recorded texture, so switching it does not
    /// invalidate a cache. [`FilterQuality::Snap`] expects `transform` to
    /// have been snapped by the caller (`Cached` and `Pager` do); the
    /// backends do not re-snap it.
    fn draw_cached(
        &mut self,
        cache: &TextureCache,
        bounds: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        opacity: f32,
        filter: FilterQuality,
        warp: Warp,
    );
}

/// Software textures are capped per side and in bytes: rasterizing more
/// than this per record is a mistake, and `tiny_skia::Pixmap::new` aborts
/// the process on allocation failure rather than returning `None`.
#[cfg(any(feature = "tiny-skia", test))]
pub(crate) const CPU_MAX_DIMENSION: u32 = 16_384;

/// 256 MiB of RGBA8. The cap counts the scratch pixmap only; the
/// straight-alpha copy behind the `image::Handle` and the clip mask roughly
/// double the real footprint of a cache at the limit.
#[cfg(any(feature = "tiny-skia", test))]
pub(crate) const CPU_MAX_BYTES: u64 = 256 * 1024 * 1024;

/// Whether both sides of `size` are within `max_dimension`.
pub(crate) fn fits(size: Size<u32>, max_dimension: u32) -> bool {
    size.width <= max_dimension && size.height <= max_dimension
}

/// Whether a software texture of `size` is allowed (see [`CPU_MAX_DIMENSION`]
/// and [`CPU_MAX_BYTES`]).
#[cfg(any(feature = "tiny-skia", test))]
pub(crate) fn fits_cpu(size: Size<u32>) -> bool {
    fits(size, CPU_MAX_DIMENSION)
        && u64::from(size.width) * u64::from(size.height) * 4 <= CPU_MAX_BYTES
}

/// Zero dimensions are clamped to one pixel.
pub(crate) fn clamp_size(size: Size<u32>) -> Size<u32> {
    Size::new(size.width.max(1), size.height.max(1))
}

/// Pure decision: must the texture be (re)recorded?
pub(crate) fn needs_record(
    existing: Option<(Size<u32>, f32)>,
    size: Size<u32>,
    scale_factor: f32,
    invalidated: bool,
) -> bool {
    match existing {
        None => true,
        Some((existing_size, existing_scale)) => {
            invalidated || existing_size != size || existing_scale != scale_factor
        }
    }
}

/// Clamps a group opacity to `0.0..=1.0`; `None` (draw nothing) for `NaN`
/// or non-positive values.
pub(crate) fn normalize_opacity(opacity: f32) -> Option<f32> {
    (opacity > 0.0).then_some(opacity.min(1.0))
}

/// What a store knows about one cache.
enum Entry<T> {
    Recorded(T),
    /// The last request exceeded the backend's limits and the warning has
    /// been logged. Replaced when a size fits again.
    Uncacheable {
        liveness: Weak<Inner>,
    },
}

/// A backend's recorded texture.
trait Texture {
    fn liveness(&self) -> &Weak<Inner>;
    /// Size and scale of the last record.
    fn recorded(&self) -> (Size<u32>, f32);
}

impl<T: Texture> Entry<T> {
    fn liveness(&self) -> &Weak<Inner> {
        match self {
            Entry::Recorded(texture) => texture.liveness(),
            Entry::Uncacheable { liveness } => liveness,
        }
    }
}

/// The backend-independent half of a store: entries by cache id.
struct Entries<T>(Mutex<HashMap<Id, Entry<T>>>);

impl<T: Texture> Entries<T> {
    fn new() -> Self {
        Self(Mutex::new(HashMap::new()))
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<Id, Entry<T>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Drops the entries of caches whose last [`TextureCache`] handle died.
    fn trim(&self) {
        self.lock()
            .retain(|_, entry| entry.liveness().strong_count() > 0);
    }

    /// Size and scale of the last record of `id`; `None` if it was never
    /// recorded or is uncacheable.
    fn recorded(map: &HashMap<Id, Entry<T>>, id: Id) -> Option<(Size<u32>, f32)> {
        match map.get(&id)? {
            Entry::Recorded(texture) => Some(texture.recorded()),
            Entry::Uncacheable { .. } => None,
        }
    }

    /// Records that `cache` cannot be cached at `size` (any stale texture is
    /// dropped) and logs once per cache.
    fn mark_uncacheable(&self, cache: &TextureCache, size: Size<u32>, limit: fmt::Arguments<'_>) {
        let mut map = self.lock();

        if !matches!(map.get(&cache.id()), Some(Entry::Uncacheable { .. })) {
            log::warn!(
                "{} at {}x{} px exceeds the {limit}; its content is drawn in place",
                cache.id(),
                size.width,
                size.height
            );
        }

        let _ = map.insert(
            cache.id(),
            Entry::Uncacheable {
                liveness: cache.liveness(),
            },
        );
    }

    #[cfg(all(test, feature = "tiny-skia"))]
    fn len(&self) -> usize {
        self.lock().len()
    }
}

/// Nested renderers, one per nesting depth, reused across records.
struct Pool<R>(Mutex<Vec<R>>);

impl<R> Pool<R> {
    fn new() -> Self {
        Self(Mutex::new(Vec::new()))
    }

    /// Pops a renderer, creating one only when the pool is empty (once per
    /// nesting level).
    fn take(&self, create: impl FnOnce() -> R) -> R {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop()
            .unwrap_or_else(create)
    }

    fn return_renderer(&self, renderer: R) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(renderer);
    }

    #[cfg(all(test, feature = "tiny-skia"))]
    fn len(&self) -> usize {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).len()
    }
}

#[cfg(feature = "wgpu")]
mod gpu {
    use std::sync::{Arc, Weak};

    use iced_core::{Color, Font, Pixels, Size};
    use iced_graphics::Viewport;

    use super::{Entries, Entry, Pool, Record, Texture, clamp_size, fits, needs_record};
    use crate::filter::FilterQuality;
    use crate::texture_cache::{Inner, TextureCache, TextureCacheId as Id};

    /// GPU objects shared by the compositor and every renderer it creates.
    pub(crate) struct GpuContext {
        pub engine: iced_wgpu::Engine,
        pub device: wgpu::Device,
        pub format: wgpu::TextureFormat,
        /// `max_texture_dimension_2d` of `device`; bounds cache sizes.
        pub max_texture_dimension: u32,
        /// The tier this adapter gets when the app sets no override.
        pub filter_quality: FilterQuality,
    }

    /// A cache recorded on wgpu.
    pub(super) struct WgpuTexture {
        /// The view keeps its texture alive; the composite pipeline keys its
        /// bindings by this `Arc`'s identity.
        view: Arc<wgpu::TextureView>,
        size: Size<u32>,
        scale_factor: f32,
        liveness: Weak<Inner>,
    }

    impl Texture for WgpuTexture {
        fn liveness(&self) -> &Weak<Inner> {
            &self.liveness
        }

        fn recorded(&self) -> (Size<u32>, f32) {
            (self.size, self.scale_factor)
        }
    }

    /// Cache storage of the wgpu backend: the device, one texture per live
    /// cache and a pool of nested renderers.
    pub(crate) struct WgpuCacheStore {
        gpu: GpuContext,
        default_font: Font,
        default_text_size: Pixels,
        pub(super) entries: Entries<WgpuTexture>,
        pub(super) pool: Pool<iced_wgpu::Renderer>,
    }

    impl WgpuCacheStore {
        pub(crate) fn new(gpu: GpuContext, default_font: Font, default_text_size: Pixels) -> Self {
            Self {
                gpu,
                default_font,
                default_text_size,
                entries: Entries::new(),
                pool: Pool::new(),
            }
        }

        pub(crate) fn gpu(&self) -> &GpuContext {
            &self.gpu
        }

        /// Marks a frame boundary: drops the state of caches whose last
        /// handle died. Called once per `present`/`screenshot`.
        pub(crate) fn begin_frame(&self) {
            self.entries.trim();
        }

        /// A renderer on this store's engine (shares the glyph atlas and
        /// pipelines; owns its own staging belt).
        pub(crate) fn new_renderer(&self) -> iced_wgpu::Renderer {
            iced_wgpu::Renderer::new(
                self.gpu.engine.clone(),
                self.default_font,
                self.default_text_size,
            )
        }

        fn create_view(&self, size: Size<u32>) -> Arc<wgpu::TextureView> {
            let texture = self.gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("iced_texture_cache cache"),
                size: wgpu::Extent3d {
                    width: size.width,
                    height: size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.gpu.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });

            Arc::new(texture.create_view(&wgpu::TextureViewDescriptor::default()))
        }

        /// Records into the texture of `cache` (see
        /// [`TextureRenderer::record`](super::TextureRenderer::record)).
        /// `run` receives a nested renderer and the texture's viewport, draws
        /// with it and hands it back.
        pub(crate) fn record(
            &self,
            cache: &TextureCache,
            size: Size<u32>,
            scale_factor: f32,
            run: impl FnOnce(iced_wgpu::Renderer, &Viewport) -> iced_wgpu::Renderer,
        ) -> Record {
            let size = clamp_size(size);
            let invalidated = cache.take_invalidated();

            if !fits(size, self.gpu.max_texture_dimension) {
                self.entries.mark_uncacheable(
                    cache,
                    size,
                    format_args!("{} px device texture limit", self.gpu.max_texture_dimension),
                );
                return Record::Uncacheable;
            }

            // The texture is chosen (or created) before the closure runs but
            // registered only after `present`, like the CPU path: a panicking
            // closure must not leave a blank texture recorded as valid.
            let view = {
                let entries = self.entries.lock();
                let existing = Entries::recorded(&entries, cache.id());

                if !needs_record(existing, size, scale_factor, invalidated) {
                    return Record::Reused;
                }

                match entries.get(&cache.id()) {
                    Some(Entry::Recorded(texture))
                        if texture.size == size && texture.scale_factor == scale_factor =>
                    {
                        texture.view.clone()
                    }
                    _ => self.create_view(size),
                }
            };

            let viewport = Viewport::with_physical_size(size, scale_factor);
            let mut renderer = run(self.pool.take(|| self.new_renderer()), &viewport);
            // `iced_wgpu::Renderer::present` submits and then trims the
            // shared engine (its API offers no way to skip the trim), so a
            // nested record trims mid-frame. That only evicts atlas/cache
            // entries unused since the last trim, which costs re-uploads
            // solely for apps that re-record every frame.
            let _ = renderer.present(Some(Color::TRANSPARENT), self.gpu.format, &view, &viewport);
            self.pool.return_renderer(renderer);

            let mut entries = self.entries.lock();
            let _ = entries.insert(
                cache.id(),
                Entry::Recorded(WgpuTexture {
                    view,
                    size,
                    scale_factor,
                    liveness: cache.liveness(),
                }),
            );
            drop(entries);
            cache.note_record();

            Record::Fresh
        }

        /// The texture of `id`, if recorded.
        pub(crate) fn view(&self, id: Id) -> Option<Arc<wgpu::TextureView>> {
            match self.entries.lock().get(&id)? {
                Entry::Recorded(texture) => Some(texture.view.clone()),
                Entry::Uncacheable { .. } => None,
            }
        }
    }
}

#[cfg(feature = "wgpu")]
pub(crate) use gpu::{GpuContext, WgpuCacheStore};

#[cfg(feature = "tiny-skia")]
mod cpu {
    use std::sync::Weak;

    use iced_core::{Color, Font, Pixels, Rectangle, Size, image};
    use iced_graphics::Viewport;

    use super::{
        CPU_MAX_BYTES, CPU_MAX_DIMENSION, Entries, Entry, Pool, Record, Texture, clamp_size,
        fits_cpu, needs_record,
    };
    use crate::texture_cache::{Inner, TextureCache, TextureCacheId as Id};

    /// A cache recorded on `tiny_skia`.
    pub(super) struct TinySkiaTexture {
        /// Straight-alpha RGBA of the last record; a new handle per record so
        /// iced's raster cache reloads it.
        handle: image::Handle,
        /// Scratch pixmap and clip mask, reused while the size is unchanged.
        /// `None` while a record is in progress.
        scratch: Option<(tiny_skia::Pixmap, tiny_skia::Mask)>,
        size: Size<u32>,
        scale_factor: f32,
        liveness: Weak<Inner>,
    }

    impl Texture for TinySkiaTexture {
        fn liveness(&self) -> &Weak<Inner> {
            &self.liveness
        }

        fn recorded(&self) -> (Size<u32>, f32) {
            (self.size, self.scale_factor)
        }
    }

    /// Cache storage of the software backend: one pixmap per live cache and
    /// a pool of nested renderers.
    pub(crate) struct TinySkiaCacheStore {
        default_font: Font,
        default_text_size: Pixels,
        pub(super) entries: Entries<TinySkiaTexture>,
        pub(super) pool: Pool<iced_tiny_skia::Renderer>,
    }

    impl TinySkiaCacheStore {
        pub(crate) fn new(default_font: Font, default_text_size: Pixels) -> Self {
            Self {
                default_font,
                default_text_size,
                entries: Entries::new(),
                pool: Pool::new(),
            }
        }

        /// Marks a frame boundary: drops the state of caches whose last
        /// handle died. Called once per `present`/`screenshot`.
        pub(crate) fn begin_frame(&self) {
            self.entries.trim();
        }

        pub(crate) fn new_renderer(&self) -> iced_tiny_skia::Renderer {
            iced_tiny_skia::Renderer::new(self.default_font, self.default_text_size)
        }

        /// Records into the pixmap of `cache` (see
        /// [`TextureRenderer::record`](super::TextureRenderer::record)).
        /// `run` receives a nested renderer and the texture's viewport, draws
        /// with it and hands it back.
        pub(crate) fn record(
            &self,
            cache: &TextureCache,
            size: Size<u32>,
            scale_factor: f32,
            run: impl FnOnce(iced_tiny_skia::Renderer, &Viewport) -> iced_tiny_skia::Renderer,
        ) -> Record {
            let size = clamp_size(size);
            let invalidated = cache.take_invalidated();

            if !fits_cpu(size) {
                self.entries.mark_uncacheable(
                    cache,
                    size,
                    format_args!(
                        "software texture limit ({CPU_MAX_DIMENSION} px per side, {} MiB)",
                        CPU_MAX_BYTES >> 20
                    ),
                );
                return Record::Uncacheable;
            }

            let scratch = {
                let mut entries = self.entries.lock();
                let existing = Entries::recorded(&entries, cache.id());

                if !needs_record(existing, size, scale_factor, invalidated) {
                    return Record::Reused;
                }

                match entries.get_mut(&cache.id()) {
                    Some(Entry::Recorded(texture)) if texture.size == size => {
                        texture.scratch.take()
                    }
                    _ => None,
                }
            };

            let (mut pixmap, mut mask) = scratch.unwrap_or_else(|| {
                // `fits_cpu` bounds both sides and the byte length, which is
                // all `Pixmap::new`/`Mask::new` check before allocating.
                let pixmap = tiny_skia::Pixmap::new(size.width, size.height)
                    .expect("size fits the software texture limit");
                let mask = tiny_skia::Mask::new(size.width, size.height)
                    .expect("size fits the software texture limit");
                (pixmap, mask)
            });

            cache.note_record();

            let viewport = Viewport::with_physical_size(size, scale_factor);
            let mut renderer = run(self.pool.take(|| self.new_renderer()), &viewport);
            // A record always re-rasterizes the whole texture, so the damage
            // is the full viewport; `mask` is the clip-mask scratch.
            renderer.draw(
                &mut pixmap.as_mut(),
                &mut mask,
                &viewport,
                &[Rectangle::with_size(viewport.logical_size())],
                Color::TRANSPARENT,
            );
            self.pool.return_renderer(renderer);

            let handle =
                image::Handle::from_rgba(size.width, size.height, pixmap_to_rgba(pixmap.data()));

            let mut entries = self.entries.lock();
            let entry = entries
                .entry(cache.id())
                .or_insert_with(|| Entry::Uncacheable {
                    liveness: cache.liveness(),
                });

            match entry {
                Entry::Recorded(texture) => {
                    texture.handle = handle;
                    texture.scratch = Some((pixmap, mask));
                    texture.size = size;
                    texture.scale_factor = scale_factor;
                }
                Entry::Uncacheable { .. } => {
                    *entry = Entry::Recorded(TinySkiaTexture {
                        handle,
                        scratch: Some((pixmap, mask)),
                        size,
                        scale_factor,
                        liveness: cache.liveness(),
                    });
                }
            }

            Record::Fresh
        }

        /// The image of `id`, if recorded.
        pub(crate) fn handle(&self, id: Id) -> Option<image::Handle> {
            match self.entries.lock().get(&id)? {
                Entry::Recorded(texture) => Some(texture.handle.clone()),
                Entry::Uncacheable { .. } => None,
            }
        }
    }

    /// Converts an `iced_tiny_skia` pixmap into the straight-alpha RGBA that
    /// `image::Handle::from_rgba` expects.
    ///
    /// `iced_tiny_skia` renders with red and blue swapped (`into_color`
    /// feeds `b, g, r` to `tiny_skia` so its buffers match softbuffer's `0RGB`
    /// layout), so the pixmap bytes are premultiplied **BGRA**; iced
    /// premultiplies again when it uploads a handle, hence the demultiply
    /// here. Fully transparent and fully opaque pixels (the overwhelming
    /// majority of UI pixels) take the fast paths; the round trip quantizes
    /// low-alpha pixels slightly, which is inherent to the straight-alpha
    /// handle API.
    pub(crate) fn pixmap_to_rgba(premultiplied_bgra: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; premultiplied_bgra.len()];
        let (out_chunks, out_rem) = out.as_chunks_mut::<4>();
        debug_assert!(out_rem.is_empty());

        for (src, dst) in premultiplied_bgra.as_chunks::<4>().0.iter().zip(out_chunks) {
            let (b, g, r, a) = (src[0], src[1], src[2], src[3]);

            match a {
                0 => {}
                255 => dst.copy_from_slice(&[r, g, b, 255]),
                _ => {
                    let alpha = u32::from(a);
                    let demultiply =
                        |c: u8| ((u32::from(c) * 255 + alpha / 2) / alpha).min(255) as u8;
                    dst.copy_from_slice(&[demultiply(r), demultiply(g), demultiply(b), a]);
                }
            }
        }

        out
    }
}

#[cfg(feature = "tiny-skia")]
pub(crate) use cpu::TinySkiaCacheStore;
#[cfg(all(test, feature = "tiny-skia"))]
use cpu::pixmap_to_rgba;

#[cfg(test)]
mod tests {
    use super::*;
    use iced_core::Size;

    const SIZE: Size<u32> = Size {
        width: 10,
        height: 20,
    };

    #[test]
    fn new_entry_always_records() {
        assert!(needs_record(None, SIZE, 1.0, false));
    }

    #[test]
    fn unchanged_and_valid_skips() {
        assert!(!needs_record(Some((SIZE, 1.0)), SIZE, 1.0, false));
    }

    #[test]
    fn invalidated_records() {
        assert!(needs_record(Some((SIZE, 1.0)), SIZE, 1.0, true));
    }

    #[test]
    fn size_or_scale_change_records() {
        assert!(needs_record(
            Some((SIZE, 1.0)),
            Size::new(11, 20),
            1.0,
            false
        ));
        assert!(needs_record(Some((SIZE, 1.0)), SIZE, 2.0, false));
    }

    #[test]
    fn oversized_requests_do_not_fit() {
        assert!(!fits(Size::new(10_000, 20), 8192));
        assert!(fits(Size::new(8192, 8192), 8192));
        assert!(!fits(Size::new(1, 8193), 8192));
    }

    #[test]
    fn software_sizes_are_capped_per_side_and_in_bytes() {
        assert!(fits_cpu(Size::new(CPU_MAX_DIMENSION, 1)));
        assert!(!fits_cpu(Size::new(CPU_MAX_DIMENSION + 1, 1)));
        // 8192 x 8192 x 4 = 256 MiB exactly: allowed.
        assert!(fits_cpu(Size::new(8192, 8192)));
        // 16384 x 16384 x 4 = 1 GiB: over the byte cap although each side fits.
        assert!(!fits_cpu(Size::new(CPU_MAX_DIMENSION, CPU_MAX_DIMENSION)));
    }

    #[test]
    fn zero_sizes_are_clamped_to_one_pixel() {
        assert_eq!(clamp_size(Size::new(0, 0)), Size::new(1, 1));
        assert_eq!(clamp_size(Size::new(0, 5)), Size::new(1, 5));
        assert_eq!(clamp_size(SIZE), SIZE);
    }

    #[test]
    fn opacity_is_normalised_once() {
        assert_eq!(normalize_opacity(0.5), Some(0.5));
        assert_eq!(normalize_opacity(3.0), Some(1.0));
        assert_eq!(normalize_opacity(0.0), None);
        assert_eq!(normalize_opacity(-1.0), None);
        assert_eq!(normalize_opacity(f32::NAN), None);
    }
}

#[cfg(all(test, feature = "tiny-skia"))]
mod store_tests {
    use super::*;
    use iced_core::Renderer as _;
    use iced_core::{Font, Pixels};

    const SIZE: Size<u32> = Size {
        width: 4,
        height: 4,
    };

    fn store() -> TinySkiaCacheStore {
        TinySkiaCacheStore::new(Font::DEFAULT, Pixels(16.0))
    }

    /// A record that draws nothing.
    fn empty(
        mut renderer: iced_tiny_skia::Renderer,
        viewport: &iced_graphics::Viewport,
    ) -> iced_tiny_skia::Renderer {
        renderer.reset(Rectangle::with_size(viewport.logical_size()));
        renderer
    }

    #[test]
    fn entries_are_dropped_after_the_last_handle_dies() {
        let store = store();
        let cache = TextureCache::new();
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        assert_eq!(store.entries.len(), 1);

        store.begin_frame();
        assert_eq!(store.entries.len(), 1, "a live handle keeps its entry");

        drop(cache);
        store.begin_frame();
        assert_eq!(store.entries.len(), 0);
    }

    #[test]
    fn nested_renderers_are_pooled_per_nesting_depth() {
        let store = store();
        let (a, b, inner) = (
            TextureCache::new(),
            TextureCache::new(),
            TextureCache::new(),
        );

        assert_eq!(store.record(&a, SIZE, 1.0, empty), Record::Fresh);
        assert_eq!(store.record(&b, SIZE, 1.0, empty), Record::Fresh);
        assert_eq!(store.pool.len(), 1, "sequential records share one renderer");

        let outer = store.record(&a, SIZE, 2.0, |renderer, viewport| {
            assert_eq!(store.record(&inner, SIZE, 1.0, empty), Record::Fresh);
            empty(renderer, viewport)
        });
        assert_eq!(outer, Record::Fresh);
        assert_eq!(store.pool.len(), 2, "one more renderer per nesting level");
    }

    #[test]
    fn an_uncacheable_cache_has_no_handle_and_recovers() {
        let store = store();
        let cache = TextureCache::new();

        let oversize = Size::new(CPU_MAX_DIMENSION + 1, 1);
        assert_eq!(
            store.record(&cache, oversize, 1.0, empty),
            Record::Uncacheable
        );
        assert!(store.handle(cache.id()).is_none());
        assert_eq!(cache.record_count(), 0);
        assert!(!cache.is_invalidated(), "the flag is consumed either way");

        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        assert!(store.handle(cache.id()).is_some());
    }

    #[test]
    fn a_valid_texture_is_reused_until_invalidated_or_resized() {
        let store = store();
        let cache = TextureCache::new();

        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Reused);
        cache.invalidate();
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        assert_eq!(
            store.record(&cache, Size::new(5, 4), 1.0, empty),
            Record::Fresh
        );
        assert_eq!(
            store.record(&cache, Size::new(5, 4), 2.0, empty),
            Record::Fresh
        );
        assert_eq!(cache.record_count(), 4);
    }

    #[test]
    fn pixmap_bytes_are_swizzled_and_demultiplied() {
        // Premultiplied BGRA: 50 %-alpha pure red, transparent, opaque (r=30,g=20,b=10).
        let out = pixmap_to_rgba(&[0, 0, 128, 128, 0, 0, 0, 0, 10, 20, 30, 255]);
        assert_eq!(&out[..4], &[255, 0, 0, 128]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
        assert_eq!(&out[8..], &[30, 20, 10, 255]);
    }
}
