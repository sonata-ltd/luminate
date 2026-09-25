use iced::font::Weight;
use iced::widget::{container, row};
use iced::{Alignment, Color, Length, Padding};
use iced_animate::widget::shape;
use iced_luminate::iced::widget::column;
use iced_luminate::router::{Page, Registry};
use iced_luminate::theme::typography::styled_text;
use iced_luminate::{
    Element, Luminate, Renderer, Theme,
    theme::typography::{DisplaySize, TextSize, TextStyle},
};
use iced_page_router::Action;

use crate::hero::Hero;

pub(crate) struct Typography {
    luminate: Luminate,
}

const DISPLAY_SIZES: [DisplaySize; 6] = [
    DisplaySize::Xxl,
    DisplaySize::Xl,
    DisplaySize::Lg,
    DisplaySize::Md,
    DisplaySize::Sm,
    DisplaySize::Xs,
];

const TEXT_SIZES: [TextSize; 5] = [
    TextSize::Xl,
    TextSize::Lg,
    TextSize::Md,
    TextSize::Sm,
    TextSize::Xs,
];

#[derive(Debug, Clone)]
pub(crate) enum Message {}

impl Page for Typography {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
        }
    }

    fn update(&mut self, _: Self::Message) -> iced_page_router::Action<Self::Message> {
        Action::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let kit = &self.luminate;

        Hero::new(
            &self.luminate,
            "Typography",
            "Type styles, fonts and scale",
            container(
                column(
                    DISPLAY_SIZES
                        .iter()
                        .map(|size| display_style_component(kit, *size, Weight::Normal)),
                )
                .extend(
                    TEXT_SIZES
                        .iter()
                        .map(|size| text_style_component(kit, *size, Weight::Normal)),
                )
                .width(450)
                .padding(Padding::from(15))
                .spacing(40),
            )
            .center_x(Length::Fill),
        )
        .build()
    }
}

fn divider<'a>() -> Element<'a, Message> {
    shape()
        .width(Length::Fill)
        .height(1)
        .fill(Color::from_rgb8(230, 231, 232))
        .into()
}

fn display_style_component<'a>(
    kit: &Luminate,
    size: DisplaySize,
    weight: Weight,
) -> Element<'a, Message> {
    let palette = kit.theme().palette;

    let style_name = get_display_style_name(size);
    let draft_style = TextStyle::display(size, Weight::Normal);
    let style = TextStyle::display(size, weight);

    column![
        row![
            column![
                styled_text(style_name, TextStyle::text(TextSize::Md, Weight::Medium))
                    .color(palette.text_primary),
                styled_text(
                    format!("{} / {}", draft_style.size, draft_style.line_height),
                    TextStyle::text(TextSize::Md, Weight::Normal)
                )
                .color(palette.text_secondary),
            ]
            .height(Length::Shrink)
            .width(Length::Fill),
            styled_text("Aa", style).color(palette.text_primary)
        ]
        .align_y(Alignment::End),
        divider()
    ]
    .spacing(5)
    .into()
}

fn get_display_style_name<'a>(size: DisplaySize) -> &'a str {
    match size {
        DisplaySize::Xxl => "Display 2XL",
        DisplaySize::Xl => "Display XL",
        DisplaySize::Lg => "Display LG",
        DisplaySize::Md => "Display MD",
        DisplaySize::Sm => "Display SM",
        DisplaySize::Xs => "Display XS",
    }
}

fn text_style_component<'a>(
    kit: &Luminate,
    size: TextSize,
    weight: Weight,
) -> Element<'a, Message> {
    let palette = kit.theme().palette;

    let style_name = get_text_style_name(size);
    let draft_style = TextStyle::text(size, Weight::Normal);
    let style = TextStyle::text(size, weight);

    column![
        row![
            column![
                styled_text(style_name, TextStyle::text(TextSize::Md, Weight::Medium))
                    .color(palette.text_primary),
                styled_text(
                    format!("{} / {}", draft_style.size, draft_style.line_height),
                    TextStyle::text(TextSize::Md, Weight::Normal)
                )
                .color(palette.text_secondary),
            ]
            .height(Length::Shrink)
            .width(Length::Fill),
            styled_text("Aa", style).color(palette.text_primary)
        ]
        .align_y(Alignment::End),
        divider()
    ]
    .spacing(5)
    .into()
}

fn get_text_style_name<'a>(size: TextSize) -> &'a str {
    match size {
        TextSize::Xl => "Text XL",
        TextSize::Lg => "Text LG",
        TextSize::Md => "Text MD",
        TextSize::Sm => "Text SM",
        TextSize::Xs => "Text XS",
    }
}
