//! Router behaviour `tests/lifecycle.rs` does not pin: the snapshot taken
//! when leaving through `forward`, the ends of the history, truncation by a
//! fresh navigation, the registry's `Result`s, and what errors and page
//! infos print.

use std::any::Any;

use iced::widget::text;
use iced_page_router::{
    Action, Key, Lifecycle, NavigationError, Page, Registry, RouteMessage, Router, Shared,
};

#[derive(Clone)]
struct Ctx;

#[derive(Debug, Clone)]
enum Msg {
    Bump,
}

struct Count;
impl Key for Count {
    type Value = u32;
}

/// A `Drop` page: its count survives only through `into_snapshot`/`restore`.
struct Counter {
    n: u32,
    seen: Shared<u32>,
}

impl Page for Counter {
    type Message = Msg;
    type NavigationOptions = ();
    type Context = Ctx;
    type Theme = iced::Theme;
    type Renderer = iced::Renderer;

    fn new(_: &Ctx, registry: &Registry) -> Self {
        Self {
            n: 0,
            seen: registry.get_or_insert_with::<Count>(|| 0),
        }
    }

    fn update(&mut self, Msg::Bump: Msg) -> Action<Msg> {
        self.n += 1;
        self.seen.set(self.n);
        Action::none()
    }

    fn view(&self) -> iced::Element<'_, Msg> {
        text(self.n).into()
    }

    fn into_snapshot(self) -> Option<Box<dyn Any>> {
        Some(Box::new(self.n))
    }

    fn restore(&mut self, snapshot: Box<dyn Any>) {
        self.n = *snapshot.downcast::<u32>().expect("our own snapshot");
        self.seen.set(self.n);
    }
}

struct Kept;

impl Page for Kept {
    type Message = Msg;
    type NavigationOptions = ();
    type Context = Ctx;
    type Theme = iced::Theme;
    type Renderer = iced::Renderer;

    const LIFECYCLE: Lifecycle = Lifecycle::Suspend;

    fn new(_: &Ctx, _: &Registry) -> Self {
        Self
    }

    fn update(&mut self, _: Msg) -> Action<Msg> {
        Action::none()
    }

    fn view(&self) -> iced::Element<'_, Msg> {
        text("kept").into()
    }
}

struct Third;

impl Page for Third {
    type Message = Msg;
    type NavigationOptions = ();
    type Context = Ctx;
    type Theme = iced::Theme;
    type Renderer = iced::Renderer;

    fn new(_: &Ctx, _: &Registry) -> Self {
        Self
    }

    fn update(&mut self, _: Msg) -> Action<Msg> {
        Action::none()
    }

    fn view(&self) -> iced::Element<'_, Msg> {
        text("third").into()
    }
}

/// Never added to the router.
struct Ghost;

impl Page for Ghost {
    type Message = Msg;
    type NavigationOptions = ();
    type Context = Ctx;
    type Theme = iced::Theme;
    type Renderer = iced::Renderer;

    fn new(_: &Ctx, _: &Registry) -> Self {
        Self
    }

    fn update(&mut self, _: Msg) -> Action<Msg> {
        Action::none()
    }

    fn view(&self) -> iced::Element<'_, Msg> {
        text("ghost").into()
    }
}

type R = Router<Ctx, iced::Theme, iced::Renderer>;

fn router() -> (R, Registry) {
    let registry = Registry::new();
    let mut router = Router::new(registry.clone(), Ctx);
    router
        .add::<Counter>("counter")
        .add::<Kept>("kept")
        .add::<Third>("third");
    (router, registry)
}

fn count(registry: &Registry) -> u32 {
    registry.get::<Count>().map_or(0, |shared| shared.get())
}

fn bump(router: &mut R) {
    let route = router
        .message::<Counter>(Msg::Bump)
        .expect("counter is live");
    let _ = router.update(route);
}

#[test]
fn forward_restores_the_latest_snapshot() {
    let (mut router, registry) = router();
    router.navigate::<Counter>().unwrap();
    bump(&mut router);
    bump(&mut router);
    router.navigate::<Kept>().unwrap();
    assert_eq!(router.back(), Some(0));
    bump(&mut router); // now 3
    assert_eq!(router.forward(), Some(1));
    registry.get::<Count>().unwrap().set(0);
    assert_eq!(router.back(), Some(0));
    assert_eq!(
        count(&registry),
        3,
        "the snapshot taken when leaving via forward wins"
    );
}

#[test]
fn a_navigation_after_back_truncates_the_forward_branch() {
    let (mut router, _) = router();
    router.navigate::<Counter>().unwrap();
    router.navigate::<Kept>().unwrap();
    assert_eq!(router.back(), Some(0));
    assert!(router.can_go_forward());
    router.navigate::<Third>().unwrap();
    assert!(!router.can_go_forward());
    assert_eq!(router.forward(), None);
    assert_eq!(router.back(), Some(0));
    assert_eq!(
        router.forward(),
        Some(2),
        "the new branch replaced the old one"
    );
}

#[test]
fn back_and_forward_at_the_ends_are_none() {
    let (mut router, _) = router();
    assert_eq!(router.back(), None);
    assert_eq!(router.forward(), None);
    assert!(!router.can_go_back() && !router.can_go_forward());
    router.navigate::<Counter>().unwrap();
    assert_eq!(router.back(), None);
    assert_eq!(router.forward(), None);
    assert_eq!(router.current(), Some(0), "the ends leave the page alone");
}

#[test]
fn an_unregistered_key_is_none_and_insert_hands_a_duplicate_back() {
    let registry = Registry::new();
    assert!(registry.get::<Count>().is_none());
    assert!(registry.insert::<Count>(3).is_ok());
    assert!(
        matches!(registry.insert::<Count>(4), Err(4)),
        "the second insert hands the value back"
    );
    assert_eq!(registry.get::<Count>().unwrap().get(), 3);
}

#[test]
fn errors_and_page_infos_are_debuggable_and_displayable() {
    let (mut router, _) = router();
    let unknown = router.navigate::<Ghost>().unwrap_err();
    assert!(matches!(unknown, NavigationError::UnknownPage { .. }));
    assert!(format!("{unknown:?}").contains("UnknownPage"));
    assert!(unknown.to_string().contains("Ghost"));
    let range = router.navigate_index(99).unwrap_err();
    assert!(matches!(
        range,
        NavigationError::IndexOutOfRange { index: 99, len: 3 }
    ));
    assert!(range.to_string().contains("99"));
    assert_eq!(
        router.current(),
        None,
        "a refused navigation changes nothing"
    );

    let infos: Vec<String> = router.pages().map(|info| format!("{info:?}")).collect();
    assert_eq!(infos.len(), 3);
    assert!(infos[0].contains("counter") && infos[0].contains("Drop"));
    assert!(infos[1].contains("kept") && infos[1].contains("Suspend"));
}

#[test]
fn a_route_message_round_trips_through_debug_without_panicking() {
    let (mut router, _) = router();
    router.navigate::<Counter>().unwrap();
    let route: RouteMessage = router.message::<Counter>(Msg::Bump).unwrap();
    assert_ne!(format!("{route:?}"), "");
    assert!(
        router.message::<Kept>(Msg::Bump).is_none(),
        "no live instance"
    );
}
