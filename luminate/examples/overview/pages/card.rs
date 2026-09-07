//! `descriptor::Card`: a titled card whose body is a `Pager`, with cached
//! header and controls, and an optional `max_height`.
//!
//! The header and the controls row are wrapped in `Cached` by the card, so
//! sliding between steps composites two textures instead of re-drawing them.
//! Step 2's "Summary" field has no `on_input`: that is how a read-only input
//! looks.

use iced_luminate::descriptor::{Button, ButtonHierarchy, Card, Input};
use iced_luminate::iced::font::Weight;
use iced_luminate::iced::widget::{column, container, row, scrollable, space};
use iced_luminate::iced::{Length, Padding};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::texture::{Cached, TextureCache};
use iced_luminate::theme::typography::{TextSize, TextStyle, styled_text};
use iced_luminate::{Element, Luminate, Renderer, Theme};

/// Steps in the card's pager.
const STEPS: usize = 2;

/// Read-only rows that pad step 2 past the card's `max_height`.
const FILLER: [&str; 2] = ["Environment", "Arguments"];

/// Messages of the card page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Previous step.
    Back,
    /// Next step.
    Next,
    /// The name field changed.
    NameChanged(String),
    /// The notes field changed.
    NotesChanged(String),
    /// The max-height field changed (parsed as pixels; empty = unlimited).
    MaxHeightChanged(String),
}

/// A two-step form inside a card.
pub(crate) struct CardPage {
    luminate: Luminate,
    current: usize,
    name: String,
    notes: String,
    max_height: String,
    header_cache: TextureCache,
    controls_cache: TextureCache,
}

impl Page for CardPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            current: 0,
            name: String::new(),
            notes: String::new(),
            max_height: String::new(),
            header_cache: TextureCache::new(),
            controls_cache: TextureCache::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Back => self.current = self.current.saturating_sub(1),
            Message::Next => self.current = (self.current + 1).min(STEPS - 1),
            Message::NameChanged(value) => self.name = value,
            Message::NotesChanged(value) => self.notes = value,
            Message::MaxHeightChanged(value) => self.max_height = value,
        }
        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;
        let can_go_back = self.current > 0;
        let can_go_next = self.current + 1 < STEPS;

        let controls = Cached::new(
            self.controls_cache.clone(),
            row![
                space().width(Length::Fill),
                luminate.button(
                    Button::new("Cancel")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press_maybe(can_go_back.then_some(Message::Back))
                ),
                luminate.button(
                    Button::new("Apply").on_press_maybe(can_go_next.then_some(Message::Next))
                ),
            ]
            .spacing(10)
            .padding(15),
        );

        let mut card = Card::new("Add External Runtime")
            .header_cache(self.header_cache.clone())
            .width(418)
            .pages([self.step_1(), self.step_2()], self.current)
            .controls(controls);
        if let Ok(height) = self.max_height.parse::<f32>() {
            card = card.max_height(height);
        }

        column![
            container(
                luminate.input(
                    Input::new("500", &self.max_height)
                        .label("Max height")
                        .on_input(Message::MaxHeightChanged)
                )
            )
            .width(160),
            container(luminate.card(card)).center(Length::Fill),
        ]
        .spacing(15)
        .into()
    }
}

impl CardPage {
    fn step_1(&self) -> Element<'_, Message> {
        column![
            self.luminate.input(
                Input::new("Name", &self.name)
                    .label("Path To Binary")
                    .hint("Path to runtime executable file")
                    .on_input(Message::NameChanged),
            )
        ]
        .padding(15)
        .into()
    }

    fn step_2(&self) -> Element<'_, Message> {
        scrollable(
            column![
                column![
                    styled_text("Step 2", TextStyle::text(TextSize::Md, Weight::Semibold))
                        .width(Length::Fill)
                        .center(),
                    styled_text(
                        "Review the values before applying them.",
                        TextStyle::text(TextSize::Sm, Weight::Normal)
                    )
                    .width(Length::Fill)
                    .center(),
                ]
                .padding(Padding::default().top(10))
                .spacing(10),
                column(
                    [
                        // No `on_input`: a read-only field.
                        self.luminate
                            .input(Input::new("Summary", &self.name).label("Summary")),
                        self.luminate.input(
                            Input::new("Notes", &self.notes)
                                .label("Notes")
                                .on_input(Message::NotesChanged),
                        ),
                    ]
                    .into_iter()
                    .chain(
                        FILLER.iter().map(|label| {
                            self.luminate.input(Input::new(label, "").label(label))
                        })
                    ),
                )
                .spacing(15),
            ]
            .spacing(20)
            .padding(15),
        )
        .into()
    }
}
