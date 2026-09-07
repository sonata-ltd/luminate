//! Pure geometry shared by [`Cached`](crate::Cached) and
//! [`Pager`](crate::Pager): device-grid snapping, the composited transform,
//! cursor mapping and the per-frame record decisions. Nothing here touches a
//! renderer, so every rule has a unit test below.

use iced_core::{Point, Rectangle, Size, Transformation, Vector, mouse};

use crate::cached::PixelSnap;
use crate::filter::FilterQuality;

/// Scales closer to 1 than this are treated as exactly 1; scales closer to
/// 0 than this are treated as 0 (no inverse). One constant so the
/// "translation only" and "cursor mapping" decisions cannot disagree.
pub(crate) const SCALE_EPS: f32 = 1e-4;

/// Rounds a logical coordinate onto the device-pixel grid of `scale`.
pub(crate) fn snap_to_grid(value: f32, scale: f32) -> f32 {
    (value * scale).round() / scale
}

/// Linear interpolation: `from` at `t = 0`, `to` at `t = 1`.
pub(crate) fn lerp(from: f32, to: f32, t: f32) -> f32 {
    from + (to - from) * t
}

/// Composes a translation and a scale about the centre of `bounds`. A raw
/// `Transformation::scale` is relative to the window origin and would slide
/// the image toward the top-left corner.
pub(crate) fn effective_transform(
    offset: Vector,
    factor: f32,
    bounds: Rectangle,
) -> Transformation {
    let mut transform = Transformation::IDENTITY;

    if offset != Vector::ZERO {
        transform = transform * Transformation::translate(offset.x, offset.y);
    }

    if (factor - 1.0).abs() < SCALE_EPS {
        return transform;
    }

    let centre_x = bounds.x + bounds.width * 0.5;
    let centre_y = bounds.y + bounds.height * 0.5;

    // Right-most applies first: shift the centre to the origin, scale, shift back.
    transform
        * Transformation::translate(centre_x, centre_y)
        * Transformation::scale(factor)
        * Transformation::translate(-centre_x, -centre_y)
}

/// `true` if `transform` is a pure 2D translation (identity linear part):
/// the case in which snapping the origin to the device grid is lossless.
pub(crate) fn is_translation_only(transform: &Transformation) -> bool {
    let matrix: &[f32; 16] = transform.as_ref();
    (matrix[0] - 1.0).abs() < SCALE_EPS
        && (matrix[5] - 1.0).abs() < SCALE_EPS
        && matrix[1].abs() < SCALE_EPS
        && matrix[4].abs() < SCALE_EPS
}

/// Adjusts `user_transform` so that `origin` (the texture's top-left in
/// layout space) lands on an integer device pixel, keeping the transform's
/// scale. The whole texel grid then sits on the device grid and the resting
/// frame samples at integer phase.
pub(crate) fn snap_transform(
    user_transform: Transformation,
    origin: Point,
    scale: f32,
) -> Transformation {
    let factor = user_transform.scale_factor();
    let translation = user_transform.translation();
    let device_x = (factor * origin.x + translation.x) * scale;
    let device_y = (factor * origin.y + translation.y) * scale;
    let snapped_x = device_x.round() / scale - factor * origin.x;
    let snapped_y = device_y.round() / scale - factor * origin.y;
    Transformation::translate(snapped_x, snapped_y) * Transformation::scale(factor)
}

/// Maps a cursor from the enclosing layout space into the content's
/// untransformed space. A scale too close to zero has no inverse: the
/// cursor becomes [`mouse::Cursor::Unavailable`] rather than NaN.
pub(crate) fn translate_cursor(cursor: mouse::Cursor, transform: Transformation) -> mouse::Cursor {
    let translation = transform.translation();
    let scale = transform.scale_factor();

    if scale.abs() < SCALE_EPS {
        return mouse::Cursor::Unavailable;
    }

    let map_point =
        |p: Point| Point::new((p.x - translation.x) / scale, (p.y - translation.y) / scale);

    match cursor {
        mouse::Cursor::Available(p) => mouse::Cursor::Available(map_point(p)),
        mouse::Cursor::Levitating(p) => mouse::Cursor::Levitating(map_point(p)),
        mouse::Cursor::Unavailable => mouse::Cursor::Unavailable,
    }
}

/// Where and how large a texture is recorded and composited.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CompositeGeometry {
    /// Texture size in physical pixels.
    pub physical: Size<u32>,
    /// Scale used while recording (`device_scale * supersample`).
    pub texture_scale: f32,
    /// Where the (padded) texture is composited, in layout coordinates,
    /// before the user transform. Its origin is also the point the record
    /// translates by, so texture and viewport always agree.
    pub cache_bounds: Rectangle,
}

/// Sizes the texture for `bounds` padded by `bleed` logical pixels on every
/// side (so bilinear filtering at the edges does not clip anti-aliased
/// pixels).
///
/// `layout_anchor` is the part of `bounds`' origin that nothing is animating —
/// where the layout would put this texture if every track were at rest.
/// [`PixelSnap::LayoutOnly`] passes one; the anchor is snapped to the device
/// grid and however far `bounds` has since travelled from it is carried on top
/// at whatever sub-pixel phase the frame asks for. That is the whole of the
/// tier: standing still is crisp, and being carried by an animating ancestor
/// is smooth, without a frame in which the two disagree. `None` leaves the
/// origin exactly where the layout put it.
pub(crate) fn composite_geometry(
    bleed: u32,
    bounds: Rectangle,
    scale: f32,
    supersample: f32,
    layout_anchor: Option<Point>,
) -> CompositeGeometry {
    // A renderer that reports no usable scale (zero, negative, NaN) records
    // at 1:1 rather than producing an empty or infinite texture.
    let scale = if scale > 0.0 && scale.is_finite() {
        scale
    } else {
        1.0
    };

    let (origin_x, origin_y) = match layout_anchor {
        Some(anchor) => (
            snap_to_grid(anchor.x, scale) + (bounds.x - anchor.x),
            snap_to_grid(anchor.y, scale) + (bounds.y - anchor.y),
        ),
        None => (bounds.x, bounds.y),
    };

    let padded = Size::new(
        bounds.width.ceil().max(1.0) + 2.0 * bleed as f32,
        bounds.height.ceil().max(1.0) + 2.0 * bleed as f32,
    );
    let texture_scale = scale * supersample;
    let physical = Size::new(
        (padded.width * texture_scale).round().max(1.0) as u32,
        (padded.height * texture_scale).round().max(1.0) as u32,
    );
    let cache_bounds = Rectangle {
        x: origin_x - bleed as f32,
        y: origin_y - bleed as f32,
        width: physical.width as f32 / texture_scale,
        height: physical.height as f32 / texture_scale,
    };

    CompositeGeometry {
        physical,
        texture_scale,
        cache_bounds,
    }
}

/// Whether the composited texel grid is snapped to the device grid this
/// frame.
///
/// [`FilterQuality::Snap`] wins over every [`PixelSnap`] policy: that tier is
/// defined as "no reconstruction", and the only way to have nothing to
/// reconstruct is to sit on the grid.
pub(crate) fn snap_decision(
    filter: FilterQuality,
    mode: PixelSnap,
    has_live_scale: bool,
    translation_only: bool,
    at_rest: bool,
) -> bool {
    if filter.snaps() {
        return true;
    }

    match mode {
        PixelSnap::Always => true,
        PixelSnap::Never | PixelSnap::LayoutOnly => false,
        PixelSnap::Auto => !has_live_scale && translation_only && at_rest,
    }
}

/// Where a [`Pager`](crate::Pager) page's texture is composited, with `mode`
/// applied to its origin.
///
/// A slide is horizontal: `page.x` carries the motion and must stay
/// fractional, or the page steps by whole device pixels. `page.y` moves only
/// because the pager interpolates its height between the two pages and
/// centres each one in the result — a side effect of the transition, not the
/// motion the eye follows.
///
/// Snapping `y` buys a great deal: with one axis at integer phase the
/// composite shader collapses its separable kernel from 9 taps to 3 and stops
/// resampling the page vertically, which is the direction text can least
/// afford to lose. `tests/sharpness_ab.rs` measures what giving that up costs
/// — the edge spread of a line of text roughly doubles for the length of the
/// slide. It is not free, though: the centring offset creeps by a fraction of
/// a device pixel per frame near the end of a slide, and rounding it turns
/// that creep into a staircase of whole-pixel steps. Sharp and stepping is the
/// trade this makes; [`PixelSnap::Never`] is the other side of it.
///
/// The pager's own origin is the *discrete* part of a page's position — it
/// jumps when the surrounding layout reshuffles — and the page's offset
/// inside the pager is the *smooth* part. Splitting those is what
/// [`PixelSnap::LayoutOnly`] adds on the horizontal axis.
///
/// Only sliding frames go through this: a resting page is drawn directly on
/// the device grid, with no texture and so nothing to resample.
pub(crate) fn pager_page_bounds(
    filter: FilterQuality,
    mode: PixelSnap,
    page: Rectangle,
    pager: Rectangle,
    scale: f32,
) -> Rectangle {
    let snapped = |value: f32| snap_to_grid(value, scale);

    // `FilterQuality::Snap` overrides the policy, exactly as it does for
    // `Cached`: that tier is defined as having nothing to reconstruct.
    if filter.snaps() || mode == PixelSnap::Always {
        return Rectangle {
            x: snapped(page.x),
            y: snapped(page.y),
            ..page
        };
    }

    // The escape hatch: resample both axes and keep the page exactly where
    // the layout put it.
    if mode == PixelSnap::Never {
        return page;
    }

    // Both tiers snap the pager's own origin vertically and let the centring
    // offset ride on top of it fractionally. They differ on the horizontal
    // axis: `LayoutOnly` gives the slide the same treatment, so the resampling
    // phase varies only with the slide and the blur level cannot "breathe"
    // when the layout shifts mid-slide, where `Auto` leaves the layout
    // untouched.
    let x = if mode == PixelSnap::LayoutOnly {
        snapped(pager.x) + (page.x - pager.x)
    } else {
        page.x
    };

    Rectangle {
        x,
        y: snapped(page.y),
        ..page
    }
}

/// The supersample factor to record at this frame.
pub(crate) fn record_supersample(base: f32, supersample_in_motion: bool, at_rest: bool) -> f32 {
    if supersample_in_motion && !at_rest {
        base.max(1.5)
    } else {
        base
    }
}

/// The rectangle (in the enclosing layout space) a composite may paint: its
/// transformed bounds intersected with the parent's clip. `None` when
/// nothing is visible.
pub(crate) fn composite_clip(
    cache_bounds: Rectangle,
    transform: Transformation,
    viewport: &Rectangle,
) -> Option<Rectangle> {
    (cache_bounds * transform).intersection(viewport)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Rectangle {
        Rectangle {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn snap_to_grid_rounds_in_device_pixels() {
        assert_eq!(snap_to_grid(10.3, 2.0), 10.5);
        assert_eq!(snap_to_grid(20.7, 2.0), 20.5);
        assert_eq!(snap_to_grid(3.49, 1.0), 3.0);
        assert_eq!(snap_to_grid(-0.6, 1.0), -1.0);
    }

    #[test]
    fn lerp_hits_both_ends_and_the_middle() {
        assert_eq!(lerp(100.0, 200.0, 0.0), 100.0);
        assert_eq!(lerp(100.0, 200.0, 0.5), 150.0);
        assert_eq!(lerp(100.0, 200.0, 1.0), 200.0);
    }

    #[test]
    fn geometry_pads_by_bleed_and_scales_to_physical() {
        let geometry = composite_geometry(2, rect(5.0, 7.0, 10.0, 20.0), 2.0, 1.0, None);
        assert_eq!(geometry.physical, Size::new(28, 48));
        assert_eq!(geometry.texture_scale, 2.0);
        assert_eq!(geometry.cache_bounds, rect(3.0, 5.0, 14.0, 24.0));
    }

    #[test]
    fn geometry_applies_supersample_to_texture_only() {
        let geometry = composite_geometry(2, rect(0.0, 0.0, 10.0, 10.0), 1.0, 2.0, None);
        assert_eq!(geometry.physical, Size::new(28, 28));
        assert_eq!(geometry.texture_scale, 2.0);
        assert_eq!(geometry.cache_bounds.width, 14.0);
    }

    #[test]
    fn geometry_never_produces_zero_texture() {
        let geometry = composite_geometry(0, rect(0.0, 0.0, 0.0, 0.0), 1.0, 1.0, None);
        assert_eq!(geometry.physical, Size::new(1, 1));
    }

    #[test]
    fn an_unusable_scale_falls_back_to_one() {
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            let geometry = composite_geometry(
                0,
                rect(0.0, 0.0, 10.0, 10.0),
                scale,
                1.0,
                Some(Point::ORIGIN),
            );
            assert_eq!(geometry.texture_scale, 1.0, "scale {scale}");
            assert_eq!(geometry.physical, Size::new(10, 10), "scale {scale}");
        }
    }

    #[test]
    fn degenerate_bounds_still_yield_a_texture() {
        for bounds in [
            rect(-5.0, -5.0, -10.0, -10.0),
            rect(f32::NAN, 0.0, f32::NAN, 4.0),
        ] {
            let geometry = composite_geometry(2, bounds, 1.0, 1.0, None);
            assert!(
                geometry.physical.width >= 1 && geometry.physical.height >= 1,
                "{bounds:?}"
            );
        }
    }

    #[test]
    fn layout_only_snaps_only_the_bounds_origin() {
        let bounds = rect(10.3, 20.7, 10.0, 10.0);
        let snapped = composite_geometry(0, bounds, 2.0, 1.0, Some(bounds.position()));
        // 10.3 * 2 = 20.6 -> 21 -> 10.5 ; 20.7 * 2 = 41.4 -> 41 -> 20.5
        assert_eq!(
            (snapped.cache_bounds.x, snapped.cache_bounds.y),
            (10.5, 20.5)
        );
        let plain = composite_geometry(0, bounds, 2.0, 1.0, None);
        assert_eq!((plain.cache_bounds.x, plain.cache_bounds.y), (10.3, 20.7));
    }

    #[test]
    fn an_anchored_origin_snaps_the_anchor_and_carries_the_travel() {
        // The card-header case: an ancestor re-centres this widget a fifth of
        // a device pixel per frame. Rounding the live origin turns that into a
        // staircase of whole-pixel jumps; anchoring it to the last resting
        // position keeps the travel intact and still lands on the grid the
        // moment the travel is zero.
        let anchor = Point::new(10.3, 20.7);
        let at_rest = composite_geometry(0, rect(10.3, 20.7, 10.0, 10.0), 2.0, 1.0, Some(anchor));
        // 10.3 * 2 = 20.6 -> 21 -> 10.5 ; 20.7 * 2 = 41.4 -> 41 -> 20.5
        assert_eq!(
            (at_rest.cache_bounds.x, at_rest.cache_bounds.y),
            (10.5, 20.5),
            "a resting widget is snapped outright"
        );

        let creep = 0.1;
        let travelled = composite_geometry(
            0,
            rect(10.3 + creep, 20.7 + creep, 10.0, 10.0),
            2.0,
            1.0,
            Some(anchor),
        );
        assert!((travelled.cache_bounds.x - (10.5 + creep)).abs() < 1e-5);
        assert!((travelled.cache_bounds.y - (20.5 + creep)).abs() < 1e-5);
        // The point of the whole exercise: sub-pixel travel survives.
        assert!(
            (travelled.cache_bounds.x - at_rest.cache_bounds.x - creep).abs() < 1e-5,
            "the travel was rounded away"
        );
    }

    #[test]
    fn scaling_about_a_centre_leaves_that_centre_fixed() {
        let bounds = rect(10.0, 20.0, 40.0, 20.0);
        let centre = Point::new(30.0, 30.0);
        let transform = effective_transform(Vector::ZERO, 2.0, bounds);
        let moved = centre * transform;
        assert!((moved.x - centre.x).abs() < 1e-4 && (moved.y - centre.y).abs() < 1e-4);
        let corner = Point::new(bounds.x, bounds.y) * transform;
        assert!((corner.x - (-10.0)).abs() < 1e-4 && (corner.y - 10.0).abs() < 1e-4);
    }

    #[test]
    fn a_scale_within_epsilon_of_one_is_identity() {
        let transform = effective_transform(
            Vector::ZERO,
            1.0 + SCALE_EPS / 2.0,
            rect(0.0, 0.0, 10.0, 10.0),
        );
        assert!(is_translation_only(&transform));
    }

    #[test]
    fn cursor_maps_through_the_inverse_transform() {
        let transform = Transformation::translate(10.0, 5.0) * Transformation::scale(2.0);
        let world = Point::new(30.0, 25.0);
        let mouse::Cursor::Available(p) =
            translate_cursor(mouse::Cursor::Available(world), transform)
        else {
            panic!("cursor kind changed")
        };
        let back = p * transform;
        assert!((back.x - world.x).abs() < 1e-4 && (back.y - world.y).abs() < 1e-4);
    }

    #[test]
    fn a_levitating_cursor_is_mapped_like_an_available_one() {
        let transform = Transformation::translate(10.0, 5.0);
        let mouse::Cursor::Levitating(p) =
            translate_cursor(mouse::Cursor::Levitating(Point::new(30.0, 25.0)), transform)
        else {
            panic!("cursor kind changed")
        };
        assert_eq!(p, Point::new(20.0, 20.0));
    }

    #[test]
    fn a_near_zero_scale_makes_the_cursor_unavailable() {
        for scale in [0.0, SCALE_EPS / 10.0, -SCALE_EPS / 10.0] {
            let transform = Transformation::scale(scale);
            assert!(
                matches!(
                    translate_cursor(mouse::Cursor::Available(Point::new(3.0, 4.0)), transform),
                    mouse::Cursor::Unavailable
                ),
                "scale {scale}"
            );
        }
    }

    #[test]
    fn snapping_lands_the_texture_origin_on_a_device_pixel() {
        let origin = Point::new(8.3, 18.7);
        let scale = 2.0;
        let user = Transformation::translate(0.37, -0.21);
        let snapped = snap_transform(user, origin, scale);
        let moved = origin * snapped;
        let device = (moved.x * scale, moved.y * scale);
        assert!(
            (device.0 - device.0.round()).abs() < 1e-3,
            "x on the grid: {device:?}"
        );
        assert!(
            (device.1 - device.1.round()).abs() < 1e-3,
            "y on the grid: {device:?}"
        );
        // …and the snap moved it by at most half a device pixel.
        let unsnapped = origin * user;
        assert!(((unsnapped.x - moved.x) * scale).abs() <= 0.5 + 1e-3);
        assert!(((unsnapped.y - moved.y) * scale).abs() <= 0.5 + 1e-3);
    }

    #[test]
    fn snapping_keeps_the_scale_and_lands_on_the_grid_when_scaled() {
        let origin = Point::new(1.3, 2.4);
        let user = Transformation::translate(1.1, 2.2) * Transformation::scale(2.0);
        let snapped = snap_transform(user, origin, 2.0);
        assert!((snapped.scale_factor() - 2.0).abs() < 1e-6);
        let moved = origin * snapped;
        assert!(((moved.x * 2.0) - (moved.x * 2.0).round()).abs() < 1e-3);
        assert!(((moved.y * 2.0) - (moved.y * 2.0).round()).abs() < 1e-3);
    }

    #[test]
    fn only_pure_translations_count_as_snappable() {
        assert!(is_translation_only(&Transformation::translate(3.0, 4.0)));
        assert!(is_translation_only(&Transformation::IDENTITY));
        assert!(!is_translation_only(&Transformation::scale(1.5)));
        assert!(!is_translation_only(
            &(Transformation::translate(1.0, 1.0) * Transformation::scale(1.5))
        ));
    }

    #[test]
    fn snap_policy_truth_table() {
        let filter = FilterQuality::CatmullRom;

        for has_live_scale in [false, true] {
            for translation_only in [false, true] {
                for at_rest in [false, true] {
                    assert!(snap_decision(
                        filter,
                        PixelSnap::Always,
                        has_live_scale,
                        translation_only,
                        at_rest
                    ));
                    assert!(!snap_decision(
                        filter,
                        PixelSnap::Never,
                        has_live_scale,
                        translation_only,
                        at_rest
                    ));
                    assert!(!snap_decision(
                        filter,
                        PixelSnap::LayoutOnly,
                        has_live_scale,
                        translation_only,
                        at_rest
                    ));
                    assert_eq!(
                        snap_decision(
                            filter,
                            PixelSnap::Auto,
                            has_live_scale,
                            translation_only,
                            at_rest
                        ),
                        !has_live_scale && translation_only && at_rest
                    );
                }
            }
        }
    }

    #[test]
    fn the_snap_tier_overrides_every_pixel_snap_policy() {
        for mode in [
            PixelSnap::Auto,
            PixelSnap::Always,
            PixelSnap::Never,
            PixelSnap::LayoutOnly,
        ] {
            for has_live_scale in [false, true] {
                for translation_only in [false, true] {
                    for at_rest in [false, true] {
                        assert!(snap_decision(
                            FilterQuality::Snap,
                            mode,
                            has_live_scale,
                            translation_only,
                            at_rest
                        ));
                    }
                }
            }
        }
    }

    #[test]
    fn the_filtering_tiers_leave_the_snap_policy_alone() {
        // `Bilinear` reconstructs in the shader, so it must not smuggle in a
        // snap the way `Snap` does.
        assert!(!snap_decision(
            FilterQuality::Bilinear,
            PixelSnap::Never,
            false,
            true,
            true
        ));
        assert!(snap_decision(
            FilterQuality::Bilinear,
            PixelSnap::Auto,
            false,
            true,
            true
        ));
    }

    /// A pager at a fractional origin, its page slid horizontally and centred
    /// vertically by fractional amounts inside it. Every component is off the
    /// device grid at scale 2 — including both `y` values — so a policy that
    /// snaps one axis, or one part of an axis, and not another is visible in
    /// the result.
    fn pager_and_page() -> (Rectangle, Rectangle) {
        let pager = Rectangle::new(Point::new(10.3, 4.1), Size::new(100.0, 50.0));
        let page = Rectangle::new(Point::new(10.3 - 21.7, 4.1 + 0.37), Size::new(100.0, 50.0));
        (pager, page)
    }

    #[test]
    fn the_fixture_sits_off_the_device_grid_on_both_axes() {
        // Guards every assertion below: on-grid values would make "snapped"
        // and "left alone" indistinguishable.
        let (pager, page) = pager_and_page();

        for value in [pager.x, pager.y, page.x, page.y] {
            assert_ne!(
                snap_to_grid(value, 2.0),
                value,
                "{value} is already on the grid"
            );
        }
    }

    #[test]
    fn always_snaps_the_whole_page_origin() {
        let (pager, page) = pager_and_page();
        let snapped = pager_page_bounds(
            FilterQuality::CatmullRom,
            PixelSnap::Always,
            page,
            pager,
            2.0,
        );

        assert_eq!(snapped.x, snap_to_grid(page.x, 2.0));
        assert_eq!(snapped.y, snap_to_grid(page.y, 2.0));
        assert_eq!(snapped.size(), page.size());
    }

    #[test]
    fn the_snap_tier_snaps_a_page_whatever_the_policy_says() {
        let (pager, page) = pager_and_page();

        for mode in [
            PixelSnap::Auto,
            PixelSnap::Always,
            PixelSnap::Never,
            PixelSnap::LayoutOnly,
        ] {
            let snapped = pager_page_bounds(FilterQuality::Snap, mode, page, pager, 2.0);
            assert_eq!(snapped.x, snap_to_grid(page.x, 2.0), "{mode:?}");
            assert_eq!(snapped.y, snap_to_grid(page.y, 2.0), "{mode:?}");
        }
    }

    #[test]
    fn layout_only_keeps_the_slide_fractional_but_still_snaps_the_vertical() {
        let (pager, page) = pager_and_page();
        let snapped = pager_page_bounds(
            FilterQuality::CatmullRom,
            PixelSnap::LayoutOnly,
            page,
            pager,
            2.0,
        );

        // Horizontally the page's offset inside the pager survives untouched,
        // so the slide still glides...
        assert!((snapped.x - snap_to_grid(pager.x, 2.0) - (page.x - pager.x)).abs() < 1e-6);
        // ...and the page's own origin stays off the grid, unlike `Always`.
        assert_ne!(snapped.x, snap_to_grid(page.x, 2.0));

        // Vertically it is snapped outright: that axis carries no motion of
        // its own, and integer phase there is what keeps text crisp.
        assert_eq!(snapped.y, snap_to_grid(page.y, 2.0));

        // Moving the pager by a sub-pixel amount must not change the
        // horizontal phase: that is what this tier is for.
        let nudged = pager_page_bounds(
            FilterQuality::CatmullRom,
            PixelSnap::LayoutOnly,
            Rectangle {
                x: page.x + 0.2,
                ..page
            },
            Rectangle {
                x: pager.x + 0.2,
                ..pager
            },
            2.0,
        );
        assert!(
            (nudged.x - snapped.x).abs() < 1e-6,
            "a sub-pixel layout shift changed the resampling phase"
        );
    }

    #[test]
    fn auto_snaps_only_the_axis_the_slide_does_not_move() {
        let (pager, page) = pager_and_page();
        let placed =
            pager_page_bounds(FilterQuality::CatmullRom, PixelSnap::Auto, page, pager, 2.0);

        // The slide is horizontal, so x must stay exactly where the layout
        // put it or the page steps by whole device pixels.
        assert_eq!(placed.x, page.x);
        // y moves only because the pager interpolates its height, so snapping
        // it keeps the text crisp and collapses the shader kernel to three taps.
        assert_eq!(placed.y, snap_to_grid(page.y, 2.0));
        assert_ne!(placed.y, page.y, "the fixture's y must be off the grid");
    }

    #[test]
    fn never_leaves_both_axes_alone() {
        let (pager, page) = pager_and_page();

        assert_eq!(
            pager_page_bounds(
                FilterQuality::CatmullRom,
                PixelSnap::Never,
                page,
                pager,
                2.0
            ),
            page
        );
    }

    #[test]
    fn every_snapping_policy_puts_the_vertical_axis_on_the_device_grid() {
        // The property the sharpness of a slide depends on: one axis at
        // integer phase. Only the explicit opt-out gives it up.
        let (pager, page) = pager_and_page();

        for mode in [PixelSnap::Auto, PixelSnap::Always, PixelSnap::LayoutOnly] {
            let placed = pager_page_bounds(FilterQuality::CatmullRom, mode, page, pager, 2.0);
            let device_y = placed.y * 2.0;
            assert!(
                (device_y - device_y.round()).abs() < 1e-4,
                "{mode:?} left y at a fractional device phase: {device_y}"
            );
        }
    }

    #[test]
    fn a_placed_page_shows_its_content_exactly_where_the_policy_says() {
        // Ties the three rules `Pager::draw` composes together: the policy
        // places the origin, `composite_geometry` insets it by `BLEED`, and
        // the record puts the content `BLEED` back in. The snap must displace
        // the *image*; if it leaked into the recorded content instead, the
        // texture (recorded once, reused all slide) would carry a stale
        // sub-pixel phase.
        const BLEED: u32 = 2;
        let (pager, page) = pager_and_page();
        let scale = 2.0;

        for (mode, expected) in [
            (PixelSnap::Always, snap_to_grid(page.x, scale)),
            (PixelSnap::Never, page.x),
            (
                PixelSnap::LayoutOnly,
                snap_to_grid(pager.x, scale) + (page.x - pager.x),
            ),
        ] {
            let placed = pager_page_bounds(FilterQuality::CatmullRom, mode, page, pager, scale);
            let composite = composite_geometry(BLEED, placed, scale, 1.0, None);
            let on_screen = composite.cache_bounds.x + BLEED as f32;

            assert!(
                (on_screen - expected).abs() < 1e-6,
                "{mode:?}: content at {on_screen}, expected {expected}"
            );
        }
    }

    #[test]
    fn supersample_in_motion_only_raises_the_factor_in_motion() {
        assert_eq!(record_supersample(1.0, false, false), 1.0);
        assert_eq!(record_supersample(1.0, true, true), 1.0);
        assert_eq!(record_supersample(1.0, true, false), 1.5);
        assert_eq!(record_supersample(2.0, true, false), 2.0);
    }

    #[test]
    fn the_composite_is_clipped_to_the_viewport() {
        let cache_bounds = rect(0.0, 0.0, 100.0, 100.0);
        let viewport = rect(0.0, 0.0, 200.0, 50.0);
        let clip = composite_clip(
            cache_bounds,
            Transformation::translate(30.0, 0.0),
            &viewport,
        )
        .expect("overlaps");
        assert_eq!(clip, rect(30.0, 0.0, 100.0, 50.0));
        assert!(
            composite_clip(
                cache_bounds,
                Transformation::translate(0.0, 60.0),
                &viewport
            )
            .is_none()
        );
    }
}
