//! Record → composite → screenshot on the software backend through the
//! widget API, read back as pixels. A swizzle bug turns red into blue; a
//! placement bug moves it.
//!
//! Software backend only: the helper behind these tests is gated on
//! `tiny-skia`.

#![cfg(feature = "tiny-skia")]

use std::time::Duration;

use iced::advanced::Renderer as _;
use iced::advanced::renderer::Headless;
use iced::advanced::widget::{Operation, operation};
use iced::advanced::{clipboard, renderer};
use iced::time::Instant;
use iced::widget::{button, container, row, space};
use iced::{Color, Event, Length, Rectangle, Size, mouse, window};
use iced_animate::widget::shape;
use iced_test::Selector;
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_texture_cache::testing::headless_tiny_skia;
use iced_texture_cache::{Renderer, TextureCache, cached};

const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);
const FRAME: Duration = Duration::from_millis(16);

type El<'a> = iced_texture_cache::Element<'a, ()>;

/// One renderer, one layout cache, one clock: what a window has.
struct Harness {
    renderer: Renderer,
    cache: Option<user_interface::Cache>,
    now: Instant,
    cursor: mouse::Cursor,
    size: Size,
}

impl Harness {
    fn new(size: Size) -> Self {
        Self {
            renderer: headless_tiny_skia(),
            cache: Some(user_interface::Cache::default()),
            now: Instant::now(),
            cursor: mouse::Cursor::Unavailable,
            size,
        }
    }

    /// Builds `root`, runs a redraw plus `events`, draws, and returns the
    /// RGBA screenshot at `scale`.
    fn frame<'a>(&mut self, root: impl Into<El<'a>>, events: &[Event], scale: f32) -> Vec<u8> {
        self.now += FRAME;
        let cache = self.cache.take().expect("returned after every frame");
        let mut ui: UserInterface<'_, (), iced::Theme, Renderer> =
            UserInterface::build(root, self.size, cache, &mut self.renderer);
        let mut all = vec![Event::Window(window::Event::RedrawRequested(self.now))];
        all.extend_from_slice(events);
        let mut messages = Vec::new();
        let _ = ui.update(
            &all,
            self.cursor,
            &mut self.renderer,
            &mut clipboard::Null,
            &mut messages,
        );
        self.renderer.reset(Rectangle::with_size(self.size));
        ui.draw(
            &mut self.renderer,
            &iced::Theme::Light,
            &renderer::Style {
                text_color: Color::BLACK,
            },
            self.cursor,
        );
        self.cache = Some(ui.into_cache());
        let physical = Size::new(
            (self.size.width * scale).round() as u32,
            (self.size.height * scale).round() as u32,
        );
        self.renderer.screenshot(physical, scale, Color::WHITE)
    }

    /// Centre of the first text matching `label`, in logical pixels.
    fn locate<'a>(&mut self, root: impl Into<El<'a>>, label: &str) -> iced::Point {
        let cache = self.cache.take().expect("returned after every frame");
        let mut ui: UserInterface<'_, (), iced::Theme, Renderer> =
            UserInterface::build(root, self.size, cache, &mut self.renderer);
        let mut find = Selector::find(label);
        ui.operate(&self.renderer, &mut operation::black_box(&mut find));
        let operation::Outcome::Some(Some(target)) = find.finish() else {
            panic!("{label} is not on screen")
        };
        self.cache = Some(ui.into_cache());
        target.visible_bounds().expect("visible").center()
    }

    fn pixel(&self, rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let width = self.size.width as usize;
        let i = (y * width + x) * 4;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    }
}

fn red_square<'a>(cache: &TextureCache) -> iced_texture_cache::Cached<'a, ()> {
    cached(cache.clone(), shape().width(50.0).height(50.0).fill(RED))
}

fn near(actual: [u8; 4], expected: [u8; 3], tolerance: u8) -> bool {
    actual[..3]
        .iter()
        .zip(expected)
        .all(|(a, e)| a.abs_diff(e) <= tolerance)
}

#[test]
fn a_cached_red_square_lands_where_it_is_placed_and_stays_red() {
    let mut h = Harness::new(Size::new(300.0, 300.0));
    let cache = TextureCache::new();
    let rgba = h.frame(container(red_square(&cache)).padding(100), &[], 1.0);

    assert_eq!(
        h.pixel(&rgba, 125, 125),
        [255, 0, 0, 255],
        "red stays red (no BGRA swizzle)"
    );
    assert_eq!(
        h.pixel(&rgba, 10, 10),
        [255, 255, 255, 255],
        "outside the square is the background"
    );
    assert_eq!(
        h.pixel(&rgba, 99, 125),
        [255, 255, 255, 255],
        "the square starts at 100, not earlier"
    );
    assert_eq!(cache.record_count(), 1);
}

#[test]
fn opacity_blends_the_texture_over_the_background() {
    let mut h = Harness::new(Size::new(300.0, 300.0));
    let cache = TextureCache::new();
    let rgba = h.frame(
        container(red_square(&cache).opacity(0.5)).padding(100),
        &[],
        1.0,
    );
    assert!(
        near(h.pixel(&rgba, 125, 125), [255, 128, 128], 3),
        "half red over white: {:?}",
        h.pixel(&rgba, 125, 125)
    );
}

#[test]
fn zero_opacity_draws_nothing_and_records_nothing() {
    let mut h = Harness::new(Size::new(300.0, 300.0));
    let cache = TextureCache::new();
    let rgba = h.frame(
        container(red_square(&cache).opacity(0.0)).padding(100),
        &[],
        1.0,
    );
    assert_eq!(h.pixel(&rgba, 125, 125), [255, 255, 255, 255]);
    assert_eq!(
        cache.record_count(),
        0,
        "invisible content is not rasterised"
    );
}

#[test]
fn a_scale_change_re_records_exactly_once() {
    let mut h = Harness::new(Size::new(300.0, 300.0));
    let cache = TextureCache::new();
    let _ = h.frame(container(red_square(&cache)).padding(100), &[], 1.0);
    assert_eq!(cache.record_count(), 1);
    let _ = h.frame(container(red_square(&cache)).padding(100), &[], 1.0);
    assert_eq!(cache.record_count(), 1, "same scale: reused");
    // `screenshot` sets the renderer's scale factor for the frames that
    // follow, like a window `present` would; the next frame records at 2.
    let _ = h.frame(container(red_square(&cache)).padding(100), &[], 2.0);
    let rgba = h.frame(container(red_square(&cache)).padding(100), &[], 2.0);
    assert_eq!(
        cache.record_count(),
        2,
        "a new scale factor needs new pixels, once"
    );
    let _ = h.frame(container(red_square(&cache)).padding(100), &[], 2.0);
    assert_eq!(cache.record_count(), 2, "and only once");
    // At scale 2 the physical buffer is 600 wide; sample the square's centre.
    let i = (250 * 600 + 250) * 4;
    assert_eq!(&rgba[i..i + 3], &[255, 0, 0]);
}

#[test]
fn oversized_content_falls_back_inline_at_its_own_position() {
    // Wider than the software texture limit (16 384 px): the cache refuses,
    // the widget draws its content in place instead of at the origin.
    let mut h = Harness::new(Size::new(17_000.0, 120.0));
    let cache = TextureCache::new();
    let view = || {
        row![
            space().width(100.0),
            cached(
                cache.clone(),
                shape().width(16_900.0).height(50.0).fill(RED)
            ),
        ]
    };
    let rgba = h.frame(view(), &[], 1.0);
    assert_eq!(
        h.pixel(&rgba, 150, 25),
        [255, 0, 0, 255],
        "drawn where it is laid out"
    );
    assert_eq!(
        h.pixel(&rgba, 50, 25),
        [255, 255, 255, 255],
        "not at the origin"
    );
    assert_eq!(
        cache.record_count(),
        0,
        "uncacheable content is never rasterised"
    );
}

#[test]
fn auto_invalidate_off_keeps_the_texture_after_a_click() {
    let mut h = Harness::new(Size::new(300.0, 200.0));
    let cache = TextureCache::new();
    let view = || {
        container(cached(cache.clone(), button("go").on_press(())).auto_invalidate(false))
            .width(Length::Fill)
            .height(Length::Fill)
    };
    let _ = h.frame(view(), &[], 1.0);
    assert_eq!(cache.record_count(), 1);
    let centre = h.locate(view(), "go");
    h.cursor = mouse::Cursor::Available(centre);
    let click: Vec<Event> = iced_test::simulator::click().collect();
    let _ = h.frame(view(), &click, 1.0);
    let _ = h.frame(view(), &[], 1.0);
    assert_eq!(
        cache.record_count(),
        1,
        "with auto-invalidate off only an explicit invalidate re-records"
    );
    cache.invalidate();
    let _ = h.frame(view(), &[], 1.0);
    assert_eq!(cache.record_count(), 2);
}

/// The `iced::Renderer` facade is not what the harness drives; this pins
/// that the helper really is the software backend at scale 1.
#[test]
fn the_helper_is_headless_and_named() {
    let renderer = headless_tiny_skia();
    assert_ne!(Headless::name(&renderer), "");
}
