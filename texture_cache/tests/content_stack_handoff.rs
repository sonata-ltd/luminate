//! A `ContentStack` must hand off from compositing to drawing without a seam.
//!
//! The widget exists for one property: the frame it stops compositing its
//! pages and starts drawing the current one directly must put every pixel
//! where the previous frame had it. Both branches of `draw` go through the
//! same `page_origin`, so the property is structural — this is the proof that
//! it survives a real record, a real composite and a real rasterizer.
//!
//! The second half of the same property is that nothing quantises the slide
//! on the way there: the offset is subtracted after the snap, so a page
//! creeping a fiftieth of a pixel per frame near the end of a spring creeps
//! rather than steps.
#![cfg(feature = "wgpu")]

use std::time::Duration;

use iced_animate::{Curve, Motion, SpringParams};
use iced_core::renderer::{Headless, Quad};
use iced_core::{Color, Element, Font, Pixels, Rectangle, Size, mouse, renderer, window};
use iced_core::{Event, Renderer as _, clipboard};
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_texture_cache::{ContentStack, Renderer};

const CANVAS: Size = Size::new(240.0, 160.0);
const WIDTH: usize = 240;
const FRAME: Duration = Duration::from_millis(16);
const PAGE_WIDTH: f32 = 120.0;

/// Slow enough that the tail of the spring spends many frames moving less
/// than a pixel — where a staircase is most visible, and most measurable.
const SLIDE: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(450)));

/// The ink-weighted horizontal centroid of the frame, and the vertical one.
///
/// Only the incoming page carries ink, so this tracks that page alone. The
/// block is drawn by a quad, which snaps inside the recorded texture whatever
/// happens outside it, so its rasterization is identical every frame: any
/// fraction here comes from where the page was placed.
fn centroid(rgba: &[u8]) -> Option<(f64, f64)> {
    let (mut mass, mut wx, mut wy) = (0.0, 0.0, 0.0);

    for (i, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let ink = (255.0 - f64::from(pixel[0])) / 255.0;
        mass += ink;
        wx += ink * (i % WIDTH) as f64;
        wy += ink * (i / WIDTH) as f64;
    }

    // The incoming page starts entirely off the stack's own clip, so the
    // first frames of a slide carry no ink at all.
    (mass > 1.0).then(|| (wx / mass, wy / mass))
}

fn off_grid(value: f64) -> f64 {
    (value - value.round()).abs()
}

struct Harness {
    renderer: Renderer,
    motion: Motion,
    ui_cache: Option<user_interface::Cache>,
    now: iced_core::time::Instant,
}

impl Harness {
    fn new() -> Self {
        let renderer = iced_test::futures::futures::executor::block_on(
            <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("wgpu")),
        )
        .expect("a GPU adapter is available");

        Self {
            renderer,
            motion: Motion::new(),
            ui_cache: Some(user_interface::Cache::default()),
            now: iced_core::time::Instant::now(),
        }
    }

    /// One frame at `current`. Page 0 is blank and short, page 1 is a black
    /// block and tall, so the stack's height interpolates as well.
    fn frame(&mut self, current: usize) -> Option<(f64, f64)> {
        self.now += FRAME;

        let blank: Element<'_, (), iced::Theme, Renderer> =
            iced::widget::Space::new().height(40.0).into();
        let block: Element<'_, (), iced::Theme, Renderer> = iced::widget::container(
            iced::widget::container(iced::widget::Space::new())
                .width(61.0)
                .height(41.0)
                .style(|_: &iced::Theme| iced::widget::container::Style {
                    background: Some(Color::BLACK.into()),
                    ..iced::widget::container::Style::default()
                }),
        )
        .height(90.0)
        .into();

        let stack: Element<'_, (), iced::Theme, Renderer> = ContentStack::new([blank, block])
            .current(current)
            .width(PAGE_WIDTH)
            .curve(SLIDE)
            .motion(self.motion.clone())
            .into();

        let ui_cache = self.ui_cache.take().expect("returned after every frame");
        let mut messages = Vec::new();
        let mut ui: UserInterface<'_, (), iced::Theme, Renderer> = UserInterface::build(
            self.motion.host(stack),
            CANVAS,
            ui_cache,
            &mut self.renderer,
        );
        let _ = ui.update(
            &[Event::Window(window::Event::RedrawRequested(self.now))],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        self.renderer.reset(Rectangle::with_size(CANVAS));
        ui.draw(
            &mut self.renderer,
            &iced::Theme::Light,
            &renderer::Style {
                text_color: Color::BLACK,
            },
            mouse::Cursor::Unavailable,
        );
        let shot = self.renderer.screenshot(
            Size::new(CANVAS.width as u32, CANVAS.height as u32),
            1.0,
            Color::WHITE,
        );
        self.ui_cache = Some(ui.into_cache());

        centroid(&shot)
    }
}

/// Drives one slide from rest and returns the frames in which the incoming
/// page was on screen, plus the index of the first one drawn at rest.
fn slide() -> (Vec<(f64, f64)>, usize) {
    let mut harness = Harness::new();

    for _ in 0..40 {
        let _ = harness.frame(0);
    }

    let measured: Vec<Option<(f64, f64)>> = (0..80).map(|_| harness.frame(1)).collect();
    // The contiguous tail in which the page is on screen.
    let first = measured
        .iter()
        .position(Option::is_some)
        .expect("the incoming page never appeared");
    let frames: Vec<(f64, f64)> = measured[first..]
        .iter()
        .map(|frame| frame.expect("the page cannot leave once it has arrived"))
        .collect();

    // The slide is over on the first frame whose position is bit-identical to
    // the one before: the spring has snapped and `draw` has taken the resting
    // branch.
    // Two identical frames in a row, so a spring that happens to produce the
    // same position twice mid-flight is not mistaken for the end.
    let resting = frames
        .windows(3)
        .position(|run| run[1].0 == run[0].0 && run[2].0 == run[1].0)
        .expect("the slide settled")
        + 1;

    (frames, resting)
}

#[test]
#[ignore = "needs a GPU adapter"]
fn the_handoff_from_texture_to_widget_moves_nothing() {
    let (frames, resting) = slide();
    let steps: Vec<f64> = frames
        .windows(2)
        .map(|pair| pair[1].0 - pair[0].0)
        .collect();

    // The handoff is the step into the first resting frame.
    let handoff = steps[resting - 1].abs();
    // What the frames just before it were doing: by then the spring is deep
    // in its tail and moving a small fraction of a pixel per frame.
    let before: f64 = steps[resting.saturating_sub(6)..resting - 1]
        .iter()
        .map(|step| step.abs())
        .fold(0.0, f64::max);

    assert!(
        handoff <= before.max(0.05),
        "the picture jumped {handoff:.4} px when compositing stopped, against \
         {before:.4} px in the frames before it: the handoff is visible"
    );

    // The vertical axis is snapped outright — the pages are centred in an
    // interpolating height and that motion is not the one the eye follows —
    // so it may step during the slide, but it must be still by the handoff.
    let vertical = (frames[resting].1 - frames[resting - 1].1).abs();
    assert!(
        vertical < 0.05,
        "the picture jumped {vertical:.4} px vertically at the handoff"
    );
}

#[test]
#[ignore = "needs a GPU adapter"]
fn the_slide_keeps_its_fraction_instead_of_landing_on_the_device_grid() {
    if !Quad::default().snap {
        println!("`crisp` is off: nothing snaps, nothing to prove");
        return;
    }

    let (frames, resting) = slide();
    let travelled = (frames[resting].0 - frames[0].0).abs();
    assert!(travelled > 20.0, "the page barely moved: {travelled:.2} px");

    // Measure where the page *is*, not how far it moved: a creep of a
    // fiftieth of a pixel is smooth motion, but as a step it is
    // indistinguishable from standing still. Only frames in which the page
    // is fully inside the stack (so the stack's own integer clip is not what
    // the centroid is tracking) and still moving are asked the question.
    let travelling: Vec<f64> = frames[..resting]
        .windows(2)
        .filter(|pair| {
            let step = (pair[1].0 - pair[0].0).abs();
            (0.02..3.0).contains(&step)
        })
        .map(|pair| pair[1].0)
        .collect();

    assert!(
        travelling.len() > 5,
        "too few clean frames to judge: {}",
        travelling.len()
    );
    let off = travelling.iter().filter(|x| off_grid(**x) > 0.05).count();
    assert!(
        off * 2 > travelling.len(),
        "only {off} of {} travelling frames sat off the device grid; the \
         composited origin is being rounded and the slide steps: {travelling:?}",
        travelling.len()
    );
}
