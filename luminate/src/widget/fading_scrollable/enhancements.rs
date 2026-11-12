//! Regression coverage for following, fractional travel and content alpha.
use super::*;
use iced::advanced::{Renderer as _, Widget as _};
use iced::advanced::{clipboard, renderer::Headless};
use iced::widget::{Column, Id, Space};
use iced::{Point, Size};

type Widget = FadingScrollable<'static, (), crate::Theme, crate::Renderer>;
struct Harness {
    widget: Widget,
    tree: Tree,
    renderer: crate::Renderer,
    node: layout::Node,
    now: Instant,
}
impl Harness {
    fn new(content: crate::Element<'static, ()>, target: Option<Id>) -> Self {
        let renderer = iced_texture_cache::testing::headless_tiny_skia();
        let mut widget = fading_scrollable(content)
            .width(200)
            .height(100)
            .subpixel_scroll(true)
            .edge_fade(20)
            .follow(target);
        let mut tree =
            Tree::new(&widget as &dyn iced::advanced::Widget<(), crate::Theme, crate::Renderer>);
        let node = widget.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
        );
        Self {
            widget,
            tree,
            renderer,
            node,
            now: Instant::now(),
        }
    }
    fn event(&mut self, event: &Event, cursor: mouse::Cursor) -> window::RedrawRequest {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        self.widget.update(
            &mut self.tree,
            event,
            Layout::new(&self.node),
            cursor,
            &self.renderer,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::with_size(Size::new(200.0, 100.0)),
        );
        shell.redraw_request()
    }
    fn frame(&mut self) -> window::RedrawRequest {
        self.now += Duration::from_millis(16);
        self.event(
            &Event::Window(window::Event::RedrawRequested(self.now)),
            mouse::Cursor::Unavailable,
        )
    }
    fn frames(&mut self, count: usize) -> window::RedrawRequest {
        let mut request = window::RedrawRequest::Wait;
        for _ in 0..count {
            request = self.frame();
        }
        request
    }
    fn offset(&self) -> f32 {
        self.tree
            .state
            .downcast_ref::<State>()
            .reach
            .vertical
            .offset
    }
}
fn rows() -> crate::Element<'static, ()> {
    Column::with_children((0..8).map(|i| {
        container(Space::new().width(180).height(60))
            .id(Id::from(format!("row-{i}")))
            .into()
    }))
    .into()
}

#[test]
fn follow_glides_fractionally_and_manual_scrolling_suspends_until_target_changes() {
    let mut h = Harness::new(rows(), Some(Id::from("row-7".to_owned())));
    let mut fractional = false;
    for _ in 0..180 {
        h.frame();
        fractional |= h.widget.content_offset.get().y.abs() > 0.001;
    }
    assert!(fractional, "smooth travel must preserve subpixel offsets");
    assert_eq!(h.offset(), 380.0);
    assert_eq!(h.frames(10), window::RedrawRequest::Wait);
    h.event(
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 3.0 },
        }),
        mouse::Cursor::Available(Point::new(40.0, 40.0)),
    );
    h.frames(180);
    let manual = h.offset();
    assert!(manual < 380.0);
    assert!(!h.tree.state.downcast_ref::<State>().following);
    h.frames(30);
    assert_eq!(h.offset(), manual);
    h.widget.follow = Some(Id::from("row-0".to_owned()));
    h.frames(180);
    assert_eq!(h.offset(), 0.0);
}

#[test]
fn following_remeasures_a_shrinking_target_in_a_hundred_pixel_viewport() {
    let make = |height| -> crate::Element<'static, ()> {
        iced::widget::column![
            Space::new().height(140),
            container(Space::new().height(height)).id("last")
        ]
        .into()
    };
    let mut h = Harness::new(make(120), Some(Id::new("last")));
    h.frames(3); // Complete the stage while the glide is still underway.
    let mut next = fading_scrollable(make(24))
        .width(200)
        .height(100)
        .subpixel_scroll(true)
        .edge_fade(20)
        .follow(Id::new("last"));
    next.diff(&mut h.tree);
    h.node = next.layout(
        &mut h.tree,
        &h.renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );
    h.widget = next;
    h.frames(180);
    assert_eq!(h.offset(), 64.0);
    assert_eq!(h.frames(10), window::RedrawRequest::Wait);
}

#[test]
fn fade_multiplies_alpha_only_where_content_overflows_and_leaves_scrollbars_separate() {
    let content =
        container(Space::new().width(200).height(600)).style(|_: &crate::Theme| container::Style {
            background: Some(Color::WHITE.into()),
            ..Default::default()
        });
    let mut h = Harness::new(content.into(), None);
    h.frame();
    h.widget.draw(
        &h.tree,
        &mut h.renderer,
        &crate::Theme::LIGHT,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        Layout::new(&h.node),
        mouse::Cursor::Unavailable,
        &Rectangle::with_size(Size::new(200.0, 100.0)),
    );
    let rgba = h
        .renderer
        .screenshot(Size::new(200, 100), 1.0, Color::TRANSPARENT);
    let alpha = |y| rgba[((y * 200 + 40) * 4 + 3) as usize];
    assert_eq!(alpha(0), 255);
    assert_eq!(alpha(50), 255);
    assert!(alpha(99) > 0 && alpha(99) < 20);

    h.tree
        .state
        .downcast_mut::<State>()
        .requested
        .to(AbsoluteOffset {
            x: None,
            y: Some(50.0),
        });
    h.frames(180);
    h.renderer
        .reset(Rectangle::with_size(Size::new(200.0, 100.0)));
    h.widget.draw(
        &h.tree,
        &mut h.renderer,
        &crate::Theme::LIGHT,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        Layout::new(&h.node),
        mouse::Cursor::Unavailable,
        &Rectangle::with_size(Size::new(200.0, 100.0)),
    );
    let rgba = h
        .renderer
        .screenshot(Size::new(200, 100), 1.0, Color::TRANSPARENT);
    assert!(rgba[(40 * 4 + 3) as usize] < 20);
    assert_eq!(rgba[((50 * 200 + 40) * 4 + 3) as usize], 255);
}

struct CountDraw(Rc<Cell<usize>>);
impl iced::advanced::Widget<(), crate::Theme, crate::Renderer> for CountDraw {
    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fixed(900.0))
    }
    fn layout(&mut self, _: &mut Tree, _: &crate::Renderer, _: &layout::Limits) -> layout::Node {
        layout::Node::new(Size::new(200.0, 900.0))
    }
    fn draw(
        &self,
        _: &Tree,
        _: &mut crate::Renderer,
        _: &crate::Theme,
        _: &renderer::Style,
        _: Layout<'_>,
        _: mouse::Cursor,
        _: &Rectangle,
    ) {
        self.0.set(self.0.get() + 1);
    }
}

#[test]
fn scrollbar_animation_repaints_inside_a_cached_pager_without_application_ticks() {
    use iced::advanced::renderer::Style;
    let kit = crate::Luminate::with_theme(crate::Theme::LIGHT);
    let draws = Rc::new(Cell::new(0));
    let scrollable = fading_scrollable(Element::new(CountDraw(Rc::clone(&draws))))
        .height(100)
        .width(200)
        .motion(kit.motion().clone())
        .edge_fade(20);
    let root = kit.host(
        kit.pager(
            crate::descriptor::Pager::new([
                Element::from(Space::new().height(100).width(200)),
                scrollable.into(),
            ])
            .current(1),
        ),
    );
    let mut renderer = iced_texture_cache::testing::headless_tiny_skia();
    let mut ui = iced_runtime::user_interface::UserInterface::build(
        root,
        Size::new(200.0, 100.0),
        iced_runtime::user_interface::Cache::default(),
        &mut renderer,
    );
    let mut now = Instant::now();
    for _ in 0..100 {
        now += Duration::from_millis(16);
        ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            mouse::Cursor::Unavailable,
            &mut renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        ui.draw(
            &mut renderer,
            &crate::Theme::LIGHT,
            &Style {
                text_color: Color::BLACK,
            },
            mouse::Cursor::Unavailable,
        );
    }
    let cursor = mouse::Cursor::Available(Point::new(197.0, 40.0));
    ui.update(
        &[Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(197.0, 40.0),
        })],
        cursor,
        &mut renderer,
        &mut clipboard::Null,
        &mut Vec::new(),
    );
    for frame in 0..12 {
        now += Duration::from_millis(16);
        ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            cursor,
            &mut renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        let before = draws.get();
        ui.draw(
            &mut renderer,
            &crate::Theme::LIGHT,
            &Style {
                text_color: Color::BLACK,
            },
            cursor,
        );
        assert!(
            draws.get() > before,
            "frame {frame}: Pager reused stale scrollbar appearance"
        );
    }
}
