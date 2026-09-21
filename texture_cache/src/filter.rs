//! The reconstruction filter used when a cached texture is composited under a
//! sub-pixel transform, and the app-wide override that selects it.

use std::sync::atomic::{AtomicU8, Ordering};

/// How a cached texture is reconstructed when it lands between device pixels.
///
/// A texture composited at a fractional device-pixel offset has to be
/// resampled, and the kernel that does it decides how crisp the result looks
/// while the texture moves. The tiers below trade fragment work for sharpness.
///
/// Without an explicit choice the tier is picked from the graphics adapter (see
/// [`set_filter_quality`]); [`Cached::filter_quality`] overrides it for one
/// widget.
///
/// # Examples
///
/// ```no_run
/// use iced::widget::text;
/// use iced_texture_cache::{FilterQuality, TextureCache, cached};
///
/// let cache = TextureCache::new();
/// let _: iced_texture_cache::Element<'_, ()> = cached(cache, text("sharp in motion"))
///     .filter_quality(FilterQuality::CatmullRom)
///     .into();
/// ```
///
/// [`Cached::filter_quality`]: crate::Cached::filter_quality
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FilterQuality {
    /// Catmull-Rom bicubic (B = 0, C = ½): the sharpest tier.
    ///
    /// An *interpolating* kernel — it passes through the source texels, so it
    /// is exact at integer phase — with a mild high-frequency boost that keeps
    /// moving text and edges crisp. Costs up to 9 hardware-bilinear taps per
    /// fragment, 3 for an axis-aligned slide and 1 once the texture is at
    /// integer phase.
    CatmullRom,
    /// A single hardware-bilinear tap: the cheapest tier that still glides.
    ///
    /// Softens edges slightly *while moving*; sharp again at rest.
    #[default]
    Bilinear,
    /// No reconstruction: the composite is snapped to whole device pixels, so
    /// there is never a sub-pixel offset to filter.
    ///
    /// Always crisp, but motion steps by whole device pixels instead of
    /// gliding. This tier **overrides [`PixelSnap`]** — it snaps whatever that
    /// policy says, because its crispness comes from the geometry, not from
    /// the shader.
    ///
    /// [`PixelSnap`]: crate::PixelSnap
    Snap,
}

impl FilterQuality {
    /// The kernel selector handed to the composite fragment shader.
    ///
    /// [`Snap`](Self::Snap) shares the single-tap value with
    /// [`Bilinear`](Self::Bilinear): it differs in [`snaps`](Self::snaps), not
    /// in the shader.
    #[cfg(feature = "wgpu")]
    pub(crate) fn shader_mode(self) -> f32 {
        match self {
            FilterQuality::CatmullRom => 0.0,
            FilterQuality::Bilinear | FilterQuality::Snap => 1.0,
        }
    }

    /// Whether this tier forces the composite onto the device-pixel grid,
    /// whatever [`PixelSnap`](crate::PixelSnap) asks for.
    pub(crate) fn snaps(self) -> bool {
        matches!(self, FilterQuality::Snap)
    }

    /// The discriminant stored in [`OVERRIDE`]. Never `UNSET`.
    fn tag(self) -> u8 {
        match self {
            FilterQuality::CatmullRom => 1,
            FilterQuality::Bilinear => 2,
            FilterQuality::Snap => 3,
        }
    }

    /// The inverse of [`FilterQuality::tag`]; `None` for `UNSET` (and for a
    /// tag this build does not know, which cannot happen through the API).
    fn from_tag(tag: u8) -> Option<Self> {
        match tag {
            1 => Some(FilterQuality::CatmullRom),
            2 => Some(FilterQuality::Bilinear),
            3 => Some(FilterQuality::Snap),
            _ => None,
        }
    }
}

/// No override set: every renderer uses the tier picked for its adapter.
const UNSET: u8 = 0;

/// The app-wide override. A process-wide atomic because it is a preference of
/// the application, not of one window or one device; the *automatic* tier is
/// per-device and lives on the GPU context instead.
static OVERRIDE: AtomicU8 = AtomicU8::new(UNSET);

/// Forces `quality` for every cached composite in this process; `None` gives
/// the automatic per-adapter choice back.
///
/// Automatic selection reads the adapter: a discrete GPU takes
/// [`FilterQuality::CatmullRom`], an integrated one [`FilterQuality::Bilinear`],
/// and anything else (software, virtual, unknown) [`FilterQuality::Snap`],
/// where a fragment-heavy filter would hurt most. The software backend has no
/// adapter and takes [`FilterQuality::Bilinear`].
///
/// Call it before `run()`: it affects every window and both backends, and a
/// widget that sets [`Cached::filter_quality`] still wins over it.
///
/// # Examples
///
/// ```
/// use iced_texture_cache::{FilterQuality, set_filter_quality};
///
/// set_filter_quality(FilterQuality::Bilinear);
/// set_filter_quality(None); // back to the per-adapter choice
/// ```
///
/// [`Cached::filter_quality`]: crate::Cached::filter_quality
pub fn set_filter_quality(quality: impl Into<Option<FilterQuality>>) {
    let tag = quality.into().map_or(UNSET, FilterQuality::tag);
    OVERRIDE.store(tag, Ordering::Relaxed);
}

/// The app-wide override set by [`set_filter_quality`], if any.
#[must_use]
pub fn filter_quality() -> Option<FilterQuality> {
    FilterQuality::from_tag(OVERRIDE.load(Ordering::Relaxed))
}

/// The tier `device_type` gets when no override is set. The only place that
/// maps hardware to a tier, because it is the only one that knows the adapter.
#[cfg(feature = "wgpu")]
pub(crate) fn auto(device_type: wgpu::DeviceType) -> FilterQuality {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => FilterQuality::CatmullRom,
        wgpu::DeviceType::IntegratedGpu => FilterQuality::Bilinear,
        _ => FilterQuality::Snap,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_catmull_rom_takes_the_bicubic_shader_path() {
        #[cfg(feature = "wgpu")]
        {
            assert_eq!(FilterQuality::CatmullRom.shader_mode(), 0.0);
            assert_eq!(FilterQuality::Bilinear.shader_mode(), 1.0);
            assert_eq!(FilterQuality::Snap.shader_mode(), 1.0);
        }
    }

    #[test]
    fn only_snap_rounds_the_geometry() {
        assert!(FilterQuality::Snap.snaps());
        assert!(!FilterQuality::Bilinear.snaps());
        assert!(!FilterQuality::CatmullRom.snaps());
    }

    #[test]
    fn every_tier_survives_the_atomic_round_trip() {
        for quality in [
            FilterQuality::CatmullRom,
            FilterQuality::Bilinear,
            FilterQuality::Snap,
        ] {
            assert_eq!(FilterQuality::from_tag(quality.tag()), Some(quality));
        }

        assert_eq!(FilterQuality::from_tag(UNSET), None);
    }

    #[cfg(feature = "wgpu")]
    #[test]
    fn the_automatic_tier_follows_the_adapter() {
        assert_eq!(
            auto(wgpu::DeviceType::DiscreteGpu),
            FilterQuality::CatmullRom
        );
        assert_eq!(
            auto(wgpu::DeviceType::IntegratedGpu),
            FilterQuality::Bilinear
        );

        for device_type in [
            wgpu::DeviceType::Cpu,
            wgpu::DeviceType::VirtualGpu,
            wgpu::DeviceType::Other,
        ] {
            assert_eq!(auto(device_type), FilterQuality::Snap);
        }
    }
}
