//! What "a blur of sigma 12" means, decided once for both backends.
//!
//! The GPU chain and the software chain use different kernels — a separable
//! Gaussian over a downscaled copy on one side, three box passes on the
//! other — so a radius expressed as "number of passes" would look different
//! depending on which backend the app happened to start on. Everything that
//! defines the blur instead lives
//! here as plain arithmetic: the quantised sigma, the downscale, how much
//! spread the downscale itself already carries, and where a pane of glass
//! falls inside its source texture. No renderer, no GPU, no backend feature.

use iced_core::{Rectangle, Size};

use crate::texture_cache::TextureCacheId;

#[cfg(feature = "tiny-skia")]
pub(crate) mod cpu;
#[cfg(feature = "wgpu")]
pub(crate) mod gaussian;
#[cfg(feature = "wgpu")]
pub(crate) mod gpu;

/// Frames a derived texture (or a pooled render target) may go unused
/// before it is released — about a second at 60 Hz.
///
/// The blur runs only when the source is re-recorded, so both pools are
/// almost always empty; this constant is what keeps "the radius changed
/// once" from pinning a texture for the life of the process. It is the
/// sibling of `BLEED` in `cached.rs`: a number that exists so there is
/// somewhere to change it.
pub(crate) const DERIVED_IDLE_FRAMES: u32 = 60;

/// Target-space pixels of residual blur the automatic downscale aims for:
/// coarse enough that the chain is cheap, fine enough that the kernel still
/// has something to work with.
///
/// It is a floor, not a midpoint. Each step of the downscale halves, so
/// what is left for the kernel lands anywhere in `[3, 6)` target pixels —
/// see [`automatic_downscale`].
const RESIDUAL_TARGET: f32 = 3.0;

/// The coarsest downscale the chain supports. Each step halves, so this is
/// three halvings.
const MAX_DOWNSCALE: u32 = 8;

/// The narrowest blur either backend can express, in pixels of the buffer
/// the kernel runs on.
///
/// The software chain is three box passes and a box has an odd integer
/// width, so the narrowest non-identity triple is `[1, 1, 3]`, whose
/// variance is `(3² - 1) / 12`. Below that there is nothing between "no
/// blur" and that kernel. Both backends stop at the same place rather than
/// one of them pretending: the GPU could interpolate a finer Gaussian, but
/// a sub-pixel difference in sigma is invisible, and a single shared floor
/// is what keeps a radius meaning the same thing on each.
///
/// `Blur::resolve` also checks a request against this same floor in
/// *source* pixels, before a downscale is even chosen — a coarser, earlier
/// use of the same number, explained at that check site.
pub(crate) const MIN_KERNEL_SIGMA: f32 = 0.816_496_6; // sqrt(2 / 3)

/// A resolved blur request: everything the backends need, and nothing that
/// changes between frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Blur {
    /// Sigma in *half* pixels of the source texture. Quantised, so the
    /// sub-pixel jitter of a scale-factor change cannot re-blur on every
    /// frame, and integral, so it can sit in a hash key.
    pub sigma_halves: u32,
    /// Power of two in `1..=MAX_DOWNSCALE`.
    pub downscale: u32,
}

impl Blur {
    /// Resolves a widget's request. `radius` is the equivalent Gaussian
    /// sigma in *logical* pixels; `downscale` is the requested factor, or
    /// `0` for the automatic choice. `None` when there is nothing to blur.
    ///
    /// `source_scale` is the scale the texture being blurred was *recorded*
    /// at — the device scale times that widget's supersample, not the
    /// window's scale. The whole chain works in the pixels of that texture,
    /// so a source recorded at twice the window's scale needs twice the
    /// sigma to look the same on screen. Only the store that holds the
    /// entry knows this number, which is why it resolves the request rather
    /// than the renderer.
    pub(crate) fn resolve(radius: f32, source_scale: f32, downscale: u32) -> Option<Self> {
        if !radius.is_finite() || radius <= 0.0 {
            return None;
        }

        // A renderer that reports no usable scale records at 1:1, exactly as
        // `geometry::composite_geometry` does.
        let scale = if source_scale > 0.0 && source_scale.is_finite() {
            source_scale
        } else {
            1.0
        };

        let sigma_halves = (radius * scale * 2.0).round();
        if !sigma_halves.is_finite() {
            return None;
        }
        let sigma_halves = sigma_halves.min(f32::from(u16::MAX)) as u32;
        // Below the shared kernel floor neither backend has a kernel to run
        // (see `MIN_KERNEL_SIGMA`), and a derivative that blurs nothing is
        // a full-resolution copy of the source plus, on the software path,
        // an `image::Handle` built from it. Nothing is cheaper than not
        // building it. A *downscale* coarse enough to take the residual
        // below the floor is still allowed: that is the caller explicitly
        // trading the blur away for the memory, and `gaussian::plan` and
        // `cpu::box_widths` both return the identity for it.
        if (sigma_halves as f32) / 2.0 < MIN_KERNEL_SIGMA {
            return None;
        }

        let downscale = if downscale == 0 {
            automatic_downscale(sigma_halves as f32 / 2.0)
        } else {
            nearest_power_of_two(downscale)
        };

        Some(Self {
            sigma_halves,
            downscale,
        })
    }

    /// The requested sigma in pixels of the *source* texture.
    pub(crate) fn sigma_source(self) -> f32 {
        self.sigma_halves as f32 / 2.0
    }

    /// The sigma left for the blur kernel once the downscale has done its
    /// part, in pixels of the *target* (downscaled) texture.
    ///
    /// Averaging `d`x`d` blocks is itself a box filter: variance
    /// `(d² - 1) / 12` in source pixels, `(d² - 1) / 12d²` in target ones.
    /// Convolution adds variances, so the kernel only has to supply the
    /// difference — which is the whole reason the chain is cheap.
    ///
    /// That `d`x`d` average is exactly what the software chain's
    /// `downsample` computes. The GPU reaches the same level in `log2(d)`
    /// halvings instead, which coincide with it only while every level's
    /// dimensions are even; on an odd one the halving resamples rather than
    /// averaging blocks (see `gaussian`'s module documentation). This
    /// number is therefore exact for the software chain and for an even
    /// GPU chain, and approximate for an odd one.
    pub(crate) fn residual_sigma(self) -> f32 {
        let d = self.downscale as f32;
        let wanted = self.sigma_source() / d;
        let carried = (d * d - 1.0) / (12.0 * d * d);
        (wanted * wanted - carried).max(0.0).sqrt()
    }

    /// Size of the derived texture for a source of `source` physical pixels.
    /// The far edge keeps its partial block rather than losing it.
    pub(crate) fn target_size(self, source: Size<u32>) -> Size<u32> {
        Size::new(
            source.width.div_ceil(self.downscale).max(1),
            source.height.div_ceil(self.downscale).max(1),
        )
    }

    /// The identity of the derived texture this request builds.
    pub(crate) fn key(self, source: TextureCacheId) -> DerivedKey {
        DerivedKey {
            source,
            sigma_halves: self.sigma_halves,
            downscale: self.downscale,
        }
    }
}

/// Identity of one derived blur texture. Two panes of glass over the same
/// source at the same radius share one; at different radii they get one
/// each, and the idle sweep takes back whichever stops being drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DerivedKey {
    source: TextureCacheId,
    sigma_halves: u32,
    downscale: u32,
}

impl DerivedKey {
    /// The blur this key was built from.
    ///
    /// Test-only: it exists so a store's tests can state what a derivative
    /// was blurred with, which is otherwise visible only to the backend
    /// that built it. Only the software store's tests reach a derivative
    /// this way, so a wgpu-only build does not compile it.
    #[cfg(all(test, feature = "tiny-skia"))]
    pub(crate) fn blur(self) -> Blur {
        Blur {
            sigma_halves: self.sigma_halves,
            downscale: self.downscale,
        }
    }
}

/// The coarsest downscale that still leaves at least [`RESIDUAL_TARGET`]
/// pixels for the kernel: `clamp(2^floor(log2(sigma / 3)), 1, 8)`.
///
/// The factor is a power of two and the floor rounds down, so the residual
/// it leaves is `sigma / d`, which for any sigma above `RESIDUAL_TARGET`
/// falls in `[3, 6)` target pixels — three at a sigma that has just crossed
/// a halving, approaching six just before the next one. Below that the
/// downscale is 1 and the residual is the sigma itself.
///
/// Past sigma 24 the clamp at [`MAX_DOWNSCALE`] binds and the residual
/// grows without bound instead: an extreme radius is blurred at 1/8 and
/// pays for the rest in kernel passes, which is the `gaussian::plan` (and,
/// on the software path, `cpu::box_widths`) side of the trade rather than a
/// reason to throw away more resolution.
fn automatic_downscale(sigma_source: f32) -> u32 {
    if sigma_source <= RESIDUAL_TARGET {
        return 1;
    }

    let exponent = (sigma_source / RESIDUAL_TARGET).log2().floor();
    // A saturating cast: an absurd sigma lands on `u32::MAX` and clamps.
    let factor = exponent.exp2() as u32;
    factor.clamp(1, MAX_DOWNSCALE)
}

/// Rounds `requested` to the nearest power of two in `1..=MAX_DOWNSCALE`;
/// a tie rounds up, because the coarser of two equally distant factors is
/// the cheaper one.
fn nearest_power_of_two(requested: u32) -> u32 {
    let clamped = requested.clamp(1, MAX_DOWNSCALE);
    let upper = clamped.next_power_of_two();

    if upper == clamped {
        return clamped;
    }

    let lower = upper / 2;
    if clamped - lower >= upper - clamped {
        upper
    } else {
        lower
    }
}

/// Where `glass` falls inside `source`, both in screen coordinates, as
/// normalised texture coordinates of the source texture.
///
/// This is the whole of the backdrop semantics: the pane samples what lies
/// under it, rather than stretching the source across itself. Values may
/// fall outside `0..1` — the caller clamps, and clamp-to-edge is what the
/// CSS `backdrop-filter` specification prescribes there.
pub(crate) fn source_uv(glass: Rectangle, source: Rectangle) -> Option<Rectangle> {
    if !(source.width > 0.0 && source.height > 0.0) {
        return None;
    }
    if !(glass.width > 0.0 && glass.height > 0.0) {
        return None;
    }

    Some(Rectangle {
        x: (glass.x - source.x) / source.width,
        y: (glass.y - source.y) / source.height,
        width: glass.width / source.width,
        height: glass.height / source.height,
    })
}

/// The block of derived texels a pane of glass covers.
///
/// Only the software store's `frosted` builds and reads one — on the GPU
/// the sub-rectangle is chosen by the sampler for free — so this is absent
/// from a wgpu-only build; the tests below exercise it unconditionally.
#[cfg(any(feature = "tiny-skia", test))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Crop {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// The texels of a `target`-sized derived texture that `uv` covers, grown
/// outwards to whole texels and clamped into the texture.
///
/// Only the software path needs this: on the GPU the sub-rectangle is
/// chosen by the sampler for free. `None` when `uv` misses the texture
/// entirely.
#[cfg(any(feature = "tiny-skia", test))]
pub(crate) fn crop(uv: Rectangle, target: Size<u32>) -> Option<Crop> {
    let (w, h) = (target.width as f32, target.height as f32);

    let left = (uv.x * w).floor().clamp(0.0, w);
    let top = (uv.y * h).floor().clamp(0.0, h);
    let right = ((uv.x + uv.width) * w).ceil().clamp(0.0, w);
    let bottom = ((uv.y + uv.height) * h).ceil().clamp(0.0, h);

    let width = (right - left) as u32;
    let height = (bottom - top) as u32;

    (width > 0 && height > 0).then_some(Crop {
        x: left as u32,
        y: top as u32,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::texture_cache::TextureCache;

    fn blur(radius: f32, scale: f32, downscale: u32) -> Blur {
        Blur::resolve(radius, scale, downscale).expect("a positive radius blurs")
    }

    /// Where a [`Crop`] of a `target`-sized derived texture *would* land on
    /// screen, given where its source was composited: the exact inverse of
    /// the mapping [`source_uv`] and [`crop`] went through, and what the GPU
    /// path's sampler reads.
    ///
    /// Test-only, because the software path cannot draw there — see
    /// `TinySkiaCacheStore::frosted` — and this is how the gap between them
    /// is measured rather than left to drift.
    fn placement(crop: Crop, target: Size<u32>, source: Rectangle) -> Rectangle {
        let (w, h) = (target.width as f32, target.height as f32);

        Rectangle {
            x: source.x + (crop.x as f32 / w) * source.width,
            y: source.y + (crop.y as f32 / h) * source.height,
            width: (crop.width as f32 / w) * source.width,
            height: (crop.height as f32 / h) * source.height,
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn a_radius_is_sigma_in_source_texture_pixels_quantised_to_half_a_pixel() {
        // The scale is the source's *recorded* one, so a supersampled
        // source asks for proportionally more sigma.
        assert_eq!(blur(12.0, 2.0, 1).sigma_source(), 24.0);
        // 3.1 logical at scale 1 -> 3.0 device (6 halves), not 3.1.
        assert_eq!(blur(3.1, 1.0, 1).sigma_source(), 3.0);
        assert_eq!(blur(3.3, 1.0, 1).sigma_source(), 3.5);
    }

    #[test]
    fn nothing_to_blur_yields_no_parameters() {
        assert!(Blur::resolve(0.0, 1.0, 1).is_none());
        assert!(Blur::resolve(-4.0, 1.0, 1).is_none());
        assert!(Blur::resolve(f32::NAN, 1.0, 1).is_none());
        assert!(Blur::resolve(f32::INFINITY, 1.0, 1).is_none());
        assert!(Blur::resolve(0.1, 1.0, 1).is_none());
    }

    #[test]
    fn a_radius_under_the_shared_kernel_floor_builds_nothing() {
        // Neither backend has a kernel below `MIN_KERNEL_SIGMA`, so a
        // derivative in that band would be a full-resolution copy that
        // blurs nothing. The quantisation puts the last rejected step at
        // half a pixel and the first accepted one at a whole pixel.
        assert!(Blur::resolve(0.5, 1.0, 1).is_none());
        assert!(Blur::resolve(0.7, 1.0, 1).is_none(), "0.5 after rounding");
        assert_eq!(blur(0.8, 1.0, 1).sigma_source(), 1.0);
        // The scale carries a small radius over the floor.
        assert!(Blur::resolve(0.4, 1.0, 1).is_none());
        assert_eq!(blur(0.4, 2.0, 1).sigma_source(), 1.0);
        // A downscale coarse enough to take the residual under the floor is
        // still the caller's to ask for: the kernels return the identity
        // and the memory saving is what was wanted.
        assert_eq!(blur(1.0, 1.0, 8).residual_sigma(), 0.0);
    }

    #[test]
    fn a_non_positive_scale_factor_records_at_one_to_one() {
        assert_eq!(blur(12.0, 0.0, 1).sigma_source(), 12.0);
        assert_eq!(blur(12.0, f32::NAN, 1).sigma_source(), 12.0);
    }

    #[test]
    fn a_requested_downscale_rounds_to_the_nearest_power_of_two() {
        let d = |requested| blur(12.0, 1.0, requested).downscale;
        assert_eq!(d(1), 1);
        assert_eq!(d(2), 2);
        assert_eq!(d(3), 4, "a tie rounds up");
        assert_eq!(d(5), 4);
        assert_eq!(d(6), 8, "a tie rounds up");
        assert_eq!(d(7), 8);
        assert_eq!(d(8), 8);
        assert_eq!(d(64), 8, "clamped to the chain's limit");
    }

    #[test]
    fn the_default_downscale_leaves_three_to_six_pixels_of_residual_blur() {
        // `downscale = 0` asks for the automatic choice. At scale 1 the
        // radius is the sigma in source pixels.
        let chosen = |sigma: f32| blur(sigma, 1.0, 0);
        let auto = |sigma: f32| chosen(sigma).downscale;
        assert_eq!(auto(2.0), 1, "nothing to gain below one target pixel");
        assert_eq!(auto(3.0), 1);
        assert_eq!(auto(7.0), 2);
        assert_eq!(auto(24.0), 8);
        assert_eq!(auto(400.0), 8, "clamped");

        // And the range that name claims: every sigma above
        // `RESIDUAL_TARGET` up to where the clamp binds leaves the kernel
        // between three and six target pixels — never less, because the
        // exponent is floored, and never six, because that is the next
        // halving.
        let mut sigma = RESIDUAL_TARGET + 0.05;
        while sigma <= 24.0 {
            let b = chosen(sigma);
            let residual = b.sigma_source() / b.downscale as f32;
            assert!(
                (RESIDUAL_TARGET..2.0 * RESIDUAL_TARGET).contains(&residual),
                "sigma {sigma} left {residual} target pixels at 1/{}",
                b.downscale
            );
            // What the kernel is actually asked for is a shade under that:
            // the block average has already carried part of the spread.
            assert!(b.residual_sigma() <= residual);
            assert!(b.residual_sigma() > residual - 0.02);
            sigma += 0.05;
        }

        // Past the clamp the residual grows instead of the downscale, and
        // `gaussian::plan` pays for it in passes.
        let extreme = chosen(400.0);
        assert_eq!(extreme.downscale, MAX_DOWNSCALE);
        assert_eq!(extreme.sigma_source() / extreme.downscale as f32, 50.0);
    }

    #[test]
    fn the_software_stretch_stays_under_one_derived_texel() {
        // The GPU samples `uv` and puts the backdrop exactly where the
        // source's own texels are. The software path draws a cut buffer,
        // already grown outwards to whole texels, stretched back into the
        // glass, because `iced_tiny_skia` can only place a pixmap at an
        // integer multiple of its own texel size. This is the test that
        // holds the size of that difference down.
        let source = rect(100.0, 50.0, 200.0, 100.0);
        let target = Size::new(40, 20);
        // One derived texel is five screen pixels across, two and a half
        // down. Nothing below lands on a texel edge.
        let (texel_x, texel_y) = (200.0 / 40.0, 100.0 / 20.0);
        let glass = rect(137.0, 73.0, 61.0, 29.0);

        let uv = source_uv(glass, source).expect("overlaps");
        let window = crop(uv, target).expect("non-empty");
        let exact = placement(window, target, source);

        // The cut really does cover more than the glass on every side —
        // which is why it cannot simply be drawn at `exact` and clipped.
        assert!(exact.x < glass.x && exact.y < glass.y);
        assert!(exact.x + exact.width > glass.x + glass.width);
        assert!(exact.y + exact.height > glass.y + glass.height);

        // Stretching `exact` into `glass` maps a point at true position `x`
        // to `glass.x + (x - exact.x) * glass.width / exact.width`. The
        // error is zero once inside and largest at the two ends, where it
        // is exactly how far the crop was grown — under one texel each.
        let left = glass.x - exact.x;
        let right = (glass.x + glass.width) - (exact.x + exact.width);
        assert!(left > 0.0 && left < texel_x, "left end off by {left}");
        assert!(right < 0.0 && -right < texel_x, "right end off by {right}");

        let top = glass.y - exact.y;
        let bottom = (glass.y + glass.height) - (exact.y + exact.height);
        assert!(top > 0.0 && top < texel_y, "top off by {top}");
        assert!(bottom < 0.0 && -bottom < texel_y, "bottom off by {bottom}");

        // Somewhere between them the two agree exactly: the stretch is
        // anchored at the centre of the pane, which is what keeps one
        // downscale looking like another.
        let ratio = glass.width / exact.width;
        let fixed = (glass.x - exact.x * ratio) / (1.0 - ratio);
        assert!(
            glass.x < fixed && fixed < glass.x + glass.width,
            "the fixed point {fixed} is outside the glass"
        );

        // And a crop that needed no growing is the glass exactly: an
        // aligned pane has no error at all, on either backend.
        let aligned = rect(105.0, 55.0, 50.0, 25.0);
        let exact = crop(source_uv(aligned, source).expect("overlaps"), target).expect("non-empty");
        assert_eq!(placement(exact, target, source), aligned);
    }

    #[test]
    fn the_key_separates_sources_radii_and_downscales() {
        let a = TextureCache::new().id();
        let b = TextureCache::new().id();
        assert_eq!(blur(12.0, 1.0, 4).key(a), blur(12.0, 1.0, 4).key(a));
        assert_ne!(blur(12.0, 1.0, 4).key(a), blur(12.0, 1.0, 4).key(b));
        assert_ne!(blur(12.0, 1.0, 4).key(a), blur(16.0, 1.0, 4).key(a));
        assert_ne!(blur(12.0, 1.0, 4).key(a), blur(12.0, 1.0, 2).key(a));
        // Quantisation is the point: a hair of jitter is the same key.
        assert_eq!(blur(12.0, 1.0, 4).key(a), blur(12.1, 1.0, 4).key(a));
    }

    #[test]
    fn the_downscale_carries_most_of_the_spread_itself() {
        // sigma 24 device px at 1/8 leaves 3 px in the target space, minus
        // the spread the 8x8 box average already applied.
        let residual = blur(24.0, 1.0, 8).residual_sigma();
        assert!(
            (residual - 2.98).abs() < 0.02,
            "residual sigma was {residual}"
        );
        // At 1:1 the chain has done nothing yet.
        assert_eq!(blur(24.0, 1.0, 1).residual_sigma(), 24.0);
        // A downscale coarser than the blur asks for leaves nothing to do.
        assert_eq!(blur(1.0, 1.0, 8).residual_sigma(), 0.0);
    }

    #[test]
    fn the_target_keeps_the_partial_block_at_the_far_edge() {
        let b = blur(24.0, 1.0, 4);
        assert_eq!(b.target_size(Size::new(300, 300)), Size::new(75, 75));
        assert_eq!(b.target_size(Size::new(301, 299)), Size::new(76, 75));
        assert_eq!(b.target_size(Size::new(1, 1)), Size::new(1, 1));
        assert_eq!(b.target_size(Size::new(0, 0)), Size::new(1, 1));
    }

    #[test]
    fn glass_reads_the_part_of_the_source_it_covers() {
        let source = rect(100.0, 50.0, 200.0, 100.0);
        // The whole source.
        assert_eq!(
            source_uv(source, source).expect("overlaps"),
            rect(0.0, 0.0, 1.0, 1.0)
        );
        // The bottom-right quarter.
        assert_eq!(
            source_uv(rect(200.0, 100.0, 100.0, 50.0), source).expect("overlaps"),
            rect(0.5, 0.5, 0.5, 0.5)
        );
    }

    #[test]
    fn a_degenerate_source_or_glass_reads_nothing() {
        let source = rect(0.0, 0.0, 200.0, 100.0);
        assert!(source_uv(rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 0.0, 0.0, 100.0)).is_none());
        assert!(source_uv(rect(0.0, 0.0, 0.0, 10.0), source).is_none());
        assert!(source_uv(rect(0.0, 0.0, 10.0, -1.0), source).is_none());
    }

    #[test]
    fn a_crop_covers_the_uv_rectangle_and_stays_inside_the_texture() {
        let target = Size::new(80, 40);
        assert_eq!(
            crop(rect(0.0, 0.0, 1.0, 1.0), target).expect("non-empty"),
            Crop {
                x: 0,
                y: 0,
                width: 80,
                height: 40
            }
        );
        assert_eq!(
            crop(rect(0.5, 0.5, 0.5, 0.5), target).expect("non-empty"),
            Crop {
                x: 40,
                y: 20,
                width: 40,
                height: 20
            }
        );
        // A fractional edge grows outwards: the crop must cover the request.
        assert_eq!(
            crop(rect(0.01, 0.01, 0.1, 0.1), target).expect("non-empty"),
            Crop {
                x: 0,
                y: 0,
                width: 9,
                height: 5
            }
        );
        // Outside the texture is clamped, not wrapped.
        assert_eq!(
            crop(rect(-1.0, -1.0, 3.0, 3.0), target).expect("non-empty"),
            Crop {
                x: 0,
                y: 0,
                width: 80,
                height: 40
            }
        );
        assert!(crop(rect(2.0, 0.0, 1.0, 1.0), target).is_none());
    }
}
