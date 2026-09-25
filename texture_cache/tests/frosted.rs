//! `frosted` end to end: record a source, put a pane of glass over it, read
//! the frame back.
//!
//! Software backend only — `testing::headless_tiny_skia` needs no GPU and
//! no executor, and being explicit about the backend is the point: a green
//! run here says nothing about the wgpu chain, which is exactly why the
//! radius is defined as a sigma rather than as a number of passes.

#![cfg(feature = "tiny-skia")]

use std::time::Duration;

use iced::advanced::Renderer as _;
use iced::advanced::renderer::Headless;
use iced::advanced::{clipboard, renderer};
use iced::time::Instant;
use iced::widget::{container, row, stack};
use iced::{Color, Event, Rectangle, Size, mouse, window};
use iced_animate::widget::shape;
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_texture_cache::testing::{blur_count, derived_len, headless_tiny_skia};
use iced_texture_cache::{Renderer, TextureCache, cached, frosted};

const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);
const FRAME: Duration = Duration::from_millis(16);
const WINDOW: Size = Size::new(80.0, 80.0);
/// Window for `the_same_sigma_survives_every_downscale` alone: see that
/// test's doc comment for why it needs a bigger canvas than the other
/// tests in this file.
const BIG_WINDOW: Size = Size::new(200.0, 60.0);

type El<'a> = iced_texture_cache::Element<'a, ()>;

struct Harness {
    renderer: Renderer,
    cache: Option<user_interface::Cache>,
    now: Instant,
    window: Size,
}

impl Harness {
    fn new(window: Size) -> Self {
        Self {
            renderer: headless_tiny_skia(),
            cache: Some(user_interface::Cache::default()),
            now: Instant::now(),
            window,
        }
    }

    /// One frame; returns the RGBA screenshot at scale 1.
    fn frame<'a>(&mut self, root: impl Into<El<'a>>) -> Vec<u8> {
        self.now += FRAME;
        let cache = self.cache.take().expect("returned after every frame");
        let mut ui: UserInterface<'_, (), iced::Theme, Renderer> =
            UserInterface::build(root, self.window, cache, &mut self.renderer);
        let _ = ui.update(
            &[Event::Window(window::Event::RedrawRequested(self.now))],
            mouse::Cursor::Unavailable,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut Vec::new(),
        );
        self.renderer.reset(Rectangle::with_size(self.window));
        ui.draw(
            &mut self.renderer,
            &iced::Theme::Light,
            &renderer::Style {
                text_color: Color::BLACK,
            },
            mouse::Cursor::Unavailable,
        );
        self.cache = Some(ui.into_cache());

        let physical = Size::new(self.window.width as u32, self.window.height as u32);
        self.renderer.screenshot(physical, 1.0, Color::WHITE)
    }

    fn blurs(&self) -> u64 {
        blur_count(&self.renderer)
    }

    fn derivatives(&self) -> usize {
        derived_len(&self.renderer)
    }
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let start = ((y * width + x) * 4) as usize;
    let px = &rgba[start..start + 4];
    [px[0], px[1], px[2], px[3]]
}

/// A red square with a pane of glass over it that overhangs its edges.
///
/// The overhang is the whole point, and it is easy to get wrong:
/// `iced_widget::Stack` shrinks to its **first** child, so a pane stacked
/// straight onto the square is clamped to the square's own 40x40 footprint
/// however it is sized — and a blurred red pane over an identically red
/// square is invisible at every coordinate and every threshold, which makes
/// the test vacuous rather than failing. Wrapping the base in a sized
/// `container` gives the stack a 60x60 first child while leaving the square
/// at 40x40, so the 50x50 pane genuinely spills past the edge and there are
/// blurred pixels to measure. Task 9 hit this; it is not hypothetical.
fn glass_over_square(source: &TextureCache, radius: f32, downscale: u32) -> El<'_> {
    stack![
        container(cached(
            source.clone(),
            shape().width(40.0).height(40.0).fill(RED)
        ))
        .width(60.0)
        .height(60.0),
        frosted(source.clone())
            .radius(radius)
            .downscale(downscale)
            .width(50.0)
            .height(50.0),
    ]
    .into()
}

/// A wide red-then-white strip, built for
/// `the_same_sigma_survives_every_downscale` alone.
///
/// Fix round 1, attempt 1 (rejected): scaling `glass_over_square`'s 40x40
/// square up 4x to 160x160 while holding sigma and downscale fixed did not
/// change the measured divergence *at all* — profiles came out bit-for-bit
/// identical to the 40x40 case. The reason: the measured edge is the
/// square's own **outer** boundary, and what the pane can composite there
/// is clamped to the source's own recorded footprint (content plus the 2px
/// `BLEED` margin from `cached.rs`) — a fixed 2 real pixels, independent of
/// how big the square is. Since `residual_sigma` is a function only of
/// sigma and the downscale factor (never of the texture's own dimensions),
/// growing the square left every number governing that boundary column
/// unchanged, so the divergence at it was identical too.
///
/// This shape fixes it differently: red on the left half, white (not
/// background/transparent) on the right, so the measured edge sits deep
/// **inside** the recorded texture, 80px from either outer edge — comfortably
/// more room than a sigma-8 kernel's few-pixel radius needs on either side,
/// at any downscale up to 4. Nothing here is clamped by `BLEED`; the
/// profile is a real, symmetric Gaussian crossing, which is what
/// `residual_sigma` actually governs.
fn glass_over_split(source: &TextureCache, radius: f32, downscale: u32) -> El<'_> {
    let content = row![
        shape().width(80.0).height(40.0).fill(RED),
        shape().width(80.0).height(40.0).fill(Color::WHITE),
    ];
    stack![
        cached(source.clone(), content),
        frosted(source.clone())
            .radius(radius)
            .downscale(downscale)
            .width(160.0)
            .height(40.0),
    ]
    .into()
}

/// The brightness profile across the red/white seam, on the green channel:
/// 0 on the red side, 255 on the white side.
///
/// The sample window below assumes the seam sits near x = 80
/// (`glass_over_split`'s two halves are each 80 wide). **Verify that before
/// trusting it** — dump the row and find the seam, then centre the window
/// on it. Adjust the window, never the tolerances: those encode what the
/// test means.
fn edge_profile(rgba: &[u8]) -> Vec<f64> {
    (55..105)
        .map(|x| f64::from(pixel(rgba, BIG_WINDOW.width as u32, x, 20)[1]) / 255.0)
        .collect()
}

/// Test 5 of the spec: storing the blur at 1/n must not change the picture.
///
/// Fix round 1: the original 40x40 geometry (`glass_over_square`, measuring
/// the square's own outer edge) made this test tolerate a 4x calibration
/// error in `residual_sigma` — widening the tolerance to cover a
/// reconstruction artifact at that boundary also covered up the bug the
/// test exists to catch, which defeats the point (see the spec: without
/// this test, storing at 1/4 could silently change the picture). Scaling
/// that same geometry up 4x did not help either — see `glass_over_split`'s
/// doc comment for why, with the bit-identical profiles as evidence. The
/// actual fix is measuring a different edge: `glass_over_split`'s red/white
/// seam sits well inside the recorded texture, away from any
/// `BLEED`-clamped boundary, so the profile is a genuine Gaussian crossing
/// rather than a boundary-clamp artifact.
///
/// Measured worst case at this geometry, 8-bit exact: downscale 2 diverges
/// from downscale 1 by at most 7/255 (at x = 76); downscale 4 diverges by
/// at most 7/255 too (at x = 79). 10/255 below leaves a little margin over
/// that measured maximum while staying far tighter than the 40x40 geometry
/// ever allowed (which needed 50/255 and still could not be made to fail on
/// a sigma-magnitude error). Confirmed (fix round 1) to go red on a genuine
/// sigma-magnitude error: dropping the `/d` in `residual_sigma` (an ~4x
/// error at downscale 4) fails immediately — see `task-10-report.md`,
/// "Fix round 1", for the exact message.
#[test]
fn the_same_sigma_survives_every_downscale() {
    let profiles: Vec<Vec<f64>> = [1, 2, 4]
        .into_iter()
        .map(|downscale| {
            let source = TextureCache::new();
            let mut harness = Harness::new(BIG_WINDOW);
            let rgba = harness.frame(glass_over_split(&source, 8.0, downscale));
            edge_profile(&rgba)
        })
        .collect();

    for (downscale, profile) in [2, 4].into_iter().zip(&profiles[1..]) {
        for (x, (reference, got)) in profiles[0].iter().zip(profile).enumerate() {
            assert!(
                (reference - got).abs() <= 10.0 / 255.0,
                "downscale {downscale} diverged at pixel x = {}: {reference} against {got}",
                x + 55
            );
        }
    }
}

/// Test 6 of the spec.
#[test]
fn a_settled_frame_costs_no_blur_and_moving_the_glass_is_free() {
    let source = TextureCache::new();
    let mut harness = Harness::new(WINDOW);

    let _ = harness.frame(glass_over_square(&source, 8.0, 4));
    assert_eq!(harness.blurs(), 1, "the first frame blurs once");

    for _ in 0..5 {
        let _ = harness.frame(glass_over_square(&source, 8.0, 4));
    }
    assert_eq!(harness.blurs(), 1, "a settled frame re-blurs nothing");

    // A narrower, fainter, differently placed pane: still one blur.
    let _ = harness.frame(stack![
        cached(source.clone(), shape().width(40.0).height(40.0).fill(RED)),
        frosted(source.clone())
            .radius(8.0)
            .downscale(4)
            .width(20.0)
            .height(20.0)
            .opacity(0.5),
    ]);
    assert_eq!(harness.blurs(), 1, "moving and fading the glass is free");

    source.invalidate();
    let _ = harness.frame(glass_over_square(&source, 8.0, 4));
    assert_eq!(harness.blurs(), 2, "a new source epoch re-blurs once");

    let _ = harness.frame(glass_over_square(&source, 16.0, 4));
    assert_eq!(harness.blurs(), 3, "a new radius re-blurs once");
    assert_eq!(harness.derivatives(), 2, "and keeps the old derivative");
}

/// Test 9 of the spec.
#[test]
fn glass_over_a_source_nobody_recorded_draws_nothing() {
    let orphan = TextureCache::new();
    let mut harness = Harness::new(WINDOW);

    let rgba = harness.frame(frosted(orphan).radius(8.0).width(40.0).height(40.0));

    assert_eq!(
        pixel(&rgba, WINDOW.width as u32, 20, 20),
        [255, 255, 255, 255]
    );
    assert_eq!(harness.blurs(), 0);
    assert_eq!(harness.derivatives(), 0);
}

/// Test 10 of the spec: drawn before its source, the pane shows the
/// previous frame and catches up on the next.
#[test]
fn glass_drawn_before_its_source_shows_the_previous_epoch() {
    let source = TextureCache::new();
    let mut harness = Harness::new(WINDOW);

    // The pane first in the stack: it draws before the source does.
    let inverted = |color| {
        stack![
            frosted(source.clone())
                .radius(8.0)
                .downscale(4)
                .width(40.0)
                .height(40.0),
            cached(source.clone(), shape().width(40.0).height(40.0).fill(color)),
        ]
    };

    let rgba = harness.frame(inverted(RED));
    assert_eq!(harness.blurs(), 0, "the source has no texture yet");
    assert_eq!(
        pixel(&rgba, WINDOW.width as u32, 60, 60),
        [255, 255, 255, 255]
    );

    let _ = harness.frame(inverted(RED));
    assert_eq!(harness.blurs(), 1, "it catches up on the next frame");
}

/// Test 2 of the spec, end to end: the halo the card's cube would have hit.
///
/// The window is `34..52`, not the `42..52` first drafted. Dumping the row
/// (same technique as `edge_profile`) at this radius (6) and downscale (1)
/// shows the edge itself sits at x = 40/41 (partial alpha: green and blue
/// both drop to about 100/108 there) and the buffer runs out immediately
/// past it — every pixel from x = 42 on is already fully transparent
/// (`[255, 255, 255, 255]`, pure background) — the same `BLEED`-clamped
/// boundary that `the_same_sigma_survives_every_downscale` above had to
/// measure a different edge to avoid: a 40x40 square with a 2px `BLEED`
/// margin leaves almost no room past the opaque edge. `42..52` therefore
/// samples only fully-transparent pixels,
/// which are the fast path in `pixmap_to_rgba` (`a == 0`) and never touch
/// the demultiply arithmetic at all — a straight-alpha bug there would not
/// fail this test. Starting the window at 34 instead brings in both the
/// partial-alpha pixels at the edge and the nominally-interior pixels just
/// inside it, which the same bug also darkens (confirmed by injecting it:
/// r dropped to 194-219 across x = 34..41, against the correct 255).
#[test]
fn a_blurred_square_has_no_dark_fringe_on_white() {
    let source = TextureCache::new();
    let mut harness = Harness::new(WINDOW);

    let rgba = harness.frame(glass_over_square(&source, 6.0, 1));

    // From just inside the square's right edge to well past it, on the
    // white background.
    for x in 34..52 {
        let [r, g, b, _] = pixel(&rgba, WINDOW.width as u32, x, 20);
        assert_eq!(r, 255, "red stays saturated at x = {x}");
        assert!(
            g.abs_diff(b) <= 2,
            "a straight-alpha blur would tint this at x = {x}: {g} against {b}"
        );
    }
}
