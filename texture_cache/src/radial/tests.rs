use super::*;

#[test]
fn stops_are_sorted_replaced_and_bounded() {
    let fill = Radial::new()
        .add_stop(1.0, Color::WHITE)
        .add_stop(0.0, Color::BLACK)
        .add_stop(1.0, Color::TRANSPARENT)
        .add_stop(f32::NAN, Color::WHITE);
    assert_eq!(fill.stops[0].unwrap().offset, 0.0);
    assert_eq!(fill.stops[1].unwrap().color, Color::TRANSPARENT);
    assert!(fill.stops[2].is_none());
    let full = (0..9).fold(Radial::new(), |fill, i| {
        fill.add_stop(i as f32 / 8.0, Color::WHITE)
    });
    assert_eq!(full.stops.iter().flatten().count(), 8);
    assert_eq!(full.stops[7].unwrap().offset, 7.0 / 8.0);
}

#[test]
fn degenerate_geometry_is_invisible() {
    let fill = Radial::new().add_stop(0.0, Color::WHITE);
    let bounds = Rectangle::with_size(Size::new(50.0, 40.0));
    assert!(drawable(bounds, fill, 1.0));
    assert!(!drawable(bounds, fill.radii([0.0, 0.5]), 1.0));
    assert!(!drawable(bounds, fill.center([f32::NAN, 0.5]), 1.0));
    assert!(!drawable(bounds, fill, f32::INFINITY));
    assert!(!drawable(Rectangle::with_size(Size::ZERO), fill, 1.0));
    assert!(!drawable(bounds, Radial::new(), 1.0));
}

#[cfg(feature = "tiny-skia")]
mod software_tests {
    use super::*;
    use crate::{TextureCache, cached, test_support::Harness};
    use iced::widget::{button, container, stack, text};
    use iced_core::time::Instant;

    fn fill() -> Radial {
        Radial::new()
            .add_stop(0.0, Color::from_rgb(1.0, 0.0, 0.0))
            .add_stop(1.0, Color::TRANSPARENT)
    }

    #[test]
    fn interpolation_does_not_darken_transparent_edges() {
        assert_eq!(software::sample(fill(), 0.5), [0.5, 0.0, 0.0, 0.5]);
        let off = fill()
            .add_stop(0.0, Color::TRANSPARENT)
            .add_stop(0.25, Color::WHITE);
        assert_eq!(software::sample(off, 0.125), [0.5; 4]);
    }

    #[test]
    fn raster_has_a_bright_centre_transparent_edge_and_rounded_corners() {
        let element: crate::Element<'_, ()> = radial_gradient(fill()).width(80).height(40).into();
        let mut harness = Harness::new(Size::new(80.0, 40.0), element);
        harness.frame(Instant::now());
        let shot = harness.screenshot(1.0);
        assert!(shot.pixel(40, 20)[1] < 20);
        assert!(shot.pixel(0, 0)[1] > 245);
        assert!(shot.pixel(60, 20)[1] > 100);
        let solid = Radial::new().add_stop(0.0, Color::BLACK);
        let mut round: Harness<'_, ()> = Harness::new(
            Size::new(40.0, 40.0),
            radial_gradient(solid).border_radius(20.0),
        );
        round.frame(Instant::now());
        assert_eq!(round.screenshot(2.0).pixel(0, 0), [255; 4]);
        assert_eq!(round.screenshot(2.0).pixel(40, 40), [0, 0, 0, 255]);
    }

    #[test]
    fn a_gradient_inside_a_cache_updates_after_view_rebuild() {
        let cache = TextureCache::new();
        let element = |color| -> crate::Element<'_, ()> {
            cached(
                cache.clone(),
                radial_gradient(Radial::new().add_stop(0.0, color))
                    .width(40)
                    .height(40),
            )
            .into()
        };
        let mut harness = Harness::new(Size::new(40.0, 40.0), element(Color::BLACK));
        harness.frame(Instant::now());
        assert_eq!(harness.screenshot(1.0).pixel(20, 20), [0, 0, 0, 255]);
        let mut harness = harness.rebuild(element(Color::WHITE));
        harness.frame(Instant::now());
        assert_eq!(harness.screenshot(1.0).pixel(20, 20), [255; 4]);
    }

    #[test]
    fn an_overlay_does_not_capture_clicks_from_content() {
        let element: crate::Element<'_, bool> =
            stack![button(text("Play")).on_press(true), radial_gradient(fill()),].into();
        let mut harness = Harness::new(Size::new(80.0, 40.0), container(element));
        harness.click("Play");
        assert_eq!(harness.into_messages(), vec![true]);
    }

    #[test]
    fn animated_paint_invalidates_cached_ancestors_and_then_stops() {
        use iced_animate::{Curve, Motion, MotionKey, SpringParams};
        use std::time::Duration;
        let motion = Motion::new();
        let key = MotionKey::unique();
        let cache = TextureCache::new();
        let curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(200)));
        let build = |target| -> crate::Element<'_, ()> {
            let opacity = motion.to(key, curve, target);
            motion
                .host(cached(
                    cache.clone(),
                    radial_gradient(fill())
                        .opacity(opacity)
                        .width(40)
                        .height(40),
                ))
                .into()
        };
        let mut harness = Harness::new(Size::new(40.0, 40.0), build(0.0_f32));
        let start = Instant::now();
        harness.frame(start);
        harness.frame(start);
        let before = cache.record_count();
        let mut harness = harness.rebuild(build(1.0));
        for frame in 1..=10 {
            harness.frame(start + Duration::from_millis(frame * 16));
        }
        assert!(
            cache.record_count() > before + 1,
            "cached pixels follow the animation"
        );
        for frame in 11..=250 {
            harness.frame(start + Duration::from_millis(frame * 16));
        }
        assert!(
            harness.screenshot(1.0).pixel(20, 20)[1] < 20,
            "final pose recorded"
        );
        let settled = cache.record_count();
        for frame in 251..=255 {
            harness.frame(start + Duration::from_millis(frame * 16));
        }
        assert_eq!(
            cache.record_count(),
            settled,
            "idle paint does not keep recording"
        );
    }

    #[test]
    fn asymmetric_corners_can_extend_beyond_the_midpoint() {
        let solid = Radial::new().add_stop(0.0, Color::BLACK);
        let corners = border::Radius {
            top_left: 40.0,
            ..Default::default()
        };
        let mut harness: Harness<'_, ()> = Harness::new(
            Size::new(40.0, 40.0),
            radial_gradient(solid).border_radius(corners),
        );
        harness.frame(Instant::now());
        let shot = harness.screenshot(1.0);
        assert_eq!(shot.pixel(0, 25), [255; 4]);
        assert_eq!(shot.pixel(30, 30), [0, 0, 0, 255]);
    }

    #[test]
    fn fading_and_moving_reuse_the_software_raster() {
        let mut cache = RadialCache::default();
        let bounds = Rectangle::with_size(Size::new(20.0, 30.0));
        let first = cache
            .image(bounds, fill(), border::Radius::default(), 1.0)
            .unwrap();
        let moved = Rectangle {
            x: 42.0,
            y: 19.0,
            ..bounds
        };
        let second = cache
            .image(moved, fill(), border::Radius::default(), 1.0)
            .unwrap();
        assert_eq!(first.id(), second.id());
        let dense = cache
            .image(bounds, fill(), border::Radius::default(), 2.0)
            .unwrap();
        assert_ne!(first.id(), dense.id());
    }
}

#[cfg(feature = "wgpu")]
#[test]
#[ignore = "requires a wgpu adapter"]
fn gpu_keeps_multiple_instances_clips_and_cached_layers_independent() {
    use crate::{
        Composite, FilterQuality, RadialRenderer, Renderer, TextureCache, TextureRenderer,
    };
    use iced_core::renderer::Headless;
    use iced_core::{Font, Pixels, Renderer as _};
    let mut renderer = iced_test::futures::futures::executor::block_on(Renderer::new(
        Font::DEFAULT,
        Pixels(16.0),
        Some("wgpu"),
    ))
    .expect("wgpu adapter");
    let size = Size::new(160, 80);
    let red = Radial::new()
        .add_stop(0.0, Color::from_rgb(1.0, 0.0, 0.0))
        .add_stop(1.0, Color::TRANSPARENT);
    let blue = Radial::new()
        .add_stop(0.0, Color::from_rgb(0.0, 0.0, 1.0))
        .add_stop(1.0, Color::TRANSPARENT);
    let paint = |renderer: &mut Renderer| {
        renderer.reset(Rectangle::with_size(Size::new(160.0, 80.0)));
        let mut cache = RadialCache::default();
        // The second gradient is clipped to its left half; clipping must not
        // shift its centre or stretch it over the remaining visible part.
        renderer.draw_radial(
            Rectangle::new(Point::ORIGIN, Size::new(80.0, 80.0)),
            red,
            20.0.into(),
            1.0,
            &mut cache,
        );
        renderer.with_layer(
            Rectangle::new(Point::new(80.0, 0.0), Size::new(40.0, 80.0)),
            |renderer| {
                renderer.draw_radial(
                    Rectangle::new(Point::new(80.0, 0.0), Size::new(80.0, 80.0)),
                    blue,
                    20.0.into(),
                    1.0,
                    &mut cache,
                );
            },
        );
    };
    paint(&mut renderer);
    let shot = renderer.screenshot(size, 1.0, Color::WHITE);
    let pixel = |shot: &[u8], x: usize, y: usize| -> [u8; 4] {
        shot[(y * 160 + x) * 4..(y * 160 + x) * 4 + 4]
            .try_into()
            .unwrap()
    };
    assert!(pixel(&shot, 40, 40)[1] < 15, "red centre");
    assert!(
        pixel(&shot, 119, 40)[0] < 20,
        "blue centre is in its original coordinates"
    );
    assert_eq!(pixel(&shot, 125, 40), [255; 4], "ancestor clip");
    assert_eq!(pixel(&shot, 0, 0), [255; 4], "rounded corner");
    // A second present reuses slots without mixing the two gradients.
    paint(&mut renderer);
    assert_eq!(renderer.screenshot(size, 1.0, Color::WHITE), shot);
    let source = TextureCache::new();
    renderer.reset(Rectangle::with_size(Size::new(160.0, 80.0)));
    let _ = renderer.record(&source, size, 1.0, paint);
    let bounds = Rectangle::with_size(Size::new(160.0, 80.0));
    renderer.draw_cached(
        &source,
        bounds,
        bounds,
        bounds,
        iced_core::Transformation::IDENTITY,
        Composite::plain(FilterQuality::Bilinear),
    );
    let recorded = renderer.screenshot(size, 1.0, Color::WHITE);
    for (a, b) in shot.iter().zip(&recorded) {
        assert!(a.abs_diff(*b) <= 2);
    }
    assert_gpu_transform(&mut renderer, red);
}

#[test]
fn invalid_paint_is_stable_including_nan() {
    let widget = radial_gradient(Radial::new().center([f32::NAN, 0.0])).opacity(f32::NAN);
    assert_eq!(widget.resolved(), widget.resolved());
}

#[cfg(feature = "wgpu")]
fn assert_gpu_transform(renderer: &mut crate::Renderer, red: Radial) {
    use crate::RadialRenderer;
    use iced_core::{Renderer as _, renderer::Headless};
    let bounds = Rectangle::with_size(Size::new(160.0, 80.0));
    renderer.reset(bounds);
    let transform =
        iced_core::Transformation::translate(80.0, 0.0) * iced_core::Transformation::scale(0.5);
    renderer.with_transformation(transform, |renderer| {
        renderer.draw_radial(
            Rectangle::with_size(Size::new(80.0, 80.0)),
            red,
            40.0.into(),
            1.0,
            &mut RadialCache::default(),
        );
    });
    let dense = renderer.screenshot(Size::new(320, 160), 2.0, Color::WHITE);
    let at = |x: usize, y: usize| &dense[(y * 320 + x) * 4..(y * 320 + x) * 4 + 4];
    assert!(
        at(200, 40)[1] < 20,
        "centre follows the inherited transform at 2x DPI"
    );
    assert_eq!(
        at(160, 0),
        [255; 4],
        "corner coverage scales with the inherited transform"
    );
    assert_eq!(
        at(245, 40),
        [255; 4],
        "the quad occupies its transformed bounds"
    );
}
