//! Controlled A/B: is the cached composite as sharp as drawing in place?
//!
//! Diagnostic, not a regression guard — it prints statistics and asserts only
//! the property that matters: a resting `LayoutOnly` composite at an integer
//! device phase must be pixel-identical to drawing the same text in place at
//! the snapped position.
#![cfg(feature = "wgpu")]

use iced_core::Renderer as _;
use iced_core::renderer::Headless;
use iced_core::text::{Alignment, LineHeight, Renderer as _, Shaping, Text, Wrapping};
use iced_core::{Color, Font, Pixels, Point, Rectangle, Size, Transformation, alignment};
use iced_texture_cache::{
    Composite, FilterQuality, Record, Renderer, TextureCache, TextureRenderer, Warp,
};

fn headless_wgpu() -> Renderer {
    iced_test::futures::futures::executor::block_on(<Renderer as Headless>::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .expect("a GPU adapter is available")
}

const CANVAS: Size<u32> = Size {
    width: 260,
    height: 60,
};
/// Matches `cached::BLEED`.
const BLEED: f32 = 2.0;
/// A deliberately fractional layout position, the normal case in a real tree.
const AT: Point = Point { x: 12.37, y: 14.61 };

fn label() -> Text<String, Font> {
    Text {
        content: "Add External Runtime".to_owned(),
        bounds: Size::new(240.0, 40.0),
        size: Pixels(16.0),
        line_height: LineHeight::Relative(1.2),
        font: Font::DEFAULT,
        align_x: Alignment::Left,
        align_y: alignment::Vertical::Top,
        shaping: Shaping::Advanced,
        wrapping: Wrapping::None,
    }
}

fn draw_text(renderer: &mut Renderer, at: Point) {
    renderer.fill_text(
        label(),
        at,
        Color::BLACK,
        Rectangle::with_size(Size::new(CANVAS.width as f32, CANVAS.height as f32)),
    );
}

fn luminance(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|p| ((u16::from(p[0]) + u16::from(p[1]) + u16::from(p[2])) / 3) as u8)
        .collect()
}

fn report(label: &str, shot: &[u8]) -> Vec<u8> {
    let lum = luminance(shot);
    let min = lum.iter().copied().min().unwrap_or(255);
    let dark = lum.iter().filter(|&&v| v < 128).count();
    let partial = lum.iter().filter(|&&v| (40..=215).contains(&v)).count();
    println!(
        "{label:22} min={min:3}  dark(<128)={dark:5}  partial(40..215)={partial:5}  \
         ink={:.1}",
        lum.iter().map(|&v| 255.0 - f64::from(v)).sum::<f64>() / 255.0
    );
    lum
}

#[test]
#[ignore = "needs a GPU adapter"]
fn a_resting_cached_composite_is_as_sharp_as_drawing_in_place() {
    let mut renderer = headless_wgpu();
    let cache = TextureCache::new();
    let scale = 1.0;

    // (1) Drawn in place at the raw fractional position.
    renderer.reset(Rectangle::with_size(Size::new(
        CANVAS.width as f32,
        CANVAS.height as f32,
    )));
    draw_text(&mut renderer, AT);
    let direct = report(
        "direct (fractional)",
        &renderer.screenshot(CANVAS, scale, Color::WHITE),
    );

    // (2) Drawn in place at the snapped position: what a `LayoutOnly`
    // composite should reproduce exactly.
    renderer.reset(Rectangle::with_size(Size::new(
        CANVAS.width as f32,
        CANVAS.height as f32,
    )));
    draw_text(&mut renderer, Point::new(AT.x.round(), AT.y.round()));
    let direct_snapped = report(
        "direct (snapped)",
        &renderer.screenshot(CANVAS, scale, Color::WHITE),
    );

    // (3) Through the cache, the way `Cached` does it with `LayoutOnly`:
    // content lands `BLEED` texels in, the texture is composited at the
    // snapped layout origin.
    let content = Rectangle::new(AT, Size::new(240.0, 40.0));
    let physical = Size::new(
        ((content.width.ceil() + 2.0 * BLEED) * scale).round() as u32,
        ((content.height.ceil() + 2.0 * BLEED) * scale).round() as u32,
    );
    let cache_bounds = Rectangle {
        x: (AT.x * scale).round() / scale - BLEED,
        y: (AT.y * scale).round() / scale - BLEED,
        width: physical.width as f32 / scale,
        height: physical.height as f32 / scale,
    };

    let record = renderer.record(&cache, physical, scale, |r| {
        draw_text(r, Point::new(BLEED, BLEED));
    });
    assert_eq!(record, Record::Fresh);

    for filter in [FilterQuality::CatmullRom, FilterQuality::Bilinear] {
        renderer.reset(Rectangle::with_size(Size::new(
            CANVAS.width as f32,
            CANVAS.height as f32,
        )));
        renderer.draw_cached(
            &cache,
            cache_bounds,
            cache_bounds,
            Rectangle::with_size(Size::new(CANVAS.width as f32, CANVAS.height as f32)),
            Transformation::IDENTITY,
            Composite {
                opacity: 1.0,
                filter,
                corners: iced::border::Radius::default(),
                warp: Warp::None,
            },
        );
        let shot = renderer.screenshot(CANVAS, scale, Color::WHITE);
        let cached = report(&format!("cached ({filter:?})"), &shot);

        let differing = cached
            .iter()
            .zip(&direct_snapped)
            .filter(|(a, b)| a != b)
            .count();
        let worst = cached
            .iter()
            .zip(&direct_snapped)
            .map(|(a, b)| u8::abs_diff(*a, *b))
            .max()
            .unwrap_or(0);
        println!("  vs direct(snapped): differing={differing}  worst delta={worst}");

        assert_eq!(
            differing, 0,
            "{filter:?} cost {worst} levels on a resting composite; a snapped \
             texture is a 1:1 blit and must reproduce the direct draw exactly"
        );
    }

    // Sanity: the fractional and snapped draws really are different images,
    // so the comparison above is not passing on a degenerate case.
    assert_ne!(direct, direct_snapped);
}

/// How much a composite spreads ink across partially-covered pixels: higher is
/// blurrier. Total ink is conserved by every kernel here, so the spread is the
/// honest measure.
fn spread(shot: &[u8]) -> f64 {
    let lum = luminance(shot);
    let solid = lum.iter().filter(|&&v| v < 90).count();
    let partial = lum.iter().filter(|&&v| (90..245).contains(&v)).count();
    partial as f64 / solid.max(1) as f64
}

#[test]
#[ignore = "needs a GPU adapter"]
fn a_fractional_vertical_offset_costs_far_more_than_a_horizontal_one() {
    // A `Pager` slide moves a page horizontally, but its height lerp also
    // moves it *vertically* by a fractional amount every frame. This measures
    // what that second axis costs: with y at integer phase the kernel
    // collapses to 3 taps along one axis, which is what keeps text crisp.
    let mut renderer = headless_wgpu();
    let cache = TextureCache::new();
    let scale = 1.0;
    let canvas = Rectangle::with_size(Size::new(CANVAS.width as f32, CANVAS.height as f32));

    let physical = Size::new(
        ((240.0_f32.ceil() + 2.0 * BLEED) * scale).round() as u32,
        ((40.0_f32.ceil() + 2.0 * BLEED) * scale).round() as u32,
    );
    let record = renderer.record(&cache, physical, scale, |r| {
        draw_text(r, Point::new(BLEED, BLEED));
    });
    assert_eq!(record, Record::Fresh);

    renderer.reset(canvas);
    draw_text(&mut renderer, Point::new(AT.x.round(), AT.y.round()));
    let reference = spread(&renderer.screenshot(CANVAS, scale, Color::WHITE));
    println!("direct (no resample)          spread={reference:.2}");

    let mut measured = Vec::new();

    for (label, dx, dy) in [
        ("x .5, y integer  (3 taps)", 0.5, 0.0),
        ("x .5, y .5       (9 taps)", 0.5, 0.5),
        ("x integer, y .5  (3 taps)", 0.0, 0.5),
    ] {
        for filter in [FilterQuality::CatmullRom, FilterQuality::Bilinear] {
            renderer.reset(canvas);
            let bounds = Rectangle {
                x: AT.x.round() - BLEED + dx,
                y: AT.y.round() - BLEED + dy,
                width: physical.width as f32 / scale,
                height: physical.height as f32 / scale,
            };
            renderer.draw_cached(
                &cache,
                bounds,
                bounds,
                canvas,
                Transformation::IDENTITY,
                Composite {
                    opacity: 1.0,
                    filter,
                    corners: iced::border::Radius::default(),
                    warp: Warp::None,
                },
            );
            let s = spread(&renderer.screenshot(CANVAS, scale, Color::WHITE));
            println!(
                "{label}  {filter:?}: spread={s:.2}  (+{:.0}% vs direct)",
                (s / reference - 1.0) * 100.0
            );

            if filter == FilterQuality::CatmullRom {
                measured.push((label, s));
            }
        }
    }

    let axial = measured[0].1;
    let diagonal = measured[1].1;
    assert!(
        diagonal > axial * 1.3,
        "a second fractional axis should cost significantly more ink spread \
         ({axial:.2} axis-aligned vs {diagonal:.2} diagonal); if these ever \
         converge, the separable collapse in the shader has stopped working"
    );
}

/// The `Pager` slide, end to end: the same page geometry the widget produces,
/// composited under each policy, measured for ink spread.
#[test]
#[ignore = "needs a GPU adapter"]
fn the_pager_policy_recovers_the_axis_aligned_sharpness() {
    use iced_texture_cache::PixelSnap;

    let mut renderer = headless_wgpu();
    let cache = TextureCache::new();
    let scale = 1.0;
    let canvas = Rectangle::with_size(Size::new(CANVAS.width as f32, CANVAS.height as f32));

    let physical = Size::new(
        ((240.0_f32.ceil() + 2.0 * BLEED) * scale).round() as u32,
        ((40.0_f32.ceil() + 2.0 * BLEED) * scale).round() as u32,
    );
    assert_eq!(
        renderer.record(&cache, physical, scale, |r| {
            draw_text(r, Point::new(BLEED, BLEED));
        }),
        Record::Fresh
    );

    renderer.reset(canvas);
    draw_text(&mut renderer, Point::new(AT.x.round(), AT.y.round()));
    let reference = spread(&renderer.screenshot(CANVAS, scale, Color::WHITE));

    // Mid-slide: x carries the slide, y the height interpolation. Both land
    // off the device grid before a policy is applied.
    let pager = Rectangle::new(Point::new(8.0, 6.0), Size::new(240.0, 40.0));
    let page = Rectangle::new(Point::new(8.0 - 37.4, 6.0 + 0.43), Size::new(240.0, 40.0));

    println!("direct (no resample)      spread={reference:.2}");
    let mut measured = Vec::new();

    for mode in [PixelSnap::Never, PixelSnap::Auto, PixelSnap::LayoutOnly] {
        let placed = iced_texture_cache::testing::pager_page_bounds(
            FilterQuality::CatmullRom,
            mode,
            page,
            pager,
            scale,
        );
        renderer.reset(canvas);
        let bounds = Rectangle {
            x: placed.x - BLEED,
            y: placed.y - BLEED,
            width: physical.width as f32 / scale,
            height: physical.height as f32 / scale,
        };
        renderer.draw_cached(
            &cache,
            bounds,
            bounds,
            canvas,
            Transformation::IDENTITY,
            Composite {
                opacity: 1.0,
                filter: FilterQuality::CatmullRom,
                corners: iced::border::Radius::default(),
                warp: Warp::None,
            },
        );
        let s = spread(&renderer.screenshot(CANVAS, scale, Color::WHITE));
        println!(
            "{mode:?}  spread={s:.2}  (+{:.0}% vs direct)",
            (s / reference - 1.0) * 100.0
        );
        measured.push((mode, s));
    }

    let never = measured[0].1;
    let auto = measured[1].1;
    assert!(
        auto < never * 0.85,
        "snapping the vertical axis should cut the spread markedly \
         ({never:.2} unsnapped vs {auto:.2} with Auto)"
    );
}
