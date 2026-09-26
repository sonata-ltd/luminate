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
use iced_animate::widget::path;
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
