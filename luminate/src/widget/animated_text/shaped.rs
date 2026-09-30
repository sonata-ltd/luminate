//! The shaped buffer behind [`AnimatedText`](super::AnimatedText).

use std::sync::Arc;

use iced::Size;
use iced::advanced::graphics::text;
use iced::advanced::text::{Alignment, Shaping, Wrapping};
use iced::font::Style;

use crate::theme::typography::{FAMILY, TextStyle};

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
