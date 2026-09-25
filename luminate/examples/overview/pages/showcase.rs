//! The remaining descriptor surface: a standalone `Luminate::pager`, an
//! `Input` showing its error bubble, and a collapsible `Sidebar` with its
//! toggle wired to `Sidebar::collapsed`.

use crate::hero::Hero;
use iced_luminate::descriptor::{Axis, Button, ButtonHierarchy, Input, Pager, Sidebar};
use iced_luminate::iced::Length;
use iced_luminate::iced::widget::{column, container, row, text};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

/// Steps of the standalone pager.
const STEPS: usize = 3;

/// Messages of the showcase page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// The sidebar's toggle was pressed; carries the new collapsed state.
    SidebarToggled(bool),
    /// The address field changed.
    AddressChanged(String),
    /// Show this step of the pager.
    Step(usize),
}

/// A pager, an input with validation and a collapsible sidebar.
pub(crate) struct ShowcasePage {
    luminate: Luminate,
    collapsed: bool,
    address: String,
    step: usize,
}

impl Page for ShowcasePage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            collapsed: false,
            address: String::new(),
            step: 0,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::SidebarToggled(collapsed) => self.collapsed = collapsed,
            Message::AddressChanged(value) => self.address = value,
            Message::Step(step) => self.step = step.min(STEPS - 1),
        }
        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        // The sidebar: its toggle reports the new state, the page stores it
        // and passes it back through `collapsed`, so the width animates.
        let items: Vec<Element<'_, Message>> = (0..STEPS)
            .map(|step| {
                luminate.button(
                    Button::new(if step == self.step { "Current" } else { "Step" })
                        .width(Length::Fill)
                        .hierarchy(ButtonHierarchy::Tertiary)
                        .on_press(Message::Step(step)),
                )
            })
            .collect();
        let sidebar = luminate.sidebar(
            Sidebar::new(items)
                .width(160)
                .height(Length::Fixed(220.0))
                .axis(Axis::Vertical)
                .collapsed(self.collapsed)
                .show_toggle(true)
                .on_toggle(Message::SidebarToggled),
        );

        // The input: an error until the value looks like an address.
        let error = (!self.address.contains('@')).then_some("Needs an @");
        let input = luminate.input(
            Input::new("name@example.com", &self.address)
                .label("Address")
                .hint("The error bubble goes away once the value has an @")
                .error(error)
                .on_input(Message::AddressChanged),
        );

        // The pager on its own, without a card around it.
        let pages = (0..STEPS).map(|step| {
            container(text(format!("Step {} of {STEPS}", step + 1)))
                .center(Length::Fill)
                .height(Length::Fixed(60.0 + 30.0 * step as f32))
        });
        let pager = luminate.pager(Pager::new(pages).current(self.step).width(280));
        let steps =
            row![
                luminate.button(
                    Button::new("Previous")
                        .hierarchy(ButtonHierarchy::Secondary)
                        .on_press_maybe(self.step.checked_sub(1).map(Message::Step))
                ),
                luminate.button(Button::new("Next").on_press_maybe(
                    (self.step + 1 < STEPS).then_some(Message::Step(self.step + 1))
                )),
            ]
            .spacing(10);

        Hero::new(
            luminate,
            "Showcase",
            "Every descriptor in one view",
            row![
                sidebar,
                column![input, pager, steps].spacing(20).width(Length::Fill),
            ]
            .spacing(20),
        )
        .build()
    }
}
