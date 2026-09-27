//! The path widget draws what its values say, draws nothing for an empty
//! range, matches iced's own canvas for a full one, and reuses its geometry
//! while nothing moves.

use std::sync::Arc;
use std::time::Duration;

use iced::advanced::renderer::{self, Headless};
use iced::widget::canvas;
use iced::{Color, Event, Font, Pixels, Point, Rectangle, Size, mouse, window};
use iced_animate::path::{DrawRange, Fit, Morph, PathData};
use iced_animate::testing::{self, FrameClock};
use iced_animate::widget::{FillRule, LineCap, path};
use iced_animate::{Curve, Easing, Motion, SpringParams, key};
use iced_test::runtime::user_interface::{self, State, UserInterface};

const SIDE: u32 = 100;
const SIZE: Size = Size::new(100.0, 100.0);

type Ui<'a> = UserInterface<'a, (), iced::Theme, iced::Renderer>;

fn backend() -> iced::Renderer {
    iced_test::futures::futures::executor::block_on(<iced::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("tiny-skia"),
    ))
    .expect("tiny_skia needs no GPU")
}

fn build<'a>(root: iced::Element<'a, ()>, backend: &mut iced::Renderer) -> Ui<'a> {
    UserInterface::build(root, SIZE, user_interface::Cache::default(), backend)
}

fn draw(ui: &mut Ui<'_>, backend: &mut iced::Renderer) -> Vec<u8> {
    ui.draw(
        backend,
        &iced::Theme::Light,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );
    backend.screenshot(Size::new(SIDE, SIDE), 1.0, Color::WHITE)
}

fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 3] {
    let i = ((y * SIDE + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2]]
}

/// A horizontal line across the middle, in widget pixels.
fn bar() -> Arc<PathData> {
    Arc::new(
        PathData::builder()
            .move_to(Point::new(10.0, 50.0))
            .line_to(Point::new(90.0, 50.0))
            .build()
            .unwrap(),
    )
}

/// One S-curve, as explicit cubics so the canvas reference can repeat it.
fn curve() -> Arc<PathData> {
    Arc::new(
        PathData::builder()
            .move_to(Point::new(10.0, 10.0))
            .cubic_to(
                Point::new(40.0, 10.0),
                Point::new(60.0, 90.0),
                Point::new(90.0, 90.0),
            )
            .build()
            .unwrap(),
    )
}

/// Widget pixels are path units: the view box is the widget's own square.
fn one_to_one() -> Rectangle {
    Rectangle::new(Point::ORIGIN, SIZE)
}

/// A vertical bar near the left edge.
fn left_bar() -> Arc<PathData> {
    Arc::new(
        PathData::builder()
            .move_to(Point::new(20.0, 10.0))
            .line_to(Point::new(20.0, 90.0))
            .build()
            .unwrap(),
    )
}

/// A vertical bar near the right edge.
fn right_bar() -> Arc<PathData> {
    Arc::new(
        PathData::builder()
            .move_to(Point::new(80.0, 10.0))
            .line_to(Point::new(80.0, 90.0))
            .build()
            .unwrap(),
    )
}

/// A vertical bar down the middle.
fn middle_bar() -> Arc<PathData> {
    Arc::new(
        PathData::builder()
            .move_to(Point::new(50.0, 10.0))
            .line_to(Point::new(50.0, 90.0))
            .build()
            .unwrap(),
    )
}

/// A filled square, in widget pixels.
fn square() -> PathData {
    PathData::builder()
        .move_to(Point::new(20.0, 20.0))
        .line_to(Point::new(80.0, 20.0))
        .line_to(Point::new(80.0, 80.0))
        .line_to(Point::new(20.0, 80.0))
        .close()
        .build()
        .unwrap()
}

/// Two squares wound the same way, one inside the other: solid under
/// [`FillRule::NonZero`] (winding 2 in the middle is still non-zero), with a
/// hole in the middle under [`FillRule::EvenOdd`] (winding 2 is even).
fn nested_squares() -> PathData {
    PathData::builder()
        .move_to(Point::new(10.0, 10.0))
        .line_to(Point::new(90.0, 10.0))
        .line_to(Point::new(90.0, 90.0))
        .line_to(Point::new(10.0, 90.0))
        .close()
        .move_to(Point::new(30.0, 30.0))
        .line_to(Point::new(70.0, 30.0))
        .line_to(Point::new(70.0, 70.0))
        .line_to(Point::new(30.0, 70.0))
        .close()
        .build()
        .unwrap()
}

#[test]
fn an_empty_range_draws_not_even_the_round_caps() {
    let mut backend = backend();
    let root = path(bar())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::BLACK, 8.0)
        .draw(DrawRange::EMPTY)
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert!(
        rgba.as_chunks::<4>()
            .0
            .iter()
            .all(|px| px[..3] == [255, 255, 255]),
        "an empty range leaves the canvas white"
    );
}

#[test]
fn half_a_range_draws_the_first_half_only() {
    let mut backend = backend();
    let root = path(bar())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::BLACK, 6.0)
        .draw(DrawRange::new(0.0, 0.5))
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert_eq!(pixel(&rgba, 20, 50), [0, 0, 0], "near the start: drawn");
    assert_eq!(
        pixel(&rgba, 80, 50),
        [255, 255, 255],
        "near the end: not drawn"
    );
}

#[test]
fn a_full_range_is_exactly_what_the_canvas_draws() {
    struct Reference;

    impl canvas::Program<()> for Reference {
        type State = ();

        fn draw(
            &self,
            _state: &(),
            renderer: &iced::Renderer,
            _theme: &iced::Theme,
            bounds: Rectangle,
            _cursor: mouse::Cursor,
        ) -> Vec<canvas::Geometry> {
            let mut frame = canvas::Frame::new(renderer, bounds.size());
            let line = canvas::Path::new(|b| {
                b.move_to(Point::new(10.0, 10.0));
                b.bezier_curve_to(
                    Point::new(40.0, 10.0),
                    Point::new(60.0, 90.0),
                    Point::new(90.0, 90.0),
                );
            });
            frame.stroke(
                &line,
                canvas::Stroke {
                    style: canvas::Style::Solid(Color::BLACK),
                    width: 4.0,
                    line_cap: canvas::LineCap::Round,
                    line_join: canvas::LineJoin::Round,
                    ..canvas::Stroke::default()
                },
            );
            vec![frame.into_geometry()]
        }
    }

    let mut backend = backend();
    let ours = path(curve())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::BLACK, 4.0)
        .into();
    let mut ui = build(ours, &mut backend);
    let drawn = draw(&mut ui, &mut backend);

    let reference = canvas(Reference).width(100.0).height(100.0).into();
    let mut ui = build(reference, &mut backend);
    let expected = draw(&mut ui, &mut backend);

    assert!(drawn == expected, "the widget and the canvas differ");
}

#[test]
fn geometry_is_rebuilt_only_when_a_value_changes() {
    let mut backend = backend();
    let m = Motion::new();
    let mut clock = FrameClock::new(&m);
    let range = m.play(
        key!(),
        Curve::ease(Easing::Linear, Duration::from_millis(100)),
        DrawRange::EMPTY,
        DrawRange::FULL,
    );

    let root = path(bar())
        .width(100.0)
        .height(100.0)
        .stroke(Color::BLACK, 4.0)
        .draw(&range)
        .into();
    let mut ui = build(root, &mut backend);

    let start = testing::path_geometry_builds();
    let _ = draw(&mut ui, &mut backend);
    let _ = draw(&mut ui, &mut backend);
    assert_eq!(
        testing::path_geometry_builds() - start,
        1,
        "a still frame reuses the geometry"
    );

    let _ = clock.run(3);
    let _ = draw(&mut ui, &mut backend);
    assert_eq!(
        testing::path_geometry_builds() - start,
        2,
        "a moved range rebuilds it once"
    );
}

#[test]
fn a_posed_path_is_drawn_about_its_origin_at_the_pose() {
    use iced::Radians;
    use iced_animate::path::Pose;

    // A 10 px square centred on the path's origin.
    let marker = PathData::builder()
        .move_to(Point::new(-5.0, -5.0))
        .line_to(Point::new(5.0, -5.0))
        .line_to(Point::new(5.0, 5.0))
        .line_to(Point::new(-5.0, 5.0))
        .close()
        .build()
        .unwrap();

    let mut backend = backend();
    let root = path(marker)
        .width(100.0)
        .height(100.0)
        .fill(Color::BLACK)
        .pose(Pose {
            position: Point::new(70.0, 30.0),
            angle: Radians(std::f32::consts::FRAC_PI_4),
        })
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert_eq!(pixel(&rgba, 70, 30), [0, 0, 0], "centred on the pose");
    assert_eq!(
        pixel(&rgba, 70, 25),
        [0, 0, 0],
        "turned 45°: a corner points up"
    );
    assert_eq!(
        pixel(&rgba, 65, 25),
        [255, 255, 255],
        "and the old corner is gone"
    );
    assert_eq!(pixel(&rgba, 10, 10), [255, 255, 255]);
}

#[test]
fn a_morph_draws_its_ends_at_zero_and_one() {
    let morph = Arc::new(Morph::new(&left_bar(), &right_bar()));
    let mut backend = backend();

    for (progress, dark, light) in [(0.0, 20, 80), (1.0, 80, 20)] {
        let root = path(&morph)
            .width(100.0)
            .height(100.0)
            .view_box(one_to_one())
            .fit(Fit::None)
            .stroke(Color::BLACK, 6.0)
            .progress(progress)
            .into();
        let mut ui = build(root, &mut backend);
        let rgba = draw(&mut ui, &mut backend);
        assert_eq!(pixel(&rgba, dark, 50), [0, 0, 0], "progress {progress}");
        assert_eq!(
            pixel(&rgba, light, 50),
            [255, 255, 255],
            "progress {progress}"
        );
    }
}

#[test]
fn a_new_target_asks_for_frames_until_the_morph_settles() {
    let mut backend = backend();
    let view = |target: Arc<PathData>| -> iced::Element<'static, ()> {
        path(target)
            .width(100.0)
            .height(100.0)
            .view_box(one_to_one())
            .fit(Fit::None)
            .stroke(Color::BLACK, 6.0)
            .morph(SpringParams::default())
            .into()
    };

    let first = build(view(left_bar()), &mut backend);
    let mut ui = UserInterface::build(view(right_bar()), SIZE, first.into_cache(), &mut backend);

    let mut now = iced::time::Instant::now();
    let mut requests = Vec::new();
    for _ in 0..120 {
        let mut messages = Vec::new();
        let (state, _) = ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            mouse::Cursor::Unavailable,
            &mut backend,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        let State::Updated { redraw_request, .. } = state else {
            panic!("nothing here invalidates widgets");
        };
        requests.push(redraw_request);
        now += Duration::from_millis(16);
    }

    assert_eq!(
        requests[0],
        window::RedrawRequest::NextFrame,
        "the retarget asks for frames"
    );
    let settled = requests
        .iter()
        .position(|r| *r == window::RedrawRequest::Wait)
        .expect("a 400 ms spring settles within two seconds");
    assert!(settled > 3, "settled suspiciously early at frame {settled}");
    assert!(
        requests[settled..]
            .iter()
            .all(|r| *r == window::RedrawRequest::Wait)
    );

    let rgba = draw(&mut ui, &mut backend);
    assert_eq!(pixel(&rgba, 80, 50), [0, 0, 0], "at rest on the new target");
}

#[test]
fn a_moved_view_box_is_not_masked_by_a_stale_cache() {
    // Review probe: the same `Arc`, only the view box moved. If the cache
    // key does not cover `view_box`, the rebuild is mistaken for a cache hit
    // and the bar stays drawn at the old place.
    let mut backend = backend();
    let shape = bar();
    let view = move |view_box: Rectangle| -> iced::Element<'static, ()> {
        path(&shape)
            .width(100.0)
            .height(100.0)
            .view_box(view_box)
            .fit(Fit::None)
            .stroke(Color::BLACK, 6.0)
            .into()
    };

    let mut first = build(view(one_to_one()), &mut backend);
    let _ = draw(&mut first, &mut backend);
    let moved = Rectangle::new(Point::new(0.0, 30.0), SIZE);
    let mut ui = UserInterface::build(view(moved), SIZE, first.into_cache(), &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert_eq!(
        pixel(&rgba, 50, 20),
        [0, 0, 0],
        "the bar follows the moved view box"
    );
    assert_eq!(
        pixel(&rgba, 50, 50),
        [255, 255, 255],
        "not stuck at the old place"
    );
}

#[test]
fn a_different_line_cap_is_not_masked_by_a_stale_cache() {
    // Review probe: Round to Butt on the same `Arc`. A cache key missing
    // `line_cap` would keep drawing the old, rounded end.
    let mut backend = backend();
    let shape = bar();
    let view = move |cap: LineCap| -> iced::Element<'static, ()> {
        path(&shape)
            .width(100.0)
            .height(100.0)
            .view_box(one_to_one())
            .fit(Fit::None)
            .stroke(Color::BLACK, 8.0)
            .line_cap(cap)
            .into()
    };

    let mut ui = build(view(LineCap::Round), &mut backend);
    let round = draw(&mut ui, &mut backend);
    assert_eq!(
        pixel(&round, 7, 50),
        [0, 0, 0],
        "a round cap extends past the end"
    );

    let mut ui = UserInterface::build(view(LineCap::Butt), SIZE, ui.into_cache(), &mut backend);
    let butt = draw(&mut ui, &mut backend);
    assert_eq!(
        pixel(&butt, 7, 50),
        [255, 255, 255],
        "a butt cap does not extend past the end"
    );
}

#[test]
fn fill_paints_the_interior() {
    let mut backend = backend();
    let root = path(square())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .fill(Color::BLACK)
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert_eq!(pixel(&rgba, 50, 50), [0, 0, 0], "the interior is filled");
    assert_eq!(pixel(&rgba, 5, 5), [255, 255, 255], "outside stays white");
}

#[test]
fn fill_follows_draw_hides_the_fill_under_a_partial_range_by_default() {
    let mut backend = backend();
    let root = path(square())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .fill(Color::BLACK)
        .draw(DrawRange::new(0.0, 0.5))
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert_eq!(
        pixel(&rgba, 50, 50),
        [255, 255, 255],
        "the fill waits for the whole range by default"
    );
}

#[test]
fn fill_follows_draw_false_shows_the_fill_under_a_partial_range() {
    let mut backend = backend();
    let root = path(square())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .fill(Color::BLACK)
        .draw(DrawRange::new(0.0, 0.5))
        .fill_follows_draw(false)
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    assert_eq!(
        pixel(&rgba, 50, 50),
        [0, 0, 0],
        "fill_follows_draw(false) paints regardless of the drawn range"
    );
}

#[test]
fn even_odd_fill_rule_leaves_a_hole_where_non_zero_fills_solid() {
    let mut backend = backend();

    let root = path(nested_squares())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .fill(Color::BLACK)
        .into();
    let mut ui = build(root, &mut backend);
    let non_zero = draw(&mut ui, &mut backend);
    assert_eq!(
        pixel(&non_zero, 50, 50),
        [0, 0, 0],
        "non-zero fills the doubly-wound inner square too"
    );

    let root = path(nested_squares())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .fill(Color::BLACK)
        .fill_rule(FillRule::EvenOdd)
        .into();
    let mut ui = build(root, &mut backend);
    let even_odd = draw(&mut ui, &mut backend);
    assert_eq!(
        pixel(&even_odd, 50, 50),
        [255, 255, 255],
        "even-odd leaves the doubly-wound inner square as a hole"
    );
    assert_eq!(
        pixel(&even_odd, 20, 20),
        [0, 0, 0],
        "the singly-wound annulus is still filled"
    );
}

#[test]
fn a_settled_morph_reuses_its_geometry_across_redraws() {
    let mut backend = backend();
    let view = |target: Arc<PathData>| -> iced::Element<'static, ()> {
        path(target)
            .width(100.0)
            .height(100.0)
            .view_box(one_to_one())
            .fit(Fit::None)
            .stroke(Color::BLACK, 6.0)
            .morph(SpringParams::default())
            .into()
    };

    let first = build(view(left_bar()), &mut backend);
    let mut ui = UserInterface::build(view(right_bar()), SIZE, first.into_cache(), &mut backend);

    let mut now = iced::time::Instant::now();
    let mut settled = false;
    for _ in 0..200 {
        let mut messages = Vec::new();
        let (state, _) = ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            mouse::Cursor::Unavailable,
            &mut backend,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        now += Duration::from_millis(16);
        let State::Updated { redraw_request, .. } = state else {
            panic!("nothing here invalidates widgets");
        };
        if redraw_request == window::RedrawRequest::Wait {
            settled = true;
            break;
        }
    }
    assert!(settled, "a 400 ms spring settles within a few seconds");

    let start = testing::path_geometry_builds();
    let _ = draw(&mut ui, &mut backend);
    let _ = draw(&mut ui, &mut backend);
    assert_eq!(
        testing::path_geometry_builds() - start,
        1,
        "a settled morph reuses its geometry across redraws"
    );
}

#[test]
fn a_retarget_mid_flight_settles_on_the_third_target_without_jumping_back() {
    let mut backend = backend();
    let view = |target: Arc<PathData>| -> iced::Element<'static, ()> {
        path(target)
            .width(100.0)
            .height(100.0)
            .view_box(one_to_one())
            .fit(Fit::None)
            .stroke(Color::BLACK, 6.0)
            .morph(SpringParams::default())
            .into()
    };

    let first = build(view(left_bar()), &mut backend);
    let mut ui = UserInterface::build(view(right_bar()), SIZE, first.into_cache(), &mut backend);

    let mut now = iced::time::Instant::now();
    // A few ticks into the first morph: retargeting below happens mid-flight.
    for _ in 0..3 {
        let mut messages = Vec::new();
        let _ = ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            mouse::Cursor::Unavailable,
            &mut backend,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        now += Duration::from_millis(16);
    }

    let mut ui = UserInterface::build(view(middle_bar()), SIZE, ui.into_cache(), &mut backend);

    let mut settled = false;
    for _ in 0..200 {
        let mut messages = Vec::new();
        let (state, _) = ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            mouse::Cursor::Unavailable,
            &mut backend,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        now += Duration::from_millis(16);
        let State::Updated { redraw_request, .. } = state else {
            panic!("nothing here invalidates widgets");
        };
        if redraw_request == window::RedrawRequest::Wait {
            settled = true;
            break;
        }
    }
    assert!(settled, "a 400 ms spring settles within a few seconds");

    let rgba = draw(&mut ui, &mut backend);
    assert_eq!(
        pixel(&rgba, 50, 50),
        [0, 0, 0],
        "settled on the third target"
    );
    assert_eq!(
        pixel(&rgba, 20, 50),
        [255, 255, 255],
        "not the first target"
    );
    assert_eq!(
        pixel(&rgba, 80, 50),
        [255, 255, 255],
        "not stuck on the second target"
    );
}

#[test]
fn a_range_shorter_than_the_stroke_thins_and_fades_to_nothing() {
    // 5 % of an 80 px bar is 4 px of line under an 8 px round cap: half the
    // stroke's own width, so it is drawn 4 px wide at half strength.
    let mut backend = backend();
    let root = path(bar())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::BLACK, 8.0)
        .draw(DrawRange::new(0.0, 0.05))
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    let [r, g, b] = pixel(&rgba, 12, 50);
    assert!(r == g && g == b, "grey: {r} {g} {b}");
    assert!((90..=170).contains(&r), "half strength over white: {r}");
    let [edge, ..] = pixel(&rgba, 12, 53);
    assert!(
        edge >= 245,
        "3 px off the axis is outside a 4 px stroke: {edge}"
    );
}

/// A circle of radius 30 round (50, 50), starting at (80, 50) and heading down.
fn ring() -> Arc<PathData> {
    const K: f32 = 0.552_284_8;
    let (c, r) = (50.0, 30.0);
    let p = |x: f32, y: f32| Point::new(c + x, c + y);
    Arc::new(
        PathData::builder()
            .move_to(p(r, 0.0))
            .cubic_to(p(r, K * r), p(K * r, r), p(0.0, r))
            .cubic_to(p(-K * r, r), p(-r, K * r), p(-r, 0.0))
            .cubic_to(p(-r, -K * r), p(-K * r, -r), p(0.0, -r))
            .cubic_to(p(K * r, -r), p(r, -K * r), p(r, 0.0))
            .close()
            .build()
            .unwrap(),
    )
}

fn translucent_ring(range: DrawRange) -> Vec<u8> {
    let mut backend = backend();
    let root = path(ring())
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::from_rgba(0.0, 0.0, 0.0, 0.5), 8.0)
        .draw(range)
        .into();
    let mut ui = build(root, &mut backend);
    draw(&mut ui, &mut backend)
}

#[test]
fn the_ends_of_a_nearly_closed_loop_meet_without_overlapping() {
    // 1 % short of the loop leaves a 1.9 px gap, far less than the 8 px
    // stroke: round caps would lap over the start and double the alpha there.
    for at in [(80, 52), (80, 48)] {
        let [nearly, ..] = pixel(&translucent_ring(DrawRange::new(0.0, 0.99)), at.0, at.1);
        let [closed, ..] = pixel(&translucent_ring(DrawRange::FULL), at.0, at.1);
        assert!(
            (110..=145).contains(&nearly),
            "one layer of 50 % black at {at:?}: {nearly}"
        );
        assert!(
            nearly.abs_diff(closed) <= 12,
            "closing the last gap changes nothing at {at:?}: {nearly} vs {closed}"
        );
    }
}

#[test]
fn an_open_gap_wider_than_the_stroke_keeps_its_round_caps() {
    // 10 % short: an 18.8 px gap, so the caps are whole and the gap shows.
    let rgba = translucent_ring(DrawRange::new(0.0, 0.9));
    let [inside_gap, ..] = pixel(&rgba, 78, 40);
    assert!(
        inside_gap >= 245,
        "the middle of the gap stays clear: {inside_gap}"
    );
    let [cap, ..] = pixel(&rgba, 80, 51);
    assert!(
        (110..=145).contains(&cap),
        "the start's cap is drawn once: {cap}"
    );
}

/// The same probe on wgpu, where a stroke is tessellated into triangles and
/// a round cap lapping over the start is blended twice. Needs a GPU.
#[test]
#[ignore = "needs a GPU adapter"]
fn on_wgpu_the_ends_of_a_nearly_closed_loop_meet_without_overlapping() {
    let mut backend = iced_test::futures::futures::executor::block_on(
        <iced::Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("wgpu")),
    )
    .expect("a wgpu adapter");
    let mut shoot = |range: DrawRange| {
        let root = path(ring())
            .width(100.0)
            .height(100.0)
            .view_box(one_to_one())
            .fit(Fit::None)
            .stroke(Color::from_rgba(0.0, 0.0, 0.0, 0.5), 8.0)
            .draw(range)
            .into();
        let mut ui = build(root, &mut backend);
        draw(&mut ui, &mut backend)
    };
    let nearly = shoot(DrawRange::new(0.0, 0.99));
    let closed = shoot(DrawRange::FULL);
    for at in [(80, 52), (80, 48)] {
        let ([a, ..], [b, ..]) = (pixel(&nearly, at.0, at.1), pixel(&closed, at.0, at.1));
        assert!(a.abs_diff(b) <= 12, "no doubled cap at {at:?}: {a} vs {b}");
    }
}

#[test]
fn a_tilted_plane_draws_its_far_side_narrower() {
    use iced_animate::path::Perspective;

    // Two bars, 10..90 across, at the top and the bottom of the widget.
    let bars = Arc::new(
        PathData::builder()
            .move_to(Point::new(10.0, 15.0))
            .line_to(Point::new(90.0, 15.0))
            .move_to(Point::new(10.0, 85.0))
            .line_to(Point::new(90.0, 85.0))
            .build()
            .unwrap(),
    );
    let mut backend = backend();
    let root = path(bars)
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::BLACK, 4.0)
        .line_cap(iced_animate::widget::LineCap::Butt)
        .perspective(Perspective::new(0.8, 0.0, 150.0))
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    // Where the top bar ends on the right, scanning along its row.
    let dark = |x: u32, y: u32| pixel(&rgba, x, y)[0] < 128;
    let row_of = |near: u32| {
        (near.saturating_sub(12)..near + 12)
            .find(|&y| dark(50, y))
            .expect("a bar")
    };
    let (top, bottom) = (row_of(20), row_of(80));
    let reach = |y: u32| (50..100).take_while(|&x| dark(x, y)).last().unwrap_or(50);
    assert!(
        reach(top) + 5 < reach(bottom),
        "the far (top) bar is shorter: {} vs {}",
        reach(top),
        reach(bottom)
    );
}

#[test]
fn a_trail_across_the_start_of_a_loop_stays_whole() {
    // From 10 % before the start to 10 % after it: one run through (80, 50).
    let rgba = translucent_ring(DrawRange::new(-0.1, 0.1));
    for at in [(79, 42), (79, 58)] {
        let [value, ..] = pixel(&rgba, at.0, at.1);
        assert!(
            value < 200,
            "drawn on both sides of the start at {at:?}: {value}"
        );
    }
    let [far, ..] = pixel(&rgba, 20, 50);
    assert!(far >= 245, "the opposite side is not drawn: {far}");
}

fn wgpu_backend() -> iced::Renderer {
    iced_test::futures::futures::executor::block_on(<iced::Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .expect("a wgpu adapter")
}

/// On wgpu a projected path is drawn from a mesh tessellated in the flat;
/// the tilt must still land where the CPU projection puts it. Needs a GPU.
#[test]
#[ignore = "needs a GPU adapter"]
fn on_wgpu_a_tilted_plane_draws_its_far_side_narrower() {
    use iced_animate::path::Perspective;

    let bars = Arc::new(
        PathData::builder()
            .move_to(Point::new(10.0, 15.0))
            .line_to(Point::new(90.0, 15.0))
            .move_to(Point::new(10.0, 85.0))
            .line_to(Point::new(90.0, 85.0))
            .build()
            .unwrap(),
    );
    let mut backend = wgpu_backend();
    let root = path(bars)
        .width(100.0)
        .height(100.0)
        .view_box(one_to_one())
        .fit(Fit::None)
        .stroke(Color::BLACK, 4.0)
        .line_cap(iced_animate::widget::LineCap::Butt)
        .perspective(Perspective::new(0.8, 0.0, 150.0))
        .into();
    let mut ui = build(root, &mut backend);
    let rgba = draw(&mut ui, &mut backend);

    let dark = |x: u32, y: u32| pixel(&rgba, x, y)[0] < 128;
    let row_of = |near: u32| {
        (near.saturating_sub(12)..near + 12)
            .find(|&y| dark(50, y))
            .expect("a bar")
    };
    let reach = |y: u32| (50..100).take_while(|&x| dark(x, y)).last().unwrap_or(50);
    let (top, bottom) = (row_of(20), row_of(80));
    assert!(
        reach(top) + 5 < reach(bottom),
        "the far (top) bar is shorter: {} vs {}",
        reach(top),
        reach(bottom)
    );
}

/// A sway alone moves only the projection: the flat mesh is tessellated
/// once and reused on every frame after. Needs a GPU.
#[test]
#[ignore = "needs a GPU adapter"]
fn on_wgpu_a_moving_perspective_reuses_the_flat_tessellation() {
    use iced_animate::path::Perspective;

    let mut backend = wgpu_backend();
    let m = Motion::new();
    let mut clock = FrameClock::new(&m);
    let tilt = m.play(
        key!(),
        Curve::ease(Easing::Linear, Duration::from_millis(500)),
        Perspective::new(0.1, 0.0, 400.0),
        Perspective::new(0.6, 0.3, 400.0),
    );
    let root = path(curve())
        .width(100.0)
        .height(100.0)
        .stroke(Color::BLACK, 4.0)
        .perspective(&tilt)
        .into();
    let mut ui = build(root, &mut backend);

    let _ = draw(&mut ui, &mut backend);
    let start = testing::path_geometry_builds();
    for _ in 0..10 {
        let _ = clock.run(1);
        let _ = draw(&mut ui, &mut backend);
    }
    assert!(tilt.is_animating(), "the tilt moved during the frames");
    assert_eq!(
        testing::path_geometry_builds(),
        start,
        "no re-tessellation while only the perspective moves"
    );
}
