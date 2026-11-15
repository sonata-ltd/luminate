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
fn following_a_target_inside_a_scrolled_nested_scrollable_uses_where_it_is_drawn() {
    let inner = fading_scrollable(iced::widget::column![
        Space::new().height(200),
        container(Space::new().width(180).height(40)).id("nested"),
        Space::new().height(200),
    ])
    .height(100);
    let content = iced::widget::column![inner, Space::new().height(300)].into();
    let mut h = Harness::new(content, None);
    h.frame();

    // Scroll the nested scrollable so the target sits at the top of the viewport.
    h.event(
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -200.0 },
        }),
        mouse::Cursor::Available(Point::new(40.0, 40.0)),
    );
    h.frames(60);
    assert_eq!(
        h.offset(),
        0.0,
        "the wheel belongs to the nested scrollable"
    );

    h.widget.follow = Some(Id::new("nested"));
    h.frames(180);
    assert_eq!(
        h.offset(),
        0.0,
        "an already visible target must not move the outer scrollable"
    );
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

#[test]
fn legacy_edge_fade_stays_vertical_and_per_side_builder_accepts_asymmetric_lengths() {
    let legacy: Widget = fading_scrollable(Space::new()).edge_fade(iced::Pixels(12.0));
    assert_eq!(
        legacy.edge_fade.unwrap().lengths,
        Padding::ZERO.top(12).bottom(12)
    );
    let lengths = Padding::ZERO.top(3).right(6).bottom(9).left(12);
    let configured: Widget = fading_scrollable(Space::new()).edge_fade_sides(lengths);
    assert_eq!(configured.edge_fade.unwrap().lengths, lengths);
}

#[test]
fn horizontal_fades_follow_hidden_content_while_vertical_edges_remain_opaque() {
    let content =
        container(Space::new().width(600).height(600)).style(|_: &crate::Theme| container::Style {
            background: Some(Color::WHITE.into()),
            ..Default::default()
        });
    let mut h = Harness::new(content.into(), None);
    h.widget = h
        .widget
        .horizontal()
        .edge_fade_sides(Padding::ZERO.left(20).right(8));
    h.node = h.widget.layout(
        &mut h.tree,
        &h.renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );

    let render = |h: &mut Harness| {
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
        h.renderer
            .screenshot(Size::new(200, 100), 1.0, Color::TRANSPARENT)
    };
    let alpha = |pixels: &[u8], x, y| pixels[((y * 200 + x) * 4 + 3) as usize];

    h.frame();
    let pixels = render(&mut h);
    assert_eq!(alpha(&pixels, 0, 50), 255);
    assert!(alpha(&pixels, 199, 50) < 40);
    assert_eq!(alpha(&pixels, 100, 0), 255);
    assert_eq!(alpha(&pixels, 100, 99), 255);

    h.tree
        .state
        .downcast_mut::<State>()
        .requested
        .to(AbsoluteOffset {
            x: Some(50.0),
            y: Some(50.0),
        });
    h.frames(180);
    let pixels = render(&mut h);
    assert!(alpha(&pixels, 0, 50) < 20);
    assert!(alpha(&pixels, 199, 50) < 40);
    assert_eq!(alpha(&pixels, 100, 0), 255);
    assert_eq!(alpha(&pixels, 100, 99), 255);

    h.tree
        .state
        .downcast_mut::<State>()
        .requested
        .to(AbsoluteOffset {
            x: Some(400.0),
            y: None,
        });
    h.frames(180);
    let pixels = render(&mut h);
    assert_eq!(alpha(&pixels, 199, 50), 255);
}

#[test]
fn four_side_fades_invalidate_the_intermediate_when_content_changes() {
    let make = |color| -> crate::Element<'static, ()> {
        container(Space::new().width(600).height(600))
            .style(move |_: &crate::Theme| container::Style {
                background: Some(color),
                ..Default::default()
            })
            .into()
    };
    let mut h = Harness::new(make(Color::WHITE.into()), None);
    h.widget = h.widget.horizontal().edge_fade_sides(20);
    h.node = h.widget.layout(
        &mut h.tree,
        &h.renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );
    h.frame();
    h.tree
        .state
        .downcast_mut::<State>()
        .requested
        .to(AbsoluteOffset {
            x: Some(50.0),
            y: Some(50.0),
        });
    h.frames(180);

    for color in [Color::WHITE, Color::from_rgb(0.0, 0.0, 1.0)] {
        let mut next = fading_scrollable(make(color.into()))
            .width(200)
            .height(100)
            .horizontal()
            .edge_fade_sides(20);
        next.diff(&mut h.tree);
        h.node = next.layout(
            &mut h.tree,
            &h.renderer,
            &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
        );
        h.widget = next;
        h.frame();
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
        let pixel = &rgba[((50 * 200 + 100) * 4)..((50 * 200 + 100) * 4 + 4)];
        let expected = if color == Color::WHITE {
            [255, 255, 255, 255]
        } else {
            [0, 0, 255, 255]
        };
        assert_eq!(
            pixel, expected,
            "both cached passes must reflect the latest content"
        );
    }
}

fn outset_event(
    h: &mut Harness,
    event: &Event,
    cursor: mouse::Cursor,
    viewport: Rectangle,
) -> bool {
    let mut messages = vec![];
    let mut shell = Shell::new(&mut messages);
    h.widget.update(
        &mut h.tree,
        event,
        Layout::new(&h.node),
        cursor,
        &h.renderer,
        &mut clipboard::Null,
        &mut shell,
        &viewport,
    );
    shell.is_event_captured()
}

fn outset_frame(h: &mut Harness, cursor: mouse::Cursor, viewport: Rectangle) {
    h.now += Duration::from_millis(16);
    let event = Event::Window(window::Event::RedrawRequested(h.now));
    outset_event(h, &event, cursor, viewport);
}

fn outset_pixels(h: &mut Harness, viewport: Rectangle, scale: f32) -> Vec<u8> {
    let size = Size::new((240.0 * scale) as u32, (140.0 * scale) as u32);
    let _ = h.renderer.screenshot(size, scale, Color::TRANSPARENT);
    h.renderer
        .reset(Rectangle::with_size(Size::new(240.0, 140.0)));
    h.widget.draw(
        &h.tree,
        &mut h.renderer,
        &crate::Theme::LIGHT,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        Layout::new(&h.node),
        mouse::Cursor::Unavailable,
        &viewport,
    );
    h.renderer.screenshot(size, scale, Color::TRANSPARENT)
}

#[test]
fn scrollbar_offsets_preserve_layout_and_move_only_their_own_axis() {
    let mut h = Harness::new(rows(), None);
    let geometry = |h: &Harness| {
        let layout = Layout::new(&h.node);
        (layout.bounds(), layout.children().next().unwrap().bounds())
    };
    let original = geometry(&h);
    let rail = h.widget.lanes(Layout::new(&h.node)).vertical.unwrap();
    h.widget = h.widget.scrollbar_offset(Vector::new(16.0, -8.0));
    h.node = h.widget.layout(
        &mut h.tree,
        &h.renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );
    assert_eq!(geometry(&h), original);
    let shifted = h.widget.lanes(Layout::new(&h.node)).vertical.unwrap();
    assert_eq!(shifted.x, rail.x + 16.0);
    assert_eq!(shifted.y, rail.y);
    assert_eq!(shifted.size(), rail.size());
    h.widget = h
        .widget
        .scrollbar_offset(Vector::new(f32::NAN, f32::INFINITY));
    assert_eq!(h.widget.bar.offset, Vector::ZERO);
    h.widget = h.widget.scrollbar_offset(Vector::new(-12.0, -8.0));
    assert_eq!(
        h.widget.lanes(Layout::new(&h.node)).vertical.unwrap().x,
        rail.x - 12.0
    );
}

#[test]
fn translated_two_axis_rails_and_hit_targets_never_overlap_at_the_corner() {
    let mut h = Harness::new(Space::new().width(600).height(600).into(), None);
    h.widget = h.widget.horizontal();
    h.node = h.widget.layout(
        &mut h.tree,
        &h.renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );
    for offset in [
        Vector::new(16.0, 16.0),
        Vector::new(-10.0, -10.0),
        Vector::ZERO,
    ] {
        h.widget = h.widget.scrollbar_offset(offset);
        let lanes = h.widget.lanes(Layout::new(&h.node));
        let hit = zones(h.widget.bar, lanes, Layout::new(&h.node).bounds());
        assert!(
            lanes
                .vertical
                .unwrap()
                .intersection(&lanes.horizontal.unwrap())
                .is_none()
        );
        assert!(
            hit.vertical
                .unwrap()
                .intersection(&hit.horizontal.unwrap())
                .is_none()
        );
        assert!(hit.vertical.unwrap().width >= 0.0);
        assert!(hit.horizontal.unwrap().height >= 0.0);
    }
}

#[test]
fn vertical_outset_bar_draws_hovers_drags_and_accepts_wheel_outside_content() {
    let viewport = Rectangle::with_size(Size::new(240.0, 140.0));
    for scale in [1.0, 1.25, 2.0] {
        let mut h = Harness::new(rows(), None);
        h.widget = h.widget.scrollbar_offset(Vector::new(16.0, 0.0));
        let cursor = mouse::Cursor::Available(Point::new(211.0, 12.0));
        outset_event(
            &mut h,
            &Event::Mouse(mouse::Event::CursorMoved {
                position: Point::new(211.0, 12.0),
            }),
            cursor,
            viewport,
        );
        outset_frame(&mut h, cursor, viewport);
        assert!(h.tree.state.downcast_ref::<State>().revealed().vertical >= GRABBABLE);
        let pixels = outset_pixels(&mut h, viewport, scale);
        let alpha = |x: f32, y: f32| {
            pixels[((((y * scale) as u32 * (240.0 * scale) as u32 + (x * scale) as u32) * 4) + 3)
                as usize]
        };
        assert!(
            alpha(211.0, 12.0) > 0,
            "bar must be painted beyond the 200px content at scale {scale}"
        );
        assert_eq!(
            alpha(195.0, 12.0),
            0,
            "old scrollbar position must stay clear"
        );
        assert!(outset_event(
            &mut h,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            cursor,
            viewport
        ));
        assert_eq!(
            h.tree.state.downcast_ref::<State>().dragging,
            Some(Axis::Vertical)
        );
        let moved = Point::new(211.0, 75.0);
        assert!(outset_event(
            &mut h,
            &Event::Mouse(mouse::Event::CursorMoved { position: moved }),
            mouse::Cursor::Available(moved),
            viewport
        ));
        assert!(h.offset() > 200.0);
        outset_event(
            &mut h,
            &Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)),
            mouse::Cursor::Unavailable,
            viewport,
        );
        assert_eq!(h.tree.state.downcast_ref::<State>().dragging, None);
    }

    let mut h = Harness::new(rows(), None);
    h.widget = h.widget.scrollbar_offset(Vector::new(16.0, 0.0));
    let cursor = mouse::Cursor::Available(Point::new(211.0, 50.0));
    outset_frame(&mut h, cursor, viewport);
    assert!(outset_event(
        &mut h,
        &Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -40.0 }
        }),
        cursor,
        viewport
    ));
    for _ in 0..180 {
        outset_frame(&mut h, cursor, viewport);
    }
    assert_eq!(h.offset(), 40.0);
}

#[test]
fn horizontal_outset_bar_draws_and_drags_below_the_content() {
    let viewport = Rectangle::with_size(Size::new(240.0, 140.0));
    let mut h = Harness::new(Space::new().width(600).height(60).into(), None);
    h.widget = h
        .widget
        .horizontal()
        .no_vertical()
        .scrollbar_offset(Vector::new(0.0, 16.0));
    h.node = h.widget.layout(
        &mut h.tree,
        &h.renderer,
        &layout::Limits::new(Size::ZERO, Size::new(200.0, 100.0)),
    );
    let cursor = mouse::Cursor::Available(Point::new(12.0, 111.0));
    outset_frame(&mut h, cursor, viewport);
    let pixels = outset_pixels(&mut h, viewport, 1.0);
    assert!(pixels[((111 * 240 + 12) * 4 + 3) as usize] > 0);
    assert!(outset_event(
        &mut h,
        &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
        cursor,
        viewport
    ));
    assert_eq!(
        h.tree.state.downcast_ref::<State>().dragging,
        Some(Axis::Horizontal)
    );
    let moved = Point::new(170.0, 111.0);
    outset_event(
        &mut h,
        &Event::Mouse(mouse::Event::CursorMoved { position: moved }),
        mouse::Cursor::Available(moved),
        viewport,
    );
    assert!(h.tree.state.downcast_ref::<State>().reach.horizontal.offset > 0.0);
}

#[test]
fn clipped_outset_rails_do_not_paint_hover_capture_or_scroll() {
    let viewport = Rectangle::with_size(Size::new(205.0, 100.0));
    for x in [204.5, 211.0] {
        let mut h = Harness::new(rows(), None);
        h.widget = h.widget.scrollbar_offset(Vector::new(16.0, 0.0));
        let cursor = mouse::Cursor::Available(Point::new(x, 12.0));
        outset_frame(&mut h, cursor, viewport);
        assert_eq!(
            h.tree.state.downcast_ref::<State>().revealed().vertical,
            0.0
        );
        assert!(!outset_event(
            &mut h,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            cursor,
            viewport
        ));
        assert_eq!(h.tree.state.downcast_ref::<State>().dragging, None);
        assert!(!outset_event(
            &mut h,
            &Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 }
            }),
            cursor,
            viewport
        ));
        for _ in 0..5 {
            outset_frame(&mut h, cursor, viewport);
        }
        assert_eq!(h.offset(), 0.0);
        assert!(
            outset_pixels(&mut h, viewport, 1.0)
                .chunks_exact(4)
                .all(|pixel| pixel[3] == 0)
        );
    }
}
