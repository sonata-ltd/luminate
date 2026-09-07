//! Button hierarchies, and `Action::navigate_with`: "Go to inputs" carries
//! `NavigationOptions` to the inputs page.

use iced_luminate::descriptor::{Button, ButtonHierarchy};
use iced_luminate::iced::widget::column;
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::pages::inputs::{InputsPage, Options as InputsOptions};

/// Messages of the buttons page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A plain action button was pressed.
    ActionPressed,
    /// Navigate to the inputs page with the input locked.
    NavigateInputs,
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
            Message::NavigateInputs => {
                Action::navigate_with::<InputsPage>(InputsOptions { locked: true })
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        column![
            luminate.button(Button::new("Go To Inputs").on_press(Message::NavigateInputs)),
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
        .spacing(15)
        .into()
    }
}
