//! Dropdown: opening, picking, the virtualized menu, search and keyboard.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use iced_luminate::iced::keyboard::{self, Key, Modifiers, key::Named};
use iced_luminate::iced::time::Instant;
use iced_luminate::iced::widget::{column, text};
use iced_luminate::iced::{Event, Point, Settings, Size, mouse, window};
use iced_luminate::widget::dropdown::dropdown;
use iced_luminate::{Element, Luminate, Renderer, Theme};
use iced_test::Simulator;

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Select(String),
    Search(String),
    Opened,
    Closed,
}

fn sim(root: Element<'_, Msg>) -> Simulator<'_, Msg, Theme, Renderer> {
    Simulator::with_size(Settings::default(), Size::new(500.0, 600.0), root)
}

fn frame() -> Event {
    Event::Window(window::Event::RedrawRequested(Instant::now()))
}

fn options(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("Option {i}")).collect()
}

fn page<'a>(field: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    column![text("top").height(40), field.into()].into()
}

fn open(ui: &mut Simulator<'_, Msg, Theme, Renderer>) {
    ui.point_at(Point::new(100.0, 55.0));
    let _ = ui.simulate(iced_test::simulator::click());
    let _ = ui.simulate([frame()]);
}

fn frames_until(ui: &mut Simulator<'_, Msg, Theme, Renderer>, label: &str) -> bool {
    for _ in 0..240 {
        std::thread::sleep(Duration::from_millis(8));
        let _ = ui.simulate([frame()]);
        if ui.find(label).is_ok() {
            return true;
        }
    }
    false
}

#[test]
fn a_click_opens_the_menu_and_a_click_on_an_item_selects_it() {
    let options = options(5);
    let view = dropdown(&options, None::<&String>, Msg::Select)
        .placeholder("Pick one")
        .on_open(Msg::Opened)
        .on_close(Msg::Closed);
    let mut ui = sim(page(view));

    assert!(ui.find("Pick one").is_ok());
    assert!(ui.find("Option 2").is_err(), "the menu is closed");

    open(&mut ui);
    ui.click("Option 2").unwrap();

    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Opened, Msg::Select("Option 2".into()), Msg::Closed]
    );
}

#[test]
fn the_menu_builds_only_visible_items() {
    static BUILT: AtomicUsize = AtomicUsize::new(0);

    let options = options(10_000);
    let view = dropdown(&options, None::<&String>, Msg::Select).item(|option: &String| {
        BUILT.fetch_add(1, Ordering::Relaxed);
        text(option.as_str()).into()
    });
    let mut ui = sim(page(view));

    open(&mut ui);
    let _ = ui.simulate([frame()]);

    BUILT.store(0, Ordering::Relaxed);
    let _ = ui.simulate([frame()]);
    let per_frame = BUILT.load(Ordering::Relaxed);
    assert!(
        per_frame < 40,
        "built {per_frame} items out of 10 000 in a frame"
    );

    assert!(ui.find("Option 0").is_ok());
    assert!(ui.find("Option 5000").is_err());
}

#[test]
fn end_reaches_the_last_item_and_enter_selects_it() {
    let options = options(10_000);
    let view = dropdown(&options, None::<&String>, Msg::Select);
    let mut ui = sim(page(view));

    open(&mut ui);
    let _ = ui.tap_key(Key::Named(Named::End));
    assert!(
        frames_until(&mut ui, "Option 9999"),
        "the last item must scroll into view"
    );

    let _ = ui.tap_key(Key::Named(Named::Enter));
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select("Option 9999".into())]
    );
}

#[test]
fn opening_reveals_the_selected_item() {
    let options = options(10_000);
    let selected = options[5000].clone();
    let view = dropdown(&options, Some(&selected), Msg::Select);
    let mut ui = sim(page(view));

    open(&mut ui);
    assert!(frames_until(&mut ui, "Option 5000"));
}

#[test]
fn search_filters_non_ascii_and_clears_the_query_after_a_pick() {
    let options: Vec<String> = ["Berlin", "Zürich", "Madrid"].map(String::from).to_vec();
    let view = dropdown(&options, None::<&String>, Msg::Select).search("ZÜR", Msg::Search);
    let mut ui = sim(page(view));

    open(&mut ui);
    assert!(ui.find("Zürich").is_ok());
    assert!(ui.find("Madrid").is_err(), "does not match the query");

    ui.click("Zürich").unwrap();
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select("Zürich".into()), Msg::Search(String::new())]
    );
}

#[test]
fn nothing_found_shows_the_empty_text() {
    let options = options(5);
    let view = dropdown(&options, None::<&String>, Msg::Select)
        .search("zzz", Msg::Search)
        .empty("Nothing found");
    let mut ui = sim(page(view));

    open(&mut ui);
    assert!(ui.find("Nothing found").is_ok());
}

#[test]
fn enter_selects_exact_search_keys_with_configurable_case_matching() {
    let options = vec![String::from("Zürich"), String::from("Berlin")];
    for (query, enabled, case_sensitive, filter, expected) in [
        ("ZÜRICH", true, false, true, true),
        ("Zürich", true, true, true, true),
        ("ZÜRICH", true, true, true, false),
        ("ZÜRICH", true, true, false, false),
        ("Zürich", false, false, true, false),
        ("Zür", true, false, true, false),
        ("unknown", true, false, true, false),
        ("", true, false, true, false),
    ] {
        let view = dropdown(&options, None::<&String>, Msg::Select)
            .search(query, Msg::Search)
            .select_on_exact_match(enabled)
            .search_case_sensitive(case_sensitive)
            .filter(filter)
            .on_close(Msg::Closed);
        let mut ui = sim(page(view));
        open(&mut ui);
        let _ = ui.tap_key(Key::Named(Named::Enter));
        let messages = ui.into_messages().collect::<Vec<_>>();
        if expected {
            assert_eq!(
                messages,
                vec![
                    Msg::Select("Zürich".into()),
                    Msg::Search(String::new()),
                    Msg::Closed
                ]
            );
        } else {
            assert!(messages.is_empty(), "unexpected selection for {query:?}");
        }
    }

    let view = dropdown(&options, None::<&String>, Msg::Select)
        .search_key(|option| format!("City: {option}"))
        .search("City: Berlin", Msg::Search)
        .on_close(Msg::Closed);
    let mut ui = sim(page(view));
    open(&mut ui);
    let _ = ui.tap_key(Key::Named(Named::Enter));
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![
            Msg::Select("Berlin".into()),
            Msg::Search(String::new()),
            Msg::Closed
        ]
    );
}

#[test]
fn dismissing_search_clears_the_query_without_changing_the_selection() {
    for dismissal in ["escape", "outside", "tab", "window"] {
        let options = options(5);
        let selected = options[1].clone();
        let view = dropdown(&options, Some(&selected), Msg::Select)
            .search("unfinished", Msg::Search)
            .on_close(Msg::Closed);
        let mut ui = sim(page(view));
        open(&mut ui);

        match dismissal {
            "escape" => {
                let _ = ui.tap_key(Key::Named(Named::Escape));
            }
            "tab" => {
                let _ = ui.tap_key(Key::Named(Named::Tab));
            }
            "outside" => {
                ui.point_at(Point::new(450.0, 580.0));
                let _ = ui.simulate(iced_test::simulator::click());
            }
            "window" => {
                let _ = ui.simulate([Event::Window(window::Event::Unfocused)]);
            }
            _ => unreachable!(),
        }

        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![Msg::Search(String::new()), Msg::Closed],
            "{dismissal} must discard the query without selecting an option",
        );
    }
}

#[test]
fn filtering_an_open_menu_discards_the_previous_highlight_before_enter() {
    use iced_luminate::texture::testing::headless_tiny_skia;
    use iced_test::runtime::user_interface::{Cache, UserInterface};

    let options = options(10_000);
    let selected = &options[4999];
    let view = |query| {
        page(
            dropdown(&options, Some(selected), Msg::Select)
                .search(query, Msg::Search)
                .select_on_exact_match(false),
        )
    };
    let mut renderer = headless_tiny_skia();
    let size = Size::new(500.0, 600.0);
    let mut clipboard = iced_luminate::iced::advanced::clipboard::Null;
    let mut messages = Vec::new();
    let mut ui = UserInterface::build(view(""), size, Cache::default(), &mut renderer);
    let _ = ui.update(
        &iced_test::simulator::click().collect::<Vec<_>>(),
        mouse::Cursor::Available(Point::new(100.0, 55.0)),
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );

    // Preserve the open menu and its old highlight, as an application rebuild does.
    // A single result which excludes the previously selected option.
    let mut ui = UserInterface::build(view("Option 9999"), size, ui.into_cache(), &mut renderer);
    let _ = ui.update(
        &iced_test::simulator::tap_key(Key::Named(Named::Enter), None).collect::<Vec<_>>(),
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert!(messages.is_empty(), "Enter must not use the old highlight");

    let events = [Named::ArrowDown, Named::Enter]
        .into_iter()
        .flat_map(|key| iced_test::simulator::tap_key(Key::Named(key), None))
        .collect::<Vec<_>>();
    let _ = ui.update(
        &events,
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard,
        &mut messages,
    );
    assert_eq!(
        messages,
        vec![
            Msg::Select("Option 9999".into()),
            Msg::Search(String::new())
        ]
    );
}

#[test]
fn a_click_outside_and_escape_close_the_menu() {
    let options = options(5);

    let view = dropdown(&options, None::<&String>, Msg::Select).on_close(Msg::Closed);
    let mut ui = sim(page(view));
    open(&mut ui);
    ui.point_at(Point::new(450.0, 580.0));
    let _ = ui.simulate(iced_test::simulator::click());
    assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Closed]);

    let view = dropdown(&options, None::<&String>, Msg::Select).on_close(Msg::Closed);
    let mut ui = sim(page(view));
    open(&mut ui);
    let _ = ui.tap_key(Key::Named(Named::Escape));
    assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Closed]);
}

#[test]
fn ctrl_wheel_cycles_like_pick_list() {
    let options = options(5);
    let selected = options[1].clone();
    let view = dropdown(&options, Some(&selected), Msg::Select);
    let mut ui = sim(page(view));

    ui.point_at(Point::new(100.0, 55.0));
    let _ = ui.simulate([
        Event::Keyboard(keyboard::Event::ModifiersChanged(Modifiers::CTRL)),
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        }),
    ]);

    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select("Option 2".into())]
    );
}

#[test]
fn a_disabled_dropdown_does_not_open() {
    let options = options(5);
    let view =
        dropdown(&options, None::<&String>, Msg::Select).on_select_maybe(None::<fn(String) -> Msg>);
    let mut ui = sim(page(view));

    open(&mut ui);
    assert!(ui.find("Option 1").is_err());
}

#[test]
fn the_kit_wrapper_keeps_the_menu_working() {
    let kit = Luminate::with_theme(Theme::LIGHT);
    let options = options(5);
    let view = kit.dropdown(
        dropdown(&options, None::<&String>, Msg::Select)
            .label("City")
            .hint("Where the trip starts")
            .error(Some("Not selected")),
    );
    let mut ui = sim(view);

    assert!(ui.find("City").is_ok());
    assert!(ui.find("Where the trip starts").is_ok());

    let field_y = ui.find("City").unwrap().visible_bounds().unwrap().y + 40.0;
    ui.point_at(Point::new(100.0, field_y));
    let _ = ui.simulate(iced_test::simulator::click());
    let _ = ui.simulate([frame()]);

    ui.click("Option 3").unwrap();
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select("Option 3".into())]
    );
}

#[test]
fn space_picks_the_highlighted_item_without_search() {
    let options = options(5);
    let view = dropdown(&options, None::<&String>, Msg::Select);
    let mut ui = sim(page(view));

    open(&mut ui);
    let _ = ui.tap_key(Key::Named(Named::ArrowDown));
    let _ = ui.tap_key(Key::Named(Named::ArrowDown));
    let _ = ui.tap_key(Key::Named(Named::Space));

    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select("Option 1".into())]
    );
}
