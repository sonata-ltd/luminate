//! The page header: the isometric icon, the page's name and one line saying
//! what the page shows, above the page's own content.

use iced::Padding;
use iced::widget::container;
use iced_luminate::iced::font::Weight;
use iced_luminate::iced::widget::{column, text};
use iced_luminate::iced::{Alignment, Length};
use iced_luminate::theme::TextClass;
use iced_luminate::theme::typography::{DisplaySize, TextSize, TextStyle, styled_text};
use iced_luminate::{Element, Luminate};

/// The gap between the icon, the title and the subtitle.
const HEADER_SPACING: f32 = 6.0;

pub(crate) struct Hero<'a, M> {
    pub luminate: &'a Luminate,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub content: Element<'a, M>,
}

impl<'a, M: 'a> Hero<'a, M> {
    pub(crate) fn new(
        luminate: &'a Luminate,
        title: &'a str,
        subtitle: &'a str,
        content: impl Into<Element<'a, M>>,
    ) -> Self {
        Self {
            luminate,
            title,
            subtitle,
            content: content.into(),
        }
    }
    /// Wraps `content` in the shared page header.
    pub(crate) fn build(self) -> Element<'a, M> {
        let theme = self.luminate.theme();

        let heading = column![
            styled_text(
                self.title,
                TextStyle::display(DisplaySize::Xs, Weight::Semibold)
            ),
            styled_text(self.subtitle, TextStyle::text(TextSize::Md, Weight::Normal)).class(
                TextClass::Custom(Box::new(|theme: &iced_luminate::Theme| text::Style {
                    color: Some(theme.palette.text_secondary),
                }))
            ),
        ]
        .padding(Padding {
            top: theme.spacing.xl5,
            bottom: theme.spacing.xl4,
            ..Default::default()
        })
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .spacing(HEADER_SPACING);

        let content = self.content;

        container(
            column![heading, content]
                .width(theme.width.sm)
                .padding(theme.spacing.xl),
        )
        .center(Length::Fill)
        .into()
    }
}
