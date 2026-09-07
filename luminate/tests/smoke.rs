//! Every Luminate builder lays out under the kit's renderer and theme, and
//! each descriptor property that changes observable behaviour does so.

use iced_luminate::descriptor::{
    Axis, Button, ButtonHierarchy, ButtonSize, Card, Input, Pager, Sidebar,
};
use iced_luminate::iced::advanced::svg;
use iced_luminate::iced::keyboard::{self, key};
use iced_luminate::iced::widget::{Space, column, text};
use iced_luminate::iced::{self, Length, Point, Size};
use iced_luminate::theme::{CardTheme, Theme};
use iced_luminate::{Element, Luminate, Renderer};
use iced_test::Simulator;

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Pressed,
    Typed(String),
    Submitted,
    Toggled(bool),
}

type Ui<'a> = Simulator<'a, Message, Theme, Renderer>;

fn simulator(root: Element<'_, Message>) -> Ui<'_> {
    Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root)
}

/// One redraw so every widget lays out, updates and settles.
fn settle(ui: &mut Ui<'_>) {
    let _ = ui.simulate([iced::Event::Window(iced::window::Event::RedrawRequested(
        iced::time::Instant::now(),
    ))]);
}

fn icon() -> svg::Handle {
    svg::Handle::from_memory(
        b"<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><rect width='20' height='20'/></svg>"
            .as_slice(),
    )
}

#[test]
fn a_button_delivers_its_message() {
    let luminate = Luminate::new();
    let root = luminate.button(Button::new("press me").on_press(Message::Pressed));

    let mut ui = simulator(root);
    let _ = ui.click("press me").expect("the button is on screen");

    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Message::Pressed]
    );
}

#[test]
fn a_disabled_button_delivers_nothing() {
    let luminate = Luminate::new();
    let root = luminate.button(Button::new("nope"));

    let mut ui = simulator(root);
    let _ = ui
        .click("nope")
        .expect("a disabled button is still on screen");

    assert_eq!(ui.into_messages().count(), 0);
}

/// The pressed ring is drawn outside the layout box, so a button occupies
/// exactly its padding plus one line of its label — nothing more. Two
/// buttons stacked with no spacing therefore sit exactly one box apart.
///
/// This is the proportion the kit is calibrated to: reserving room for the
/// ring instead would add `2 × (ring_width + ring_offset)` to every button
/// in the interface, at rest, forever.
#[test]
fn a_button_reserves_no_room_for_its_pressed_ring() {
    let luminate = Luminate::new();
    let theme = Theme::LIGHT;
    let items: Vec<Element<'_, Message>> = ["top", "bottom"]
        .into_iter()
        .map(|label| luminate.button(Button::new(label).on_press(Message::Pressed)))
        .collect();

    let root: Element<'_, Message> = column(items).spacing(0).into();
    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    let top = ui.find("top").expect("on screen").bounds();
    let bottom = ui.find("bottom").expect("on screen").bounds();

    let padding = theme.button.small.text;
    let expected = padding.top + padding.bottom + theme.button.label.line_height;
    assert_eq!(expected, 34.0, "the shipped small button is 7 + 20 + 7");
    assert!(
        (bottom.y - top.y - expected).abs() < 0.5,
        "button box is {} px, expected {expected}",
        bottom.y - top.y
    );
}

#[test]
fn every_button_variant_lays_out() {
    let luminate = Luminate::new();
    let hierarchies = [
        ButtonHierarchy::Primary,
        ButtonHierarchy::Secondary,
        ButtonHierarchy::Tertiary,
        ButtonHierarchy::Destructive,
    ];
    let sizes = [ButtonSize::Small, ButtonSize::Medium, ButtonSize::Large];
    let labels: Vec<String> = (0..12).map(|i| format!("b{i}")).collect();

    let mut items: Vec<Element<'_, Message>> = Vec::new();
    for (i, (hierarchy, size)) in hierarchies
        .iter()
        .flat_map(|h| sizes.iter().map(move |s| (*h, *s)))
        .enumerate()
    {
        items.push(
            luminate.button(
                Button::new(labels[i].as_str())
                    .hierarchy(hierarchy)
                    .size(size)
                    .on_press(Message::Pressed),
            ),
        );
    }
    items.push(luminate.button(Button::with_icon(icon()).on_press(Message::Pressed)));
    items.push(
        luminate.button(
            Button::new("with icon")
                .icon(icon())
                .on_press(Message::Pressed),
        ),
    );

    let root: Element<'_, Message> = column(items).into();
    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    for label in &labels {
        let _ = ui
            .find(label.as_str())
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
    }
    let _ = ui.find("with icon").expect("combined button laid out");
}

#[test]
fn an_input_accepts_typing() {
    let luminate = Luminate::new();
    let root = luminate.input(Input::new("placeholder", "").on_input(Message::Typed));

    let mut ui = simulator(root);
    let _ = ui.click("placeholder").expect("the input is on screen");
    assert_eq!(ui.typewrite("hi"), iced::event::Status::Captured);

    // The input edits its own buffer, so every keystroke reports the
    // accumulated value.
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Message::Typed("h".into()), Message::Typed("hi".into())]
    );
}

#[test]
fn a_read_only_input_ignores_typing() {
    let luminate = Luminate::new();
    let root = luminate.input(Input::new("placeholder", "fixed"));

    let mut ui = simulator(root);
    let _ = ui.click("fixed").expect("the input is on screen");
    assert_eq!(ui.typewrite("x"), iced::event::Status::Ignored);
    assert_eq!(ui.into_messages().count(), 0);
}

#[test]
fn an_input_submits_on_enter() {
    let luminate = Luminate::new();
    let root = luminate.input(
        Input::new("placeholder", "")
            .on_input(Message::Typed)
            .on_submit(Message::Submitted),
    );

    let mut ui = simulator(root);
    let _ = ui.click("placeholder").expect("the input is on screen");
    let _ = ui.tap_key(keyboard::Key::Named(key::Named::Enter));

    assert!(ui.into_messages().any(|m| m == Message::Submitted));
}

#[test]
fn label_hint_and_error_lay_out_around_the_field() {
    let luminate = Luminate::new();
    let root = luminate.input(
        Input::new("placeholder", "")
            .label("Name")
            .hint("Required")
            .error(Some("Too short"))
            .on_input(Message::Typed),
    );

    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    let label = ui.find("Name").expect("label above").bounds();
    let field = ui.find("placeholder").expect("field").bounds();
    let hint = ui.find("Required").expect("hint below").bounds();
    assert!(
        label.y < field.y && field.y < hint.y,
        "{label:?} {field:?} {hint:?}"
    );
}

#[test]
fn the_sidebar_toggle_publishes_the_flipped_value() {
    let luminate = Luminate::new();
    let item: Element<'_, Message> = text("item").into();
    let root = luminate.sidebar(
        Sidebar::new([item])
            .width(200)
            .height(Length::Fill)
            .axis(Axis::Vertical)
            .show_toggle(true)
            .collapsed(false)
            .on_toggle(Message::Toggled),
    );

    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    // The toggle icon sits `(header_size - icon_size) / 2` = 12 px from the
    // sidebar's corner and is 20 px square.
    let tokens = Theme::LIGHT.sidebar;
    let center = (tokens.header_size / 2.0).round();
    ui.point_at(Point::new(center, center));
    let _ = ui.simulate(iced_test::simulator::click());

    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Message::Toggled(true)]
    );
}

#[test]
fn a_card_keeps_its_controls_inside_the_cap() {
    let luminate = Luminate::new();
    let tall: Element<'_, Message> = Space::new().height(1000).into();
    let root = luminate.card(
        Card::new("Title")
            .pages([tall], 0)
            .controls(luminate.button(Button::new("control").on_press(Message::Pressed)))
            .max_height(200),
    );

    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    let title = ui.find("Title").expect("header laid out").bounds();
    let control = ui.find("control").expect("controls laid out").bounds();
    assert!(
        title.y < control.y,
        "header above controls: {title:?} {control:?}"
    );
    assert!(
        control.y + control.height <= 200.0 + 0.5,
        "controls inside the 200 px cap: {control:?}"
    );
}

#[test]
fn a_pager_shows_the_current_page() {
    let luminate = Luminate::new();
    let pages: Vec<Element<'_, Message>> = vec![text("first").into(), text("second").into()];
    let root = luminate.pager(Pager::new(pages).current(1));

    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    let _ = ui.find("second").expect("the current page is laid out");
}

#[test]
fn a_custom_theme_reaches_the_widgets() {
    let theme = Theme {
        card: CardTheme {
            width: 333.0,
            ..Theme::LIGHT.card
        },
        ..Theme::LIGHT
    };
    let luminate = Luminate::with_theme(theme);
    let page: Element<'_, Message> = text("page").width(Length::Fill).into();
    let root = luminate.card(Card::new("Title").pages([page], 0));

    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);

    let page = ui.find("page").expect("page laid out").bounds();
    assert!(
        (page.width - 333.0).abs() < 1.0,
        "card width from the theme: {page:?}"
    );
}

#[test]
fn a_disabled_icon_button_tints_its_icon_like_its_label() {
    use iced_luminate::iced::widget::svg;
    use iced_luminate::theme::SvgClass;

    // `Luminate::button` gives the icon this class; the theme resolves it to
    // the same colour the label gets in the same status.
    let theme = Theme::LIGHT;
    let class = SvgClass::ButtonIcon {
        hierarchy: ButtonHierarchy::Secondary,
        disabled: true,
    };
    let tint = svg::Catalog::style(&theme, &class, svg::Status::Idle);
    let label = iced::widget::button::Catalog::style(
        &theme,
        &iced_luminate::theme::ButtonClass::Hierarchy(ButtonHierarchy::Secondary),
        iced::widget::button::Status::Disabled,
    );
    assert_eq!(tint.color, Some(label.text_color));
    assert_eq!(tint.color, Some(theme.button.secondary.disabled.text));

    // And the built button still lays out with the tinted icon inside.
    let luminate = Luminate::new();
    let root = luminate.button(Button::with_icon(icon()).label("disabled icon"));
    let mut ui = simulator(luminate.host(root));
    settle(&mut ui);
    let _ = ui.find("disabled icon").expect("icon button laid out");
}

#[test]
fn light_and_dark_render_a_button_differently() {
    // A broken `Catalog` (every class drawing the same thing) would make the
    // two looks render identically; the hash of the first is compared with
    // the second through iced_test's own snapshot machinery.
    let dir = std::env::temp_dir().join(format!(
        "luminate-smoke-{}-{}",
        std::process::id(),
        iced::time::Instant::now().elapsed().as_nanos()
    ));
    let path = dir.join("button");

    let render = |theme: Theme| {
        let luminate = Luminate::with_theme(theme);
        let root = luminate.button(Button::new("snapshot").on_press(Message::Pressed));
        let mut ui = simulator(luminate.host(root));
        settle(&mut ui);
        ui.snapshot(&theme).expect("headless snapshot")
    };

    // The first call records the hash; the second compares against it.
    assert!(
        render(Theme::LIGHT)
            .matches_hash(&path)
            .expect("write hash")
    );
    assert!(
        !render(Theme::DARK).matches_hash(&path).expect("read hash"),
        "LIGHT and DARK produced identical pixels"
    );
    let _ = std::fs::remove_dir_all(dir);
}
