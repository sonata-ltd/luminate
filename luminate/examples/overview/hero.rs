//! The page header: the isometric icon, the page's name and one line saying
//! what the page shows, above the page's own content.

use iced::border::Radius;
use iced::widget::{Space, container, row};
use iced::{Border, Padding};
use iced_luminate::iced::font::Weight;
use iced_luminate::iced::widget::{column, text};
use iced_luminate::iced::{Alignment, Length};
use iced_luminate::theme::TextClass;
use iced_luminate::theme::palette::ColorScale;
use iced_luminate::theme::typography::{DisplaySize, TextSize, TextStyle, styled_text};
use iced_luminate::{Element, Luminate};

use crate::iso::{self, Scene};

/// The gap between the icon, the title and the subtitle.
const HEADER_SPACING: f32 = 6.0;

pub(crate) struct Hero<'a, M> {
    pub luminate: &'a Luminate,
    pub scene: &'static Scene,
    pub title: &'a str,
    pub subtitle: &'a str,
    pub showcase: Option<Element<'a, M>>,
    pub content: Element<'a, M>,
}

impl<'a, M: 'a> Hero<'a, M> {
    pub(crate) fn new(
        luminate: &'a Luminate,
        scene: &'static Scene,
        title: &'a str,
        subtitle: &'a str,
        content: impl Into<Element<'a, M>>,
    ) -> Self {
        Self {
            luminate,
            scene,
            title,
            subtitle,
            showcase: None,
            content: content.into(),
        }
    }

    pub(crate) fn showcase(mut self, content: Element<'a, M>) -> Self {
        self.showcase = Some(content);
        self
    }

    pub(crate) fn view(self) -> Element<'a, M> {
        let theme = self.luminate.theme();

        let heading = column![
            iso::icon(self.scene),
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
            // bottom: theme.spacing.xl4,
            ..Default::default()
        })
        .width(Length::Fill)
        .align_x(Alignment::Center)
        .spacing(HEADER_SPACING);

        let showcase: Element<'a, M> = if self.showcase.is_some() {
            container(
                row(self.showcase)
                    .padding(Padding::from(theme.spacing.xl4))
                    .spacing(theme.spacing.md),
            )
            .center_x(Length::Fill)
            .style(|_| container::Style {
                background: Some(iced::Background::Color(ColorScale::GRAY.s30)),
                border: Border {
                    color: ColorScale::GRAY.s100,
                    radius: Radius::from(theme.radius.xl3),
                    width: 1.0,
                },
                ..Default::default()
            })
            .into()
        } else {
            Space::new().into()
        };

        let content = self.content;

        container(
            column![heading, showcase, content]
                .width(theme.width.sm)
                .padding(theme.spacing.xl)
                .spacing(theme.spacing.xl5),
        )
        .center(Length::Fill)
        .into()
    }
}

pub(crate) fn category<'a, Message: 'a>(
    luminate: &'a Luminate,
    label: &'a str,
    content: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![
        styled_text(label, TextStyle::text(TextSize::Xs, Weight::Normal)),
        content.into()
    ]
    .width(Length::Fill)
    .spacing(luminate.theme().spacing.lg)
    .align_x(Alignment::Center)
    .into()
}
