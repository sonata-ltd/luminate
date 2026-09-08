//! The Luminate overview: every descriptor, the theme, the router and the
//! motion engine in one window.
//!
//! Each page's module says which feature it demonstrates. Everything is
//! reached through `iced_luminate` paths (`iced_luminate::iced`,
//! `iced_luminate::router`, `iced_luminate::descriptor`, …): an application needs
//! one dependency line.

use iced_luminate::descriptor::{Axis, Button, ButtonHierarchy, Sidebar};
use iced_luminate::iced::widget::{container, row, text};
use iced_luminate::iced::{self, Length, Subscription, Task};
use iced_luminate::router::{Registry, RouteMessage};
use iced_luminate::theme::typography::FONT;
use iced_luminate::{Element, Luminate, Router};
use iced_texture_cache::{FilterQuality, set_filter_quality};
use luminate_examples_support::bench::Bench;

use crate::pages::{
    buttons::ButtonsPage, card::CardPage, inputs::InputsPage, motion::MotionPage,
    nested_sidebar::NestedSidebar, showcase::ShowcasePage, snapshot::SnapshotPage,
    weight::WeightPage,
};

mod pages;

fn main() -> iced::Result {
    // `RUST_LOG=info` shows the adapter and the surface-format choice.
    env_logger::init();

    // The sharpest reconstruction tier, for every cached composite in the
    // process. Left to itself the tier is picked per adapter, and an
    // integrated GPU would take the cheaper single tap — worth having, but it
    // softens sliding text enough that the moment a `Pager` stops compositing
    // and draws its page directly reads as a change in sharpness. This is a
    // showcase; it can afford the taps. Set it once, here: the override is
    // process-wide, so a page that sets it in passing changes the whole app.
    set_filter_quality(FilterQuality::CatmullRom);

    let app = iced::application(App::new, App::update, App::view)
        .title("iced_luminate: overview")
        .theme(|app: &App| *app.luminate.theme())
        .default_font(FONT)
        .subscription(App::subscription);

    // The bundled Inter faces (feature `bundled-font`, on by default).
    Luminate::fonts()
        .into_iter()
        .fold(app, iced_luminate::iced::Application::font)
        .run()
}

/// The application: one `Luminate` (theme + motion engine) and the page router.
struct App {
    luminate: Luminate,
    router: Router,
    bench: Bench,
}

/// Application messages.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A sidebar entry was clicked: show the page with this index.
    Navigate(usize),
    /// Anything addressed to a page or to the router's history.
    Route(RouteMessage),
    /// A frame timestamp, while `LUMINATE_BENCH=1`.
    Tick(iced::time::Instant),
}

impl App {
    fn new() -> (Self, Task<Message>) {
        let luminate = Luminate::new();

        let mut router = Router::new(Registry::new(), luminate.clone());
        router
            .add::<ButtonsPage>("Buttons")
            .add::<InputsPage>("Inputs")
            .add::<SnapshotPage>("Snapshot")
            .add::<CardPage>("Card")
            .add::<MotionPage>("Motion")
            .add::<WeightPage>("Weight")
            .add::<ShowcasePage>("Showcase")
            .add::<NestedSidebar>("Nested sidebar");
        router
            .navigate::<ButtonsPage>()
            .expect("ButtonsPage was added above");

        (
            Self {
                luminate,
                router,
                bench: Bench::from_env(),
            },
            Task::none(),
        )
    }

    fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            self.router.subscription().map(Message::Route),
            self.bench.subscription().map(Message::Tick),
        ])
    }

    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Navigate(index) => {
                if let Err(error) = self.router.navigate_index(index) {
                    eprintln!("overview: {error}");
                }
                Task::none()
            }
            Message::Route(message) => self.router.update(message).map(Message::Route),
            Message::Tick(now) => {
                self.bench.tick(now);
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let items: Vec<Element<'_, Message>> = self
            .router
            .pages()
            .map(|page| {
                self.luminate.button(
                    Button::new(page.name)
                        .width(Length::Fill)
                        .hierarchy(if page.is_current {
                            ButtonHierarchy::Secondary
                        } else {
                            ButtonHierarchy::Tertiary
                        })
                        .on_press(Message::Navigate(page.index)),
                )
            })
            .collect();

        let page: Element<'_, Message> = match self.router.view() {
            Some(page) => container(page.map(Message::Route)).padding(15).into(),
            None => container(text("no page")).padding(15).into(),
        };

        // `host` wraps the root in the engine's clock: without it nothing
        // animates.
        self.luminate.host(row![
            self.luminate.sidebar(
                Sidebar::new(items)
                    .width(200)
                    .height(Length::Fill)
                    .axis(Axis::Vertical)
            ),
            page,
        ])
    }
}
