//! The residual blur of the GPU chain: a separable Gaussian over the
//! already downscaled buffer, and the tap table the shader runs it with.
//!
//! The resolution reduction is a plain 2x2 box average per halving — but
//! only while every level's dimensions are even. An odd dimension puts the
//! destination centres between source texels rather than on the corner
//! between four of them, and the halving becomes a resample of the whole
//! row that `blur::cpu::downsample`, which averages explicit whole and
//! partial blocks, does not match. See the `fs_halve` documentation in
//! `shader/blur.wgsl` for what that looks like.
//!
//! So [`Blur::residual_sigma`](super::Blur::residual_sigma) describes both
//! backends with one number exactly in the even case and approximately
//! otherwise; the residual Gaussian below absorbs the difference. What is
//! left over after the reduction is a small sigma — three to six pixels of
//! an image up to sixty-four times smaller than the source, for any radius
//! the automatic downscale can keep up with — and a separable Gaussian
//! delivers exactly that sigma for two passes of a handful of taps. Past
//! the coarsest downscale the residual grows instead, and [`plan`] answers
//! with passes.
//!
//! The taps are *linear-sampled*: neighbouring weights are folded into one
//! bilinear fetch at a fractional offset, which halves the tap count and is
//! exact — the hardware's interpolation puts the two samples back with the
//! weights they were folded from.

/// Taps one pass may use, centre included. The uniform block is sized for
/// this, and [`plan`] adds passes rather than taps when a sigma needs more.
///
/// `gpu.rs`'s `SHADER_MAX_TAPS` is defined from this constant rather than
/// restating it, and asserts there that the two still match the literal in
/// `shader/blur.wgsl`, so the shader's uniform block and the planner
/// cannot drift apart.
pub(crate) const MAX_TAPS: usize = 8;

/// The largest sigma one pass covers within [`MAX_TAPS`]: a kernel is cut
/// at three sigma, neighbouring weights fold in pairs, so `MAX_TAPS` taps
/// reach `2 * (MAX_TAPS - 1) / 3` sigma. Four leaves a little headroom.
const SIGMA_PER_PASS: f32 = 4.0;

// Below `MIN_KERNEL_SIGMA` the residual is not worth a pass. The floor is
// shared with the software chain (see `blur::MIN_KERNEL_SIGMA`): the GPU
// could interpolate a finer Gaussian, but three boxes of odd integer width
// cannot, and one floor for both is what keeps a radius meaning the same
// thing on each backend.

/// How the residual blur runs: a separable pair of passes (horizontal then
/// vertical), repeated `passes` times, with `taps` each time.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    /// How many times the separable pair of passes runs. `0` means no
    /// blur at all — the residual sigma did not clear the shared floor.
    pub passes: u32,
    /// `(offset in texels, weight)`, centre first. Every tap but the centre
    /// is applied on both sides, so the weights here sum to
    /// `centre + 2 * sides = 1`.
    pub taps: Vec<(f32, f32)>,
}

/// One half of a normalised kernel, `g[0]` at the centre, cut at three
/// sigma, whose *discrete* variance is `sigma²`. The mirrored whole sums to
/// one.
///
/// Sampling a continuous Gaussian of parameter `sigma` on the texel grid
/// and normalising does **not** give a kernel of that variance: the mass
/// that falls between texels is redistributed, and the cut at three sigma
/// drops a little more. The shortfall is largest where the grid is
/// coarsest relative to the kernel — 7 % at parameter 0.5 — and stays
/// under 1 % across the whole range [`plan`] ever asks for, which starts
/// at [`MIN_KERNEL_SIGMA`](super::MIN_KERNEL_SIGMA), where parameter 0.82
/// comes out as 0.8199, and ends at [`SIGMA_PER_PASS`]. So in the live
/// range the agreement between the backends would hold anyway; solving for
/// the parameter is what makes it hold *by construction* rather than by
/// luck, for a bisection that runs once per plan. This is still a sampled
/// Gaussian, but of whatever parameter makes its sampled variance come out
/// right.
pub(crate) fn kernel(sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil().max(1.0) as usize;
    let wanted = sigma * sigma;

    // Sampled variance grows monotonically with the parameter and the
    // bracket below always contains the answer (at the top end the kernel
    // is nearly uniform over the radius, whose variance far exceeds
    // `sigma²`), so it bisects. Thirty halvings take the bracket below
    // what `f32` can tell apart.
    let (mut low, mut high) = (1e-3, 3.0 * sigma.max(1.0));
    for _ in 0..30 {
        let middle = f32::midpoint(low, high);
        if half_variance(&sampled(middle, radius)) < wanted {
            low = middle;
        } else {
            high = middle;
        }
    }

    sampled(f32::midpoint(low, high), radius)
}

/// A normalised half-kernel sampled from a Gaussian of `parameter`, cut at
/// `radius` texels.
fn sampled(parameter: f32, radius: usize) -> Vec<f32> {
    let mut half: Vec<f32> = (0..=radius)
        .map(|index| {
            let position = index as f32;
            (-position * position / (2.0 * parameter * parameter)).exp()
        })
        .collect();

    let total = half[0] + 2.0 * half[1..].iter().sum::<f32>();
    for weight in &mut half {
        *weight /= total;
    }

    half
}

/// The variance of a mirrored half-kernel, in texels.
fn half_variance(half: &[f32]) -> f32 {
    2.0 * half[1..]
        .iter()
        .enumerate()
        .map(|(index, weight)| ((index + 1) as f32).powi(2) * weight)
        .sum::<f32>()
}

/// The variance of a mirrored tap set, in texels.
///
/// Test-only: it exists to check what a [`plan`] delivers against the sigma
/// it was asked for, not because production code needs the value back.
#[cfg(test)]
pub(crate) fn tap_variance(taps: &[(f32, f32)]) -> f32 {
    // Each tap but the centre stands for two samples, and a fractional
    // offset stands for two texels; both are accounted for by expanding the
    // fetch the way the sampler does.
    taps.iter()
        .map(|&(offset, weight)| {
            let lower = offset.floor();
            let fraction = offset - lower;
            let moment =
                (1.0 - fraction) * lower * lower + fraction * (lower + 1.0) * (lower + 1.0);
            let mirrored = if offset == 0.0 { 1.0 } else { 2.0 };
            weight * mirrored * moment
        })
        .sum()
}

/// The cheapest separable Gaussian that spreads by `residual_sigma` target
/// texels.
///
/// A sigma too wide for one pass is split across several: convolving a
/// kernel with itself adds variances, so `n` passes of `sigma / sqrt(n)`
/// deliver `sigma` exactly, at a fixed tap count per pass.
///
/// The pass count therefore grows as the *square* of the residual sigma.
/// The automatic downscale keeps the residual under six target pixels, so
/// in the ordinary case this is one pass or two; only a radius past the
/// clamp at the coarsest downscale (see `super::automatic_downscale`) runs
/// many, over a buffer sixty-four times smaller than the source, and only
/// once per source epoch.
pub(crate) fn plan(residual_sigma: f32) -> Plan {
    if !residual_sigma.is_finite() || residual_sigma < super::MIN_KERNEL_SIGMA {
        return Plan {
            passes: 0,
            taps: Vec::new(),
        };
    }

    let passes = (residual_sigma / SIGMA_PER_PASS).powi(2).ceil().max(1.0) as u32;
    let half = kernel(residual_sigma / (passes as f32).sqrt());

    // The centre stands alone; the sides fold in pairs, each pair becoming
    // one bilinear fetch at the weighted midpoint of the two texels.
    let mut taps = vec![(0.0, half[0])];
    let mut index = 1;
    while index < half.len() {
        let (first, second) = (half[index], half.get(index + 1).copied().unwrap_or(0.0));
        let weight = first + second;
        let offset = if weight > 0.0 {
            (index as f32 * first + (index + 1) as f32 * second) / weight
        } else {
            index as f32
        };
        taps.push((offset, weight));
        index += 2;
    }

    Plan { passes, taps }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Expands the linear-sampled taps back into the point samples the
    /// hardware actually averages, as `(offset, weight)` over the full
    /// mirrored kernel.
    fn expand(taps: &[(f32, f32)]) -> Vec<(f32, f32)> {
        let mut out = Vec::new();

        for &(offset, weight) in taps {
            let lower = offset.floor();
            let fraction = offset - lower;
            let mut push = |position: f32, share: f32| {
                if share > 0.0 {
                    out.push((position, weight * share));
                    if position != 0.0 {
                        out.push((-position, weight * share));
                    }
                }
            };
            push(lower, 1.0 - fraction);
            push(lower + 1.0, fraction);
        }

        out
    }

    fn total(samples: &[(f32, f32)]) -> f32 {
        samples.iter().map(|&(_, w)| w).sum()
    }

    #[test]
    fn a_kernel_has_the_variance_it_was_asked_for() {
        for sigma in [0.5_f32, 1.0, 2.5, 4.0] {
            let half = kernel(sigma);
            assert!(!half.is_empty());
            // The mirrored kernel sums to one.
            let sum = half[0] + 2.0 * half[1..].iter().sum::<f32>();
            assert!((sum - 1.0).abs() < 1e-4, "sigma {sigma} summed to {sum}");
            // It is a Gaussian: monotonically decreasing away from the centre.
            assert!(
                half.windows(2).all(|w| w[0] >= w[1]),
                "sigma {sigma} is not monotone"
            );
            // And it has the variance it was asked for.
            let variance: f32 = 2.0
                * half[1..]
                    .iter()
                    .enumerate()
                    .map(|(index, weight)| ((index + 1) as f32).powi(2) * weight)
                    .sum::<f32>();
            assert!(
                (variance.sqrt() - sigma).abs() < 0.05 * sigma,
                "sigma {sigma} came out as {}",
                variance.sqrt()
            );
        }
    }

    #[test]
    fn the_taps_reproduce_the_kernel_they_were_folded_from() {
        // Pairing two neighbouring weights into one linear fetch is exact:
        // the hardware's own interpolation puts the two samples back with
        // the weights they started with. This is the test that says so.
        let chosen = plan(3.0);
        let expanded = expand(&chosen.taps);
        let half = kernel(3.0 / (chosen.passes as f32).sqrt());

        for (index, expected) in half.iter().enumerate() {
            let position = index as f32;
            let got: f32 = expanded
                .iter()
                .filter(|&&(offset, _)| (offset - position).abs() < 1e-3)
                .map(|&(_, weight)| weight)
                .sum();
            // `expand` mirrors every tap but the centre, and the filter
            // above picks only the positive side, so each position should
            // carry exactly its half-kernel weight.
            assert!(
                (got - expected).abs() < 1e-4,
                "tap at {position}: {got} against {expected}"
            );
        }

        assert!((total(&expanded) - 1.0).abs() < 1e-4);
    }

    #[test]
    fn a_plan_delivers_the_sigma_it_was_asked_for() {
        for sigma in [1.0_f32, 2.0, 3.0, 4.5, 6.0, 12.0] {
            let chosen = plan(sigma);
            // Variances add: `passes` applications of the same kernel give
            // sigma * sqrt(passes).
            let delivered = (tap_variance(&chosen.taps) * chosen.passes as f32).sqrt();
            assert!(
                (delivered - sigma).abs() <= 0.05 * sigma,
                "sigma {sigma} came out as {delivered} from {} passes",
                chosen.passes
            );
        }
    }

    #[test]
    fn a_pass_never_needs_more_taps_than_the_uniform_block_holds() {
        for sigma in [1.0_f32, 3.0, 6.0, 12.0, 40.0, 200.0] {
            let chosen = plan(sigma);
            assert!(
                chosen.taps.len() <= MAX_TAPS,
                "sigma {sigma} wanted {} taps in {} passes",
                chosen.taps.len(),
                chosen.passes
            );
            assert!(chosen.passes >= 1);
        }
    }

    #[test]
    fn the_first_tap_is_the_centre_and_the_rest_walk_outwards() {
        let chosen = plan(4.0);
        assert_eq!(chosen.taps[0].0, 0.0, "the centre tap is not mirrored");
        assert!(
            chosen.taps.windows(2).all(|w| w[1].0 > w[0].0),
            "offsets must increase: {:?}",
            chosen.taps
        );
    }

    #[test]
    fn a_vanishing_residual_runs_no_passes() {
        // Below the shared kernel floor there is nothing between the
        // identity and a kernel half again too wide, so the chain stops.
        for sigma in [0.0_f32, -1.0, f32::NAN, 0.1, 0.5, 0.8] {
            let chosen = plan(sigma);
            assert_eq!(chosen.passes, 0, "sigma {sigma}");
            assert!(chosen.taps.is_empty());
        }
    }
}
