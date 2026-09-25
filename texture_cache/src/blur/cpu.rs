//! The software blur chain: box-average downsample, three box passes, crop.
//!
//! `tiny_skia` is a rasteriser and has no blur of its own, so the chain
//! runs over the pixel buffer directly — over the pixmap's own
//! premultiplied BGRA (`record.rs`), with no conversion in or out. Three
//! box passes converge on a Gaussian, and a box with a running sum costs
//! `O(pixels)` whatever its radius, so six linear passes over an already
//! downscaled buffer is the whole cost. The downscale is not an
//! optimisation here but the condition that makes the approach viable at
//! all.
//!
//! Single-threaded on purpose: the record and composite pipeline of this
//! crate is single-threaded by construction, and dragging a thread pool in
//! for a couple of hundred thousand pixels would trade an invariant for
//! nothing.

use iced_core::Size;

use super::Crop;

/// Averages `factor` x `factor` blocks. `factor` is a power of two in
/// `1..=8`; the partial blocks at the right and bottom edges average the
/// texels they actually cover, so the far edge keeps its content instead of
/// fading into whatever a zero-padded block would give.
pub(crate) fn downsample(src: &[u8], size: Size<u32>, factor: u32) -> (Vec<u8>, Size<u32>) {
    if factor <= 1 {
        return (src.to_vec(), size);
    }

    let out_size = Size::new(
        size.width.div_ceil(factor).max(1),
        size.height.div_ceil(factor).max(1),
    );
    let mut out = vec![0u8; (out_size.width as usize) * (out_size.height as usize) * 4];
    let stride = size.width as usize * 4;

    for out_y in 0..out_size.height {
        let y0 = out_y * factor;
        let y1 = (y0 + factor).min(size.height);

        for out_x in 0..out_size.width {
            let x0 = out_x * factor;
            let x1 = (x0 + factor).min(size.width);
            let count = (x1 - x0) * (y1 - y0);
            let mut sums = [0u32; 4];

            for y in y0..y1 {
                let row = y as usize * stride;
                for x in x0..x1 {
                    let at = row + x as usize * 4;
                    for (sum, byte) in sums.iter_mut().zip(&src[at..at + 4]) {
                        *sum += u32::from(*byte);
                    }
                }
            }

            let at = ((out_y * out_size.width + out_x) * 4) as usize;
            for (byte, sum) in out[at..at + 4].iter_mut().zip(sums) {
                *byte = ((sum + count / 2) / count) as u8;
            }
        }
    }

    (out, out_size)
}

/// The three box widths whose convolution approximates a Gaussian of
/// `sigma`, in the pixels of the buffer they run on. Always odd; `[1, 1, 1]`
/// is the identity.
///
/// The widths come from matching the variance of the three-box convolution
/// to `sigma²` and splitting the remainder between the two odd widths that
/// bracket the ideal one — the standard construction, and the reason the
/// calibration can be stated exactly in [`effective_sigma`]. A `sigma`
/// below [`super::MIN_KERNEL_SIGMA`] returns the identity outright, because
/// no triple of odd integer widths narrower than `[1, 1, 3]` exists for the
/// rounding to land on.
pub(crate) fn box_widths(sigma: f32) -> [u32; 3] {
    const PASSES: f32 = 3.0;

    if !sigma.is_finite() || sigma < super::MIN_KERNEL_SIGMA {
        return [1, 1, 1];
    }

    let variance = 12.0 * sigma * sigma;
    let ideal = (variance / PASSES + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;

    let lower_f = lower as f32;
    let shorter = ((variance - PASSES * lower_f * lower_f - 4.0 * PASSES * lower_f - 3.0 * PASSES)
        / (-4.0 * lower_f - 4.0))
        .round()
        .clamp(0.0, PASSES) as u32;

    let mut widths = [0u32; 3];
    for (index, width) in widths.iter_mut().enumerate() {
        *width = if (index as u32) < shorter {
            lower as u32
        } else {
            upper as u32
        };
    }

    widths
}

/// The sigma `widths` actually delivers.
///
/// Convolution adds variances and a box of odd width `w` has variance
/// `(w² - 1) / 12`, so this is exact — it is the number the GPU chain is
/// calibrated against, not an estimate.
///
/// Test-only: it exists to state the calibration the tests check against,
/// not because production code needs the value back.
#[cfg(test)]
pub(crate) fn effective_sigma(widths: [u32; 3]) -> f32 {
    let variance: f32 = widths.iter().map(|&w| ((w * w - 1) as f32) / 12.0).sum();

    variance.sqrt()
}

/// Three horizontal and three vertical box passes over premultiplied BGRA.
pub(crate) fn blur(buf: &mut [u8], size: Size<u32>, sigma: f32) {
    let widths = box_widths(sigma);
    if widths == [1, 1, 1] {
        return;
    }

    let mut scratch = vec![0u8; buf.len()];

    for width in widths {
        let radius = (width - 1) / 2;
        box_pass_horizontal(buf, &mut scratch, size, radius);
        box_pass_vertical(&scratch, buf, size, radius);
    }
}

/// Copies `crop` out of a `size` buffer into a tight one.
pub(crate) fn crop_out(src: &[u8], size: Size<u32>, crop: Crop) -> Vec<u8> {
    let stride = size.width as usize * 4;
    let out_stride = crop.width as usize * 4;
    let mut out = vec![0u8; out_stride * crop.height as usize];

    for row in 0..crop.height as usize {
        let from = (crop.y as usize + row) * stride + crop.x as usize * 4;
        let to = row * out_stride;
        out[to..to + out_stride].copy_from_slice(&src[from..from + out_stride]);
    }

    out
}

/// One horizontal box pass with a running sum.
///
/// The window is clamped to the edge rather than padded with zeros: that is
/// the duplication the CSS `backdrop-filter` specification prescribes, and
/// it is what keeps an opaque edge opaque instead of fading it out.
fn box_pass_horizontal(src: &[u8], dst: &mut [u8], size: Size<u32>, radius: u32) {
    if radius == 0 {
        dst.copy_from_slice(src);
        return;
    }

    // `i64` rather than `isize`: the cast from `u32` is then infallible on
    // every target, where `isize` would risk wrapping on a 32-bit one.
    let (width, height) = (i64::from(size.width), i64::from(size.height));
    let span = 2 * radius + 1;
    let half = span / 2;
    let radius = i64::from(radius);

    for y in 0..height {
        let row = (y * width * 4) as usize;

        for channel in 0..4 {
            let at = |x: i64| -> u32 {
                let x = x.clamp(0, width - 1) as usize;
                u32::from(src[row + x * 4 + channel])
            };

            let mut sum: u32 = (-radius..=radius).map(at).sum();

            for x in 0..width {
                dst[row + x as usize * 4 + channel] = ((sum + half) / span) as u8;
                sum = sum + at(x + radius + 1) - at(x - radius);
            }
        }
    }
}

/// One vertical box pass; see [`box_pass_horizontal`].
fn box_pass_vertical(src: &[u8], dst: &mut [u8], size: Size<u32>, radius: u32) {
    if radius == 0 {
        dst.copy_from_slice(src);
        return;
    }

    // See `box_pass_horizontal`: `i64` keeps the cast from `u32` infallible.
    let (width, height) = (i64::from(size.width), i64::from(size.height));
    let stride = (width * 4) as usize;
    let span = 2 * radius + 1;
    let half = span / 2;
    let radius = i64::from(radius);

    for x in 0..width {
        let column = x as usize * 4;

        for channel in 0..4 {
            let at = |y: i64| -> u32 {
                let y = y.clamp(0, height - 1) as usize;
                u32::from(src[y * stride + column + channel])
            };

            let mut sum: u32 = (-radius..=radius).map(at).sum();

            for y in 0..height {
                dst[y as usize * stride + column + channel] = ((sum + half) / span) as u8;
                sum = sum + at(y + radius + 1) - at(y - radius);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPAQUE: u8 = 255;

    /// A `width` x `height` buffer built from a per-pixel closure returning
    /// premultiplied BGRA.
    fn buffer(size: Size<u32>, f: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
        let mut out = Vec::with_capacity((size.width * size.height * 4) as usize);
        for y in 0..size.height {
            for x in 0..size.width {
                out.extend_from_slice(&f(x, y));
            }
        }
        out
    }

    fn pixel(buf: &[u8], size: Size<u32>, x: u32, y: u32) -> [u8; 4] {
        let start = ((y * size.width + x) * 4) as usize;
        [buf[start], buf[start + 1], buf[start + 2], buf[start + 3]]
    }

    /// The sigma a step edge actually came out with: the derivative of the
    /// edge profile *is* the blur kernel, so its second moment is the
    /// variance we asked for. Measuring it — rather than comparing the
    /// profile to a Gaussian CDF point by point — is what pins the
    /// calibration down, because three box passes only approximate a
    /// Gaussian and the approximation error would swamp the tolerance.
    fn measured_sigma(profile: &[f64]) -> f64 {
        let derivative: Vec<f64> = profile.windows(2).map(|w| w[1] - w[0]).collect();
        let mass: f64 = derivative.iter().sum();
        let mean: f64 = derivative
            .iter()
            .enumerate()
            .map(|(i, d)| i as f64 * d)
            .sum::<f64>()
            / mass;
        let variance: f64 = derivative
            .iter()
            .enumerate()
            .map(|(i, d)| (i as f64 - mean).powi(2) * d)
            .sum::<f64>()
            / mass;

        variance.sqrt()
    }

    #[test]
    fn box_widths_are_odd_and_deliver_the_requested_sigma() {
        for sigma in [1.0_f32, 2.0, 3.7, 8.0, 20.0] {
            let widths = box_widths(sigma);
            assert!(
                widths.iter().all(|w| w % 2 == 1),
                "sigma {sigma} gave even widths {widths:?}"
            );
            let effective = effective_sigma(widths);
            assert!(
                (effective - sigma).abs() <= 0.35,
                "sigma {sigma} came out as {effective} from {widths:?}"
            );
        }

        assert_eq!(box_widths(0.0), [1, 1, 1], "no blur is three unit boxes");
        assert_eq!(box_widths(f32::NAN), [1, 1, 1]);
        assert_eq!(effective_sigma([1, 1, 1]), 0.0);
    }

    #[test]
    fn a_sigma_below_the_narrowest_kernel_is_no_blur_at_all() {
        // Three boxes of odd integer width cannot express a sigma below
        // `[1, 1, 3]`'s sqrt(2/3): there is nothing between that and the
        // identity, so the chain stops rather than rounding up to a kernel
        // half again too wide.
        assert_eq!(box_widths(0.5), [1, 1, 1]);
        assert_eq!(box_widths(0.8), [1, 1, 1]);
        assert_eq!(box_widths(0.82), [1, 1, 3]);
        assert!((effective_sigma([1, 1, 3]) - 0.816_5).abs() < 1e-3);

        // And `blur` leaves the buffer untouched there.
        let size = Size::new(8, 8);
        let mut buf = buffer(size, |x, _| [(x * 30) as u8, 0, 0, OPAQUE]);
        let before = buf.clone();
        blur(&mut buf, size, 0.5);
        assert_eq!(buf, before);
    }

    /// Test 1 of the spec: the edge profile is the calibration.
    #[test]
    fn a_step_edge_spreads_by_exactly_the_requested_sigma() {
        let size = Size::new(96, 8);
        let sigma = 6.0_f32;
        // Opaque black on the left, opaque white on the right.
        let mut buf = buffer(size, |x, _| {
            if x < 48 {
                [0, 0, 0, OPAQUE]
            } else {
                [OPAQUE; 4]
            }
        });

        blur(&mut buf, size, sigma);

        let profile: Vec<f64> = (0..size.width)
            .map(|x| f64::from(pixel(&buf, size, x, 4)[2]) / 255.0)
            .collect();

        let measured = measured_sigma(&profile);
        let promised = f64::from(effective_sigma(box_widths(sigma)));
        assert!(
            (measured - promised).abs() < 0.15,
            "measured {measured}, kernel promises {promised}"
        );
        assert!(
            (measured - f64::from(sigma)).abs() < 0.35,
            "measured {measured} against the requested {sigma}"
        );
    }

    /// Test 2 of the spec: premultiplied alpha leaves no halo. Straight
    /// alpha would drag black out of the transparent texels and ring the
    /// square in grey — exactly the bug the card's cube would have hit.
    #[test]
    fn blurring_into_transparency_leaves_no_dark_fringe() {
        let size = Size::new(64, 64);
        // Premultiplied BGRA of opaque pure red is (0, 0, 255, 255).
        let mut buf = buffer(size, |x, y| {
            if (16..48).contains(&x) && (16..48).contains(&y) {
                [0, 0, OPAQUE, OPAQUE]
            } else {
                [0; 4]
            }
        });

        blur(&mut buf, size, 4.0);

        // Composite over white, the way a screenshot would.
        for x in 48..60 {
            let [b, g, r, a] = pixel(&buf, size, x, 32);
            let over = |c: u8| u32::from(c) + (255 - u32::from(a));
            assert_eq!(over(r), 255, "red stays saturated at x = {x}");
            assert_eq!(over(g), over(b), "no colour cast at x = {x}");
            assert!(
                over(g) >= 255 - u32::from(a),
                "the pixel sits on the red-to-white line at x = {x}"
            );
        }
    }

    /// Test 3 of the spec: an unnormalised box pass shows up here first.
    #[test]
    fn a_blur_conserves_alpha() {
        let size = Size::new(64, 64);
        let mut buf = buffer(size, |x, y| {
            if (20..44).contains(&x) && (20..44).contains(&y) {
                [10, 20, 30, OPAQUE]
            } else {
                [0; 4]
            }
        });

        let before: u64 = buf.as_chunks::<4>().0.iter().map(|p| u64::from(p[3])).sum();
        blur(&mut buf, size, 3.0);
        let after: u64 = buf.as_chunks::<4>().0.iter().map(|p| u64::from(p[3])).sum();

        let drift = (after as f64 - before as f64).abs() / before as f64;
        assert!(
            drift < 0.005,
            "alpha drifted by {drift} ({before} -> {after})"
        );
    }

    /// Test 4 of the spec: the kernel duplicates the edge row instead of
    /// padding with zeros, so a fully opaque texture stays fully opaque.
    #[test]
    fn the_kernel_clamps_to_the_edge_instead_of_padding_with_zeros() {
        let size = Size::new(32, 32);
        let mut buf = buffer(size, |_, _| [40, 80, 120, OPAQUE]);

        blur(&mut buf, size, 5.0);

        for (x, y) in [(0, 0), (31, 0), (0, 31), (31, 31), (0, 16), (16, 31)] {
            assert_eq!(
                pixel(&buf, size, x, y),
                [40, 80, 120, OPAQUE],
                "a uniform texture is its own blur at ({x}, {y})"
            );
        }
    }

    #[test]
    fn downsampling_averages_whole_and_partial_blocks() {
        let size = Size::new(4, 2);
        // Columns 0..4 carry 0, 40, 80, 120 in blue; alpha opaque.
        let src = buffer(size, |x, _| [(x * 40) as u8, 0, 0, OPAQUE]);

        let (out, out_size) = downsample(&src, size, 2);
        assert_eq!(out_size, Size::new(2, 1));
        assert_eq!(pixel(&out, out_size, 0, 0), [20, 0, 0, OPAQUE]);
        assert_eq!(pixel(&out, out_size, 1, 0), [100, 0, 0, OPAQUE]);

        // A partial block at the far edge averages what it covers.
        let odd = Size::new(3, 1);
        let src = buffer(odd, |x, _| [(x * 40) as u8, 0, 0, OPAQUE]);
        let (out, out_size) = downsample(&src, odd, 2);
        assert_eq!(out_size, Size::new(2, 1));
        assert_eq!(pixel(&out, out_size, 0, 0), [20, 0, 0, OPAQUE]);
        assert_eq!(pixel(&out, out_size, 1, 0), [80, 0, 0, OPAQUE]);

        // A factor of one is the identity, not a copy through arithmetic.
        let (same, same_size) = downsample(&src, odd, 1);
        assert_eq!(same_size, odd);
        assert_eq!(same, src);
    }

    #[test]
    fn a_crop_lifts_exactly_its_rectangle() {
        let size = Size::new(4, 4);
        let src = buffer(size, |x, y| [x as u8, y as u8, 0, OPAQUE]);
        let window = Crop {
            x: 1,
            y: 2,
            width: 2,
            height: 2,
        };

        let out = crop_out(&src, size, window);
        let out_size = Size::new(2, 2);
        assert_eq!(out.len(), 16);
        assert_eq!(pixel(&out, out_size, 0, 0), [1, 2, 0, OPAQUE]);
        assert_eq!(pixel(&out, out_size, 1, 1), [2, 3, 0, OPAQUE]);
    }
}
