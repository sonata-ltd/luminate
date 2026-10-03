//! Descriptor properties `tests/smoke.rs` does not cover: `on_press_maybe`,
//! the button id, icon-only buttons, secure inputs, the card header cache,
//! pager index clamping, shared engines, and a custom colour reaching the
//! pixels.

use std::time::Duration;

use iced_luminate::descriptor::{Button, ButtonHierarchy, Card, Input, Pager};
use iced_luminate::iced::advanced::Renderer as _;
use iced_luminate::iced::advanced::renderer::Headless;
use iced_luminate::iced::advanced::svg;
use iced_luminate::iced::advanced::widget::{Operation, operation};
use iced_luminate::iced::advanced::{clipboard, renderer};
use iced_luminate::iced::theme::Base;
use iced_luminate::iced::time::Instant;
use iced_luminate::iced::widget::{column, text};
use iced_luminate::iced::{Color, Event, Length, Rectangle, Settings, Size, mouse, window};
use iced_luminate::texture::TextureCache;
use iced_luminate::texture::testing::headless_tiny_skia;
use iced_luminate::{Element, Luminate, Renderer, Theme};
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_test::selector;
use iced_test::{Selector, Simulator};

const SIZE: Size = Size::new(600.0, 400.0);
const SVG: &[u8] = b"<svg xmlns='http://www.w3.org/2000/svg' width='16' height='16'><rect width='16' height='16' fill='black'/></svg>";

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Pressed,
    Typed(String),
}

type Ui<'a> = Simulator<'a, Message, Theme, Renderer>;

fn simulator(root: Element<'_, Message>) -> Ui<'_> {
    Simulator::with_size(Settings::default(), SIZE, root)
}

fn redraw(ui: &mut Ui<'_>) {
    let _ = ui.simulate([Event::Window(
        window::Event::RedrawRequested(Instant::now()),
    )]);
}

fn messages(ui: Ui<'_>) -> Vec<Message> {
    ui.into_messages().collect()
}

#[test]
fn on_press_maybe_follows_its_option() {
    let luminate = Luminate::new();
    let root = column![
        luminate.button(Button::new("no").on_press_maybe(None)),
        luminate.button(Button::new("yes").on_press_maybe(Some(Message::Pressed))),
    ];
    let mut ui = simulator(luminate.host(root));
    let _ = ui.click("no").expect("on screen");
    let _ = ui.click("yes").expect("on screen");
    assert_eq!(messages(ui), vec![Message::Pressed]);
}

#[test]
fn an_icon_only_button_is_reachable_by_its_id() {
    let luminate = Luminate::new();
    let handle = svg::Handle::from_memory(SVG);
    let root = column![
        luminate.button(
            Button::with_icon(handle.clone())
                .id("icon-only")
                .on_press(Message::Pressed)
        ),
        luminate.button(
            Button::with_icon(handle)
                .label("Save")
                .on_press(Message::Pressed)
        ),
    ];
    let mut ui = simulator(luminate.host(root));
    redraw(&mut ui);
    let _ = ui
        .click(selector::id("icon-only"))
        .expect("the icon button carries its id");
    let _ = ui
        .click("Save")
        .expect("the combined button shows its label");
    assert_eq!(messages(ui), vec![Message::Pressed, Message::Pressed]);
}

#[test]
fn a_secure_input_still_accepts_typing() {
    let luminate = Luminate::new();
    let mut ui = simulator(
        luminate.input(
            Input::new("secret", "")
                .secure(true)
                .on_input(Message::Typed),
        ),
    );
    let _ = ui.click("secret").expect("on screen");
    let _ = ui.typewrite("x");
    assert_eq!(messages(ui), vec![Message::Typed("x".into())]);
}

#[test]
fn a_card_header_goes_through_its_cache_once() {
    let luminate = Luminate::new();
    let header = TextureCache::new();
    let root = luminate.card(
        Card::new("Title")
            .content(luminate.pager(
                Pager::new([Element::from(text("one")), Element::from(text("two"))]).current(1),
            ))
            .controls(Element::from(text("controls")))
            .max_height(300)
            .header_cache(header.clone()),
    );
    let mut ui = simulator(luminate.host(root));
    redraw(&mut ui);
    let _ = ui.snapshot(&Theme::LIGHT).expect("draws");
    let _ = ui.find("Title").expect("header");
    let _ = ui.find("two").expect("the current page");
    let _ = ui.find("controls").expect("controls row");
    assert_eq!(
        header.record_count(),
        1,
        "the header went through its cache"
    );
}

#[test]
fn a_pager_clamps_an_out_of_range_index() {
    let luminate = Luminate::new();
    let pages: Vec<Element<'_, Message>> = vec![text("first").into(), text("last").into()];
    let mut ui = simulator(luminate.host(luminate.pager(Pager::new(pages).current(7))));
    redraw(&mut ui);
    let _ = ui
        .find("last")
        .expect("the index is clamped to the last page");
}

#[test]
fn luminate_clones_share_one_engine_and_theme() {
    let a = Luminate::with_theme(Theme::DARK);
    let b = a.clone();
    let key = iced_luminate::animate::key!();
    let _anim = b
        .motion()
        .to(key, iced_luminate::animate::curves::SMOOTH, 1.0_f32);
    assert!(a.motion().get::<f32>(key).is_some());
    assert_eq!(b.theme(), &Theme::DARK);
}

/// Draws `root` under `theme` on the software backend and returns the RGBA
/// pixels plus the bounds of the widget with `id`.
fn render(root: Element<'_, Message>, theme: &Theme, id: &'static str) -> (Vec<u8>, Rectangle) {
    let mut renderer = headless_tiny_skia();
    let mut ui: UserInterface<'_, Message, Theme, Renderer> =
        UserInterface::build(root, SIZE, user_interface::Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    let _ = ui.update(
        &[Event::Window(window::Event::RedrawRequested(
            Instant::now() + Duration::from_millis(16),
        ))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    let mut find = Selector::find(selector::id(id));
    ui.operate(&renderer, &mut operation::black_box(&mut find));
    let bounds = match find.finish() {
        operation::Outcome::Some(Some(target)) => target.bounds(),
        _ => panic!("{id} is on screen"),
    };
    renderer.reset(Rectangle::with_size(SIZE));
    ui.draw(
        &mut renderer,
        theme,
        &renderer::Style {
            text_color: theme.base().text_color,
        },
        mouse::Cursor::Unavailable,
    );
    let rgba = renderer.screenshot(
        Size::new(SIZE.width as u32, SIZE.height as u32),
        1.0,
        Color::WHITE,
    );
    (rgba, bounds)
}

#[test]
fn a_custom_button_colour_reaches_the_pixels() {
    let mut theme = Theme::LIGHT;
    theme.button.primary.active.background = Color::from_rgb(0.0, 1.0, 0.0);
    let luminate = Luminate::with_theme(theme);
    let root = luminate.host(
        luminate.button(
            Button::new("go")
                .hierarchy(ButtonHierarchy::Primary)
                .width(Length::Fixed(200.0))
                .id("go")
                .on_press(Message::Pressed),
        ),
    );
    let (rgba, bounds) = render(root, luminate.theme(), "go");
    // Just inside the left edge, on the vertical centre: fill, not label.
    let x = (bounds.x + 6.0) as usize;
    let y = bounds.center_y() as usize;
    let i = (y * SIZE.width as usize + x) * 4;
    let pixel = [rgba[i], rgba[i + 1], rgba[i + 2]];
    assert!(
        pixel[1] > 200 && pixel[0] < 80 && pixel[2] < 80,
        "expected the custom green at ({x}, {y}) in {bounds:?}, got {pixel:?}"
    );
}
