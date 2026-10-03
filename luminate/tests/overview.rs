//! The overview app as a user runs it: `Luminate` + `Router` with two pages,
//! `luminate.host` around the view, a `Cached` card header per page, driven
//! frame by frame on the software backend.

use std::time::Duration;

use iced_luminate::descriptor::{Button, ButtonHierarchy, Card, Sidebar};
use iced_luminate::iced::advanced::Renderer as _;
use iced_luminate::iced::advanced::widget::{Operation, operation};
use iced_luminate::iced::advanced::{clipboard, renderer};
use iced_luminate::iced::theme::Base;
use iced_luminate::iced::time::Instant;
use iced_luminate::iced::widget::{row, text};
use iced_luminate::iced::{Event, Length, Rectangle, Size, mouse, window};
use iced_luminate::router::{Action, Key, Lifecycle, Page, Registry, RouteMessage};
use iced_luminate::texture::TextureCache;
use iced_luminate::texture::testing::headless_tiny_skia;
use iced_luminate::{Element, Luminate, Renderer, Router, Theme};
use iced_test::Selector;
use iced_test::runtime::user_interface::{self, UserInterface};

const SIZE: Size = Size::new(640.0, 400.0);
const FRAME: Duration = Duration::from_millis(16);

struct HomeHeader;
impl Key for HomeHeader {
    type Value = TextureCache;
}

struct AboutHeader;
impl Key for AboutHeader {
    type Value = TextureCache;
}

/// A `Drop` page (the default lifecycle).
struct Home {
    luminate: Luminate,
    header: TextureCache,
}

impl Page for Home {
    type Message = ();
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, registry: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            header: registry
                .get_or_insert_with::<HomeHeader>(TextureCache::new)
                .get(),
        }
    }

    fn update(&mut self, (): ()) -> Action<()> {
        Action::none()
    }

    fn view(&self) -> Element<'_, ()> {
        self.luminate.card(
            Card::new("Home")
                .content(text("home body"))
                .header_cache(self.header.clone()),
        )
    }
}

/// A `Suspend` page: kept alive while away.
struct About {
    luminate: Luminate,
    header: TextureCache,
}

impl Page for About {
    type Message = ();
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    const LIFECYCLE: Lifecycle = Lifecycle::Suspend;

    fn new(luminate: &Luminate, registry: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            header: registry
                .get_or_insert_with::<AboutHeader>(TextureCache::new)
                .get(),
        }
    }

    fn update(&mut self, (): ()) -> Action<()> {
        Action::none()
    }

    fn view(&self) -> Element<'_, ()> {
        self.luminate.card(
            Card::new("About")
                .content(text("about body"))
                .header_cache(self.header.clone()),
        )
    }
}

#[derive(Debug, Clone)]
enum Message {
    Navigate(usize),
    Route(RouteMessage),
}

struct App {
    luminate: Luminate,
    router: Router,
}

impl App {
    fn new() -> Self {
        let luminate = Luminate::new();
        let mut router = Router::new(Registry::new(), luminate.clone());
        router.add::<Home>("Home").add::<About>("About");
        router.navigate::<Home>().expect("registered");
        Self { luminate, router }
    }

    fn update(&mut self, message: Message) {
        match message {
            Message::Navigate(index) => {
                self.router.navigate_index(index).expect("a sidebar index");
            }
            Message::Route(route) => {
                let _ = self.router.update(route);
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;
        let items: Vec<Element<'_, Message>> = self
            .router
            .pages()
            .map(|info| {
                luminate.button(
                    Button::new(info.name)
                        .width(Length::Fill)
                        .hierarchy(if info.is_current {
                            ButtonHierarchy::Secondary
                        } else {
                            ButtonHierarchy::Tertiary
                        })
                        .on_press(Message::Navigate(info.index)),
                )
            })
            .collect();
        let page = self
            .router
            .view()
            .expect("a page is current")
            .map(Message::Route);
        luminate.host(row![
            luminate.sidebar(Sidebar::new(items).width(200).height(Length::Fill)),
            page,
        ])
    }
}

struct Harness {
    app: App,
    renderer: Renderer,
    cache: Option<user_interface::Cache>,
    now: Instant,
    cursor: mouse::Cursor,
}

impl Harness {
    fn new() -> Self {
        Self {
            app: App::new(),
            renderer: headless_tiny_skia(),
            cache: Some(user_interface::Cache::default()),
            now: Instant::now(),
            cursor: mouse::Cursor::Unavailable,
        }
    }

    /// One frame: build the view, `RedrawRequested` plus `events`, draw, feed
    /// the messages back into the app.
    fn frame(&mut self, events: &[Event]) {
        self.now += FRAME;
        let cache = self.cache.take().expect("returned after every frame");
        let mut messages = Vec::new();
        let mut ui: UserInterface<'_, Message, Theme, Renderer> =
            UserInterface::build(self.app.view(), SIZE, cache, &mut self.renderer);
        let mut all = vec![Event::Window(window::Event::RedrawRequested(self.now))];
        all.extend_from_slice(events);
        let _ = ui.update(
            &all,
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        let theme = Theme::LIGHT;
        self.renderer.reset(Rectangle::with_size(SIZE));
        ui.draw(
            &mut self.renderer,
            &theme,
            &renderer::Style {
                text_color: theme.base().text_color,
            },
            self.cursor,
        );
        self.cache = Some(ui.into_cache());
        for message in messages {
            self.app.update(message);
        }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            self.frame(&[]);
        }
    }

    /// Clicks the first text matching `label`.
    fn click(&mut self, label: &str) {
        let cache = self.cache.take().expect("returned after every frame");
        let mut ui: UserInterface<'_, Message, Theme, Renderer> =
            UserInterface::build(self.app.view(), SIZE, cache, &mut self.renderer);
        let mut find = Selector::find(label);
        ui.operate(&self.renderer, &mut operation::black_box(&mut find));
        let operation::Outcome::Some(Some(target)) = find.finish() else {
            panic!("{label} is not on screen")
        };
        let centre = target
            .visible_bounds()
            .unwrap_or_else(|| panic!("{label} is hidden"))
            .center();
        self.cache = Some(ui.into_cache());
        self.cursor = mouse::Cursor::Available(centre);
        let click: Vec<Event> = iced_test::simulator::click().collect();
        self.frame(&click);
    }

    fn tracks(&self) -> usize {
        self.app.luminate.motion().track_count()
    }
}

#[test]
fn the_overview_navigates_by_sidebar_and_re_records_its_headers_only_on_navigation() {
    let mut h = Harness::new();
    let registry = h.app.router.registry().clone();
    let home = registry
        .get::<HomeHeader>()
        .expect("Home built its header when it became current")
        .get();

    h.frames(3);
    assert_eq!(h.app.router.current(), Some(0));
    let home_first = home.record_count();
    assert!(home_first >= 1, "the header was rasterised");
    h.frames(30);
    assert_eq!(
        home.record_count(),
        home_first,
        "idle frames never re-record"
    );

    h.click("About");
    assert_eq!(
        h.app.router.current(),
        Some(1),
        "the sidebar button navigated"
    );
    let about = registry
        .get::<AboutHeader>()
        .expect("About built its header when it became current")
        .get();
    h.frames(3);
    let about_first = about.record_count();
    assert!(about_first >= 1);
    h.frames(120);
    assert_eq!(
        about.record_count(),
        about_first,
        "idle frames after arrival never re-record"
    );
    assert_eq!(
        home.record_count(),
        home_first,
        "the page we left is untouched"
    );

    let settled = h.tracks();
    h.frames(120);
    assert!(
        h.tracks() <= settled,
        "tracks must not accumulate across idle frames: {settled} -> {}",
        h.tracks()
    );
    assert!(
        h.tracks() <= 32,
        "a two-page app with two buttons needs a handful of tracks, not {}",
        h.tracks()
    );

    h.click("Home");
    assert_eq!(h.app.router.current(), Some(0));
    h.frames(3);
    let home_again = home.record_count();
    assert!(
        home_again <= home_first + 1,
        "at most one re-record for a page rebuilt after Drop"
    );
    h.frames(60);
    assert_eq!(home.record_count(), home_again, "quiet again");
    assert_eq!(
        about.record_count(),
        about_first,
        "the suspended page's header is untouched"
    );
}

#[test]
fn a_clone_of_luminate_shares_the_engine_the_host_ticks() {
    let h = Harness::new();
    let twin = h.app.luminate.clone();
    let key = iced_luminate::animate::key!();
    let _anim = twin
        .motion()
        .to(key, iced_luminate::animate::curves::SMOOTH, 1.0_f32);
    assert!(
        h.app.luminate.motion().get::<f32>(key).is_some(),
        "one engine behind every clone"
    );
}
