//! Opt-in alpha fades, without imposing `TextureRenderer` on other scrollables.
use iced::{Rectangle, Size, Transformation, Vector};

use iced_texture_cache::{Composite, Record, TextureCache, TextureRenderer};

/// Record only the viewport, then composite one opaque middle and pixel-wide
/// horizontal bands at its edges. Alpha is multiplied, never painted over with
/// a background colour. Bands share physical pixel boundaries to avoid seams.
#[allow(clippy::too_many_arguments)]
fn draw<Renderer: TextureRenderer>(
    renderer: &mut Renderer,
    cache: &TextureCache,
    bounds: Rectangle,
    viewport: &Rectangle,
    top: f32,
    bottom: f32,
    opacity: f32,
    content: &mut dyn FnMut(&mut Renderer),
) {
    let Some(visible) = bounds.intersection(viewport) else {
        return;
    };
    if opacity <= 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
        return;
    }
    if top == 0.0 && bottom == 0.0 && opacity >= 1.0 {
        renderer.with_layer(visible, &mut *content);
        return;
    }
    let scale = renderer.scale_factor();
    let x = (bounds.x * scale).floor();
    let y = (bounds.y * scale).floor();
    let width = (bounds.width * scale + bounds.x * scale).ceil() - x;
    let height = (bounds.height * scale + bounds.y * scale).ceil() - y;
    let recorded = Rectangle::new(
        iced::Point::new(x / scale, y / scale),
        Size::new(width / scale, height / scale),
    );
    let result = renderer.record(
        cache,
        Size::new(width as u32, height as u32),
        scale,
        |renderer| {
            renderer.with_translation(Vector::new(-recorded.x, -recorded.y), |renderer| {
                renderer.with_layer(visible, &mut *content);
            });
        },
    );
    // Preserve content if an unusually large viewport exceeds texture limits.
    // Group alpha cannot be applied without the intermediate texture.
    if matches!(result, Record::Uncacheable) {
        renderer.with_layer(visible, &mut *content);
        return;
    }
    let filter = renderer.filter_quality();
    let mut composite_band = |start: f32, end: f32, alpha: f32| {
        let strip = Rectangle {
            y: start,
            height: end - start,
            ..bounds
        };
        if let Some(clip) = strip.intersection(&visible) {
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
    if top == 0.0 && bottom == 0.0 {
        composite_band(bounds.y, bounds.y + bounds.height, 1.0);
        return;
    }
    let mut row = 0_u32;
    while row < height as u32 {
        let start = (y + row as f32) / scale;
        let distance_top = start + 0.5 / scale - bounds.y;
        let distance_bottom = bounds.y + bounds.height - start - 0.5 / scale;
        let alpha = edge_alpha(distance_top, distance_bottom, top, bottom);
        // The unmasked middle needs just one composite, regardless of height.
        let end_row = if alpha >= 1.0 {
            (((bounds.y + bounds.height - bottom) * scale - y).floor() as u32)
                .max(row + 1)
                .min(height as u32)
        } else {
            row + 1
        };
        composite_band(start, (y + end_row as f32) / scale, alpha);
        row = end_row;
    }
}

fn edge_alpha(distance_top: f32, distance_bottom: f32, top: f32, bottom: f32) -> f32 {
    let edge = |distance: f32, length: f32| {
        if length > 0.0 {
            (distance / length).clamp(0.0, 1.0)
        } else {
            1.0
        }
    };
    edge(distance_top, top).min(edge(distance_bottom, bottom))
}

type Paint<Renderer> = fn(
    &mut Renderer,
    &TextureCache,
    Rectangle,
    &Rectangle,
    f32,
    f32,
    f32,
    &mut dyn FnMut(&mut Renderer),
);
pub(super) struct EdgeFade<Renderer> {
    pub(super) length: f32,
    pub(super) paint: Paint<Renderer>,
}
impl<Renderer: TextureRenderer> EdgeFade<Renderer> {
    pub(super) fn new(length: f32) -> Self {
        Self {
            length: if length.is_finite() {
                length.max(0.0)
            } else {
                0.0
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
                bounds,
                &Rectangle::with_size(Size::new(80.0, 80.0)),
                20.0,
                20.0,
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
}
