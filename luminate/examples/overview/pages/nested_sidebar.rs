//! A page that hosts its own `Router`: the same page types as the top level,
//! in a second sidebar with its toggle shown.
//!
//! The nested router shares the outer [`Registry`], so its inputs page edits
//! the same draft as the outer one. Mouse back/forward navigation is left to
//! the outer router.

use iced_luminate::descriptor::{Axis, Button, ButtonHierarchy, Sidebar};
use iced_luminate::iced::widget::{container, row, text};
use iced_luminate::iced::{Length, Padding, Subscription};
use iced_luminate::router::{Action, Page, Registry, RouteMessage};
use iced_luminate::{Element, Luminate, Renderer, Router, Theme};

use crate::hero::Hero;
use crate::iso::scenes_flat;
use crate::pages::{buttons::ButtonsPage, inputs::InputsPage};

/// Messages of the nested sidebar page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A nested sidebar entry was clicked.
    Navigate(usize),
    /// Anything addressed to the nested router.
    Route(RouteMessage),
}

/// A router inside a page.
pub(crate) struct NestedSidebar {
    luminate: Luminate,
    router: Router,
}

impl Page for NestedSidebar {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, registry: &Registry) -> Self {
        let mut router = Router::new(registry.clone(), luminate.clone())
            // The outer router already listens for the mouse buttons.
            .mouse_navigation(false);
        router
            .add::<ButtonsPage>("Buttons")
            .add::<InputsPage>("Inputs");
        router
            .navigate::<ButtonsPage>()
            .expect("ButtonsPage was added above");

        Self {
            luminate: luminate.clone(),
            router,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Navigate(index) => {
                if let Err(error) = self.router.navigate_index(index) {
                    eprintln!("nested sidebar: {error}");
                }
                Action::none()
            }
            Message::Route(message) => {
                Action::task(self.router.update(message).map(Message::Route))
            }
        }
    }

    fn subscription(&self) -> Subscription<Message> {
        self.router.subscription().map(Message::Route)
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        let mut items: Vec<Element<'_, Message>> = vec![
            container(text("Nested sidebar"))
                .padding(Padding::from([15, 5]))
                .into(),
        ];
        items.extend(self.router.pages().map(|page| {
            luminate.button(
                Button::new(page.name)
                    .width(Length::Fill)
                    .hierarchy(if page.is_current {
                        ButtonHierarchy::Secondary
                    } else {
                        ButtonHierarchy::Tertiary
                    })
                    .on_press(Message::Navigate(page.index)),
            )
        }));

        let content: Element<'_, Message> = match self.router.view() {
            Some(page) => page.map(Message::Route),
            None => text("no page").into(),
        };

        Hero::new(
            luminate,
            &scenes_flat::NESTED_SIDEBAR,
            "Nested sidebar",
            "A sidebar inside a sidebar",
            row![
                luminate.sidebar(
                    Sidebar::new(items)
                        .width(200)
                        .height(Length::Fill)
                        .axis(Axis::Vertical)
                        .show_toggle(true)
                ),
                container(content).padding(15),
            ],
        )
        .build()
    }
}
