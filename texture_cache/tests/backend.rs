//! End-to-end tests of the render backend through the public API:
//! record → composite → screenshot, with pixel assertions.
//!
//! The `tiny_skia` tests need no GPU. The wgpu tests are `#[ignore]`d: run them
//! locally on a machine with an adapter with
//! `cargo test -p iced_texture_cache, --include-ignored`; the CI `gpu` job
//! runs them on lavapipe.

use iced_core::Renderer as _;
use iced_core::renderer::{Headless, Quad};
use iced_core::{Color, Point, Rectangle, Size, Transformation};
use iced_texture_cache::{
    Backend, Composite, FilterQuality, Record, Renderer, TextureCache, TextureRenderer, Warp,
};

const CANVAS: Size<u32> = Size {
    width: 8,
    height: 8,
};
const TEXTURE: Size<u32> = Size {
    width: 4,
    height: 4,
};

fn canvas() -> Rectangle {
    Rectangle::with_size(Size::new(8.0, 8.0))
}

/// RGBA of the pixel at `(x, y)` of an 8 x 8 screenshot.
fn pixel(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * CANVAS.width + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

/// Records a solid red 4 x 4 px texture into `cache` at `scale_factor`.
fn record_red(renderer: &mut Renderer, cache: &TextureCache, scale_factor: f32) -> Record {
    renderer.record(cache, TEXTURE, scale_factor, |r| {
        let logical = 4.0 / scale_factor;
        r.fill_quad(
            Quad {
                bounds: Rectangle::with_size(Size::new(logical, logical)),
                ..Quad::default()
            },
            Color::from_rgb(1.0, 0.0, 0.0),
        );
    })
}

/// Composites `cache` at (2, 2) over white and returns the screenshot.
fn composite(renderer: &mut Renderer, cache: &TextureCache, opacity: f32) -> Vec<u8> {
    composite_with(renderer, cache, opacity, FilterQuality::Bilinear, 0.0)
}

/// Composites `cache` at (2, 2) shifted by `offset` on both axes, through
/// `filter`, over white.
fn composite_with(
    renderer: &mut Renderer,
    cache: &TextureCache,
    opacity: f32,
    filter: FilterQuality,
    offset: f32,
) -> Vec<u8> {
    renderer.reset(canvas());
    let bounds = Rectangle::new(Point::new(2.0 + offset, 2.0 + offset), Size::new(4.0, 4.0));
    renderer.draw_cached(
        cache,
        bounds,
        bounds,
        canvas(),
        Transformation::IDENTITY,
        Composite {
            opacity,
            filter,
            corners: iced::border::Radius::default(),
            warp: Warp::None,
        },
    );
    renderer.screenshot(CANVAS, 1.0, Color::WHITE)
}

const WHITE: [u8; 4] = [255, 255, 255, 255];

/// Composites `cache` at (2, 2) over white, cut to a radius far larger than
/// its 4 x 4 px: the corners have to be clamped to a circle, on every
/// backend alike.
fn composite_pill(renderer: &mut Renderer, cache: &TextureCache) -> Vec<u8> {
    renderer.reset(canvas());
    let bounds = Rectangle::new(Point::new(2.0, 2.0), Size::new(4.0, 4.0));
    renderer.draw_cached(
        cache,
        bounds,
        bounds,
        canvas(),
        Transformation::IDENTITY,
        Composite {
            opacity: 1.0,
            filter: FilterQuality::Bilinear,
            corners: iced::border::Radius::from(1000.0),
            warp: Warp::None,
        },
    );
    renderer.screenshot(CANVAS, 1.0, Color::WHITE)
}

/// A radius too large for the rectangle rounds it into a circle rather than
/// cutting it away: the middle stays, the corner pixel goes.
fn assert_clamped_to_a_circle(shot: &[u8]) {
    assert_red(pixel(shot, 3, 3));
    assert_red(pixel(shot, 4, 4));
    let corner = pixel(shot, 2, 2);
    assert!(corner[1] > 60, "the corner is cut off: {corner:?}");
}

fn assert_red(px: [u8; 4]) {
    assert!(
        px[0] >= 250 && px[1] <= 3 && px[2] <= 3 && px[3] == 255,
        "pure red: {px:?}"
    );
}

#[cfg(feature = "tiny-skia")]
mod tiny_skia {
    use std::cell::Cell;

    use super::*;
    use iced_texture_cache::testing::headless_tiny_skia;

    /// Red over white at 50 %: full red, half green and blue. A BGRA swizzle
    /// bug would put the 255 in the blue channel; a premultiply bug would
    /// darken red.
    fn assert_half_red(px: [u8; 4]) {
        assert!(px[0] >= 250, "red channel: {px:?}");
        assert!(
            (120..=136).contains(&px[1]) && (120..=136).contains(&px[2]),
            "half white: {px:?}"
        );
        assert_eq!(px[3], 255, "opaque canvas: {px:?}");
    }

    #[test]
    fn the_helper_is_a_software_renderer_at_scale_one() {
        let renderer = headless_tiny_skia();
        assert_eq!(renderer.backend(), Backend::TinySkia);
        assert_eq!(renderer.scale_factor(), 1.0);
    }

    #[test]
    fn a_recorded_red_quad_composites_red_with_the_given_opacity() {
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();

        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);

        let half = composite(&mut renderer, &cache, 0.5);
        assert_half_red(pixel(&half, 3, 3));
        assert_eq!(pixel(&half, 0, 0), WHITE, "outside the composite");
        assert_eq!(pixel(&half, 7, 7), WHITE, "outside the composite");

        let full = composite(&mut renderer, &cache, 1.0);
        assert_red(pixel(&full, 2, 2));
        assert_red(pixel(&full, 5, 5));
        assert_eq!(pixel(&full, 6, 6), WHITE, "the texture is 4 px wide");
    }

    #[test]
    fn nan_or_non_positive_opacity_draws_nothing_and_large_opacity_is_clamped() {
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);

        assert_eq!(
            pixel(&composite(&mut renderer, &cache, f32::NAN), 3, 3),
            WHITE
        );
        assert_eq!(pixel(&composite(&mut renderer, &cache, 0.0), 3, 3), WHITE);
        assert_eq!(pixel(&composite(&mut renderer, &cache, -1.0), 3, 3), WHITE);
        assert_red(pixel(&composite(&mut renderer, &cache, 7.0), 3, 3));
    }

    #[test]
    fn a_valid_texture_is_reused_until_invalidated() {
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();

        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Reused);
        assert_eq!(cache.record_count(), 1);

        cache.invalidate();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_eq!(cache.record_count(), 2);

        // A size change re-records too, and the new content is what shows.
        let ran = Cell::new(false);
        let record = renderer.record(&cache, Size::new(5, 5), 1.0, |_| ran.set(true));
        assert_eq!(record, Record::Fresh);
        assert!(ran.get());
        assert_eq!(cache.record_count(), 3);
    }

    #[test]
    fn oversize_requests_are_uncacheable_and_never_run_the_closure() {
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();
        let ran = Cell::new(false);

        let per_side = renderer.record(&cache, Size::new(16_385, 1), 1.0, |_| ran.set(true));
        assert_eq!(per_side, Record::Uncacheable);

        let bytes = renderer.record(&cache, Size::new(16_384, 16_384), 1.0, |_| ran.set(true));
        assert_eq!(bytes, Record::Uncacheable, "1 GiB exceeds the 256 MiB cap");

        assert!(!ran.get(), "an uncacheable request never runs the closure");
        assert_eq!(cache.record_count(), 0);
        assert!(!cache.is_invalidated(), "the flag is consumed anyway");

        // Nothing to composite.
        assert_eq!(pixel(&composite(&mut renderer, &cache, 1.0), 3, 3), WHITE);

        // A fitting request recovers.
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_red(pixel(&composite(&mut renderer, &cache, 1.0), 3, 3));
    }

    #[test]
    fn screenshot_sets_the_recording_scale_factor() {
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();

        assert_eq!(renderer.scale_factor(), 1.0);
        let _ = renderer.screenshot(CANVAS, 2.0, Color::WHITE);
        assert_eq!(renderer.scale_factor(), 2.0);

        // Recording at the renderer's scale works, and a scale change re-records.
        let scale = renderer.scale_factor();
        assert_eq!(record_red(&mut renderer, &cache, scale), Record::Fresh);
        assert_eq!(record_red(&mut renderer, &cache, scale), Record::Reused);
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_eq!(cache.record_count(), 2);
    }

    #[test]
    fn a_nested_record_is_baked_into_the_outer_texture() {
        let mut renderer = headless_tiny_skia();
        let outer = TextureCache::new();
        let inner = TextureCache::new();
        let quad = Rectangle::with_size(Size::new(4.0, 4.0));

        let record = renderer.record(&outer, TEXTURE, 1.0, |r| {
            assert_eq!(record_red(r, &inner, 1.0), Record::Fresh);
            r.draw_cached(
                &inner,
                quad,
                quad,
                quad,
                Transformation::IDENTITY,
                Composite {
                    opacity: 1.0,
                    filter: FilterQuality::Bilinear,
                    corners: iced::border::Radius::default(),
                    warp: Warp::None,
                },
            );
        });
        assert_eq!(record, Record::Fresh);
        assert_eq!((outer.record_count(), inner.record_count()), (1, 1));

        assert_red(pixel(&composite(&mut renderer, &outer, 1.0), 3, 3));
    }

    #[test]
    fn dropping_every_handle_frees_the_texture_at_the_next_frame_boundary() {
        // Observable only indirectly through the public API: a new cache with
        // a fresh identity records afresh, and the old one's texture cannot be
        // composited any more because there is no handle to name it. The
        // store-level assertion lives in `record.rs` (`store_tests`).
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        drop(cache);
        let _ = renderer.screenshot(CANVAS, 1.0, Color::WHITE); // frame boundary
        let again = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &again, 1.0), Record::Fresh);
        assert_red(pixel(&composite(&mut renderer, &again, 1.0), 3, 3));
    }
    #[test]
    fn a_radius_past_half_the_rectangle_is_clamped_to_a_circle() {
        let mut renderer = headless_tiny_skia();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_clamped_to_a_circle(&composite_pill(&mut renderer, &cache));
    }
}

#[cfg(feature = "wgpu")]
mod wgpu {
    use super::*;
    use iced_core::{Font, Pixels};

    /// Records a 4 x 4 px texture with a hard interior edge: the left half red,
    /// the right half black. A uniform texture would reconstruct identically under
    /// every kernel (they all sum to 1), so a filtering test needs structure.
    fn record_edge(renderer: &mut Renderer, cache: &TextureCache) -> Record {
        renderer.record(cache, TEXTURE, 1.0, |r| {
            r.fill_quad(
                Quad {
                    bounds: Rectangle::new(Point::new(0.0, 0.0), Size::new(2.0, 4.0)),
                    ..Quad::default()
                },
                Color::from_rgb(1.0, 0.0, 0.0),
            );
            r.fill_quad(
                Quad {
                    bounds: Rectangle::new(Point::new(2.0, 0.0), Size::new(2.0, 4.0)),
                    ..Quad::default()
                },
                Color::BLACK,
            );
        })
    }

    fn headless_wgpu() -> Renderer {
        iced_test::futures::futures::executor::block_on(<Renderer as Headless>::new(
            Font::DEFAULT,
            Pixels(16.0),
            Some("wgpu"),
        ))
        .expect("a GPU adapter is available")
    }

    /// Blending happens in linear light when `web-colors` is off, so only
    /// gamma-agnostic properties are asserted here.
    fn assert_half_red_any_gamma(px: [u8; 4]) {
        assert!(px[0] >= 250, "red channel: {px:?}");
        assert_eq!(px[1], px[2], "green and blue agree: {px:?}");
        assert!((100..=200).contains(&px[1]), "half-mixed: {px:?}");
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn a_recorded_red_quad_composites_red_with_the_given_opacity() {
        let mut renderer = headless_wgpu();
        assert_eq!(renderer.backend(), Backend::Wgpu);
        let cache = TextureCache::new();

        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_half_red_any_gamma(pixel(&composite(&mut renderer, &cache, 0.5), 3, 3));
        assert_red(pixel(&composite(&mut renderer, &cache, 1.0), 3, 3));
        assert_eq!(pixel(&composite(&mut renderer, &cache, 1.0), 0, 0), WHITE);
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn a_valid_texture_is_reused_until_invalidated() {
        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Reused);
        cache.invalidate();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_eq!(cache.record_count(), 2);
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn screenshot_sets_the_recording_scale_factor() {
        let mut renderer = headless_wgpu();
        let _ = renderer.screenshot(CANVAS, 2.0, Color::WHITE);
        assert_eq!(renderer.scale_factor(), 2.0);
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn a_nested_record_is_baked_into_the_outer_texture() {
        let mut renderer = headless_wgpu();
        let outer = TextureCache::new();
        let inner = TextureCache::new();
        let quad = Rectangle::with_size(Size::new(4.0, 4.0));

        let record = renderer.record(&outer, TEXTURE, 1.0, |r| {
            assert_eq!(record_red(r, &inner, 1.0), Record::Fresh);
            r.draw_cached(
                &inner,
                quad,
                quad,
                quad,
                Transformation::IDENTITY,
                Composite {
                    opacity: 1.0,
                    filter: FilterQuality::Bilinear,
                    corners: iced::border::Radius::default(),
                    warp: Warp::None,
                },
            );
        });
        assert_eq!(record, Record::Fresh);
        assert_red(pixel(&composite(&mut renderer, &outer, 1.0), 3, 3));
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn two_textures_for_one_cache_in_a_frame_draw_their_own_content() {
        // G-008: the bindings are keyed by texture, not by cache id, so a
        // re-record at another size within one frame composites the right
        // texture at each place.
        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        renderer.reset(canvas());

        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(4.0, 4.0));
        renderer.draw_cached(
            &cache,
            bounds,
            bounds,
            canvas(),
            Transformation::IDENTITY,
            Composite {
                opacity: 1.0,
                filter: FilterQuality::Bilinear,
                corners: iced::border::Radius::default(),
                warp: Warp::None,
            },
        );

        // Same cache, new size: a new texture, blue this time.
        let record = renderer.record(&cache, Size::new(2, 2), 1.0, |r| {
            r.fill_quad(
                Quad {
                    bounds: Rectangle::with_size(Size::new(2.0, 2.0)),
                    ..Quad::default()
                },
                Color::from_rgb(0.0, 0.0, 1.0),
            );
        });
        assert_eq!(record, Record::Fresh);
        let bounds = Rectangle::new(Point::new(6.0, 6.0), Size::new(2.0, 2.0));
        renderer.draw_cached(
            &cache,
            bounds,
            bounds,
            canvas(),
            Transformation::IDENTITY,
            Composite {
                opacity: 1.0,
                filter: FilterQuality::Bilinear,
                corners: iced::border::Radius::default(),
                warp: Warp::None,
            },
        );

        let shot = renderer.screenshot(CANVAS, 1.0, Color::WHITE);
        assert_red(pixel(&shot, 1, 1));
        let blue = pixel(&shot, 7, 7);
        assert!(
            blue[2] >= 250 && blue[0] <= 3 && blue[1] <= 3,
            "blue: {blue:?}"
        );
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn a_genie_draws_nothing_past_its_anchor() {
        use iced_texture_cache::{Genie, GenieShape};

        // The 4 x 4 texture stands for a 2 x 2 content with a pixel of
        // bleed on every side, composited at (2, 2): the content's top-left
        // corner, the anchor, is at (3, 3) and the padding above it is row
        // 2. Rows that have travelled through the anchor must not be drawn
        // there, and fully collapsed nothing must be drawn at all.
        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        let bounds = Rectangle::new(Point::new(2.0, 2.0), Size::new(4.0, 4.0));
        let content = Rectangle::new(Point::new(3.0, 3.0), Size::new(2.0, 2.0));
        let mut composite = |progress: f32, corner_radius: f32| {
            renderer.reset(canvas());
            renderer.draw_cached(
                &cache,
                bounds,
                content,
                canvas(),
                Transformation::IDENTITY,
                Composite {
                    opacity: 1.0,
                    filter: FilterQuality::Bilinear,
                    corners: corner_radius.into(),
                    warp: Warp::Genie(Genie::new(progress, GenieShape::default())),
                },
            );
            renderer.screenshot(CANVAS, 1.0, Color::WHITE)
        };

        for corner_radius in [0.0, 8.0] {
            let gone = composite(0.0, corner_radius);
            for y in 0..CANVAS.height {
                for x in 0..CANVAS.width {
                    assert_eq!(
                        pixel(&gone, x, y),
                        WHITE,
                        "radius {corner_radius}: collapsed content at ({x}, {y})"
                    );
                }
            }
        }

        let half = composite(0.5, 0.0);
        assert_red(pixel(&half, 3, 3));
        assert_eq!(pixel(&half, 3, 2), WHITE, "a consumed row past the anchor");
        assert_eq!(pixel(&half, 2, 3), WHITE, "the padding beside the anchor");
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn the_filter_tiers_reconstruct_a_sub_pixel_offset_differently() {
        // The composite's viewport is set from unsnapped bounds, so a
        // fractional offset reaches the fragment shader as sub-pixel phase.
        // Each tier must then do something different with it.
        //
        // The offset is a quarter pixel, not a half: at phase ½ the two
        // outer Catmull-Rom taps of a step edge read the same values as
        // their neighbours, so the negative lobes (-1/16) cancel the boost
        // (9/16) exactly and the kernel *does* reduce to the bilinear
        // midpoint. Every other phase separates them.
        const OFFSET: f32 = 0.25;
        /// The pixel the edge falls in once the composite is offset.
        const EDGE: (u32, u32) = (4, 4);

        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        assert_eq!(record_edge(&mut renderer, &cache), Record::Fresh);

        let aligned = composite_with(&mut renderer, &cache, 1.0, FilterQuality::Bilinear, 0.0);
        let bilinear = composite_with(&mut renderer, &cache, 1.0, FilterQuality::Bilinear, OFFSET);
        let catmull_rom = composite_with(
            &mut renderer,
            &cache,
            1.0,
            FilterQuality::CatmullRom,
            OFFSET,
        );
        let snapped = composite_with(&mut renderer, &cache, 1.0, FilterQuality::Snap, OFFSET);

        // The offset is real: it lands the edge between two texels, which an
        // aligned composite never does.
        let blend = pixel(&bilinear, EDGE.0, EDGE.1);
        assert!(
            blend[0] > 0 && blend[0] < 255,
            "sub-pixel phase never reached the shader: {blend:?}"
        );
        assert_ne!(bilinear, aligned);

        // Catmull-Rom's negative lobes overshoot, pulling the transition
        // pixel further towards the dark side than the single tap does: that
        // overshoot is what keeps a moving edge looking sharp.
        let sharp = pixel(&catmull_rom, EDGE.0, EDGE.1);
        assert!(
            sharp[0] < blend[0],
            "the bicubic path did not sharpen the edge: {sharp:?} vs {blend:?}"
        );

        // `Snap` composites the bounds it is handed — snapping is the
        // caller's job — so here it must match the single tap exactly. What
        // it must not do is take the bicubic path.
        assert_eq!(
            snapped, bilinear,
            "Snap must share the single-tap shader path"
        );
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn an_aligned_composite_is_identical_across_the_tiers() {
        // At integer phase Catmull-Rom collapses to one exact tap, so a
        // resting frame must not depend on the tier at all.
        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        assert_eq!(record_edge(&mut renderer, &cache), Record::Fresh);

        let bilinear = composite_with(&mut renderer, &cache, 1.0, FilterQuality::Bilinear, 0.0);

        for filter in [FilterQuality::CatmullRom, FilterQuality::Snap] {
            let shot = composite_with(&mut renderer, &cache, 1.0, filter, 0.0);
            assert_eq!(shot, bilinear, "{filter:?} changed a pixel-aligned frame");
        }
    }

    #[test]
    #[ignore = "needs a GPU adapter"]
    fn changing_the_filter_does_not_re_record() {
        // The tier only affects the composite, so it must not be part of the
        // key that decides whether a texture is still valid.
        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);

        for filter in [
            FilterQuality::CatmullRom,
            FilterQuality::Bilinear,
            FilterQuality::Snap,
        ] {
            assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Reused);
            let _ = composite_with(&mut renderer, &cache, 1.0, filter, 0.5);
        }

        assert_eq!(cache.record_count(), 1);
    }
    /// The shader took the radii as given, and past half the rectangle its
    /// distance field turned inside out: the software backend drew a circle
    /// and the GPU drew nothing at all.
    #[test]
    #[ignore = "needs a GPU adapter"]
    fn a_radius_past_half_the_rectangle_is_clamped_to_a_circle() {
        let mut renderer = headless_wgpu();
        let cache = TextureCache::new();
        assert_eq!(record_red(&mut renderer, &cache, 1.0), Record::Fresh);
        assert_clamped_to_a_circle(&composite_pill(&mut renderer, &cache));
    }
}
