//! Table: virtualization, selection, keyboard and scrolling to a row.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use iced::Padding;
use iced_luminate::iced::advanced::widget::operation::Scrollable;
use iced_luminate::iced::advanced::widget::{Id, Operation};
use iced_luminate::iced::keyboard::{Key, key::Named};
use iced_luminate::iced::time::Instant;
use iced_luminate::iced::widget::operation::AbsoluteOffset;
use iced_luminate::iced::widget::{Space, button, column as col, scrollable, text};
use iced_luminate::iced::{Event, Length, Point, Rectangle, Settings, Size, Vector, mouse, window};
use iced_luminate::texture::testing::headless_tiny_skia;
use iced_luminate::theme::TableTheme;
use iced_luminate::widget::table::{self, column, table};
use iced_luminate::{Element, Luminate, Renderer, Theme};
use iced_test::Simulator;
use iced_test::runtime::user_interface::{Cache, UserInterface};

#[derive(Debug, Clone, PartialEq)]
enum Msg {
    Select(usize),
    Delete(usize),
}

#[derive(Debug, Clone)]
struct Row {
    id: usize,
    name: String,
    remove: String,
}

fn rows(n: usize) -> Vec<Row> {
    (0..n)
        .map(|id| Row {
            id,
            name: format!("Row {id}"),
            remove: format!("Delete {id}"),
        })
        .collect()
}

fn sim(root: Element<'_, Msg>) -> Simulator<'_, Msg, Theme, Renderer> {
    Simulator::with_size(Settings::default(), Size::new(600.0, 400.0), root)
}

fn frame() -> Event {
    Event::Window(window::Event::RedrawRequested(Instant::now()))
}

fn scrolled(rows: &[Row]) -> Element<'_, Msg> {
    table(
        [
            column(text("Name"), |row: &Row| text(&row.name)),
            column(text("Action"), |row: &Row| {
                button(text(&row.remove)).on_press(Msg::Delete(row.id))
            })
            .width(160),
        ],
        rows,
    )
    .on_select(|row| Msg::Select(row.id))
    .height(300)
    .into()
}

#[test]
fn only_visible_rows_are_built_and_scrolling_brings_new_ones() {
    static BUILT: AtomicUsize = AtomicUsize::new(0);

    let rows = rows(100_000);
    let view = table(
        [column(text("Name"), |row: &Row| {
            BUILT.fetch_add(1, Ordering::Relaxed);
            text(&row.name)
        })],
        &rows,
    )
    .on_select(|row| Msg::Select(row.id))
    .height(300);

    let mut ui = sim(view.into());
    let _ = ui.simulate([frame()]);
    let first = BUILT.swap(0, Ordering::Relaxed);
    assert!(first < 80, "built {first} rows out of 100 000");
    assert!(ui.find("Row 0").is_ok());
    assert!(ui.find("Row 500").is_err());

    // A wheel in pixels sets the offset at once instead of gliding.
    ui.point_at(Point::new(100.0, 150.0));
    let _ = ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels {
            x: 0.0,
            y: -15_000.0,
        },
    })]);
    let _ = ui.simulate([frame()]);

    assert!(ui.find("Row 470").is_ok(), "row 470 must be built");
    assert!(ui.find("Row 0").is_err(), "row 0 must be gone");

    ui.click("Row 471").unwrap();
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select(471)]
    );
}

#[test]
fn the_header_stays_while_rows_scroll() {
    let rows = rows(1_000);
    let mut ui = sim(scrolled(&rows));
    let _ = ui.simulate([frame()]);
    let before = ui.find("Name").unwrap().visible_bounds().unwrap();

    ui.point_at(Point::new(100.0, 150.0));
    let _ = ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels {
            x: 0.0,
            y: -3_000.0,
        },
    })]);
    let _ = ui.simulate([frame()]);

    let after = ui.find("Name").unwrap().visible_bounds().unwrap();
    assert_eq!(before, after);
}

#[test]
fn a_button_in_a_cell_keeps_its_click() {
    let rows = rows(10);
    let mut ui = sim(scrolled(&rows));
    let _ = ui.simulate([frame()]);

    ui.click("Delete 3").unwrap();
    assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Delete(3)]);
}

fn with_disabled(rows: &[Row]) -> Element<'_, Msg> {
    table([column(text("Name"), |row: &Row| text(&row.name))], rows)
        .on_select(|row| Msg::Select(row.id))
        .disabled(|row| row.id % 2 == 1)
        .height(300)
        .into()
}

#[test]
fn a_disabled_row_ignores_clicks() {
    let rows = rows(10);
    let mut ui = sim(with_disabled(&rows));
    let _ = ui.simulate([frame()]);

    ui.click("Row 1").unwrap();
    assert!(ui.into_messages().next().is_none());
}

#[test]
fn the_inset_gutter_does_not_select_a_row() {
    let rows = rows(10);
    let view = table([column(text("Name"), |row: &Row| text(&row.name))], &rows)
        .on_select(|row| Msg::Select(row.id))
        .inset(Padding::from(12.0))
        .height(300)
        .into();
    let mut ui = sim(view);
    let _ = ui.simulate([frame()]);

    // The header is 28 high and a row 32, so y = 76 is the middle of row 1.
    let click = |ui: &mut Simulator<'_, Msg, Theme, Renderer>, x: f32| {
        ui.point_at(Point::new(x, 76.0));
        let _ = ui.simulate([
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
    };

    click(&mut ui, 5.0);
    click(&mut ui, 595.0);
    click(&mut ui, 300.0);

    assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Select(1)]);
}

#[test]
fn the_kit_table_takes_its_metrics_from_the_tokens() {
    let theme = Theme {
        table: TableTheme {
            row_height: 40.0,
            header_height: 30.0,
            inset: Padding::from(12.0),
            ..Theme::LIGHT.table
        },
        ..Theme::LIGHT
    };
    let luminate = Luminate::with_theme(theme);

    let rows = rows(10);
    let view = luminate
        .table([column(text("Name"), |row: &Row| text(&row.name))], &rows)
        .on_select(|row| Msg::Select(row.id))
        .height(300)
        .into();
    let mut ui = sim(view);
    let _ = ui.simulate([frame()]);

    // Row 1 spans y = 82..122 with these tokens (30 header, 12 inset, 40 row),
    // and 60..92 with the defaults.
    let click = |ui: &mut Simulator<'_, Msg, Theme, Renderer>, x: f32| {
        ui.point_at(Point::new(x, 100.0));
        let _ = ui.simulate([
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
        ]);
    };

    click(&mut ui, 5.0);
    click(&mut ui, 300.0);

    assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Msg::Select(1)]);
}

#[test]
fn arrows_skip_disabled_rows_and_enter_selects() {
    let rows = rows(10);
    let mut ui = sim(with_disabled(&rows));
    let _ = ui.simulate([frame()]);

    ui.click("Row 2").unwrap();
    let _ = ui.tap_key(Key::Named(Named::ArrowDown));
    let _ = ui.tap_key(Key::Named(Named::Enter));

    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Msg::Select(2), Msg::Select(4)]
    );
}

#[test]
fn end_scrolls_the_last_row_into_view() {
    let rows = rows(10_000);
    let mut ui = sim(with_disabled(&rows));
    let _ = ui.simulate([frame()]);

    ui.click("Row 0").unwrap();
    let _ = ui.tap_key(Key::Named(Named::End));

    let mut found = false;
    for _ in 0..240 {
        // The scroll glides on a spring: give it frames with real pauses.
        std::thread::sleep(Duration::from_millis(8));
        let _ = ui.simulate([frame()]);
        if ui.find("Row 9998").is_ok() {
            found = true;
            break;
        }
    }
    assert!(found, "the last enabled row (9998) must scroll into view");
}

#[test]
fn an_in_flow_table_virtualizes_against_the_page_scroll() {
    static BUILT: AtomicUsize = AtomicUsize::new(0);

    let rows = rows(100_000);
    let page = scrollable(col![
        Space::new().height(500),
        table(
            [column(text("Name"), |row: &Row| {
                BUILT.fetch_add(1, Ordering::Relaxed);
                text(&row.name)
            })],
            &rows,
        ),
        Space::new().height(500),
    ])
    .height(Length::Fill);

    let mut ui = sim(page.into());
    let _ = ui.simulate([frame()]);
    assert!(BUILT.swap(0, Ordering::Relaxed) < 80);

    ui.point_at(Point::new(100.0, 150.0));
    let _ = ui.simulate([Event::Mouse(mouse::Event::WheelScrolled {
        delta: mouse::ScrollDelta::Pixels {
            x: 0.0,
            y: -(500.0 + 26.0 + 32_000.0),
        },
    })]);
    let _ = ui.simulate([frame()]);

    assert!(ui.find("Row 1002").is_ok());
    assert!(BUILT.load(Ordering::Relaxed) < 80);
}

#[test]
fn an_empty_table_shows_its_placeholder() {
    let rows: Vec<Row> = Vec::new();
    let view = table([column(text("Name"), |row: &Row| text(&row.name))], &rows)
        .placeholder(text("This table is empty"))
        .height(200);

    let mut ui = sim(view.into());
    assert!(ui.find("This table is empty").is_ok());
}

#[derive(Default)]
struct Offsets(Vec<(String, f32)>);

impl Operation for Offsets {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        operate(self);
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        bounds: Rectangle,
        _: Rectangle,
        t: Vector,
        _: &mut dyn Scrollable,
    ) {
        let name = id.map_or_else(|| format!("anon {}", bounds.height), |id| format!("{id:?}"));
        self.0.push((name, t.y));
    }
}

fn redraw(ui: &mut UserInterface<'_, Msg, Theme, Renderer>, renderer: &mut Renderer) {
    let mut messages = Vec::new();
    let mut clipboard = iced_luminate::iced::advanced::clipboard::Null;
    let _ = ui.update(
        &[frame()],
        mouse::Cursor::Unavailable,
        renderer,
        &mut clipboard,
        &mut messages,
    );
}

#[test]
fn scroll_to_row_brings_an_unbuilt_row_into_view() {
    let rows = rows(10_000);
    let id = Id::new("rows");
    let mut renderer = headless_tiny_skia();

    let view = table([column(text("Name"), |row: &Row| text(&row.name))], &rows)
        .on_select(|row| Msg::Select(row.id))
        .id(id.clone())
        .height(300);

    let mut ui = UserInterface::build(
        Element::from(view),
        Size::new(600.0, 400.0),
        Cache::default(),
        &mut renderer,
    );
    redraw(&mut ui, &mut renderer);

    ui.operate(
        &renderer,
        &mut table::operation::scroll_to_row::<()>(id, 7_000),
    );

    let mut offset = 0.0;
    for _ in 0..240 {
        std::thread::sleep(Duration::from_millis(8));
        redraw(&mut ui, &mut renderer);
        let mut probe = Offsets::default();
        ui.operate(&renderer, &mut probe);
        offset = probe.0.first().map_or(0.0, |(_, y)| *y);
        let top = 7_000.0 * 32.0;
        if offset <= top && top + 32.0 <= offset + 274.0 {
            return;
        }
    }
    panic!("row 7000 did not arrive: offset {offset}");
}

#[test]
fn row_key_keeps_cell_state_with_its_row() {
    fn view(order: &[Row], keyed: bool) -> Element<'_, Msg> {
        let t = table(
            [column(text("Log"), |row: &Row| {
                scrollable(col![Space::new().height(500)])
                    .height(24)
                    .id(Id::from(format!("log{}", row.id)))
            })],
            order,
        )
        .on_select(|row| Msg::Select(row.id))
        .height(300);

        if keyed {
            t.row_key(|row| row.id).into()
        } else {
            t.into()
        }
    }

    fn after_reverse(keyed: bool) -> Vec<(String, f32)> {
        let forward = rows(3);
        let backward: Vec<Row> = forward.iter().rev().cloned().collect();
        let mut renderer = headless_tiny_skia();

        let mut ui = UserInterface::build(
            view(&forward, keyed),
            Size::new(600.0, 400.0),
            Cache::default(),
            &mut renderer,
        );
        redraw(&mut ui, &mut renderer);
        ui.operate(
            &renderer,
            &mut iced_luminate::iced::advanced::widget::operation::scrollable::scroll_to::<()>(
                Id::new("log0"),
                AbsoluteOffset {
                    x: None,
                    y: Some(120.0),
                },
            ),
        );
        let cache = ui.into_cache();

        let mut ui = UserInterface::build(
            view(&backward, keyed),
            Size::new(600.0, 400.0),
            cache,
            &mut renderer,
        );
        redraw(&mut ui, &mut renderer);
        let mut probe = Offsets::default();
        ui.operate(&renderer, &mut probe);
        probe
            .0
            .into_iter()
            .filter(|(name, _)| name.contains("log"))
            .collect()
    }

    let by_position = after_reverse(false);
    let by_key = after_reverse(true);
    println!("by position: {by_position:?}\nby key:      {by_key:?}");

    assert!(by_position.contains(&("Id(Custom(\"log2\"))".to_owned(), 120.0)));
    assert!(by_key.contains(&("Id(Custom(\"log0\"))".to_owned(), 120.0)));
    assert_eq!(by_key.iter().filter(|(_, y)| *y != 0.0).count(), 1);
}
