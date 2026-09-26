//! The path widget draws what its values say, draws nothing for an empty
//! range, matches iced's own canvas for a full one, and reuses its geometry
//! while nothing moves.

use std::sync::Arc;
use std::time::Duration;

use iced::advanced::renderer::{self, Headless};
use iced::widget::canvas;
use iced::{Color, Font, Pixels, Point, Rectangle, Size, mouse};
use iced_animate::path::{DrawRange, Fit, PathData};
use iced_animate::testing::{self, FrameClock};
use iced_animate::widget::path;
use iced_animate::{Curve, Easing, Motion, key};
use iced_test::runtime::user_interface::{self, UserInterface};

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
