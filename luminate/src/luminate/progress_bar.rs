//! [`Luminate::progress_bar`].

use crate::theme::typography::styled_text;
use crate::widget::progress_bar::{ProgressBar, fraction};
use crate::{Element, Luminate, descriptor};
use iced::widget::{column, row};

impl Luminate {
    /// Builds a progress bar, optionally with a styled percentage beside it.
    /// Errors without percentages fill the track, as in the design system.
    #[must_use]
    pub fn progress_bar<'a, M: 'a>(&self, descriptor: descriptor::ProgressBar) -> Element<'a, M> {
        let tokens = self.theme.progress_bar;
        let show = descriptor.show_progress && (!descriptor.indeterminate || descriptor.errored);
        let amount = fraction(&descriptor.range, descriptor.value);
        let (range, value) = if descriptor.errored && !show {
            (0.0..=1.0, 1.0)
        } else {
            (descriptor.range, descriptor.value)
        };
        let mut bar = ProgressBar::new(range, value)
            .length(if show {
                iced::Length::Fill
            } else {
                descriptor.length
            })
            .girth(descriptor.girth.unwrap_or(tokens.girth.into()))
            .indeterminate(descriptor.indeterminate)
            .errored(descriptor.errored);
        if descriptor.smoothed {
            bar = bar
                .smoothed()
                .smoothing_curve(descriptor.smoothing_curve)
                .motion(self.motion.clone());
        }
        if descriptor.vertical {
            bar = bar.vertical();
        }
        if !show {
            return bar.into();
        }
        let label = styled_text(format!("{:.0}%", amount * 100.0), tokens.text_style).style(
            |theme: &crate::Theme| iced::widget::text::Style {
                color: Some(theme.progress_bar.text),
            },
        );
        if descriptor.vertical {
            column![bar, label]
                .height(descriptor.length)
                .spacing(tokens.spacing)
                .align_x(iced::Center)
                .into()
        } else {
            row![bar, label]
                .width(descriptor.length)
                .spacing(tokens.spacing)
                .align_y(iced::Center)
                .into()
        }
    }
}
