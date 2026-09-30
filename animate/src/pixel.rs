//! The device pixel: what a layout that moves is snapped to.
//!
//! Text in iced sits on whole device pixels vertically — cosmic-text
//! truncates a line's position — while quads, borders and images are drawn
//! wherever the layout puts them, fractions included. A layout that glides
//! by fractions of a pixel therefore moves its boxes smoothly and its text in
//! whole-pixel jumps, a little out of step with them: the text visibly snaps
//! against its own background. Moving the layout itself in whole device
//! pixels puts everything on the same steps, which is what a browser does
//! when it snaps its boxes to the pixel grid.
//!
//! Layout is measured in logical pixels, and nothing iced hands a widget in
//! `layout` says how large a device pixel is, so the scale factor is kept
//! here, process-wide. `iced_texture_cache`'s renderer sets it on every frame
//! it presents: the factor the frame was drawn at, the application's own
//! scale included. The window's `Rescaled` event is no substitute — it
//! carries the window's factor without the application's, and would put the
//! grid off the device pixels whenever the two differ. An application on
//! another renderer calls [`set_scale_factor`] itself.
//!
//! With several windows at different scale factors the last one presented
//! wins; a widget that can reach its own renderer's factor should prefer it,
//! as `iced_luminate`'s animated text does.

use std::sync::atomic::{AtomicU32, Ordering};

/// `1.0_f32.to_bits()`: until a frame says otherwise, a device pixel is a
/// logical one.
static SCALE_FACTOR: AtomicU32 = AtomicU32::new(0x3F80_0000);

/// The number of device pixels per logical pixel, as last set.
#[must_use]
pub fn scale_factor() -> f32 {
    f32::from_bits(SCALE_FACTOR.load(Ordering::Relaxed))
}

/// Records how many device pixels there are per logical pixel. Anything not
/// finite and positive is ignored.
pub fn set_scale_factor(scale_factor: f32) {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        SCALE_FACTOR.store(scale_factor.to_bits(), Ordering::Relaxed);
    }
}

/// `value` moved to a whole number of device pixels away from `target`, at
/// `scale_factor` device pixels per logical one.
///
/// The grid is anchored at the target, for the reason the kit anchors every
/// grid there: a value that has arrived is exactly its target, not the
/// nearest pixel to it, so snapping a motion never moves where it rests.
/// Only the way there is quantised: every frame of it differs from the next
/// by whole device pixels, the steps text takes anyway.
#[must_use]
pub fn snap_toward(value: f32, target: f32, scale_factor: f32) -> f32 {
    let sane = value.is_finite() && target.is_finite() && scale_factor.is_finite();

    if sane && scale_factor > 0.0 {
        ((value - target) * scale_factor).round() / scale_factor + target
    } else {
        value
    }
}

/// `value`, on its way from `from` to `target`, moved to a whole number of
/// device pixels from whichever of the two it is nearer.
///
/// A grid anchored at the target alone ([`snap_toward`]) is off the start by
/// a fraction of a pixel whenever the distance is not whole device pixels,
/// and the first frame of the motion jumps by that fraction — on the frame
/// the eye is watching for the motion to begin, while it is still slow.
/// Anchored at the start for the first half and at the target for the
/// second, the motion leaves exactly from where it rested and lands exactly
/// where it is going, and the one step that is not a whole pixel falls in
/// the middle, where the motion is fastest and a fraction goes unseen.
#[must_use]
pub fn snap_between(value: f32, from: f32, target: f32, scale_factor: f32) -> f32 {
    let anchor = if from.is_finite() && (value - from).abs() < (value - target).abs() {
        from
    } else {
        target
    };

    snap_toward(value, anchor, scale_factor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_snapped_value_is_whole_device_pixels_from_its_target() {
        for scale_factor in [1.0_f32, 1.25, 1.5, 2.0] {
            for tenth in -300..300 {
                let value = 18.3 + tenth as f32 / 10.0;
                let snapped = snap_toward(value, 18.3, scale_factor);
                let steps = (snapped - 18.3) * scale_factor;

                assert!(
                    (steps - steps.round()).abs() < 1e-3,
                    "{value} at {scale_factor}x lands {steps} device px from the target"
                );
                assert!(
                    (snapped - value).abs() * scale_factor <= 0.5 + 1e-3,
                    "moved by more than half a device pixel"
                );
            }
        }
    }

    #[test]
    fn a_value_at_its_target_is_left_exactly_there() {
        for target in [0.0, 5.0, 18.3, 27.77] {
            assert_eq!(snap_toward(target, target, 1.25), target);
        }
    }

    /// The first half of a snapped motion is whole pixels from where it
    /// started, the second whole pixels from where it ends, and both ends
    /// are exact.
    #[test]
    fn a_motion_snapped_between_its_ends_leaves_and_lands_exactly() {
        let (from, target, scale) = (40.0_f32, 18.3_f32, 1.25_f32);

        assert_eq!(snap_between(from, from, target, scale), from);
        assert_eq!(snap_between(target, from, target, scale), target);

        for hundredth in 0..=100 {
            let value = from + (target - from) * hundredth as f32 / 100.0;
            let snapped = snap_between(value, from, target, scale);
            let anchor = if (value - from).abs() < (value - target).abs() {
                from
            } else {
                target
            };
            let steps = (snapped - anchor) * scale;

            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "{value} landed {steps} device px from {anchor}"
            );
        }

        assert_eq!(
            snap_between(3.3, f32::NAN, 10.0, 1.0),
            snap_toward(3.3, 10.0, 1.0),
            "an unknown start falls back to the target"
        );
    }

    #[test]
    fn nonsense_is_left_alone() {
        assert!(snap_toward(f32::NAN, 1.0, 1.0).is_nan());
        assert_eq!(snap_toward(3.3, f32::INFINITY, 1.0), 3.3);
        assert_eq!(snap_toward(3.3, 1.0, 0.0), 3.3);
    }
}
