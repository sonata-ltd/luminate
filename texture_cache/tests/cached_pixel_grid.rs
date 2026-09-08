//! A cached layer moved by an ancestor must not step by whole pixels.
//!
//! `Cached` snaps its composited origin to the device grid while it is at
//! rest, which is what keeps a still texture crisp. Rest used to be judged
//! from the widget's own `translate` and `scale` alone — and a layer with
//! neither bound can still be travelling, because an ancestor is animating
//! the space it sits in (a card re-centring its header as the page stack
//! below interpolates its height). Such a layer stepped one whole device
//! pixel at a time.
//!
//! This is the end-to-end proof for both policies: `Auto` lets a moving layer
//! land between pixels and puts it back on the grid once it stops, and the
//! default `LayoutOnly` reaches the same two resting places while gliding
//! between them, because it snaps the position the layer last rested at
//! rather than the one it is passing through.
//!
//! It lives here rather than in a unit test because the snap is applied to a
//! real composite: only a rendered frame can say where the texture landed.
#![cfg(feature = "wgpu")]

use std::time::Duration;

use iced_animate::widget::sized;
use iced_animate::{Curve, Motion, SpringParams, key};
use iced_core::renderer::{Headless, Quad};
use iced_core::{Color, Element, Font, Padding, Pixels, Rectangle, Size, mouse, renderer, window};
use iced_core::{Event, Renderer as _, clipboard};
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_texture_cache::{FilterQuality, PixelSnap, Renderer, TextureCache, cached};

const CANVAS: Size = Size::new(200.0, 200.0);
const WIDTH: usize = 200;
const FRAME: Duration = Duration::from_millis(16);
/// Odd, so a grid-aligned square has an integer centroid and the assertions
/// do not have to carry a half-pixel around.
const SIDE: f32 = 41.0;
const FROM: f32 = 10.0;
const TO: f32 = 46.0;

/// A fast spring, so the travel is over in a dozen frames.
const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

/// Pinned, because the tier is otherwise the adapter's to choose and
/// [`FilterQuality::Snap`] — what a software or virtual adapter gets, which is
/// what CI runs on — rounds the composite whatever `PixelSnap` asks for. That
/// is the documented order of precedence, not a bug; it just leaves nothing
/// for these tests to measure. Either non-snapping tier proves the same thing.
const FILTER: FilterQuality = FilterQuality::Bilinear;

/// The ink-weighted vertical centroid of the frame.
///
/// The square is drawn by a quad, which snaps inside the recorded texture
/// whatever happens outside it, so its rasterization is identical every
/// frame. Any fraction in this number therefore comes from the composite
/// origin, which is exactly what is under test.
fn centroid_y(rgba: &[u8]) -> f64 {
    let (mut mass, mut weighted) = (0.0, 0.0);

    for (i, pixel) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let ink = (255.0 - f64::from(pixel[0])) / 255.0;
        mass += ink;
        weighted += ink * (i / WIDTH) as f64;
    }

    weighted / mass
}

/// How far `value` sits from the nearest whole pixel.
fn off_grid(value: f64) -> f64 {
    (value - value.round()).abs()
}

struct Harness {
    snap: PixelSnap,
    renderer: Renderer,
    motion: Motion,
    cache: TextureCache,
    ui_cache: Option<user_interface::Cache>,
    now: iced_core::time::Instant,
}

impl Harness {
    fn new(snap: PixelSnap) -> Self {
        let renderer = iced_test::futures::futures::executor::block_on(
            <Renderer as Headless>::new(Font::DEFAULT, Pixels(16.0), Some("wgpu")),
        )
        .expect("a GPU adapter is available");

        Self {
            snap,
            renderer,
            motion: Motion::new(),
            cache: TextureCache::new(),
            ui_cache: Some(user_interface::Cache::default()),
            now: iced_core::time::Instant::now(),
        }
    }

    /// One frame of a cached square pushed down by an animated padding: the
    /// layer itself animates nothing, an ancestor moves it.
    fn frame(&mut self, top: f32) -> f64 {
        self.now += FRAME;

        let padding = self
            .motion
            .to(key!(), FAST, Padding::ZERO.top(top).left(10.0));
        let square = iced::widget::container(iced::widget::Space::new())
            .width(SIDE)
            .height(SIDE)
            .style(|_: &iced::Theme| iced::widget::container::Style {
                background: Some(Color::BLACK.into()),
                ..iced::widget::container::Style::default()
            });
        let layer: Element<'_, (), iced::Theme, Renderer> = sized(
            cached(self.cache.clone(), square)
                .pixel_snap(self.snap)
                .filter_quality(FILTER),
        )
        .padding(padding)
        .into();

        let ui_cache = self.ui_cache.take().expect("returned after every frame");
        let mut messages = Vec::new();
        let mut ui: UserInterface<'_, (), iced::Theme, Renderer> = UserInterface::build(
            self.motion.host(layer),
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

        centroid_y(&shot)
    }
}

#[test]
#[ignore = "needs a GPU adapter"]
fn auto_lets_a_layer_moved_by_an_ancestor_land_between_pixels() {
    if !Quad::default().snap {
        println!("`crisp` is off: nothing snaps, nothing to prove");
        return;
    }

    let mut harness = Harness::new(PixelSnap::Auto);

    // Standing still, the composite is on the grid: this is the crispness
    // the snap exists for, and it must survive the fix.
    for _ in 0..40 {
        let _ = harness.frame(FROM);
    }
    let resting = harness.frame(FROM);
    assert!(
        off_grid(resting) < 0.02,
        "at rest the square must sit on the device grid, centroid {resting}"
    );

    let travel: Vec<f64> = (0..14).map(|_| harness.frame(TO)).collect();

    assert!(
        travel.windows(2).all(|pair| pair[1] >= pair[0] - 0.01),
        "the square only moves down: {travel:?}"
    );
    assert!(
        travel.last().is_some_and(|last| *last - resting > 20.0),
        "the square really did travel: {resting} -> {travel:?}"
    );

    // The point of the test. Snapped, every step between frames is a whole
    // number of pixels; unsnapped, most of them are not.
    let steps: Vec<f64> = travel.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let fractional = steps.iter().filter(|step| off_grid(**step) > 0.05).count();
    assert!(
        fractional * 2 > steps.len(),
        "only {fractional} of {} steps were fractional; the composite is being \
         snapped while an ancestor moves it: {steps:?}",
        steps.len()
    );

    // And once it stops, it is crisp again rather than left between pixels.
    for _ in 0..40 {
        let _ = harness.frame(TO);
    }
    let settled = harness.frame(TO);
    assert!(
        off_grid(settled) < 0.02,
        "the square must land back on the device grid, centroid {settled}"
    );
}

/// The default policy, and the reason it is the default: `LayoutOnly` snaps
/// only the *discrete* part of the origin. For a layer with no animation of
/// its own that is its last resting position, so it is crisp when stopped and
/// glides when carried — without the one frame where the picture changes how
/// it is drawn that `Auto` pays on the way in and on the way out.
#[test]
#[ignore = "needs a GPU adapter"]
fn the_default_lets_a_travelling_layer_glide_and_still_rests_on_the_grid() {
    if !Quad::default().snap {
        println!("`crisp` is off: nothing snaps, nothing to prove");
        return;
    }

    assert_eq!(
        PixelSnap::default(),
        PixelSnap::LayoutOnly,
        "this test describes the default; if it moved, so must the reasoning"
    );

    let mut harness = Harness::new(PixelSnap::default());

    for _ in 0..40 {
        let _ = harness.frame(FROM);
    }
    let resting = harness.frame(FROM);
    assert!(off_grid(resting) < 0.02, "at rest, on the grid: {resting}");

    let travel: Vec<f64> = (0..14).map(|_| harness.frame(TO)).collect();
    assert!(
        travel.last().is_some_and(|last| *last - resting > 20.0),
        "the square really did travel: {resting} -> {travel:?}"
    );

    // The staircase this policy used to draw: rounding the live origin turned
    // a sub-pixel creep into whole-pixel jumps, worst exactly where the motion
    // is slowest. Anchoring the snap to the last resting position keeps the
    // travel intact.
    let steps: Vec<f64> = travel.windows(2).map(|pair| pair[1] - pair[0]).collect();
    let fractional = steps.iter().filter(|step| off_grid(**step) > 0.05).count();
    assert!(
        fractional * 2 > steps.len(),
        "only {fractional} of {} steps were fractional; the origin is still \
         being rounded while an ancestor moves it: {steps:?}",
        steps.len()
    );

    // And the half of the bargain `Auto` cannot keep: the new resting place is
    // back on the grid, reached by re-anchoring rather than by a visible flip.
    for _ in 0..40 {
        let _ = harness.frame(TO);
    }
    let settled = harness.frame(TO);
    assert!(
        off_grid(settled) < 0.02,
        "the square must land back on the device grid, centroid {settled}"
    );
}
