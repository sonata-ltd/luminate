//! Kit widgets through `iced_test`: a custom focus probe and touch on
//! `MultiBorder`, collapsed-vs-expanded sidebar geometry on both axes, and
//! the error bubble over a `pick_list` and at the right edge.

use std::cell::Cell;
use std::rc::Rc;

use iced_luminate::descriptor::{Axis, Button, Sidebar};
use iced_luminate::iced::advanced::Renderer as _;
use iced_luminate::iced::advanced::clipboard;
use iced_luminate::iced::advanced::renderer::{self, Headless};
use iced_luminate::iced::time::Instant;
use iced_luminate::iced::widget::{button, container, pick_list, row, space, text, text_input};
use iced_luminate::iced::{
    Color, Event, Length, Padding, Point, Rectangle, Settings, Size, mouse, touch, window,
};
use iced_luminate::texture::testing::headless_tiny_skia;
use iced_luminate::widget::error_bubble::error_bubble;
use iced_luminate::widget::multi_border::{Focus, Status, Style, multi_border};
use iced_luminate::{Element, Luminate, Renderer, Theme};
use iced_test::Simulator;
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_test::selector;

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Picked(&'static str),
    Edited(String),
}

type Plain<'a> = iced_luminate::iced::Element<'a, Message, iced_luminate::iced::Theme, Renderer>;

fn plain(
    root: Plain<'_>,
    size: Size,
) -> Simulator<'_, Message, iced_luminate::iced::Theme, Renderer> {
    Simulator::with_size(Settings::default(), size, root)
}

fn kit(root: Element<'_, Message>, size: Size) -> Simulator<'_, Message, Theme, Renderer> {
    Simulator::with_size(Settings::default(), size, root)
}

fn redraw() -> Event {
    Event::Window(window::Event::RedrawRequested(Instant::now()))
}

fn recording(
    status: Rc<Cell<Status>>,
    content: Plain<'static>,
    focus: Focus<'static>,
) -> Plain<'static> {
    multi_border(content)
        .focus(focus)
        .style(move |_, s| {
            status.set(s);
            Style::default()
        })
        .into()
}

#[test]
fn a_custom_focus_probe_drives_the_focused_state_without_a_click() {
    let status = Rc::new(Cell::new(Status::default()));
    let root = recording(
        status.clone(),
        text_input("type", "").on_input(Message::Edited).into(),
        Focus::Custom(Box::new(|_| true)),
    );
    let mut ui = plain(root, Size::new(300.0, 100.0));
    let _ = ui.simulate([redraw()]);
    let _ = ui
        .snapshot(&iced_luminate::iced::Theme::Light)
        .expect("draws");
    assert!(status.get().is_focused, "{:?}", status.get());
}

#[test]
fn a_touch_press_inside_focuses() {
    let status = Rc::new(Cell::new(Status::default()));
    let root = recording(status.clone(), button(text("tap")).into(), Focus::Click);
    let mut ui = plain(root, Size::new(300.0, 100.0));
    let centre = ui.find("tap").expect("on screen").bounds().center();
    ui.point_at(centre);
    let _ = ui.simulate([Event::Touch(touch::Event::FingerPressed {
        id: touch::Finger(0),
        position: centre,
    })]);
    let _ = ui
        .snapshot(&iced_luminate::iced::Theme::Light)
        .expect("draws");
    assert!(status.get().is_focused, "{:?}", status.get());
}

fn sidebar_bounds(axis: Axis, collapsed: bool) -> (Rectangle, Rectangle) {
    let luminate = Luminate::new();
    let items = [luminate.button(Button::new("item"))];
    let root: Element<'_, Message> = container(
        luminate.sidebar(
            Sidebar::new(items)
                .axis(axis)
                .collapsed(collapsed)
                .show_toggle(true)
                .width(Length::Shrink)
                .height(Length::Shrink),
        ),
    )
    .id("sidebar")
    .into();
    let mut ui = kit(luminate.host(root), Size::new(400.0, 400.0));
    let _ = ui.simulate([redraw()]);
    let sidebar = ui.find(selector::id("sidebar")).expect("laid out").bounds();
    let item = ui.find("item").expect("laid out").bounds();
    (sidebar, item)
}

#[test]
fn a_vertical_sidebar_collapses_its_width_and_keeps_children_below_the_header() {
    let (open, item_open) = sidebar_bounds(Axis::Vertical, false);
    let (closed, _) = sidebar_bounds(Axis::Vertical, true);
    assert!(
        closed.width < open.width,
        "collapsed {closed:?} vs open {open:?}"
    );
    assert!(
        item_open.y > open.y,
        "the header row comes first: {item_open:?} in {open:?}"
    );
}

#[test]
fn a_horizontal_sidebar_collapses_its_height_and_keeps_children_right_of_the_header() {
    let (open, item_open) = sidebar_bounds(Axis::Horizontal, false);
    let (closed, _) = sidebar_bounds(Axis::Horizontal, true);
    assert!(
        closed.height < open.height,
        "collapsed {closed:?} vs open {open:?}"
    );
    assert!(
        item_open.x > open.x,
        "the header column comes first: {item_open:?} in {open:?}"
    );
}

/// Draws `root` under `iced::Theme::Light` on the software backend and
/// returns the RGBA pixels.
fn render(root: Plain<'_>, size: Size) -> Vec<u8> {
    let mut renderer = headless_tiny_skia();
    let mut ui: UserInterface<'_, Message, iced_luminate::iced::Theme, Renderer> =
        UserInterface::build(root, size, user_interface::Cache::default(), &mut renderer);
    let mut messages = Vec::new();
    let _ = ui.update(
        &[redraw()],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );
    renderer.reset(Rectangle::with_size(size));
    ui.draw(
        &mut renderer,
        &iced_luminate::iced::Theme::Light,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );
    renderer.screenshot(
        Size::new(size.width as u32, size.height as u32),
        1.0,
        Color::WHITE,
    )
}

/// The bubble fill under `iced::Theme::Light`, as the compositor writes it.
fn bubble_fill() -> [u8; 3] {
    let color = iced_luminate::iced::Theme::Light
        .extended_palette()
        .danger
        .weak
        .color;
    let [r, g, b, _] = color.into_rgba8();
    [r, g, b]
}

/// Bounding box `(min_x, max_x, min_y, max_y)` of the pixels painted in
/// `fill`.
fn extent(rgba: &[u8], width: usize, fill: [u8; 3]) -> Option<(usize, usize, usize, usize)> {
    let mut span: Option<(usize, usize, usize, usize)> = None;
    for (i, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let close = px[..3].iter().zip(fill).all(|(a, b)| a.abs_diff(b) <= 2);
        if close {
            let (x, y) = (i % width, i / width);
            span = Some(span.map_or((x, x, y, y), |(l, r, t, b)| {
                (l.min(x), r.max(x), t.min(y), b.max(y))
            }));
        }
    }
    span
}

fn bubbled<'a>(padding: Padding) -> Plain<'a> {
    container(error_bubble(
        pick_list(["beta"], None::<&'static str>, Message::Picked),
        Some("something is wrong"),
    ))
    .padding(padding)
    .into()
}

#[test]
fn the_bubble_over_a_pick_list_still_lets_the_menu_open() {
    // `pick_list` reports no text to operations, so the test works from
    // geometry: the list sits at (0, 100) and is ~31 px tall; its single
    // option opens below it, spanning roughly y ∈ [131, 162]. The bubble
    // itself draws a paragraph, not a text widget, so it is checked by pixel.
    let top = Padding {
        top: 100.0,
        ..Padding::ZERO
    };
    let size = Size::new(400.0, 300.0);
    let rgba = render(bubbled(top), size);
    assert!(
        extent(&rgba, 400, bubble_fill()).is_some(),
        "the bubble is painted"
    );

    let mut ui = plain(bubbled(top), size);
    let _ = ui.simulate([redraw()]);
    ui.point_at(Point::new(10.0, 110.0));
    let _ = ui.simulate(iced_test::simulator::click());
    let _ = ui
        .snapshot(&iced_luminate::iced::Theme::Light)
        .expect("draws with the menu and the bubble");
    let option = Point::new(10.0, 145.0);
    ui.point_at(option);
    let _ = ui.simulate([Event::Mouse(mouse::Event::CursorMoved { position: option })]);
    let _ = ui.simulate(iced_test::simulator::click());
    assert_eq!(
        ui.into_messages().collect::<Vec<_>>(),
        vec![Message::Picked("beta")],
        "the menu opened through the bubble's overlay group"
    );
}

#[test]
fn the_bubble_is_clamped_to_the_right_edge() {
    let view = || -> Plain<'_> {
        row![
            space().width(Length::Fill),
            error_bubble(text("x"), Some("a rather long message that would overflow")),
        ]
        .padding(40)
        .into()
    };
    // Room to spare: the bubble's natural size.
    let wide = render(view(), Size::new(600.0, 200.0));
    let (l, r, _, _) = extent(&wide, 600, bubble_fill()).expect("the bubble is painted");
    let natural_width = r - l;
    assert!(natural_width > 60, "a readable bubble: {natural_width} px");

    // Use a viewport narrower than the bubble's natural width on every platform.
    // Font metrics vary, so keep it well below the smallest width observed. The
    // bubble should stay inside the viewport and wrap its message instead of
    // being cut off.
    let narrow = render(view(), Size::new(180.0, 200.0));
    let (l, r, _, _) = extent(&narrow, 180, bubble_fill()).expect("the bubble is painted");
    assert!(r < 179, "clamped inside the viewport: {l}..={r}");
    assert!(
        r - l < natural_width,
        "re-wrapped to the narrower viewport: {} vs {natural_width}",
        r - l
    );
}

/// The fading scrollable driven event by event, reading back the offset it
/// actually left the content at.
mod fading_scrollable {
    use std::time::Duration;

    use iced_luminate::Renderer;
    use iced_luminate::iced::advanced::clipboard;
    use iced_luminate::iced::advanced::renderer::Headless;
    use iced_luminate::iced::advanced::widget::{Operation, Tree, operation};
    use iced_luminate::iced::advanced::{Layout, Shell, Widget, layout, mouse};
    use iced_luminate::iced::time::Instant;
    use iced_luminate::iced::widget::{container, text};
    use iced_luminate::iced::{self, Event, Length, Point, Rectangle, Size, Vector};
    use iced_luminate::widget::fading_scrollable::{FadingScrollable, fading_scrollable};

    const FRAME: Size = Size::new(200.0, 100.0);

    /// A 900 px page in a 200 x 100 frame, reporting its vertical offset.
    struct Rig {
        widget: FadingScrollable<'static, f32, iced::Theme, Renderer>,
        tree: Tree,
        renderer: Renderer,
        node: layout::Node,
        now: Instant,
    }

    impl Rig {
        fn new() -> Self {
            let renderer =
                iced_test::futures::futures::executor::block_on(<Renderer as Headless>::new(
                    iced::Font::DEFAULT,
                    iced::Pixels(16.0),
                    Some("tiny-skia"),
                ))
                .expect("tiny_skia needs no GPU");
            let mut widget = fading_scrollable(container(text("a long page")).height(900.0))
                .width(Length::Fill)
                .height(Length::Fill)
                .on_scroll(|viewport| viewport.absolute_offset().y);
            let mut tree = Tree::new(&widget as &dyn Widget<f32, iced::Theme, Renderer>);
            let node = widget.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, FRAME),
            );

            Self {
                widget,
                tree,
                renderer,
                node,
                now: Instant::now(),
            }
        }

        /// Hands the widget `event` with the cursor at `at`; returns what it
        /// published.
        fn event(&mut self, event: &Event, at: Point) -> Vec<f32> {
            let mut messages = Vec::new();
            self.widget.update(
                &mut self.tree,
                event,
                Layout::new(&self.node),
                mouse::Cursor::Available(at),
                &self.renderer,
                &mut clipboard::Null,
                &mut Shell::new(&mut messages),
                &Rectangle::with_size(FRAME),
            );
            messages
        }

        /// The next 16 ms frame.
        fn frame(&mut self, at: Point) -> Vec<f32> {
            self.now += Duration::from_millis(16);
            let now = self.now;
            self.event(
                &Event::Window(iced::window::Event::RedrawRequested(now)),
                at,
            )
        }

        /// The vertical offset the content is drawn at.
        fn offset(&mut self) -> f32 {
            #[derive(Default)]
            struct Read(f32);

            impl Operation for Read {
                fn traverse(&mut self, _: &mut dyn FnMut(&mut dyn Operation)) {}

                fn scrollable(
                    &mut self,
                    _: Option<&iced::advanced::widget::Id>,
                    _: Rectangle,
                    _: Rectangle,
                    translation: Vector,
                    _: &mut dyn operation::Scrollable,
                ) {
                    self.0 = translation.y;
                }
            }

            let mut read = Read::default();
            self.widget.operate(
                &mut self.tree,
                Layout::new(&self.node),
                &self.renderer,
                &mut read,
            );
            read.0
        }
    }

    /// A press on the rail below the pill used to set only the grip, and
    /// the pill went there on the first movement after it: a click with a
    /// still mouse did nothing.
    #[test]
    fn a_click_on_the_rail_scrolls_without_the_cursor_moving() {
        let mut rig = Rig::new();
        let rail = Point::new(195.0, 75.0);

        // With no engine the reveal is immediate, so the bar is grabbable.
        let _ = rig.frame(rail);
        let _ = rig.event(
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            rail,
        );
        let _ = rig.event(
            &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            rail,
        );
        let _ = rig.frame(rail);

        assert!(rig.offset() > 0.0, "the click scrolled: {}", rig.offset());
    }

    /// The inner scrollable reported a wheel turn's destination before the
    /// widget pulled the content back to glide there, then reported where
    /// it really was: one notch down read `[60, 0]` and then the glide.
    #[test]
    fn a_smooth_wheel_turn_reports_only_offsets_the_content_is_drawn_at() {
        let mut rig = Rig::new();
        let cursor = Point::new(50.0, 50.0);
        let _ = rig.frame(cursor);

        let mut reports = rig.event(
            &Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            }),
            cursor,
        );
        let mut drawn = vec![0.0, rig.offset()];

        for _ in 0..120 {
            reports.extend(rig.frame(cursor));
            drawn.push(rig.offset());
        }

        let rest = rig.offset();
        assert!(rest > 0.0, "the turn scrolled");
        assert!(
            reports.windows(2).all(|pair| pair[1] >= pair[0]),
            "one notch down never reports going back up: {reports:?}"
        );
        assert!(
            reports.iter().all(|report| drawn.contains(report)),
            "every report is an offset that was drawn: {reports:?}, drawn {drawn:?}"
        );
        assert_eq!(reports.last(), Some(&rest), "the resting offset is heard");
    }
}
