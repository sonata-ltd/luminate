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

use crate::hero::{Hero, category};
use crate::iso::scenes_flat;

/// Messages of the inputs page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// The mail draft changed.
    Mail(String),
    /// The search draft changed.
    Search(String),
    /// The name draft changed.
    Name(String),
    /// The comment draft changed.
    Comment(String),
}

/// One input bound to a shared draft.
pub(crate) struct InputsPage {
    luminate: Luminate,
    mail_draft: String,
    search_draft: String,
    name_draft: String,
    comment_draft: String,
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
            mail_draft: String::from("mail@domain.com"),
            search_draft: String::new(),
            name_draft: String::new(),
            comment_draft: String::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Mail(value) => {
                self.mail_draft = value;
            }
            Message::Search(value) => {
                self.search_draft = value;
            }
            Message::Name(value) => {
                self.name_draft = value;
            }
            Message::Comment(value) => {
                self.comment_draft = value;
            }
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        let error_message = (!self.mail_draft.contains('@')).then_some("Needs an @");

        Hero::new(
            luminate,
            &scenes_flat::INPUTS,
            "Inputs",
            "Text fields, labels and validation",
            example(self),
        )
        .showcase(
            luminate.input(
                Input::new("main@domain.com", &self.mail_draft)
                    .hint("The error bubble goes away once the value has an @")
                    .error(error_message)
                    .on_input(Message::Mail),
            ),
        )
        .view()
    }
}

fn example(data: &InputsPage) -> Element<'_, Message> {
    let luminate = &data.luminate;

    category(
        luminate,
        "Variants",
        column![
            luminate.input(
                Input::new("Search components", &data.search_draft).on_input(Message::Search)
            ),
            luminate.input(
                Input::new("Name", &data.name_draft)
                    .label("Display Name")
                    .on_input(Message::Name)
            ),
            luminate.input(
                Input::new("Your message", &data.comment_draft)
                    .label("Comment")
                    .hint("Your comment will be visible to all users")
                    .on_input(Message::Comment)
            ),
        ]
        .spacing(luminate.theme().spacing.xl),
    )
}
