//! The rounding that keeps an animation's cost finite: every weight and every
//! size the text is shaped at is work the text stack does once more, so an
//! animated value is rounded to a step before it gets there.
//!
//! Both grids are anchored at the target, so the value an animation ends on
//! is always exact.

/// The lightest weight the `wght` axis carries.
pub const MIN_WEIGHT: f32 = 100.0;

/// The heaviest weight the `wght` axis carries.
pub const MAX_WEIGHT: f32 = 900.0;

/// The smallest size text is shaped at, in logical pixels. A spring that
/// overshoots below it is held there.
pub const MIN_SIZE: f32 = 1.0;

/// The step an animated size is rounded to when none is given, in logical
/// pixels.
///
/// A quarter of a pixel of font size moves a cap height by well under a
/// fifth of a pixel at any size the kit sets, which reads as motion rather
/// than as a stair. Every distinct size is a fresh raster of every glyph in
/// the line, so the step is what bounds how many an animation asks for.
pub const DEFAULT_SIZE_STEP: f32 = 0.25;

/// The step an animated weight is rounded to when none is given, in `wght`
/// units.
///
/// A step is invisible while it moves the stem by well under a pixel, so the
/// larger the text the finer the step has to be: `250 / size`, bounded to a
/// sane range. 14 px gets 18 units, 36 px gets 7, 72 px gets 3.
///
/// The step is what keeps the cost of an animation bounded. Every distinct
/// weight is a miss in cosmic-text's `(face, weight)` font cache and a fresh
/// `Font`: the face parsed again and its shaper tables rebuilt. A 500 → 600
/// transition at 14 px asks for about six instances instead of one per frame,
/// and every later transition between the same two weights reuses them.
#[must_use]
pub fn default_weight_step(size: f32) -> f32 {
    if size.is_finite() && size > 0.0 {
        (250.0 / size).round().clamp(2.0, 20.0)
    } else {
        20.0
    }
}

/// Rounds `weight` to a multiple of `step` on a grid anchored at `target`.
///
/// Anchoring at the target rather than at zero is what makes the end of an
/// animation exact: a settled track reads `weight == target` and quantises to
/// it unchanged, so a label resting at 600 is drawn at exactly 600 — the
/// weight `DECLARED_WEIGHTS` declares and the static text path would use.
/// Quantising on an absolute grid would land it on 594 and the handover would
/// pop.
///
/// Off-grid by up to half a step at the *start* of the animation is the price,
/// and half a step is invisible by construction of [`default_weight_step`].
#[must_use]
pub fn quantize_weight(weight: f32, target: f32, step: f32) -> u16 {
    let target = if target.is_finite() {
        target.clamp(MIN_WEIGHT, MAX_WEIGHT)
    } else {
        400.0
    };

    if !weight.is_finite() {
        return target as u16;
    }

    let steps = ((weight - target) / at_least_one(step)).round();

    steps
        .mul_add(at_least_one(step), target)
        .clamp(MIN_WEIGHT, MAX_WEIGHT) as u16
}

/// Rounds `size` to a multiple of `step` on a grid anchored at `target`, for
/// the reason [`quantize_weight`] anchors its grid there: a settled size is
/// drawn at exactly the size its style names, and the handover to the stock
/// paragraph path does not pop.
#[must_use]
pub fn quantize_size(size: f32, target: f32, step: f32) -> f32 {
    let target = if target.is_finite() {
        target.max(MIN_SIZE)
    } else {
        // The body size: somewhere certainly legible.
        16.0
    };

    if !size.is_finite() {
        return target;
    }

    let step = if step.is_finite() && step > 0.0 {
        step
    } else {
        DEFAULT_SIZE_STEP
    };

    let steps = ((size - target) / step).round();

    steps.mul_add(step, target).max(MIN_SIZE)
}

/// A weight step the grid can use: a whole `wght` unit or more.
fn at_least_one(step: f32) -> f32 {
    if step.is_finite() && step >= 1.0 {
        step
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settled_weight_quantises_to_itself() {
        // The end of every animation must be exact: the rest weight is the
        // one `DECLARED_WEIGHTS` declares and the static path would draw.
        for target in [400.0, 500.0, 600.0, 700.0] {
            assert_eq!(quantize_weight(target, target, 18.0), target as u16);
        }
    }

    #[test]
    fn the_weight_grid_is_anchored_at_the_target() {
        assert_eq!(quantize_weight(582.0, 600.0, 18.0), 582);
        assert_eq!(quantize_weight(564.0, 600.0, 18.0), 564);
    }

    #[test]
    fn weights_within_a_step_collapse() {
        let a = quantize_weight(541.0, 600.0, 18.0);
        let b = quantize_weight(546.0, 600.0, 18.0);

        assert_eq!(a, b, "half a step either way is the same font instance");
    }

    #[test]
    fn weights_outside_the_axis_are_clamped() {
        assert_eq!(quantize_weight(2000.0, 600.0, 18.0), 900);
        assert_eq!(quantize_weight(-5.0, 600.0, 18.0), 100);
    }

    /// A weight the caller cannot mean falls back to the one place that is
    /// certainly meaningful — where the animation is going — rather than to
    /// an end of the axis. `Anim` sanitises its own tracks, so this only ever
    /// catches a hand-built handle.
    #[test]
    fn a_non_finite_weight_rests_at_the_target() {
        assert_eq!(quantize_weight(f32::NAN, 600.0, 18.0), 600);
        assert_eq!(quantize_weight(f32::INFINITY, 600.0, 18.0), 600);
        assert_eq!(quantize_weight(f32::NEG_INFINITY, 600.0, 18.0), 600);
        assert_eq!(
            quantize_weight(f32::NAN, f32::NAN, 18.0),
            400,
            "and a target that is not a weight either falls back to Regular"
        );
    }

    #[test]
    fn a_degenerate_weight_step_still_produces_a_weight() {
        assert_eq!(quantize_weight(550.0, 600.0, 0.0), 550);
        assert_eq!(quantize_weight(550.0, 600.0, f32::NAN), 550);
    }

    #[test]
    fn bigger_text_gets_a_finer_weight_step() {
        assert!(
            default_weight_step(72.0) < default_weight_step(14.0),
            "a display size shows steps a caption hides"
        );
    }

    #[test]
    fn the_weight_step_stays_in_range() {
        for size in [1.0, 12.0, 14.0, 16.0, 36.0, 72.0, 400.0] {
            let step = default_weight_step(size);
            assert!((2.0..=20.0).contains(&step), "{size} px gave {step}");
        }

        assert_eq!(default_weight_step(f32::NAN), 20.0);
        assert_eq!(default_weight_step(0.0), 20.0);
    }

    #[test]
    fn a_settled_size_quantises_to_itself() {
        // Sizes off the quarter-pixel grid included: the target is the
        // anchor, not zero.
        for target in [12.0, 14.0, 18.0, 36.0, 17.1] {
            assert_eq!(quantize_size(target, target, DEFAULT_SIZE_STEP), target);
        }
    }

    #[test]
    fn the_size_grid_is_anchored_at_the_target() {
        assert_eq!(quantize_size(16.6, 18.0, 0.5), 16.5);
        assert_eq!(quantize_size(17.1, 17.1, 0.5), 17.1);
        assert_eq!(quantize_size(16.14, 17.1, 0.5), 16.1);
    }

    #[test]
    fn sizes_within_a_step_collapse() {
        assert_eq!(
            quantize_size(15.02, 18.0, 0.25),
            quantize_size(14.93, 18.0, 0.25),
            "half a step either way is the same raster"
        );
    }

    #[test]
    fn an_overshooting_size_is_held_above_zero() {
        assert_eq!(quantize_size(-4.0, 2.0, 0.25), MIN_SIZE);
        assert_eq!(quantize_size(0.2, 0.0, 0.25), MIN_SIZE);
    }

    #[test]
    fn a_non_finite_size_rests_at_the_target() {
        assert_eq!(quantize_size(f32::NAN, 18.0, 0.25), 18.0);
        assert_eq!(quantize_size(f32::INFINITY, 18.0, 0.25), 18.0);
        assert_eq!(quantize_size(f32::NAN, f32::NAN, 0.25), 16.0);
    }

    #[test]
    fn a_degenerate_size_step_falls_back_to_the_default() {
        assert_eq!(quantize_size(16.3, 18.0, 0.0), 16.25);
        assert_eq!(quantize_size(16.3, 18.0, f32::NAN), 16.25);
    }
}
