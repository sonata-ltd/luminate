//! Text inputs, `NavigationOptions`, `Shared` state and `Lifecycle::Suspend`.
//!
//! This page is **suspended**, not dropped, when the user leaves it: the
//! instance stays alive, `on_suspend`/`on_resume` run, and `on_resume`
//! re-reads the draft that the nested sidebar's copy of this page may have
//! edited through the same [`Registry`] entry. Compare
//! [`snapshot`](crate::pages::snapshot), which is dropped and restored.

use iced_luminate::descriptor::Input;
use iced_luminate::iced::widget::column;
use iced_luminate::router::{Action, Lifecycle, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;

/// Messages of the inputs page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// The draft changed.
    InputChanged(String),
}

/// One input bound to a shared draft.
pub(crate) struct InputsPage {
    luminate: Luminate,
    draft: String,
}

impl Page for InputsPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    /// Keep the instance while another page is shown.
    const LIFECYCLE: Lifecycle = Lifecycle::Suspend;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            draft: String::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::InputChanged(value) => {
                self.draft = value;
            }
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        Hero::new(
            luminate,
            "Inputs",
            "Text fields, labels and validation",
            inputs(&self.luminate, &self.draft),
        )
        .build()
    }
}

fn inputs<'a>(luminate: &Luminate, draft: &'a str) -> Element<'a, Message> {
    column![
        luminate.input(Input::new("Type something", draft).on_input(Message::InputChanged)),
        luminate.input(
            Input::new("Type something", draft)
                .label("Draft")
                .on_input(Message::InputChanged)
        ),
        luminate.input(
            Input::new("Type something", draft)
                .label("Draft")
                .hint("A hint to help user")
                .on_input(Message::InputChanged)
        )
    ]
    .spacing(luminate.theme().spacing.xl)
    .into()
}
