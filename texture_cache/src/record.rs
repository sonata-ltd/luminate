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

use iced_core::{Rectangle, Size, Transformation, border};

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

/// How a pane of frosted glass is blurred.
///
/// Grouped rather than passed as loose scalars because they answer one
/// question — *what blur* — while the rest of
/// [`draw_frosted`](TextureRenderer::draw_frosted)'s arguments answer
/// *where* and *how opaque*. That keeps the argument count down and gives
/// the group a name; it buys no room to grow, since callers build this
/// with a struct literal and a new field is a breaking change wherever it
/// is added.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frost {
    /// The sigma of the equivalent Gaussian, in **logical** pixels. Both
    /// backends calibrate their own kernel to it, so the same radius looks
    /// the same on wgpu and on `tiny_skia`. Zero, negative and non-finite
    /// values draw nothing.
    pub radius: f32,
    /// The power of two in `1..=8` the blur is computed and stored at.
    /// `0` derives one from the radius; anything else is rounded to the
    /// nearest power of two and clamped.
    pub downscale: u32,
    /// Corners of the pane itself, in **logical** pixels. Zero is a
    /// rectangle.
    ///
    /// Named apart from `radius` above because the two are different
    /// quantities: that one is how far the blur reaches, this one is the
    /// shape it is cut to. It shapes the glass, not the backdrop behind it —
    /// a blur spreads outwards, so an opaque source under a rounded pane
    /// would otherwise bleed past the rounding with nothing to clip it back.
    pub corners: border::Radius,
}

/// How a cached texture is painted into its place.
///
/// Grouped for the same reason as [`Frost`]: these answer *how* the texture
/// is drawn, while the rest of [`draw_cached`](TextureRenderer::draw_cached)
/// answers *where*. Without the grouping the method would carry eight loose
/// arguments, which is both harder to read and a thing the caller can get
/// out of order silently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Composite {
    /// Group opacity, clamped to `0.0..=1.0` by the implementation; `NaN`
    /// or `<= 0` draws nothing.
    pub opacity: f32,
    /// The reconstruction kernel for a sub-pixel composite. It only affects
    /// the composite, never the recorded texture, so switching it does not
    /// invalidate a cache. [`FilterQuality::Snap`] expects the transform to
    /// have been snapped by the caller (`Cached` and `Pager` do); the
    /// backends do not re-snap it.
    pub filter: FilterQuality,
    /// Corners of the **content** rectangle, in **logical** pixels. Zero is
    /// a rectangle. The implementation grows them by the padding around the
    /// content, so the arc lands on the content's corner rather than in the
    /// transparent margin, and scales them down together when two on one
    /// side would overlap.
    ///
    /// Nothing in iced rounds this for us: `iced_tiny_skia` never reads
    /// `image::Image::border_radius`, and this crate's own wgpu composite
    /// draws an unmasked quad. Changing it never re-records a texture — it
    /// is a property of the composite, not of the recording.
    ///
    /// A warp keeps the same corners at the same radius, in screen pixels,
    /// however far it squeezes the content (wgpu only: the software
    /// backend's affine stand-in scales the rounded texture with it).
    pub corners: border::Radius,
    /// The non-affine warp applied to the texture; [`Warp::None`] draws it
    /// as recorded. See [`Cached::genie`](crate::Cached::genie).
    pub warp: Warp,
}

impl Composite {
    /// Opaque, square and unwarped: the texture painted exactly as it was
    /// recorded, through `filter`.
    #[must_use]
    pub const fn plain(filter: FilterQuality) -> Self {
        Self {
            opacity: 1.0,
            filter,
            corners: border::Radius {
                top_left: 0.0,
                top_right: 0.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            },
            warp: Warp::None,
        }
    }
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
///
/// Before 1.0 this trait is extended in minor versions when a new widget
/// needs a new render-to-texture operation — [`draw_frosted`](Self::draw_frosted)
/// arrived this way. That is a breaking change for a third-party
/// implementor (a new required method), even though it is additive for
/// every caller that only uses the trait generically.
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
    /// otherwise escape it. No-op if the cache was never recorded or is
    /// uncacheable.
    ///
    /// `content` is the content's own rectangle, in the same space as
    /// `bounds`, where `bounds` may carry padding recorded around it. The
    /// [`Composite`]'s corners round it and its warp collapses into one of
    /// its corners, not the texture's. Without padding, pass `bounds`.
    ///
    /// `composite` says how the texture is painted; see [`Composite`] for
    /// what each field does and what it costs.
    fn draw_cached(
        &mut self,
        cache: &TextureCache,
        bounds: Rectangle,
        content: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        composite: Composite,
    );

    /// Composites a blurred copy of the texture of `source` into `bounds`
    /// under `transform`, clipped to `clip`.
    ///
    /// The pane reads what lies **under it**: the part of the source
    /// texture its own screen bounds cover, not the source stretched across
    /// it. Nothing is recorded — `source` is written by somebody else's
    /// [`Cached`](crate::Cached), and this only reads it, which is why the
    /// "one handle per widget" rule is not broken.
    ///
    /// `frost` is the blur applied (see [`Frost`]). `opacity` is clamped to
    /// `0.0..=1.0`; `NaN` or `<= 0` draws nothing.
    ///
    /// The blur is computed lazily, once per source rasterisation: moving,
    /// resizing, fading or clipping the pane costs an integer comparison
    /// and a composite. Reconstruction is always
    /// [`FilterQuality::Bilinear`], whatever the app's tier — sharpening
    /// what was just deliberately blurred would ring on flat gradients and
    /// cost nine taps instead of one.
    ///
    /// Draws nothing if `source` was never recorded, is
    /// [`Uncacheable`](Record::Uncacheable), or does not lie under the
    /// pane. If `source` is drawn *after* this pane within the same frame,
    /// the pane shows the previous frame's texture; that is documented
    /// behaviour, not a bug — fixing it would need a two-phase draw of the
    /// whole tree.
    fn draw_frosted(
        &mut self,
        source: &TextureCache,
        bounds: Rectangle,
        clip: Rectangle,
        transform: Transformation,
        opacity: f32,
        frost: Frost,
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

/// What a pane of glass needs to know about the texture it reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Source {
    /// Epoch of the last rasterisation. A derived blur texture compares
    /// this against its own to decide whether it is still current.
    epoch: u64,
    /// Where that texture was last composited, in screen coordinates.
    bounds: Rectangle,
    /// Its size in physical pixels — the buffer the blur chain runs over.
    size: Size<u32>,
    /// The scale it was recorded at: the device scale times the widget's
    /// supersample, not the window's scale. A blur radius is stated in
    /// logical pixels and applied to *this* texture, so this is the number
    /// it has to be converted with.
    scale: f32,
}

/// A backend's recorded texture.
trait Texture {
    fn liveness(&self) -> &Weak<Inner>;
    /// Size and scale of the last record.
    fn recorded(&self) -> (Size<u32>, f32);
    /// Everything a pane of glass over this texture reads: see [`Source`].
    // Both backends' `frosted` reach this through `Entries::source`, and
    // `lib.rs` refuses a build with neither backend, so there is a caller
    // in every configuration that compiles.
    fn source(&self) -> Source;
    /// Adopts a new screen placement after a composite.
    fn note_composited(&mut self, bounds: Rectangle);
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

    /// What a pane of glass over `id` reads (see [`Source`]); `None` if it
    /// was never recorded or is uncacheable.
    // See `Texture::source`: each backend's `frosted` calls this directly,
    // rather than through a per-store wrapper.
    fn source(&self, id: Id) -> Option<Source> {
        match self.lock().get(&id)? {
            Entry::Recorded(texture) => Some(texture.source()),
            Entry::Uncacheable { .. } => None,
        }
    }

    /// Records where `id` was last composited. A no-op for an unknown or
    /// uncacheable cache: a composite of one draws nothing anyway.
    fn note_composited(&self, id: Id, bounds: Rectangle) {
        if let Some(Entry::Recorded(texture)) = self.lock().get_mut(&id) {
            texture.note_composited(bounds);
        }
    }

    /// Records that `cache` cannot be cached at `size` (any stale texture is
    /// dropped) and logs once per cache.
    fn mark_uncacheable(&self, cache: &TextureCache, size: Size<u32>, limit: fmt::Arguments<'_>) {
        cache.note_uncacheable();
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

/// Source of rasterisation epochs. A derived blur texture remembers the
/// epoch it was built from and compares it against the source's: one
/// integer comparison per frame, and nothing is re-blurred until the source
/// is re-recorded. Kept here rather than on [`TextureCache`] because
/// [`TextureCache::record_count`] is documented as diagnostics and should
/// not become part of the contract for this.
static NEXT_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The next rasterisation epoch.
pub(crate) fn next_epoch() -> u64 {
    NEXT_EPOCH.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

    use iced_core::{Color, Font, Pixels, Rectangle, Size};
    use iced_graphics::Viewport;

    use super::{Entries, Entry, Pool, Record, Source, Texture, clamp_size, fits, needs_record};
    use crate::blur::gpu::{BlurPipelines, TargetPool};
    use crate::filter::FilterQuality;
    use crate::texture_cache::{Inner, TextureCache, TextureCacheId as Id};

    /// GPU objects shared by the compositor and every renderer it creates.
    pub(crate) struct GpuContext {
        pub engine: iced_wgpu::Engine,
        pub device: wgpu::Device,
        /// The device's queue. `iced_wgpu::Engine` owns its own clone; the
        /// blur chain submits its passes on this one, during the draw pass,
        /// so they land in the queue before the frame that samples them.
        pub queue: wgpu::Queue,
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
        /// Epoch of the last rasterisation, from [`super::next_epoch`]: a
        /// derived blur texture compares it against its own to decide
        /// whether it is still current.
        epoch: u64,
        /// Where this texture was last composited, in screen coordinates.
        /// Zero-sized until the first composite; `blur::source_uv` treats
        /// that as "no backdrop to read".
        screen_bounds: Rectangle,
    }

    impl Texture for WgpuTexture {
        fn liveness(&self) -> &Weak<Inner> {
            &self.liveness
        }

        fn recorded(&self) -> (Size<u32>, f32) {
            (self.size, self.scale_factor)
        }

        fn source(&self) -> Source {
            Source {
                epoch: self.epoch,
                bounds: self.screen_bounds,
                size: self.size,
                scale: self.scale_factor,
            }
        }

        fn note_composited(&mut self, bounds: Rectangle) {
            self.screen_bounds = bounds;
        }
    }

    /// A blurred, downscaled copy of one source texture.
    ///
    /// The GPU counterpart of `cpu::Derived`, and shorter by exactly the
    /// crop and the size: the sampler picks the sub-rectangle a pane of
    /// glass covers for free, so nothing is cut out and nothing is cached
    /// per pane, and normalised texture coordinates need no size to sample.
    struct DerivedView {
        view: Arc<wgpu::TextureView>,
        /// The source epoch this was built from.
        epoch: u64,
        /// Frames since this was last drawn.
        idle: u32,
        liveness: Weak<Inner>,
    }

    /// Cache storage of the wgpu backend: the device, one texture per live
    /// cache and a pool of nested renderers.
    pub(crate) struct WgpuCacheStore {
        gpu: GpuContext,
        default_font: Font,
        default_text_size: Pixels,
        pub(super) entries: Entries<WgpuTexture>,
        pub(super) pool: Pool<iced_wgpu::Renderer>,
        /// Blurred derivatives, one per `(source, radius, downscale)`; see
        /// [`DerivedView`].
        derived: Mutex<HashMap<crate::blur::DerivedKey, DerivedView>>,
        /// Built on the first blur and never rebuilt: an app that never asks
        /// for one must not pay for two pipelines and a shader compile.
        pipelines: OnceLock<BlurPipelines>,
        /// Intermediate render targets of the chain. Named `targets` rather
        /// than `pool`, which the nested-renderer pool above already holds.
        targets: TargetPool,
        /// Blurs run so far, for [`WgpuCacheStore::blur_count`].
        blurs: AtomicU64,
    }

    impl WgpuCacheStore {
        pub(crate) fn new(gpu: GpuContext, default_font: Font, default_text_size: Pixels) -> Self {
            Self {
                gpu,
                default_font,
                default_text_size,
                entries: Entries::new(),
                pool: Pool::new(),
                derived: Mutex::new(HashMap::new()),
                pipelines: OnceLock::new(),
                targets: TargetPool::new(),
                blurs: AtomicU64::new(0),
            }
        }

        pub(crate) fn gpu(&self) -> &GpuContext {
            &self.gpu
        }

        /// Marks a frame boundary: drops the state of caches whose last
        /// handle died. Called once per `present`/`screenshot`.
        pub(crate) fn begin_frame(&self) {
            self.entries.trim();

            // Derivatives die with their source, and also on their own: a
            // radius that was set once and abandoned must not pin a texture.
            // `entries.trim()` above has already released its lock, and the
            // target pool is aged after this one is dropped, so the order
            // `derived` -> `entries` that `frosted` relies on is never
            // inverted and no two of the three are ever held at once.
            let mut derived = self.derived.lock().unwrap_or_else(PoisonError::into_inner);
            derived.retain(|_, entry| {
                entry.idle += 1;
                entry.liveness.strong_count() > 0 && entry.idle <= crate::blur::DERIVED_IDLE_FRAMES
            });
            drop(derived);

            self.targets.age();
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
            let screen_bounds = match entries.get(&cache.id()) {
                Some(Entry::Recorded(texture)) => texture.screen_bounds,
                _ => Rectangle::with_size(Size::new(0.0, 0.0)),
            };
            let _ = entries.insert(
                cache.id(),
                Entry::Recorded(WgpuTexture {
                    view,
                    size,
                    scale_factor,
                    liveness: cache.liveness(),
                    epoch: super::next_epoch(),
                    screen_bounds,
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

        /// Records where the texture of `id` was just composited, so a pane
        /// of glass over it can work out which part of it lies underneath.
        pub(crate) fn note_composited(&self, id: Id, bounds: Rectangle) {
            self.entries.note_composited(id, bounds);
        }

        /// The blurred backdrop for a pane of glass at `glass`: the derived
        /// view, the sub-rectangle of it to sample in normalised
        /// coordinates, and the screen rectangle to draw into.
        ///
        /// The blur runs at most once per source epoch: a derivative
        /// remembers the epoch it was built from, so moving, resizing,
        /// fading or clipping the glass costs one integer comparison.
        /// `None` when there is nothing to show — the source was never
        /// recorded, is uncacheable, has never been composited, or does not
        /// lie under the glass at all.
        ///
        /// Unlike the software path this returns no crop: the sub-rectangle
        /// is normalised texture coordinates of the derived view, and the
        /// sampler cuts it out for free.
        ///
        /// `frost` is resolved here rather than by the caller because the
        /// scale it must be measured against is the source's own recorded
        /// one, which only this entry knows (see [`Source::scale`]).
        pub(crate) fn frosted(
            &self,
            id: Id,
            frost: super::Frost,
            glass: Rectangle,
        ) -> Option<(Arc<wgpu::TextureView>, Rectangle, Rectangle)> {
            // The source's size is read here and the view itself only after
            // `derived` is taken, so a re-record landing between the two
            // would pair a size with a texture of a different one. The
            // record and draw pipeline of this crate is single-threaded by
            // construction, so that cannot happen; and if it ever could,
            // the epoch stored alongside would be stale and the next frame
            // would rebuild the derivative anyway.
            let source = self.entries.source(id)?;
            let blur = crate::blur::Blur::resolve(frost.radius, source.scale, frost.downscale)?;
            let (epoch, source_bounds, source_size) = (source.epoch, source.bounds, source.size);
            let visible = glass.intersection(&source_bounds)?;
            let uv = crate::blur::source_uv(visible, source_bounds)?;
            let key = blur.key(id);

            // Lock order is always derived -> entries, matching the software
            // store: `view` and the liveness lookup below take the entries
            // lock nested inside this one, and nothing here ever takes them
            // the other way round.
            let mut derived = self.derived.lock().unwrap_or_else(PoisonError::into_inner);

            if derived.get(&key).is_none_or(|entry| entry.epoch != epoch) {
                let source = self.view(id)?;
                let liveness = {
                    let entries = self.entries.lock();
                    entries.get(&id)?.liveness().clone()
                };
                let pipelines = self
                    .pipelines
                    .get_or_init(|| BlurPipelines::new(&self.gpu.device, self.gpu.format));
                // The chain's own size is only needed to size intermediate
                // targets; the caller samples it through normalised
                // coordinates, so nothing here needs it back.
                let (view, _size) = pipelines.run(
                    &self.gpu.device,
                    &self.gpu.queue,
                    &source,
                    source_size,
                    blur,
                    &self.targets,
                );
                let _ = self.blurs.fetch_add(1, Ordering::Relaxed);
                // Replacing a stale derivative drops its texture rather
                // than returning it to the target pool, so the next blur of
                // this source allocates one afresh: correct, and wasteful
                // for a cache that re-records every frame.
                let _ = derived.insert(
                    key,
                    DerivedView {
                        view,
                        epoch,
                        idle: 0,
                        liveness,
                    },
                );
            }

            let entry = derived.get_mut(&key)?;
            entry.idle = 0;

            Some((entry.view.clone(), uv, visible))
        }

        /// Blurs run so far. Diagnostics only.
        pub(crate) fn blur_count(&self) -> u64 {
            self.blurs.load(Ordering::Relaxed)
        }

        /// Live derived textures. Diagnostics only.
        pub(crate) fn derived_len(&self) -> usize {
            self.derived
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len()
        }
    }
}

#[cfg(feature = "wgpu")]
pub(crate) use gpu::{GpuContext, WgpuCacheStore};

#[cfg(feature = "tiny-skia")]
mod cpu {
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Mutex, PoisonError, Weak};

    use iced_core::{Color, Font, Pixels, Point, Rectangle, Size, image};
    use iced_graphics::Viewport;

    use super::{
        CPU_MAX_BYTES, CPU_MAX_DIMENSION, Entries, Entry, Pool, Record, Source, Texture,
        clamp_size, fits_cpu, needs_record,
    };
    use crate::texture_cache::{Inner, TextureCache, TextureCacheId as Id};

    /// A cache recorded on `tiny_skia`.
    pub(super) struct TinySkiaTexture {
        /// Straight-alpha RGBA of the last record; a new handle per record so
        /// iced's raster cache reloads it.
        handle: image::Handle,
        /// The last rounded variant of `handle`, and the corners it was cut
        /// to. `iced_tiny_skia` never reads `image::Image::border_radius`,
        /// so a rounded composite has to be a rounded *handle*; cutting it
        /// is a pass over the texture, so it is kept until the corners, the
        /// shape they are cut on or the recording change. One slot, because
        /// a texture composited at two different radii in one frame is not a
        /// thing any widget here does.
        rounded: Option<(iced_core::border::Radius, Rectangle, image::Handle)>,
        /// Scratch pixmap and clip mask, reused while the size is unchanged.
        /// `None` while a record is in progress.
        scratch: Option<(tiny_skia::Pixmap, tiny_skia::Mask)>,
        size: Size<u32>,
        scale_factor: f32,
        liveness: Weak<Inner>,
        /// Epoch of the last rasterisation, from [`super::next_epoch`]: a
        /// derived blur texture compares it against its own to decide
        /// whether it is still current.
        epoch: u64,
        /// Where this texture was last composited, in screen coordinates.
        /// Zero-sized until the first composite; `blur::source_uv` treats
        /// that as "no backdrop to read".
        screen_bounds: Rectangle,
    }

    impl Texture for TinySkiaTexture {
        fn liveness(&self) -> &Weak<Inner> {
            &self.liveness
        }

        fn recorded(&self) -> (Size<u32>, f32) {
            (self.size, self.scale_factor)
        }

        fn source(&self) -> Source {
            Source {
                epoch: self.epoch,
                bounds: self.screen_bounds,
                size: self.size,
                scale: self.scale_factor,
            }
        }

        fn note_composited(&mut self, bounds: Rectangle) {
            self.screen_bounds = bounds;
        }
    }

    /// A blurred, downscaled copy of one source texture, plus the crop of it
    /// that was last asked for.
    struct Derived {
        /// Premultiplied BGRA at the downscaled size — the same
        /// representation the pixmap uses, so nothing is converted until
        /// the handle is built.
        pixels: Vec<u8>,
        size: Size<u32>,
        /// The source epoch this was built from.
        epoch: u64,
        /// The last crop, the corners and the pane shape (in the cut's own
        /// pixels) its mask was built for, and the handle cut from it. The
        /// shape is part of the key: two panes can share one crop and one
        /// radius yet round different corners, when one overhangs the
        /// source and the other does not.
        ///
        /// On the GPU the sub-rectangle is free — the sampler picks it. Here
        /// an `image::Handle` is drawn whole, so the crop costs a copy. A
        /// still pane pays it once; a moving one pays a copy of a buffer
        /// `downscale²` times smaller than the source per frame.
        ///
        /// Exactly one crop is kept, so two panes at the same radius over
        /// one source but at different positions evict each other and both
        /// recut every frame. That is the intended trade: the derivative
        /// itself — the expensive part — is still shared between them, and
        /// the cut is the cheap part the cost model budgets for.
        crop: Option<(
            crate::blur::Crop,
            iced_core::border::Radius,
            Rectangle,
            image::Handle,
        )>,
        /// Frames since this was last drawn.
        idle: u32,
        liveness: Weak<Inner>,
    }

    /// Cache storage of the software backend: one pixmap per live cache and
    /// a pool of nested renderers.
    pub(crate) struct TinySkiaCacheStore {
        default_font: Font,
        default_text_size: Pixels,
        pub(super) entries: Entries<TinySkiaTexture>,
        pub(super) pool: Pool<iced_tiny_skia::Renderer>,
        /// Blurred derivatives, one per `(source, radius, downscale)`; see
        /// [`Derived`].
        derived: Mutex<HashMap<crate::blur::DerivedKey, Derived>>,
        /// Blurs run so far, for [`TinySkiaCacheStore::blur_count`].
        blurs: AtomicU64,
    }

    impl TinySkiaCacheStore {
        pub(crate) fn new(default_font: Font, default_text_size: Pixels) -> Self {
            Self {
                default_font,
                default_text_size,
                entries: Entries::new(),
                pool: Pool::new(),
                derived: Mutex::new(HashMap::new()),
                blurs: AtomicU64::new(0),
            }
        }

        /// Marks a frame boundary: drops the state of caches whose last
        /// handle died. Called once per `present`/`screenshot`.
        pub(crate) fn begin_frame(&self) {
            self.entries.trim();

            // Derivatives die with their source, and also on their own: a
            // radius that was set once and abandoned must not pin memory.
            // `entries.trim()` above has already released its lock, so this
            // never holds both at once.
            let mut derived = self.derived.lock().unwrap_or_else(PoisonError::into_inner);
            derived.retain(|_, entry| {
                entry.idle += 1;
                entry.liveness.strong_count() > 0 && entry.idle <= crate::blur::DERIVED_IDLE_FRAMES
            });
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
                    texture.rounded = None;
                    texture.scratch = Some((pixmap, mask));
                    texture.size = size;
                    texture.scale_factor = scale_factor;
                    texture.epoch = super::next_epoch();
                    // `screen_bounds` is left as-is: the texture is updated
                    // in place, so its last composited placement still
                    // applies until the next composite moves it.
                }
                Entry::Uncacheable { .. } => {
                    *entry = Entry::Recorded(TinySkiaTexture {
                        handle,
                        rounded: None,
                        scratch: Some((pixmap, mask)),
                        size,
                        scale_factor,
                        liveness: cache.liveness(),
                        epoch: super::next_epoch(),
                        screen_bounds: Rectangle::with_size(Size::new(0.0, 0.0)),
                    });
                }
            }

            Record::Fresh
        }

        /// The image of `id` cut to `corners` on `shape`, if recorded.
        ///
        /// `shape` is the rounded rectangle's place in the texture, in
        /// logical pixels from its origin — the content, inside the padding
        /// recorded around it. Zero corners hand back the square handle
        /// untouched. Otherwise the cut is rebuilt from the pixmap — the
        /// radii and the shape are in logical pixels and the texture is in
        /// its own, so they are scaled by the recording scale on the way in.
        pub(crate) fn handle_rounded(
            &self,
            id: Id,
            corners: iced_core::border::Radius,
            shape: Rectangle,
        ) -> Option<image::Handle> {
            let radii: [f32; 4] = corners.into();
            if radii.iter().all(|radius| *radius <= 0.0) {
                return self.handle(id);
            }

            let mut entries = self.entries.lock();
            let Some(Entry::Recorded(texture)) = entries.get_mut(&id) else {
                return None;
            };

            if let Some((cached, cached_shape, handle)) = &texture.rounded
                && *cached == corners
                && *cached_shape == shape
            {
                return Some(handle.clone());
            }

            let (pixmap, _) = texture.scratch.as_ref()?;
            let mut rgba = pixmap_to_rgba(pixmap.data());
            let scale = texture.scale_factor;
            round_corners(
                &mut rgba,
                texture.size,
                Rectangle {
                    x: shape.x * scale,
                    y: shape.y * scale,
                    width: shape.width * scale,
                    height: shape.height * scale,
                },
                radii.map(|radius| radius * scale),
            );

            let handle = image::Handle::from_rgba(texture.size.width, texture.size.height, rgba);
            texture.rounded = Some((corners, shape, handle.clone()));

            Some(handle)
        }

        /// The image of `id`, if recorded.
        pub(crate) fn handle(&self, id: Id) -> Option<image::Handle> {
            match self.entries.lock().get(&id)? {
                Entry::Recorded(texture) => Some(texture.handle.clone()),
                Entry::Uncacheable { .. } => None,
            }
        }

        /// Epoch, screen placement and size of the texture of `id`.
        ///
        /// Exists for the store's own tests; production code (`frosted`)
        /// reaches `entries.source` directly and has no need of this
        /// wrapper.
        #[cfg(all(test, feature = "tiny-skia"))]
        pub(crate) fn source(&self, id: Id) -> Option<Source> {
            self.entries.source(id)
        }

        /// Records where the texture of `id` was just composited, so a pane
        /// of glass over it can work out which part of it lies underneath.
        pub(crate) fn note_composited(&self, id: Id, bounds: Rectangle) {
            self.entries.note_composited(id, bounds);
        }

        /// The blurred backdrop a pane of glass at `glass` reads from the
        /// texture of `id`, and the screen rectangle to draw it into.
        ///
        /// # Where the two backends differ
        ///
        /// The cut is grown outwards to whole derived texels (see
        /// [`crate::blur::crop`]), so it covers a little more than the
        /// rectangle returned here, and the caller stretches it back into
        /// that rectangle. The GPU path has no such step: it hands the
        /// sampler the unsnapped normalised coordinates and reads exactly
        /// them. So the software backdrop carries a scale error of up to
        /// one derived texel across the pane — zero somewhere inside the
        /// glass, growing towards its edges, and central only when the crop
        /// happened to grow by the same amount on both sides — and a moving
        /// pane shifts that error about as the crop snaps.
        ///
        /// Drawing the cut at the texels' own screen positions instead
        /// would be exact, and is not expressible: `iced_tiny_skia`'s
        /// raster pipeline places a pixmap at an integer multiple of its
        /// own texel size (`raster.rs`: `(bounds.x / width_scale) as i32`),
        /// which truncates towards zero rather than rounding, with any
        /// layer transformation already folded into the bounds, so the
        /// whole texel it truncates to is the best it can do — a rigid
        /// offset of up to a full texel over the *whole* pane, which is
        /// worse where it is most visible and, measured, makes one
        /// downscale disagree with another by 23/255 where stretching
        /// disagrees by 7/255 (`tests/frosted.rs`,
        /// `the_same_sigma_survives_every_downscale`). Making it exact
        /// means resampling the cut by the sub-texel residual, which is a
        /// design change rather than a correction.
        ///
        /// The blur runs at most once per source epoch: a derivative
        /// remembers the epoch it was built from, so moving, resizing,
        /// fading or clipping the glass costs one integer comparison.
        /// `None` when there is nothing to show — the source was never
        /// recorded, is uncacheable, has never been composited, or does not
        /// lie under the glass at all.
        ///
        /// `frost` is resolved here rather than by the caller because the
        /// scale it must be measured against is the source's own recorded
        /// one, which only this entry knows (see [`Source::scale`]).
        pub(crate) fn frosted(
            &self,
            id: Id,
            frost: super::Frost,
            glass: Rectangle,
        ) -> Option<(image::Handle, Rectangle)> {
            let source = self.entries.source(id)?;
            let blur = crate::blur::Blur::resolve(frost.radius, source.scale, frost.downscale)?;
            let (epoch, source_bounds, source_size) = (source.epoch, source.bounds, source.size);
            let visible = glass.intersection(&source_bounds)?;
            let uv = crate::blur::source_uv(visible, source_bounds)?;

            let key = blur.key(id);
            let target = blur.target_size(source_size);
            let window = crate::blur::crop(uv, target)?;

            // Lock order is always derived -> entries: `blurred_pixels`
            // below takes and releases the entries lock on its own, nested
            // inside this one, and never the other way around.
            let mut derived = self.derived.lock().unwrap_or_else(PoisonError::into_inner);

            let stale = derived
                .get(&key)
                .is_none_or(|existing| existing.epoch != epoch);

            if stale {
                let pixels = self.blurred_pixels(id, blur, target)?;
                let liveness = {
                    let entries = self.entries.lock();
                    entries.get(&id)?.liveness().clone()
                };
                let _ = self.blurs.fetch_add(1, Ordering::Relaxed);
                let _ = derived.insert(
                    key,
                    Derived {
                        pixels,
                        size: target,
                        epoch,
                        crop: None,
                        idle: 0,
                        liveness,
                    },
                );
            }

            let entry = derived.get_mut(&key)?;
            entry.idle = 0;

            // The corners belong to the whole pane, but the cut is only the
            // part of it lying over the source, at the derived texture's
            // resolution — so both the shape and the radii are mapped into
            // the cut's own pixels.
            let scale = window.width as f32 / visible.width;
            let shape = Rectangle {
                x: (glass.x - visible.x) * scale,
                y: (glass.y - visible.y) * scale,
                width: glass.width * scale,
                height: glass.height * scale,
            };

            let handle = match &entry.crop {
                Some((cached, corners, cached_shape, handle))
                    if *cached == window && *corners == frost.corners && *cached_shape == shape =>
                {
                    handle.clone()
                }
                _ => {
                    let cut = crate::blur::cpu::crop_out(&entry.pixels, entry.size, window);
                    let mut rgba = pixmap_to_rgba(&cut);

                    let radii: [f32; 4] = frost.corners.into();
                    round_corners(
                        &mut rgba,
                        Size::new(window.width, window.height),
                        shape,
                        radii.map(|radius| radius * scale),
                    );

                    let handle = image::Handle::from_rgba(window.width, window.height, rgba);
                    entry.crop = Some((window, frost.corners, shape, handle.clone()));
                    handle
                }
            };

            Some((handle, visible))
        }

        /// Downscales and blurs the pixmap of `id`. `None` if the pixmap is
        /// not available (a record is in progress, or the entry is gone).
        ///
        /// Drops the entries lock before blurring: `downsample` returns an
        /// owned buffer, so the blur itself — the expensive part — never
        /// runs with the lock held.
        fn blurred_pixels(
            &self,
            id: Id,
            blur: crate::blur::Blur,
            target: Size<u32>,
        ) -> Option<Vec<u8>> {
            let entries = self.entries.lock();
            let Some(Entry::Recorded(texture)) = entries.get(&id) else {
                return None;
            };
            let (pixmap, _) = texture.scratch.as_ref()?;
            let (mut pixels, size) =
                crate::blur::cpu::downsample(pixmap.data(), texture.size, blur.downscale);
            drop(entries);

            debug_assert_eq!(size, target, "the downscale agrees with `target_size`");
            crate::blur::cpu::blur(&mut pixels, size, blur.residual_sigma());

            Some(pixels)
        }

        /// Blurs run so far. Diagnostics only.
        pub(crate) fn blur_count(&self) -> u64 {
            self.blurs.load(Ordering::Relaxed)
        }

        /// Live derived textures. Diagnostics only.
        pub(crate) fn derived_len(&self) -> usize {
            self.derived
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len()
        }

        /// The blurs the live derivatives were built with. Test-only: it is
        /// how a test states what a radius resolved to, which production
        /// code never needs back.
        #[cfg(test)]
        pub(crate) fn derived_blurs(&self) -> Vec<crate::blur::Blur> {
            self.derived
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .keys()
                .map(|key| key.blur())
                .collect()
        }
    }

    /// Multiplies the alpha of a straight-alpha RGBA buffer by the coverage
    /// of a rounded rectangle, so a composited texture can have corners.
    ///
    /// Alpha only: in straight alpha the colour of a partly covered pixel is
    /// unchanged and only its coverage differs, so scaling the colour too
    /// would darken every rounded edge — the same mistake, in the other
    /// direction, that blurring in straight alpha would make.
    ///
    /// `iced_tiny_skia` never reads `image::Image::border_radius` (it
    /// destructures the field away), so the software backend has no rounded
    /// image of its own and this is where the corners come from. Runs once
    /// per buffer, not per frame.
    ///
    /// `shape` and `radii` are both in this buffer's pixels, not in logical
    /// ones: the caller scales them, because only it knows what the buffer
    /// is a picture of.
    pub(crate) fn round_corners(
        rgba: &mut [u8],
        size: Size<u32>,
        shape: Rectangle,
        radii: [f32; 4],
    ) {
        if radii.iter().all(|radius| *radius <= 0.0) {
            return;
        }

        let extent = iced_core::Size::new(shape.width, shape.height);

        for y in 0..size.height {
            for x in 0..size.width {
                // Sampled at the pixel's centre: a pixel is fully covered
                // when its middle is inside, not when its top-left corner is.
                // `shape` is where the rounded rectangle sits in this
                // buffer's own pixels — usually the whole of it, but a pane
                // of glass clipped to its source draws only part of a shape
                // that extends past the buffer.
                let centre = Point::new(x as f32 + 0.5 - shape.x, y as f32 + 0.5 - shape.y);
                let coverage = crate::geometry::rounded_coverage(centre, extent, radii);

                if coverage < 1.0 {
                    let alpha = ((y * size.width + x) * 4 + 3) as usize;
                    rgba[alpha] = (f32::from(rgba[alpha]) * coverage).round() as u8;
                }
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
use cpu::{pixmap_to_rgba, round_corners};

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

    /// Straight-alpha RGBA, `width` x `height`, every pixel opaque red.
    fn opaque_red(width: u32, height: u32) -> Vec<u8> {
        [255u8, 0, 0, 255].repeat((width * height) as usize)
    }

    #[test]
    fn a_zero_radius_leaves_every_byte_alone() {
        let mut pixels = opaque_red(8, 8);
        let before = pixels.clone();
        round_corners(
            &mut pixels,
            Size::new(8, 8),
            Rectangle::with_size(iced_core::Size::new(8.0, 8.0)),
            [0.0; 4],
        );
        assert_eq!(pixels, before);
    }

    #[test]
    fn rounding_clears_the_corners_and_spares_the_centre() {
        let mut pixels = opaque_red(16, 16);
        round_corners(
            &mut pixels,
            Size::new(16, 16),
            Rectangle::with_size(iced_core::Size::new(16.0, 16.0)),
            [6.0; 4],
        );

        let alpha = |x: u32, y: u32| pixels[((y * 16 + x) * 4 + 3) as usize];
        for (x, y) in [(0, 0), (15, 0), (15, 15), (0, 15)] {
            assert_eq!(alpha(x, y), 0, "corner ({x}, {y}) should be cut away");
        }
        assert_eq!(alpha(8, 8), 255, "the centre is untouched");
        assert_eq!(alpha(8, 0), 255, "the middle of an edge is untouched");
    }

    #[test]
    fn rounding_scales_alpha_only_and_never_the_colour() {
        // Straight alpha: the colour of a partly covered pixel does not
        // change, only how much of it there is. Scaling RGB here would
        // darken every rounded edge — the same class of bug premultiplied
        // blurring exists to avoid.
        let mut pixels = opaque_red(16, 16);
        round_corners(
            &mut pixels,
            Size::new(16, 16),
            Rectangle::with_size(iced_core::Size::new(16.0, 16.0)),
            [6.0; 4],
        );

        for pixel in pixels.as_chunks::<4>().0 {
            assert_eq!(pixel[0], 255, "red channel untouched");
            assert_eq!(pixel[1], 0);
            assert_eq!(pixel[2], 0);
        }
    }

    #[test]
    fn the_arc_is_antialiased_rather_than_stepped() {
        let mut pixels = opaque_red(32, 32);
        round_corners(
            &mut pixels,
            Size::new(32, 32),
            Rectangle::with_size(iced_core::Size::new(32.0, 32.0)),
            [10.0; 4],
        );

        let alpha = |x: u32, y: u32| pixels[((y * 32 + x) * 4 + 3) as usize];
        let partial = (0..32)
            .flat_map(|y| (0..32).map(move |x| (x, y)))
            .filter(|&(x, y)| (1..255).contains(&alpha(x, y)))
            .count();

        assert!(
            partial >= 8,
            "expected a soft arc, found {partial} partly covered pixels"
        );
    }

    #[test]
    fn pixmap_bytes_are_swizzled_and_demultiplied() {
        // Premultiplied BGRA: 50 %-alpha pure red, transparent, opaque (r=30,g=20,b=10).
        let out = pixmap_to_rgba(&[0, 0, 128, 128, 0, 0, 0, 0, 10, 20, 30, 255]);
        assert_eq!(&out[..4], &[255, 0, 0, 128]);
        assert_eq!(&out[4..8], &[0, 0, 0, 0]);
        assert_eq!(&out[8..], &[30, 20, 10, 255]);
    }

    #[test]
    fn the_store_counts_rasterisations_of_its_own() {
        let store = store();
        let cache = TextureCache::new();

        assert!(store.source(cache.id()).is_none(), "never recorded");

        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        let first = store.source(cache.id()).expect("recorded");
        assert_eq!(first.size, SIZE);
        assert_eq!(first.scale, 1.0);

        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Reused);
        let second = store.source(cache.id()).expect("recorded");
        assert_eq!(second, first, "a reuse is not a new epoch");

        cache.invalidate();
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        let third = store.source(cache.id()).expect("recorded");
        assert_ne!(third, second, "a re-record is a new epoch");
    }

    #[test]
    fn a_composite_records_where_the_texture_landed_on_screen() {
        let store = store();
        let cache = TextureCache::new();
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);

        let bounds = store.source(cache.id()).expect("recorded").bounds;
        assert_eq!(
            bounds,
            Rectangle::with_size(Size::new(0.0, 0.0)),
            "a texture that was never composited claims no place on screen"
        );

        let placed = Rectangle {
            x: 10.0,
            y: 20.0,
            width: 4.0,
            height: 4.0,
        };
        store.note_composited(cache.id(), placed);
        let bounds = store.source(cache.id()).expect("recorded").bounds;
        assert_eq!(bounds, placed);

        // A re-record must not lose the placement: the texture is replaced,
        // but where it sits on screen has not moved. This is the branch a
        // blurred derivative depends on — losing it here would leave a pane
        // of glass sampling nothing.
        cache.invalidate();
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        let bounds = store.source(cache.id()).expect("recorded").bounds;
        assert_eq!(bounds, placed, "an invalidation keeps the placement");

        // A resize reaches the same entry down a different path.
        assert_eq!(
            store.record(&cache, Size::new(5, 4), 1.0, empty),
            Record::Fresh
        );
        let bounds = store.source(cache.id()).expect("recorded").bounds;
        assert_eq!(bounds, placed, "so does a resize");

        // An unknown or uncacheable id is a no-op, not a panic.
        store.note_composited(TextureCache::new().id(), placed);
    }

    #[test]
    fn a_cache_recovering_from_uncacheable_starts_with_no_placement() {
        let store = store();
        let cache = TextureCache::new();

        let oversize = Size::new(CPU_MAX_DIMENSION + 1, 1);
        assert_eq!(
            store.record(&cache, oversize, 1.0, empty),
            Record::Uncacheable
        );
        assert!(store.source(cache.id()).is_none(), "nothing to place");

        // The stale texture was dropped, so the new one has never been
        // composited — unlike a re-record, there is no placement to carry.
        assert_eq!(store.record(&cache, SIZE, 1.0, empty), Record::Fresh);
        let bounds = store.source(cache.id()).expect("recorded").bounds;
        assert_eq!(bounds.width, 0.0);
        assert_eq!(bounds.height, 0.0);
    }

    use iced_core::Rectangle;

    /// A record that fills the whole texture with opaque red.
    fn red(
        mut renderer: iced_tiny_skia::Renderer,
        viewport: &iced_graphics::Viewport,
    ) -> iced_tiny_skia::Renderer {
        let bounds = Rectangle::with_size(viewport.logical_size());
        renderer.reset(bounds);
        renderer.fill_quad(
            iced_core::renderer::Quad {
                bounds,
                border: iced_core::Border::default(),
                shadow: iced_core::Shadow::default(),
                snap: false,
            },
            iced_core::Background::Color(iced_core::Color::from_rgb(1.0, 0.0, 0.0)),
        );
        renderer
    }

    const GLASS: Rectangle = Rectangle {
        x: 0.0,
        y: 0.0,
        width: 4.0,
        height: 4.0,
    };

    fn frost_of(radius: f32, downscale: u32) -> Frost {
        Frost {
            radius,
            downscale,
            corners: border::Radius::default(),
        }
    }

    /// A source recorded and composited over `GLASS`, ready to be frosted.
    fn frosted_source(store: &TinySkiaCacheStore) -> TextureCache {
        let cache = TextureCache::new();
        assert_eq!(store.record(&cache, SIZE, 1.0, red), Record::Fresh);
        store.note_composited(cache.id(), GLASS);
        cache
    }

    #[test]
    fn a_settled_frame_blurs_nothing_and_a_new_epoch_blurs_once() {
        let store = store();
        let cache = frosted_source(&store);
        let blur = frost_of(2.0, 2);

        assert!(store.frosted(cache.id(), blur, GLASS).is_some());
        assert_eq!(store.blur_count(), 1);

        // Redrawing the same glass, and moving, resizing and clipping it,
        // are all free: the source is blurred whole, once.
        for glass in [
            GLASS,
            Rectangle { x: 1.0, ..GLASS },
            Rectangle {
                width: 2.0,
                ..GLASS
            },
        ] {
            assert!(store.frosted(cache.id(), blur, glass).is_some());
        }
        assert_eq!(store.blur_count(), 1, "moving the glass is free");

        cache.invalidate();
        assert_eq!(store.record(&cache, SIZE, 1.0, red), Record::Fresh);
        assert!(store.frosted(cache.id(), blur, GLASS).is_some());
        assert_eq!(store.blur_count(), 2, "a new source epoch re-blurs once");
    }

    #[test]
    fn one_derivative_per_radius_not_per_pane_of_glass() {
        let store = store();
        let cache = frosted_source(&store);

        let _ = store.frosted(cache.id(), frost_of(2.0, 2), GLASS);
        let _ = store.frosted(cache.id(), frost_of(2.0, 2), GLASS);
        assert_eq!(store.derived_len(), 1, "same radius, one derivative");
        assert_eq!(store.blur_count(), 1);

        let _ = store.frosted(cache.id(), frost_of(4.0, 2), GLASS);
        assert_eq!(store.derived_len(), 2, "a second radius, a second one");
        assert_eq!(store.blur_count(), 2);
    }

    #[test]
    fn derivatives_die_with_their_source() {
        let store = store();
        let cache = frosted_source(&store);
        let _ = store.frosted(cache.id(), frost_of(2.0, 2), GLASS);
        assert_eq!(store.derived_len(), 1);

        store.begin_frame();
        assert_eq!(store.derived_len(), 1, "a live handle keeps it");

        drop(cache);
        store.begin_frame();
        assert_eq!(store.derived_len(), 0);
        assert_eq!(store.entries.len(), 0);
    }

    #[test]
    fn a_derivative_nobody_draws_is_released() {
        let store = store();
        let cache = frosted_source(&store);
        let _ = store.frosted(cache.id(), frost_of(2.0, 2), GLASS);

        for _ in 0..crate::blur::DERIVED_IDLE_FRAMES {
            store.begin_frame();
        }
        assert_eq!(store.derived_len(), 1, "still within its idle window");

        store.begin_frame();
        assert_eq!(store.derived_len(), 0);
    }

    #[test]
    fn glass_over_nothing_draws_nothing_and_creates_nothing() {
        let store = store();
        let blur = frost_of(2.0, 2);

        // Never recorded.
        let missing = TextureCache::new();
        assert!(store.frosted(missing.id(), blur, GLASS).is_none());

        // Uncacheable.
        let oversized = TextureCache::new();
        let too_big = Size::new(CPU_MAX_DIMENSION + 1, 1);
        assert_eq!(
            store.record(&oversized, too_big, 1.0, empty),
            Record::Uncacheable
        );
        assert!(store.frosted(oversized.id(), blur, GLASS).is_none());

        // Recorded but never composited: it claims no place on screen.
        let unplaced = TextureCache::new();
        assert_eq!(store.record(&unplaced, SIZE, 1.0, red), Record::Fresh);
        assert!(store.frosted(unplaced.id(), blur, GLASS).is_none());

        // Composited, but the glass is somewhere else entirely.
        let elsewhere = frosted_source(&store);
        let away = Rectangle {
            x: 500.0,
            y: 500.0,
            ..GLASS
        };
        assert!(store.frosted(elsewhere.id(), blur, away).is_none());

        assert_eq!(store.blur_count(), 0);
        assert_eq!(store.derived_len(), 0);
    }

    #[test]
    fn a_blur_is_measured_in_the_pixels_the_source_was_recorded_at() {
        let store = store();
        let cache = TextureCache::new();
        // `Cached::supersample(2.0)` over a window at scale 1 records at a
        // texture scale of 2: the entry's own scale, not the window's, is
        // the one a radius has to be measured in.
        assert_eq!(store.record(&cache, SIZE, 2.0, red), Record::Fresh);
        store.note_composited(cache.id(), GLASS);

        assert!(store.frosted(cache.id(), frost_of(6.0, 1), GLASS).is_some());

        let blurs = store.derived_blurs();
        assert_eq!(blurs.len(), 1);
        assert_eq!(
            blurs[0].sigma_source(),
            12.0,
            "a radius of 6 logical pixels is 12 pixels of a texture recorded at scale 2"
        );
    }

    #[test]
    fn frosted_returns_the_part_of_the_glass_the_source_covers() {
        let store = store();
        let cache = frosted_source(&store);
        // Glass that hangs off the right-hand edge of the source.
        let overhang = Rectangle {
            x: 2.0,
            y: 0.0,
            width: 8.0,
            height: 4.0,
        };

        let (_, drawn) = store
            .frosted(cache.id(), frost_of(2.0, 2), overhang)
            .expect("they overlap");

        assert_eq!(
            drawn,
            Rectangle {
                x: 2.0,
                y: 0.0,
                width: 2.0,
                height: 4.0
            }
        );
    }
}
