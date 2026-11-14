//! Opt-in alpha fades, without imposing `TextureRenderer` on other scrollables.
use iced::{Padding, Rectangle, Size, Transformation, Vector};

use iced_texture_cache::{Composite, Record, TextureCache, TextureRenderer};

use super::Axis;

/// Record content once. With fades on both axes, a second viewport-sized cache
/// applies the vertical mask before the horizontal pass. This keeps composition
/// linear in the edge lengths instead of building a pixel grid at the corners.
#[allow(clippy::too_many_arguments)]
fn draw<Renderer: TextureRenderer>(
    renderer: &mut Renderer,
    source: &TextureCache,
    intermediate: &TextureCache,
    bounds: Rectangle,
    viewport: &Rectangle,
    edges: Padding,
    opacity: f32,
    content: &mut dyn FnMut(&mut Renderer),
) {
    let Some(visible) = bounds.intersection(viewport) else {
        return;
    };
    if opacity <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }
    let vertical = edges.top > 0.0 || edges.bottom > 0.0;
    let horizontal = edges.left > 0.0 || edges.right > 0.0;
    if !vertical && !horizontal && opacity >= 1.0 {
        renderer.with_layer(visible, &mut *content);
        return;
    }

    let scale = renderer.scale_factor();
    let x = (bounds.x * scale).floor();
    let y = (bounds.y * scale).floor();
    let width = ((bounds.x + bounds.width) * scale).ceil() - x;
    let height = ((bounds.y + bounds.height) * scale).ceil() - y;
    let size = Size::new(width as u32, height as u32);
    let recorded = Rectangle::new(
        iced::Point::new(x / scale, y / scale),
        Size::new(width / scale, height / scale),
    );
    let result = renderer.record(source, size, scale, |renderer| {
        renderer.with_translation(Vector::new(-recorded.x, -recorded.y), |renderer| {
            renderer.with_layer(visible, &mut *content);
        });
    });
    // Preserve content when a viewport exceeds the renderer's texture limits.
    if matches!(result, Record::Uncacheable) {
        renderer.with_layer(visible, &mut *content);
        return;
    }

    if vertical && horizontal {
        let result = renderer.record(intermediate, size, scale, |renderer| {
            renderer.with_translation(Vector::new(-recorded.x, -recorded.y), |renderer| {
                composite_axis(
                    renderer,
                    source,
                    recorded,
                    bounds,
                    &visible,
                    edges,
                    Axis::Vertical,
                    1.0,
                );
            });
        });
        if !matches!(result, Record::Uncacheable) {
            composite_axis(
                renderer,
                intermediate,
                recorded,
                bounds,
                &visible,
                edges,
                Axis::Horizontal,
                opacity,
            );
            return;
        }
        // A second texture is optional: preserve the source and vertical mask.
    }

    let axis = if horizontal && !vertical {
        Axis::Horizontal
    } else {
        Axis::Vertical
    };
    composite_axis(
        renderer, source, recorded, bounds, &visible, edges, axis, opacity,
    );
}

/// Composite pixel-wide bands with shared physical boundaries and one opaque
/// middle band. Corners multiply alpha because each axis is applied once.
#[allow(clippy::too_many_arguments)]
fn composite_axis<Renderer: TextureRenderer>(
    renderer: &mut Renderer,
    cache: &TextureCache,
    recorded: Rectangle,
    bounds: Rectangle,
    visible: &Rectangle,
    edges: Padding,
    axis: Axis,
    opacity: f32,
) {
    let scale = renderer.scale_factor();
    let (origin, length, near, far) = match axis {
        Axis::Vertical => (bounds.y, bounds.height, edges.top, edges.bottom),
        Axis::Horizontal => (bounds.x, bounds.width, edges.left, edges.right),
    };
    let first_pixel = (origin * scale).floor();
    let pixel_count = (((origin + length) * scale).ceil() - first_pixel) as u32;
    let filter = renderer.filter_quality();
    let mut composite_band = |start: f32, end: f32, alpha: f32| {
        let strip = match axis {
            Axis::Vertical => Rectangle {
                y: start,
                height: end - start,
                ..bounds
            },
            Axis::Horizontal => Rectangle {
                x: start,
                width: end - start,
                ..bounds
            },
        };
        if let Some(clip) = strip.intersection(visible) {
            renderer.draw_cached(
                cache,
                recorded,
                recorded,
                clip,
                Transformation::IDENTITY,
                Composite {
                    opacity: opacity * alpha,
                    ..Composite::plain(filter)
                },
            );
        }
    };
    if near == 0.0 && far == 0.0 {
        composite_band(origin, origin + length, 1.0);
        return;
    }

    let mut pixel = 0;
    while pixel < pixel_count {
        let start = (first_pixel + pixel as f32) / scale;
        let center = start + 0.5 / scale;
        let alpha = edge_alpha(center - origin, origin + length - center, near, far);
        let end_pixel = if alpha >= 1.0 {
            (((origin + length - far) * scale - first_pixel).floor() as u32)
                .max(pixel + 1)
                .min(pixel_count)
        } else {
            pixel + 1
        };
        composite_band(start, (first_pixel + end_pixel as f32) / scale, alpha);
        pixel = end_pixel;
    }
}

fn edge_alpha(distance_near: f32, distance_far: f32, near: f32, far: f32) -> f32 {
    let edge = |distance: f32, length: f32| {
        if length > 0.0 {
            (distance / length).clamp(0.0, 1.0)
        } else {
            1.0
        }
    };
    edge(distance_near, near).min(edge(distance_far, far))
}

type Paint<Renderer> = fn(
    &mut Renderer,
    &TextureCache,
    &TextureCache,
    Rectangle,
    &Rectangle,
    Padding,
    f32,
    &mut dyn FnMut(&mut Renderer),
);
pub(super) struct EdgeFade<Renderer> {
    pub(super) lengths: Padding,
    pub(super) paint: Paint<Renderer>,
}
impl<Renderer: TextureRenderer> EdgeFade<Renderer> {
    pub(super) fn new(lengths: Padding) -> Self {
        let sanitize = |length: f32| {
            if length.is_finite() {
                length.max(0.0)
            } else {
                0.0
            }
        };
        Self {
            lengths: Padding {
                top: sanitize(lengths.top),
                right: sanitize(lengths.right),
                bottom: sanitize(lengths.bottom),
                left: sanitize(lengths.left),
            },
            paint: draw::<Renderer>,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::Color;
    use iced::advanced::Renderer as _;
    use iced::advanced::renderer::{Headless, Quad};

    #[test]
    fn edges_multiply_real_alpha_on_transparent_background_without_pixel_seams() {
        for scale in [1.0, 1.25, 2.0] {
            let mut renderer = iced_texture_cache::testing::headless_tiny_skia();
            let size = Size::new((80.0 * scale) as u32, (80.0 * scale) as u32);
            // Set the renderer's recording scale before testing composition.
            let _ = renderer.screenshot(size, scale, Color::TRANSPARENT);
            let cache = TextureCache::new();
            let bounds = Rectangle::new(iced::Point::new(10.0, 10.0), Size::new(60.0, 60.0));
            draw(
                &mut renderer,
                &cache,
                &TextureCache::new(),
                bounds,
                &Rectangle::with_size(Size::new(80.0, 80.0)),
                Padding::ZERO.top(20).bottom(20),
                1.0,
                &mut |renderer| {
                    renderer.fill_quad(
                        Quad {
                            bounds,
                            ..Quad::default()
                        },
                        Color::WHITE,
                    );
                },
            );
            let pixels = renderer.screenshot(size, scale, Color::TRANSPARENT);
            let alpha = |x: u32, y: u32| pixels[((y * size.width + x) * 4 + 3) as usize];
            let x = (40.0 * scale) as u32;
            let first = (10.0 * scale).ceil() as u32;
            let last = (70.0 * scale).floor() as u32 - 1;
            assert_eq!(alpha(x, first - 1), 0);
            assert!(alpha(x, first) > 0 && alpha(x, first) < 30, "scale {scale}");
            assert_eq!(alpha(x, (40.0 * scale) as u32), 255);
            assert!(alpha(x, last) > 0 && alpha(x, last) < 30);
            assert_eq!(alpha(x, last + 1), 0);
            for y in first..(30.0 * scale) as u32 {
                assert!(alpha(x, y) > 0, "transparent seam at {y}, scale {scale}");
                assert!(alpha(x, y + 1) >= alpha(x, y));
            }
        }
    }

    fn render_edges(
        edges: Padding,
        scale: f32,
        opacity: f32,
        content_alpha: f32,
        clip: Rectangle,
    ) -> (Vec<u8>, Size<u32>) {
        let mut renderer = iced_texture_cache::testing::headless_tiny_skia();
        let size = Size::new((80.0 * scale) as u32, (80.0 * scale) as u32);
        let _ = renderer.screenshot(size, scale, Color::TRANSPARENT);
        let bounds = Rectangle::new(iced::Point::new(10.0, 10.0), Size::new(60.0, 60.0));
        draw(
            &mut renderer,
            &TextureCache::new(),
            &TextureCache::new(),
            bounds,
            &clip,
            edges,
            opacity,
            &mut |renderer| {
                renderer.fill_quad(
                    Quad {
                        bounds,
                        ..Default::default()
                    },
                    Color::from_rgba(1.0, 1.0, 1.0, content_alpha),
                );
            },
        );
        (renderer.screenshot(size, scale, Color::TRANSPARENT), size)
    }

    #[test]
    fn each_edge_can_be_enabled_without_fading_the_other_three() {
        for scale in [1.0, 1.25, 2.0] {
            for side in 0..4 {
                let edges = match side {
                    0 => Padding::ZERO.top(20),
                    1 => Padding::ZERO.right(20),
                    2 => Padding::ZERO.bottom(20),
                    _ => Padding::ZERO.left(20),
                };
                let (pixels, size) = render_edges(
                    edges,
                    scale,
                    1.0,
                    1.0,
                    Rectangle::with_size(Size::new(80.0, 80.0)),
                );
                let alpha = |x: u32, y: u32| pixels[((y * size.width + x) * 4 + 3) as usize];
                let first = (10.0 * scale).ceil() as u32;
                let last = (70.0 * scale).floor() as u32 - 1;
                let center = (40.0 * scale) as u32;
                let samples = [
                    alpha(center, first),
                    alpha(last, center),
                    alpha(center, last),
                    alpha(first, center),
                ];
                for (index, value) in samples.into_iter().enumerate() {
                    if side == index {
                        assert!(
                            value > 0 && value < 30,
                            "side {side}, scale {scale}: {value}"
                        );
                    } else {
                        assert_eq!(value, 255, "unrequested side {index}, scale {scale}");
                    }
                }
            }
        }
    }

    #[test]
    fn simultaneous_axes_multiply_corners_without_seams_or_reapplying_group_opacity() {
        for scale in [1.0, 1.25, 2.0] {
            let (pixels, size) = render_edges(
                Padding::from(20),
                scale,
                0.5,
                0.5,
                Rectangle::with_size(Size::new(80.0, 80.0)),
            );
            let alpha = |x: u32, y: u32| pixels[((y * size.width + x) * 4 + 3) as usize];
            let center = (40.0 * scale) as u32;
            let near = (20.0 * scale) as u32;
            let middle_alpha = alpha(center, center);
            assert!(
                (62..=65).contains(&middle_alpha),
                "opacity must be applied once: {middle_alpha}"
            );
            assert!(alpha(near, near) < alpha(near, center));
            assert!(alpha(near, center) < middle_alpha);
            for x in (11.0 * scale).ceil() as u32..(69.0 * scale).floor() as u32 {
                assert!(alpha(x, center) > 0, "vertical seam at {x}, scale {scale}");
            }
            for y in (11.0 * scale).ceil() as u32..(69.0 * scale).floor() as u32 {
                assert!(
                    alpha(center, y) > 0,
                    "horizontal seam at {y}, scale {scale}"
                );
            }
        }
    }

    #[test]
    fn asymmetric_edges_preserve_the_parent_clip() {
        let clip = Rectangle::new(iced::Point::new(20.0, 20.0), Size::new(40.0, 40.0));
        let (pixels, size) = render_edges(
            Padding {
                top: 10.0,
                right: 8.0,
                bottom: 24.0,
                left: 16.0,
            },
            1.0,
            1.0,
            1.0,
            clip,
        );
        let alpha = |x: u32, y: u32| pixels[((y * size.width + x) * 4 + 3) as usize];
        assert_eq!(alpha(19, 40), 0);
        assert_eq!(alpha(60, 40), 0);
        assert_eq!(alpha(40, 19), 0);
        assert_eq!(alpha(40, 60), 0);
        assert_eq!(alpha(40, 40), 255);
        assert!(alpha(40, 20) > alpha(40, 59));
        assert!(alpha(20, 40) < alpha(59, 40));
    }

    #[test]
    fn invalid_edge_lengths_are_disabled_individually() {
        let fade = EdgeFade::<crate::Renderer>::new(Padding {
            top: f32::NAN,
            right: f32::INFINITY,
            bottom: -4.0,
            left: 12.0,
        });
        assert_eq!(fade.lengths, Padding::ZERO.left(12));
    }
}
