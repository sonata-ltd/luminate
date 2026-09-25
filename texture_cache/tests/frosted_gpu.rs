//! The wgpu blur chain, measured against the sigma it was asked for.
//!
//! `tests/frosted.rs` proves the software chain end to end; a green run
//! there says nothing about this one — that is the whole reason `radius` is
//! defined as a sigma rather than as a number of passes, and calibrating
//! only one backend against it would be pointless. Ignored by default: it
//! needs a real adapter, which CI does not have.

#![cfg(feature = "wgpu")]

use iced::advanced::Renderer as _;
use iced::advanced::renderer::{self, Headless};
use iced::time::Instant;
use iced::widget::{row, stack};
use iced::{Color, Event, Font, Pixels, Rectangle, Size, mouse, window};
use iced_animate::widget::shape;
use iced_test::runtime::user_interface::{self, UserInterface};
use iced_texture_cache::{Renderer, TextureCache, cached, frosted};

const RED: Color = Color::from_rgb(1.0, 0.0, 0.0);

/// Matches `tests/frosted.rs::BIG_WINDOW`: wide enough that the red/white
/// seam sits far from both the source's `BLEED` margin and the canvas edge.
const WINDOW: Size = Size::new(200.0, 60.0);

type El<'a> = iced_texture_cache::Element<'a, ()>;

fn headless_wgpu() -> Renderer {
    iced_test::futures::futures::executor::block_on(<Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .expect("a GPU adapter is available")
}

/// A red/white strip under a pane of glass — the same shape
/// `tests/frosted.rs::glass_over_split` uses, and for the same reason: the
/// square's own outer edge is clamped to the source's recorded footprint
/// (content plus the fixed 2px `BLEED` from `cached.rs`), so a profile
/// measured there is an edge artefact, not the kernel. The seam sits deep
/// inside the recorded texture, 80px from either outer edge.
fn glass_over_split(source: &TextureCache, radius: f32) -> El<'_> {
    let content = row![
        shape().width(80.0).height(40.0).fill(RED),
        shape().width(80.0).height(40.0).fill(Color::WHITE),
    ];
    stack![
        cached(source.clone(), content),
        frosted(source.clone())
            .radius(radius)
            .width(160.0)
            .height(40.0),
    ]
    .into()
}

fn pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
    let start = ((y * width + x) * 4) as usize;
    [
        rgba[start],
        rgba[start + 1],
        rgba[start + 2],
        rgba[start + 3],
    ]
}

/// The brightness profile across the red/white seam, on the green channel:
/// 0 on the red side, 255 on the white side.
///
/// Mirrors `tests/frosted.rs::edge_profile` exactly (same window, same
/// channel, same row) so the two measurements are directly comparable;
/// duplicated rather than shared because integration test binaries do not
/// share modules with one another.
fn edge_profile(rgba: &[u8]) -> Vec<f64> {
    (55..105)
        .map(|x| f64::from(pixel(rgba, WINDOW.width as u32, x, 20)[1]) / 255.0)
        .collect()
}

/// Duplicates `blur::cpu`'s private `measured_sigma`. The derivative of a
/// step edge's profile is the kernel itself, so its second moment is the
/// variance actually delivered — measuring it, rather than comparing the
/// profile to a Gaussian point by point, is what pins a calibration down.
/// The disagreement between this backend's measurement and the software
/// one is exactly the subject of this test; see `blur::gpu`'s "Colour
/// space" section for the source of it in a gamma-corrected build.
fn measured_sigma(profile: &[f64]) -> f64 {
    let derivative: Vec<f64> = profile.windows(2).map(|w| w[1] - w[0]).collect();
    let mass: f64 = derivative.iter().sum();
    let mean: f64 = derivative
        .iter()
        .enumerate()
        .map(|(i, d)| i as f64 * d)
        .sum::<f64>()
        / mass;
    let variance: f64 = derivative
        .iter()
        .enumerate()
        .map(|(i, d)| (i as f64 - mean).powi(2) * d)
        .sum::<f64>()
        / mass;

    variance.sqrt()
}

#[test]
#[ignore = "needs a GPU adapter"]
fn the_gpu_chain_delivers_the_sigma_it_was_asked_for() {
    let mut renderer = headless_wgpu();
    let source = TextureCache::new();
    let sigma = 6.0_f32;

    let cache = user_interface::Cache::default();
    let mut ui: UserInterface<'_, (), iced::Theme, Renderer> = UserInterface::build(
        glass_over_split(&source, sigma),
        WINDOW,
        cache,
        &mut renderer,
    );
    let now = Instant::now();
    let _ = ui.update(
        &[Event::Window(window::Event::RedrawRequested(now))],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut iced::advanced::clipboard::Null,
        &mut Vec::new(),
    );
    renderer.reset(Rectangle::with_size(WINDOW));
    ui.draw(
        &mut renderer,
        &iced::Theme::Light,
        &renderer::Style {
            text_color: Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );

    let physical = Size::new(WINDOW.width as u32, WINDOW.height as u32);
    let rgba = renderer.screenshot(physical, 1.0, Color::WHITE);

    let profile = edge_profile(&rgba);
    let measured = measured_sigma(&profile);

    // The same 15% tolerance the software chain is held to against its own
    // kernel promise: a genuine calibration error (a missing `/downscale`
    // in `residual_sigma`, say) is many times this size, so this is not a
    // license to be sloppy — it is room for the two backends' different
    // kernels (a separable Gaussian here, three box passes in software) and
    // for the colour-space divergence `blur::gpu` documents.
    assert!(
        (measured - f64::from(sigma)).abs() <= 0.15 * f64::from(sigma),
        "measured sigma {measured} against the requested {sigma} (more than 15% off)"
    );
}
