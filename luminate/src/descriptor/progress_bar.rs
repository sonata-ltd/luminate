//! The progress-bar descriptor.

use iced::Length;
use iced_animate::{Curve, curves};
use std::ops::RangeInclusive;

/// A progress bar with optional percentage text.
///
/// ```
/// use iced_luminate::{Luminate, Element, descriptor::ProgressBar};
/// let kit = Luminate::new();
/// let bar: Element<'_, ()> = kit.progress_bar(
///     ProgressBar::new(0.0..=100.0, 30.0).show_progress(true),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct ProgressBar {
    /// Inclusive range of possible values.
    pub range: RangeInclusive<f32>,
    /// Current value, clamped when displayed.
    pub value: f32,
    /// Length along the direction of progress (default `Fill`).
    pub length: Length,
    /// Optional thickness; otherwise taken from the theme.
    pub girth: Option<Length>,
    /// Whether progress travels bottom to top (default false).
    pub vertical: bool,
    /// Whether the classic macOS animation is shown (default false).
    pub indeterminate: bool,
    /// Whether the error variant is shown (default false).
    pub errored: bool,
    /// Whether a percentage is shown beside the bar (default false).
    /// Hidden for indeterminate bars. An error without text fills the track.
    pub show_progress: bool,
    /// Whether changes of the filled fraction are animated (default false).
    pub smoothed: bool,
    /// Curve used when smoothing is enabled (default [`curves::QUICK`]).
    pub smoothing_curve: Curve,
}

impl ProgressBar {
    /// Creates a determinate bar, following iced's range/value API.
    #[must_use]
    pub fn new(range: RangeInclusive<f32>, value: f32) -> Self {
        Self {
            range,
            value,
            length: Length::Fill,
            girth: None,
            vertical: false,
            indeterminate: false,
            errored: false,
            show_progress: false,
            smoothed: false,
            smoothing_curve: curves::QUICK,
        }
    }

    /// Sets the length.
    #[must_use]
    pub fn length(mut self, length: impl Into<Length>) -> Self {
        self.length = length.into();
        self
    }

    /// Overrides the theme's thickness.
    #[must_use]
    pub fn girth(mut self, girth: impl Into<Length>) -> Self {
        self.girth = Some(girth.into());
        self
    }

    /// Makes progress travel bottom to top; the label sits below the bar.
    #[must_use]
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// Enables the indeterminate animation and hides the percentage.
    #[must_use]
    pub fn indeterminate(mut self, enabled: bool) -> Self {
        self.indeterminate = enabled;
        self
    }

    /// Enables the error variant, taking precedence over indeterminate mode.
    #[must_use]
    pub fn errored(mut self, enabled: bool) -> Self {
        self.errored = enabled;
        self
    }

    /// Shows the clamped, normalized percentage beside the bar.
    #[must_use]
    pub fn show_progress(mut self, enabled: bool) -> Self {
        self.show_progress = enabled;
        self
    }

    /// Smooths changes of the filled fraction through Luminate's shared motion.
    /// The initial value appears immediately; percentage text shows the target.
    #[must_use]
    pub fn smoothed(mut self) -> Self {
        self.smoothed = true;
        self
    }

    /// Sets the smoothing curve. Call [`Self::smoothed`] to enable smoothing.
    #[must_use]
    pub fn smoothing_curve(mut self, curve: Curve) -> Self {
        self.smoothing_curve = curve;
        self
    }
}
