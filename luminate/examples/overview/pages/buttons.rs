//! Button hierarchies, and `Action::navigate_with`: "Go to inputs" carries
//! `NavigationOptions` to the inputs page.

use iced::Alignment;
use iced::widget::row;
use iced_luminate::descriptor::{Button, ButtonHierarchy, ButtonSize};
use iced_luminate::iced::widget::column;
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::assets::{ARROW_RIGHT, DOWNLOAD_03, SETTINGS_02};
use crate::hero::{Hero, category};
use crate::iso::scenes_flat;

/// Messages of the buttons page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A plain action button was pressed.
    ActionPressed,
}

/// Three hierarchies of the same button.
pub(crate) struct ButtonsPage {
    luminate: Luminate,
}

impl Page for ButtonsPage {
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

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::ActionPressed => {
                eprintln!("overview: action pressed");
                Action::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        Hero::new(
            luminate,
            &scenes_flat::BUTTONS,
            "Buttons",
            "Three hierarchies of one control",
            example(luminate),
        )
        .showcase(luminate.button(Button::new("Action").on_press(Message::ActionPressed)))
        .view()
    }
}

fn example(luminate: &Luminate) -> Element<'_, Message> {
    column![
        category(
            luminate,
            "Variants",
            row![
                luminate.button(Button::new("Primary").on_press(Message::ActionPressed)),
                luminate.button(
                    Button::new("Secondary")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press(Message::ActionPressed)
                ),
                luminate.button(
                    Button::new("Tertiary")
                        .hierarchy(ButtonHierarchy::Tertiary)
                        .on_press(Message::ActionPressed)
                ),
                luminate.button(
                    Button::new("Destructive")
                        .hierarchy(ButtonHierarchy::Destructive)
                        .on_press(Message::ActionPressed)
                ),
            ]
            .spacing(luminate.theme().spacing.lg)
        ),
        category(
            luminate,
            "Sizes",
            row![
                luminate.button(
                    Button::new("Small")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press(Message::ActionPressed)
                ),
                luminate.button(
                    Button::new("Medium")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .size(ButtonSize::Medium)
                        .on_press(Message::ActionPressed)
                ),
                luminate.button(
                    Button::new("Large")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .size(ButtonSize::Large)
                        .on_press(Message::ActionPressed)
                ),
            ]
            .align_y(Alignment::Center)
            .spacing(luminate.theme().spacing.lg)
        ),
        category(
            luminate,
            "With icons",
            row![
                luminate.button(
                    Button::with_icon(DOWNLOAD_03.clone())
                        .label("Download")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press(Message::ActionPressed)
                ),
                luminate.button(
                    Button::with_icon(ARROW_RIGHT.clone())
                        .label("Next")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press(Message::ActionPressed)
                ),
                luminate.button(
                    Button::with_icon(SETTINGS_02.clone())
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press(Message::ActionPressed)
                ),
            ]
            .spacing(luminate.theme().spacing.lg)
        ),
    ]
    .spacing(luminate.theme().spacing.xl)
    .into()
}
