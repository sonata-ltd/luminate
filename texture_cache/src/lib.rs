#![doc = include_str!("../README.md")]

mod ancestors;
mod cached;
#[cfg(feature = "wgpu")]
mod composite;
mod compositor;
mod content_stack;
mod filter;
mod geometry;
mod pager;
mod reaction;
mod record;
mod renderer;
#[cfg(all(test, feature = "tiny-skia"))]
mod test_support;
mod texture_cache;

#[cfg(not(any(feature = "wgpu", feature = "tiny-skia")))]
compile_error!(
    "iced_texture_cache needs at least one backend: enable the `wgpu` or the `tiny-skia` \
     feature (or both)"
);

pub use iced_animate;

pub use cached::{Cached, PixelSnap, cached};
pub use compositor::Compositor;
pub use content_stack::{ContentStack, content_stack};
pub use filter::{FilterQuality, filter_quality, set_filter_quality};
pub use pager::{Pager, pager};
pub use record::{Record, TextureRenderer};
pub use renderer::{Backend, Renderer};
pub use texture_cache::{TextureCache, TextureCacheId};

// The halves are nameable because the aliases mention them, but they have
// no public constructors: use `Renderer`/`Compositor`.
#[doc(hidden)]
#[cfg(feature = "tiny-skia")]
pub use compositor::TinySkiaCompositor;
#[doc(hidden)]
#[cfg(feature = "wgpu")]
pub use compositor::WgpuCompositor;
#[doc(hidden)]
#[cfg(feature = "tiny-skia")]
pub use renderer::TinySkiaRenderer;
#[doc(hidden)]
#[cfg(feature = "wgpu")]
pub use renderer::WgpuRenderer;

/// An iced element rendered by this crate's [`Renderer`].
pub type Element<'a, Message, Theme = iced_core::Theme> =
    iced_core::Element<'a, Message, Theme, Renderer>;

/// Test scaffolding shared with downstream crates' tests. Not part of the
/// stable API.
#[doc(hidden)]
pub mod testing {
    /// A software [`Renderer`](crate::Renderer) with cache storage that
    /// needs neither a GPU nor an async executor: record, composite and
    /// `iced_core::renderer::Headless::screenshot` all work on it.
    #[must_use]
    #[cfg(feature = "tiny-skia")]
    pub fn headless_tiny_skia() -> crate::Renderer {
        crate::renderer::headless_tiny_skia()
    }

    /// The page placement a [`Pager`](crate::Pager) computes for one sliding
    /// frame, so tests can measure a policy without driving a whole widget.
    #[must_use]
    pub fn pager_page_bounds(
        filter: crate::FilterQuality,
        mode: crate::PixelSnap,
        page: iced_core::Rectangle,
        pager: iced_core::Rectangle,
        scale: f32,
    ) -> iced_core::Rectangle {
        crate::geometry::pager_page_bounds(filter, mode, page, pager, scale)
    }
}
