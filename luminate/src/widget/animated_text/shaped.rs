//! The shaped buffer behind [`WeightedText`](super::WeightedText), and the
//! quantisation that keeps the number of font instances behind it finite.

use std::sync::Arc;

use iced::Size;
use iced::advanced::graphics::text;
use iced::advanced::text::{Alignment, Shaping, Wrapping};
use iced::font::Style;

use crate::theme::typography::{FAMILY, TextStyle};

/// The lightest weight the `wght` axis carries.
pub const MIN: f32 = 100.0;

/// The heaviest weight the `wght` axis carries.
pub const MAX: f32 = 900.0;

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
pub fn default_step(size: f32) -> f32 {
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
/// and half a step is invisible by construction of [`default_step`].
#[must_use]
pub fn quantize(weight: f32, target: f32, step: f32) -> u16 {
    let target = if target.is_finite() {
        target.clamp(MIN, MAX)
    } else {
        400.0
    };

    if !weight.is_finite() {
        return target as u16;
    }

    let step = if step.is_finite() && step >= 1.0 {
        step
    } else {
        1.0
    };

    let steps = ((weight - target) / step).round();

    steps.mul_add(step, target).clamp(MIN, MAX) as u16
}

/// A `cosmic_text::Buffer` and the inputs it was shaped from.
///
/// The buffer sits behind an `Arc` because that is what
/// [`Raw`](iced::advanced::graphics::text::Raw) holds a `Weak` of: the
/// renderer borrows the buffer at paint time, and the widget's `tree::State`
/// is what keeps it alive until then.
#[derive(Debug)]
pub(super) struct Shaped {
    buffer: Arc<text::cosmic_text::Buffer>,
    /// `None` until the first shape, so that it always runs.
    inputs: Option<Inputs>,
    min_bounds: Size,
}

/// Everything a shape depends on. Unchanged inputs mean the buffer stands.
#[derive(Debug, PartialEq)]
struct Inputs {
    content: String,
    style: TextStyle,
    weight: u16,
    bounds: Size,
    align_x: Alignment,
    wrapping: Wrapping,
    fonts: text::Version,
}

impl Default for Shaped {
    fn default() -> Self {
        Self::new()
    }
}

impl Shaped {
    /// An empty buffer, shaped by the first [`update`](Self::update).
    #[must_use]
    pub(super) fn new() -> Self {
        Self {
            buffer: Arc::new(text::cosmic_text::Buffer::new_empty(
                text::cosmic_text::Metrics::new(1.0, 1.0),
            )),
            inputs: None,
            min_bounds: Size::ZERO,
        }
    }

    /// The buffer as it currently stands.
    #[must_use]
    pub(super) fn buffer(&self) -> &Arc<text::cosmic_text::Buffer> {
        &self.buffer
    }

    /// Reshapes at `weight` unless the buffer already stands at exactly these
    /// inputs, and returns the resulting minimum bounds.
    ///
    /// Called from whichever of `layout` and `draw` reaches the buffer first
    /// in a frame, so a moving weight costs one shape per frame and not two.
    pub(super) fn update(
        &mut self,
        content: &str,
        style: TextStyle,
        weight: u16,
        bounds: Size,
        align_x: Alignment,
        wrapping: Wrapping,
    ) -> Size {
        let mut font_system = text::font_system()
            .write()
            .expect("the font system is not poisoned");

        let inputs = Inputs {
            content: content.to_owned(),
            style,
            weight,
            bounds,
            align_x,
            wrapping,
            fonts: font_system.version(),
        };

        if self.inputs.as_ref() == Some(&inputs) {
            return self.min_bounds;
        }

        let metrics = text::cosmic_text::Metrics::new(style.size, style.line_height);

        // The renderer may still hold a `Weak` to the buffer this frame's
        // predecessor was drawn from; then this frame gets its own rather
        // than reshaping one out from under it.
        if Arc::get_mut(&mut self.buffer).is_none() {
            self.buffer = Arc::new(text::cosmic_text::Buffer::new_empty(metrics));
        }

        let buffer = Arc::get_mut(&mut self.buffer).expect("the buffer is not shared");
        let font_system = font_system.raw();

        buffer.set_metrics_and_size(
            font_system,
            metrics,
            Some(bounds.width),
            Some(bounds.height),
        );
        buffer.set_wrap(font_system, text::to_wrap(wrapping));
        buffer.set_text(
            font_system,
            content,
            &attributes(style, weight),
            // `Shaping::Advanced` for the reason `TextStyle::apply` gives:
            // the basic shaper reads advances from the face's *default*
            // instance and skips `kern`, which would set every weight on
            // Regular's spacing however heavy the glyphs are drawn.
            text::to_shaping(Shaping::Advanced, content),
            None,
        );

        self.min_bounds = text::align(buffer, font_system, align_x);
        self.inputs = Some(inputs);

        self.min_bounds
    }
}

/// The cosmic-text attributes for `style` at `weight`.
///
/// The family is always [`FAMILY`], so face matching stays inside the bundled
/// Inter: a weight nothing declares exactly — 437, say — picks the nearest
/// declared copy of the *variable* face, which is then rendered by setting its
/// `wght` axis, instead of falling out to whatever other installed family
/// happens to declare 437.
fn attributes(style: TextStyle, weight: u16) -> text::cosmic_text::Attrs<'static> {
    text::cosmic_text::Attrs::new()
        .family(text::cosmic_text::Family::Name(FAMILY))
        .weight(text::cosmic_text::Weight(weight))
        .style(match style.slant {
            Style::Normal => text::cosmic_text::Style::Normal,
            Style::Italic => text::cosmic_text::Style::Italic,
            Style::Oblique => text::cosmic_text::Style::Oblique,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_settled_weight_quantises_to_itself() {
        // The end of every animation must be exact: the rest weight is the
        // one `DECLARED_WEIGHTS` declares and the static path would draw.
        for target in [400.0, 500.0, 600.0, 700.0] {
            assert_eq!(quantize(target, target, 18.0), target as u16);
        }
    }

    #[test]
    fn the_grid_is_anchored_at_the_target() {
        assert_eq!(quantize(582.0, 600.0, 18.0), 582);
        assert_eq!(quantize(564.0, 600.0, 18.0), 564);
    }

    #[test]
    fn values_within_a_step_collapse() {
        let a = quantize(541.0, 600.0, 18.0);
        let b = quantize(546.0, 600.0, 18.0);

        assert_eq!(a, b, "half a step either way is the same font instance");
    }

    #[test]
    fn weights_outside_the_axis_are_clamped() {
        assert_eq!(quantize(2000.0, 600.0, 18.0), 900);
        assert_eq!(quantize(-5.0, 600.0, 18.0), 100);
    }

    /// A weight the caller cannot mean falls back to the one place that is
    /// certainly meaningful — where the animation is going — rather than to
    /// an end of the axis. `Anim` sanitises its own tracks, so this only ever
    /// catches a hand-built handle.
    #[test]
    fn a_non_finite_weight_rests_at_the_target() {
        assert_eq!(quantize(f32::NAN, 600.0, 18.0), 600);
        assert_eq!(quantize(f32::INFINITY, 600.0, 18.0), 600);
        assert_eq!(quantize(f32::NEG_INFINITY, 600.0, 18.0), 600);
        assert_eq!(
            quantize(f32::NAN, f32::NAN, 18.0),
            400,
            "and a target that is not a weight either falls back to Regular"
        );
    }

    #[test]
    fn a_degenerate_step_still_produces_a_weight() {
        assert_eq!(quantize(550.0, 600.0, 0.0), 550);
        assert_eq!(quantize(550.0, 600.0, f32::NAN), 550);
    }

    #[test]
    fn bigger_text_gets_a_finer_step() {
        assert!(
            default_step(72.0) < default_step(14.0),
            "a display size shows steps a caption hides"
        );
    }

    #[test]
    fn the_step_stays_in_range() {
        for size in [1.0, 12.0, 14.0, 16.0, 36.0, 72.0, 400.0] {
            let step = default_step(size);
            assert!((2.0..=20.0).contains(&step), "{size} px gave {step}");
        }

        assert_eq!(default_step(f32::NAN), 20.0);
        assert_eq!(default_step(0.0), 20.0);
    }
}
