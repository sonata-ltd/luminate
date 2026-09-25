//! Button hierarchies, and `Action::navigate_with`: "Go to inputs" carries
//! `NavigationOptions` to the inputs page.

use iced::Length;
use iced::widget::{container, row};
use iced_luminate::descriptor::{Button, ButtonHierarchy};
use iced_luminate::iced::widget::column;
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;
use crate::iso::scenes_flat;

const ICON_SVG: &str = r#"<svg width="24" height="24" viewBox="0 0 24 24" fill="none" xmlns="http://www.w3.org/2000/svg">
<path d="M5 4.98963C5 4.01847 5 3.53289 5.20249 3.26522C5.37889 3.03203 5.64852 2.88773 5.9404 2.8703C6.27544 2.8503 6.67946 3.11965 7.48752 3.65835L18.0031 10.6687C18.6708 11.1139 19.0046 11.3364 19.1209 11.6169C19.2227 11.8622 19.2227 12.1378 19.1209 12.3831C19.0046 12.6636 18.6708 12.8862 18.0031 13.3313L7.48752 20.3417C6.67946 20.8804 6.27544 21.1497 5.9404 21.1297C5.64852 21.1123 5.37889 20.968 5.20249 20.7348C5 20.4671 5 19.9815 5 19.0104V4.98963Z" fill="black" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"/>
</svg>"#;

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
        let theme = luminate.theme();

        let icon = ICON_SVG;
        let column_spacing = theme.spacing.lg;

        Hero::new(
            luminate,
            &scenes_flat::BUTTONS,
            "Buttons",
            "Three hierarchies of one control",
            container(
                row![
                    column![
                        luminate.button(Button::with_icon(icon).on_press(Message::ActionPressed)),
                        luminate.button(
                            Button::with_icon(icon)
                                .hierarchy(ButtonHierarchy::Secondary)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::with_icon(icon)
                                .hierarchy(ButtonHierarchy::Tertiary)
                                .on_press(Message::ActionPressed)
                        ),
                    ]
                    .spacing(column_spacing),
                    column![
                        luminate.button(Button::new("Action").on_press(Message::ActionPressed)),
                        luminate.button(
                            Button::new("Action")
                                .hierarchy(ButtonHierarchy::Secondary)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::new("Action")
                                .hierarchy(ButtonHierarchy::Tertiary)
                                .on_press(Message::ActionPressed)
                        ),
                    ]
                    .spacing(column_spacing),
                    column![
                        luminate.button(
                            Button::new("Action")
                                .icon(icon)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::new("Action")
                                .icon(icon)
                                .hierarchy(ButtonHierarchy::Secondary)
                                .on_press(Message::ActionPressed)
                        ),
                        luminate.button(
                            Button::new("Action")
                                .icon(icon)
                                .hierarchy(ButtonHierarchy::Tertiary)
                                .on_press(Message::ActionPressed)
                        ),
                    ]
                    .spacing(column_spacing),
                ]
                .spacing(theme.spacing.lg),
            )
            .center_x(Length::Fill),
        )
        .build()
    }
}
