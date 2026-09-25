//! Compositors that own the cache stores.
//!
//! The wgpu compositor keeps its device so renderers can record into
//! offscreen textures; the `tiny_skia` compositor is iced's own plus a software
//! store. With both backends enabled, [`Compositor`] is iced's fallback
//! compositor over the two: wgpu when an adapter is available, `tiny_skia`
//! otherwise or when `ICED_BACKEND` asks for it. Uses only public API
//! (`Engine::new`, `Renderer::new`, `compositor::present`).
//!
//! Native only: the instance is created with `wgpu::Instance::new` (no
//! WebGPU detection) and the stores use `std::sync::Mutex`.

use std::sync::Arc;

use iced_core::Color;
use iced_graphics::compositor::{self, Information, SurfaceError};
use iced_graphics::{Shell, Viewport};

/// The compositor behind [`Renderer`](crate::Renderer); iced selects it
/// through `compositor::Default`, so apps never name it.
#[cfg(all(feature = "wgpu", feature = "tiny-skia"))]
pub type Compositor = iced_renderer::fallback::Compositor<WgpuCompositor, TinySkiaCompositor>;

/// The compositor behind [`Renderer`](crate::Renderer); iced selects it
/// through `compositor::Default`, so apps never name it.
#[cfg(all(feature = "wgpu", not(feature = "tiny-skia")))]
pub type Compositor = WgpuCompositor;

/// The compositor behind [`Renderer`](crate::Renderer); iced selects it
/// through `compositor::Default`, so apps never name it.
#[cfg(all(not(feature = "wgpu"), feature = "tiny-skia"))]
pub type Compositor = TinySkiaCompositor;

#[cfg(feature = "wgpu")]
mod gpu {
    use super::{Arc, Color, Information, Shell, SurfaceError, Viewport, compositor};

    use iced_core::Size;
    use iced_graphics::error::Reason;
    use iced_wgpu::Engine;

    use crate::record::{GpuContext, WgpuCacheStore};
    use crate::renderer::WgpuRenderer;

    /// Integer surface formats that store iced's sRGB-encoded "web colours"
    /// unchanged, in order of preference: 8-bit first, so the bytes are
    /// exactly the bytes iced computed. Float formats (`Rgba16Float`) hold
    /// linear values and would double-encode every mid-tone; see the README's
    /// "Surface format" section.
    const WEB_COLOR_FORMATS: [wgpu::TextureFormat; 3] = [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
        wgpu::TextureFormat::Rgb10a2Unorm,
    ];

    /// Picks the swapchain format for iced's colour convention.
    ///
    /// With gamma correction iced writes linear colours and needs an sRGB
    /// format. Without it (iced's default `web-colors`) it writes
    /// sRGB-encoded values that must land in one of [`WEB_COLOR_FORMATS`];
    /// only if the adapter offers none is its first usable format taken,
    /// with a warning.
    pub(crate) fn choose_format(
        formats: &[wgpu::TextureFormat],
        gamma_correction: bool,
    ) -> Option<wgpu::TextureFormat> {
        let usable = || {
            formats
                .iter()
                .copied()
                .filter(|format| format.required_features() == wgpu::Features::empty())
        };

        if gamma_correction {
            return usable()
                .find(wgpu::TextureFormat::is_srgb)
                .or_else(|| {
                    log::warn!("no sRGB surface format; colours may be off");
                    usable().next()
                })
                .or_else(|| formats.first().copied());
        }

        WEB_COLOR_FORMATS
            .into_iter()
            .find(|wanted| usable().any(|format| format == *wanted))
            .or_else(|| {
                log::warn!("no 8-bit or 10-bit unorm surface format; colours may be off");
                usable().next()
            })
            .or_else(|| formats.first().copied())
    }

    /// Validation layers only with `strict-assertions` (as in `iced_wgpu`);
    /// otherwise none, but `WGPU_DEBUG`/`WGPU_VALIDATION` still work.
    pub(crate) fn instance_flags() -> wgpu::InstanceFlags {
        if cfg!(feature = "strict-assertions") {
            wgpu::InstanceFlags::debugging()
        } else {
            wgpu::InstanceFlags::empty().with_env()
        }
    }

    /// Requests a device from `adapter` (default limits first, then
    /// downlevel) and builds the shared GPU state around it.
    pub(crate) async fn request_gpu(
        adapter: &wgpu::Adapter,
        format: wgpu::TextureFormat,
        antialiasing: Option<iced_graphics::Antialiasing>,
        shell: Shell,
        label: &'static str,
    ) -> Result<GpuContext, Vec<String>> {
        let mut errors = Vec::new();

        for limits in [wgpu::Limits::default(), wgpu::Limits::downlevel_defaults()] {
            let required_limits = wgpu::Limits {
                max_bind_groups: 2,
                max_non_sampler_bindings: 2048,
                ..limits
            };

            match adapter
                .request_device(&wgpu::DeviceDescriptor {
                    label: Some(label),
                    required_features: wgpu::Features::empty(),
                    required_limits: required_limits.clone(),
                    memory_hints: wgpu::MemoryHints::MemoryUsage,
                    trace: wgpu::Trace::Off,
                    experimental_features: wgpu::ExperimentalFeatures::disabled(),
                })
                .await
            {
                Ok((device, queue)) => {
                    let max_texture_dimension = device.limits().max_texture_dimension_2d;
                    let filter_quality = crate::filter::auto(adapter.get_info().device_type);
                    let engine = Engine::new(
                        adapter,
                        device.clone(),
                        queue.clone(),
                        format,
                        antialiasing,
                        shell,
                    );

                    return Ok(GpuContext {
                        engine,
                        device,
                        queue,
                        format,
                        max_texture_dimension,
                        filter_quality,
                    });
                }
                Err(error) => errors.push(format!("{required_limits:?}: {error}")),
            }
        }

        Err(errors)
    }

    /// The format iced's own headless renderer uses.
    pub(crate) fn headless_format() -> wgpu::TextureFormat {
        if iced_graphics::color::GAMMA_CORRECTION {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        }
    }

    fn request_failed(reason: impl std::fmt::Display) -> iced_graphics::Error {
        iced_graphics::Error::GraphicsAdapterNotFound {
            backend: "wgpu",
            reason: Reason::RequestFailed(reason.to_string()),
        }
    }

    /// The wgpu compositor: mirrors `iced_wgpu::window::Compositor`, but
    /// keeps the device in a `WgpuCacheStore` so nested renderers can be
    /// created.
    ///
    /// Not constructible by user code; see [`Compositor`](super::Compositor).
    pub struct WgpuCompositor {
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        alpha_mode: wgpu::CompositeAlphaMode,
        settings: iced_wgpu::Settings,
        store: Arc<WgpuCacheStore>,
    }

    impl std::fmt::Debug for WgpuCompositor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WgpuCompositor")
                .field("adapter", &self.adapter.get_info().name)
                .field("alpha_mode", &self.alpha_mode)
                .finish_non_exhaustive()
        }
    }

    impl WgpuCompositor {
        fn configure(&self, surface: &mut wgpu::Surface<'static>, size: Size<u32>) {
            let gpu = self.store.gpu();

            surface.configure(
                &gpu.device,
                &wgpu::SurfaceConfiguration {
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                    format: gpu.format,
                    present_mode: self.settings.present_mode,
                    width: size.width,
                    height: size.height,
                    alpha_mode: self.alpha_mode,
                    view_formats: vec![],
                    desired_maximum_frame_latency: 1,
                },
            );
        }
    }

    impl iced_graphics::Compositor for WgpuCompositor {
        type Renderer = WgpuRenderer;
        type Surface = wgpu::Surface<'static>;

        async fn with_backend(
            settings: iced_graphics::Settings,
            _display: impl compositor::Display + Clone,
            compatible_window: impl compositor::Window + Clone,
            shell: Shell,
            backend: Option<&str>,
        ) -> Result<Self, iced_graphics::Error> {
            if let Some(backend) = backend
                && backend != "wgpu"
            {
                return Err(iced_graphics::Error::GraphicsAdapterNotFound {
                    backend: "wgpu",
                    reason: Reason::DidNotMatch {
                        preferred_backend: backend.to_owned(),
                    },
                });
            }

            let mut settings = iced_wgpu::Settings::from(settings);

            if let Some(backends) = wgpu::Backends::from_env() {
                settings.backends = backends;
            }

            if let Some(present_mode) = iced_wgpu::settings::present_mode_from_env() {
                settings.present_mode = present_mode;
            }

            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
                backends: settings.backends,
                flags: instance_flags(),
                ..wgpu::InstanceDescriptor::default()
            });

            let compatible_surface = instance
                .create_surface(compatible_window)
                .map_err(request_failed)?;

            let adapter_options = wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::from_env()
                    .unwrap_or(wgpu::PowerPreference::HighPerformance),
                compatible_surface: Some(&compatible_surface),
                force_fallback_adapter: false,
            };

            let adapter = instance
                .request_adapter(&adapter_options)
                .await
                .map_err(|error| request_failed(format!("no adapter: {error}")))?;

            log::info!("selected adapter: {:#?}", adapter.get_info());

            let capabilities = compatible_surface.get_capabilities(&adapter);

            log::info!(
                "surface formats (in adapter order): {:?}; alpha modes: {:?}",
                capabilities.formats,
                capabilities.alpha_modes
            );

            let format = choose_format(
                &capabilities.formats,
                iced_graphics::color::GAMMA_CORRECTION,
            )
            .ok_or_else(|| request_failed("no compatible surface format"))?;

            // `PostMultiplied` first, mirroring `iced_wgpu`, although every
            // iced pipeline (and the composite pipeline) blends premultiplied.
            // Opaque windows do not care; for transparent windows this keeps
            // the result identical to stock iced, which is the point of parity.
            let alpha_mode = if capabilities
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PostMultiplied)
            {
                wgpu::CompositeAlphaMode::PostMultiplied
            } else if capabilities
                .alpha_modes
                .contains(&wgpu::CompositeAlphaMode::PreMultiplied)
            {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                wgpu::CompositeAlphaMode::Auto
            };

            log::info!("selected format {format:?} with alpha mode {alpha_mode:?}");

            drop(compatible_surface);

            let gpu = request_gpu(
                &adapter,
                format,
                settings.antialiasing,
                shell,
                "iced_texture_cache device",
            )
            .await
            .map_err(|errors| request_failed(format!("no device request succeeded: {errors:?}")))?;

            let store = Arc::new(WgpuCacheStore::new(
                gpu,
                settings.default_font,
                settings.default_text_size,
            ));

            Ok(Self {
                instance,
                adapter,
                alpha_mode,
                settings,
                store,
            })
        }

        fn create_renderer(&self) -> Self::Renderer {
            WgpuRenderer::new(self.store.new_renderer(), Arc::clone(&self.store), 1.0)
        }

        /// # Panics
        ///
        /// Panics if the platform cannot create a surface for `window`,
        /// mirroring `iced_wgpu` (the trait offers no error channel).
        fn create_surface<W: compositor::Window + Clone>(
            &mut self,
            window: W,
            width: u32,
            height: u32,
        ) -> Self::Surface {
            let mut surface = self
                .instance
                .create_surface(window)
                .expect("create wgpu surface");

            if width > 0 && height > 0 {
                self.configure(&mut surface, Size::new(width, height));
            }

            surface
        }

        fn configure_surface(&mut self, surface: &mut Self::Surface, width: u32, height: u32) {
            self.configure(surface, Size::new(width, height));
        }

        fn information(&self) -> Information {
            let info = self.adapter.get_info();

            Information {
                adapter: info.name,
                backend: format!("{:?}", info.backend),
            }
        }

        fn present(
            &mut self,
            renderer: &mut Self::Renderer,
            surface: &mut Self::Surface,
            viewport: &Viewport,
            background_color: Color,
            on_pre_present: impl FnOnce(),
        ) -> Result<(), SurfaceError> {
            self.store.begin_frame();
            renderer.set_scale_factor(viewport.scale_factor());

            iced_wgpu::window::compositor::present(
                renderer.inner_mut(),
                surface,
                viewport,
                background_color,
                on_pre_present,
            )
        }

        fn screenshot(
            &mut self,
            renderer: &mut Self::Renderer,
            viewport: &Viewport,
            background_color: Color,
        ) -> Vec<u8> {
            self.store.begin_frame();
            renderer.set_scale_factor(viewport.scale_factor());
            renderer.inner_mut().screenshot(viewport, background_color)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use wgpu::TextureFormat as F;

        // Adapter orders observed on NVIDIA and Intel Vulkan/Wayland.
        const NVIDIA: [F; 6] = [
            F::Bgra8UnormSrgb,
            F::Rgba8UnormSrgb,
            F::Rgba16Float,
            F::Rgb10a2Unorm,
            F::Bgra8Unorm,
            F::Rgba8Unorm,
        ];
        // Intel lists `Rgba16Unorm` before `Rgb10a2Unorm`; it needs a device
        // feature, so `Rgb10a2Unorm` is the first *usable* non-sRGB format.
        const INTEL: [F; 7] = [
            F::Bgra8UnormSrgb,
            F::Rgba8UnormSrgb,
            F::Rgba16Unorm,
            F::Rgb10a2Unorm,
            F::Bgra8Unorm,
            F::Rgba8Unorm,
            F::Rgba16Float,
        ];

        #[test]
        fn web_colours_never_land_in_a_float_format() {
            assert_eq!(choose_format(&NVIDIA, false), Some(F::Bgra8Unorm));
            assert_eq!(choose_format(&INTEL, false), Some(F::Bgra8Unorm));
        }

        #[test]
        fn gamma_correction_takes_an_srgb_format() {
            assert_eq!(choose_format(&NVIDIA, true), Some(F::Bgra8UnormSrgb));
        }

        #[test]
        fn ten_bit_unorm_is_the_third_choice() {
            let only_ten_bit = [F::Bgra8UnormSrgb, F::Rgba16Float, F::Rgb10a2Unorm];
            assert_eq!(choose_format(&only_ten_bit, false), Some(F::Rgb10a2Unorm));
        }

        #[test]
        fn unlisted_integer_formats_are_not_picked_over_the_first_usable_one() {
            // `Rgb10a2Uint` is integer and non-sRGB, but not a web-colour
            // format: the allow-list falls through to the first usable format.
            let odd = [F::Bgra8UnormSrgb, F::Rgb10a2Uint];
            assert_eq!(choose_format(&odd, false), Some(F::Bgra8UnormSrgb));
        }

        #[test]
        fn feature_gated_formats_are_skipped_even_as_a_last_resort() {
            let gated_first = [F::Rgba16Unorm, F::Bgra8UnormSrgb];
            assert_eq!(choose_format(&gated_first, false), Some(F::Bgra8UnormSrgb));
            assert_eq!(choose_format(&gated_first, true), Some(F::Bgra8UnormSrgb));
        }

        #[test]
        fn last_resort_is_the_adapters_first_usable_format() {
            let only_float_and_srgb = [F::Bgra8UnormSrgb, F::Rgba16Float];
            assert_eq!(
                choose_format(&only_float_and_srgb, false),
                Some(F::Bgra8UnormSrgb)
            );
            let no_srgb = [F::Bgra8Unorm];
            assert_eq!(choose_format(&no_srgb, true), Some(F::Bgra8Unorm));
            assert_eq!(choose_format(&[], false), None);
        }
    }
}

#[cfg(feature = "wgpu")]
pub use gpu::WgpuCompositor;
#[cfg(feature = "wgpu")]
pub(crate) use gpu::{headless_format, instance_flags, request_gpu};

#[cfg(feature = "tiny-skia")]
mod cpu {
    use super::{Arc, Color, Information, Shell, SurfaceError, Viewport, compositor};

    use crate::record::TinySkiaCacheStore;
    use crate::renderer::TinySkiaRenderer;

    /// The software compositor: iced's `tiny_skia` compositor plus a
    /// `TinySkiaCacheStore`.
    ///
    /// Not constructible by user code; see [`Compositor`](super::Compositor).
    pub struct TinySkiaCompositor {
        inner: iced_tiny_skia::window::Compositor,
        store: Arc<TinySkiaCacheStore>,
    }

    impl std::fmt::Debug for TinySkiaCompositor {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("TinySkiaCompositor").finish_non_exhaustive()
        }
    }

    impl iced_graphics::Compositor for TinySkiaCompositor {
        type Renderer = TinySkiaRenderer;
        type Surface = iced_tiny_skia::window::Surface;

        async fn with_backend(
            settings: iced_graphics::Settings,
            display: impl compositor::Display + Clone,
            compatible_window: impl compositor::Window + Clone,
            shell: Shell,
            backend: Option<&str>,
        ) -> Result<Self, iced_graphics::Error> {
            let inner =
                <iced_tiny_skia::window::Compositor as iced_graphics::Compositor>::with_backend(
                    settings,
                    display,
                    compatible_window,
                    shell,
                    backend,
                )
                .await?;

            log::info!("using the tiny_skia software backend");

            Ok(Self {
                inner,
                store: Arc::new(TinySkiaCacheStore::new(
                    settings.default_font,
                    settings.default_text_size,
                )),
            })
        }

        fn create_renderer(&self) -> Self::Renderer {
            TinySkiaRenderer::new(self.inner.create_renderer(), Arc::clone(&self.store), 1.0)
        }

        fn create_surface<W: compositor::Window + Clone>(
            &mut self,
            window: W,
            width: u32,
            height: u32,
        ) -> Self::Surface {
            self.inner.create_surface(window, width, height)
        }

        fn configure_surface(&mut self, surface: &mut Self::Surface, width: u32, height: u32) {
            self.inner.configure_surface(surface, width, height);
        }

        fn information(&self) -> Information {
            self.inner.information()
        }

        fn present(
            &mut self,
            renderer: &mut Self::Renderer,
            surface: &mut Self::Surface,
            viewport: &Viewport,
            background_color: Color,
            on_pre_present: impl FnOnce(),
        ) -> Result<(), SurfaceError> {
            self.store.begin_frame();
            renderer.set_scale_factor(viewport.scale_factor());

            self.inner.present(
                renderer.inner_mut(),
                surface,
                viewport,
                background_color,
                on_pre_present,
            )
        }

        fn screenshot(
            &mut self,
            renderer: &mut Self::Renderer,
            viewport: &Viewport,
            background_color: Color,
        ) -> Vec<u8> {
            self.store.begin_frame();
            renderer.set_scale_factor(viewport.scale_factor());
            self.inner
                .screenshot(renderer.inner_mut(), viewport, background_color)
        }
    }
}

#[cfg(feature = "tiny-skia")]
pub use cpu::TinySkiaCompositor;
