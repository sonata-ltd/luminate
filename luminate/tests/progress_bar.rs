//! Progress geometry, theme tokens, percentage composition and animation pixels.

use std::sync::Mutex;

// The headless renderer and Simulator share iced's process-wide text system.
// Keep layout and rasterization on one thread at a time in this test binary.
static RENDER_LOCK: Mutex<()> = Mutex::new(());

use iced_luminate::iced::advanced::{
    Renderer as _, clipboard,
    renderer::{self, Headless},
};
use iced_luminate::iced::{
    Color, Event, Rectangle, Settings, Size, mouse,
    time::{Duration, Instant},
    window,
};
use iced_luminate::texture::testing::headless_tiny_skia;
use iced_luminate::{Element, Luminate, Renderer, Theme, descriptor};
use iced_test::{
    Simulator,
    runtime::user_interface::{self, UserInterface},
};

fn render(root: Element<'_, ()>, theme: &Theme, seconds: f32, size: Size) -> Vec<u8> {
    let mut renderer = headless_tiny_skia();
    let mut ui = UserInterface::build(root, size, user_interface::Cache::default(), &mut renderer);
    let start = Instant::now();
    let mut messages = Vec::new();
    for now in [start, start + Duration::from_secs_f32(seconds)] {
        let _ = ui.update(
            &[Event::Window(window::Event::RedrawRequested(now))],
            mouse::Cursor::Unavailable,
            &mut renderer,
            &mut clipboard::Null,
            &mut messages,
        );
    }
    renderer.reset(Rectangle::with_size(size));
    ui.draw(
        &mut renderer,
        theme,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );
    renderer.screenshot(
        Size::new(size.width as u32, size.height as u32),
        1.0,
        Color::TRANSPARENT,
    )
}

fn pixel(pixels: &[u8], width: usize, x: usize, y: usize) -> [u8; 4] {
    pixels[(y * width + x) * 4..(y * width + x + 1) * 4]
        .try_into()
        .unwrap()
}

#[test]
fn all_variants_use_custom_theme_tokens_and_errors_have_the_requested_extent() {
    let _guard = RENDER_LOCK.lock().unwrap();
    let theme = Theme::light(iced_luminate::theme::palette::Palette {
        accent: Color::from_rgb8(10, 150, 70),
        red: iced_luminate::theme::palette::ColorScale {
            s500: Color::from_rgb8(190, 30, 60),
            ..iced_luminate::theme::palette::ColorScale::RED
        },
        ..iced_luminate::theme::palette::Palette::LIGHT
    });
    let kit = Luminate::with_theme(theme);
    for (error, show) in [(false, false), (false, true), (true, false), (true, true)] {
        let pixels = render(
            kit.progress_bar(
                descriptor::ProgressBar::new(20.0..=60.0, 30.0)
                    .errored(error)
                    .show_progress(show),
            ),
            &theme,
            0.0,
            Size::new(300.0, 30.0),
        );
        let expected = if error {
            theme.palette.red.s500
        } else {
            theme.palette.accent
        }
        .into_rgba8();
        // Percentage rows center their 6px track in a 20px line box.
        let y = if show { 10 } else { 3 };
        assert_eq!(pixel(&pixels, 300, 10, y), expected);
        assert_eq!(
            pixel(&pixels, 300, 200, y),
            if error && !show {
                expected
            } else {
                theme.progress_bar.background.into_rgba8()
            }
        );
        assert!(
            pixel(&pixels, 300, 0, 0)[3] < 64,
            "antialiased rounded corners"
        );
    }
}

#[test]
fn percentage_uses_the_range_and_is_hidden_only_while_indeterminate() {
    let _guard = RENDER_LOCK.lock().unwrap();
    let kit = Luminate::new();
    for (indeterminate, error, visible) in [
        (false, false, true),
        (true, false, false),
        (true, true, true),
    ] {
        let root: Element<'_, ()> = kit.progress_bar(
            descriptor::ProgressBar::new(20.0..=60.0, 30.0)
                .show_progress(true)
                .indeterminate(indeterminate)
                .errored(error),
        );
        let mut ui: Simulator<'_, (), Theme, Renderer> =
            Simulator::with_size(Settings::default(), Size::new(300.0, 30.0), root);
        assert_eq!(ui.find("25%").is_ok(), visible);
    }
}

#[test]
fn rendered_highlights_match_css_source_over_at_startup_and_across_cycles() {
    let _guard = RENDER_LOCK.lock().unwrap();
    let kit = Luminate::new();
    let theme = *kit.theme();
    for seconds in [0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.25, 5.25] {
        let pixels = render(
            kit.progress_bar(descriptor::ProgressBar::new(0.0..=1.0, 0.0).indeterminate(true)),
            &theme,
            seconds,
            Size::new(300.0, 6.0),
        );
        for x in (10..290).step_by(10) {
            let mut expected = [
                theme.progress_bar.bar.r,
                theme.progress_bar.bar.g,
                theme.progress_bar.bar.b,
            ];
            let light = theme.progress_bar.highlight;
            for delay in [0.0, 1.0, 2.0] {
                let local = if seconds < delay {
                    0.0
                } else {
                    (seconds - delay) % 2.0 - 1.0
                };
                let position = (x as f32 + 0.5) / 300.0 - local;
                let alpha = if (0.0..=1.0).contains(&position) {
                    1.0 - (position * 2.0 - 1.0).abs()
                } else {
                    0.0
                };
                for (channel, highlight) in expected.iter_mut().zip([light.r, light.g, light.b]) {
                    *channel = *channel * (1.0 - alpha) + highlight * alpha;
                }
            }
            let actual = pixel(&pixels, 300, x, 3);
            for (actual, expected) in actual[..3].iter().zip(expected) {
                assert!(
                    actual.abs_diff((expected * 255.0).round() as u8) <= 3,
                    "t={seconds}, x={x}: {actual} vs {}",
                    expected * 255.0
                );
            }
        }
    }
}

#[test]
fn vertical_progress_fills_from_the_bottom() {
    let _guard = RENDER_LOCK.lock().unwrap();
    let kit = Luminate::new();
    let pixels = render(
        kit.progress_bar(descriptor::ProgressBar::new(0.0..=100.0, 25.0).vertical()),
        kit.theme(),
        0.0,
        Size::new(6.0, 100.0),
    );
    assert_eq!(
        pixel(&pixels, 6, 3, 90),
        kit.theme().palette.accent.into_rgba8()
    );
    assert_eq!(
        pixel(&pixels, 6, 3, 50),
        kit.theme().progress_bar.background.into_rgba8()
    );
}

#[test]
fn fixed_length_includes_the_percentage_label() {
    let _guard = RENDER_LOCK.lock().unwrap();
    let kit = Luminate::new();
    for vertical in [false, true] {
        let mut descriptor = descriptor::ProgressBar::new(0.0..=100.0, 30.0)
            .length(180.0)
            .show_progress(true);
        if vertical {
            descriptor = descriptor.vertical();
        }
        let root: Element<'_, ()> = kit.progress_bar(descriptor);
        let mut ui: Simulator<'_, (), Theme, Renderer> =
            Simulator::with_size(Settings::default(), Size::new(300.0, 300.0), root);
        let label = ui.find("30%").expect("percentage").bounds();
        assert!(
            if vertical {
                label.y + label.height <= 180.0
            } else {
                label.x + label.width <= 180.0
            },
            "{label:?}"
        );
    }
}
