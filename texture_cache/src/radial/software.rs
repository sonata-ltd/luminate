//! Bounded, per-widget software fallback with the same stop interpolation.

use super::{Radial, RadialCache, packed, radii};
use iced_core::{Rectangle, Size, border, image};

#[derive(Debug, PartialEq)]
struct Key {
    fill: Radial,
    size: Size,
    pixels: Size<u32>,
    corners: [f32; 4],
}
#[derive(Debug)]
pub(super) struct Raster {
    key: Key,
    handle: image::Handle,
}

impl RadialCache {
    pub(crate) fn image(
        &mut self,
        bounds: Rectangle,
        fill: Radial,
        corners: border::Radius,
        scale: f32,
    ) -> Option<image::Handle> {
        let pixels = Size::new(
            (bounds.width * scale).ceil() as u32,
            (bounds.height * scale).ceil() as u32,
        );
        // A decorative fallback must not allocate an unbounded image. Each
        // widget holds one raster, capped at 16 Mi pixels (64 MiB RGBA).
        if pixels.width == 0
            || pixels.height == 0
            || pixels.width > 16384
            || pixels.height > 16384
            || u64::from(pixels.width) * u64::from(pixels.height) > 16 * 1024 * 1024
        {
            self.raster = None;
            log::warn!("Radial gradient exceeds software raster limits");
            return None;
        }
        let key = Key {
            fill,
            size: bounds.size(),
            pixels,
            corners: radii(bounds.size(), corners),
        };
        if let Some(raster) = &self.raster
            && raster.key == key
        {
            return Some(raster.handle.clone());
        }
        let mut rgba = vec![0u8; pixels.width as usize * pixels.height as usize * 4];
        for y in 0..pixels.height {
            for x in 0..pixels.width {
                let u = (x as f32 + 0.5) / pixels.width as f32;
                let v = (y as f32 + 0.5) / pixels.height as f32;
                let t = ((u - fill.center.x) / fill.radii.x)
                    .hypot((v - fill.center.y) / fill.radii.y)
                    .clamp(0.0, 1.0);
                let mut color = sample(fill, t);
                let p = [u * key.size.width, v * key.size.height];
                let mut coverage = 1.0_f32;
                for (i, radius) in key.corners.into_iter().enumerate() {
                    let corner = [
                        if i == 1 || i == 2 {
                            key.size.width - p[0]
                        } else {
                            p[0]
                        },
                        if i >= 2 { key.size.height - p[1] } else { p[1] },
                    ];
                    if radius > 0.0 && corner[0] < radius && corner[1] < radius {
                        let distance = (corner[0] - radius).hypot(corner[1] - radius) - radius;
                        coverage = coverage.min(
                            (0.5 - distance * pixels.width as f32 / key.size.width).clamp(0.0, 1.0),
                        );
                    }
                }
                // Handle::from_rgba expects straight sRGB; interpolate in the
                // renderer's colour space first, then unpremultiply and encode.
                let alpha = color[3];
                for c in &mut color[..3] {
                    *c = if alpha > 0.0 { *c / alpha } else { 0.0 };
                    if iced_graphics::color::GAMMA_CORRECTION {
                        *c = linear_to_srgb(*c);
                    }
                }
                color[3] *= coverage;
                let index = ((y * pixels.width + x) * 4) as usize;
                rgba[index..index + 4]
                    .copy_from_slice(&color.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
        let handle = image::Handle::from_rgba(pixels.width, pixels.height, rgba);
        self.raster = Some(Raster {
            key,
            handle: handle.clone(),
        });
        Some(handle)
    }
}

pub(super) fn sample(fill: Radial, t: f32) -> [f32; 4] {
    let mut stops = fill.stops.into_iter().flatten();
    let Some(mut previous) = stops.next() else {
        return [0.0; 4];
    };
    for next in stops {
        if t < next.offset {
            let weight = ((t - previous.offset) / (next.offset - previous.offset)).clamp(0.0, 1.0);
            let a = packed(previous.color);
            let b = packed(next.color);
            return std::array::from_fn(|i| a[i] + (b[i] - a[i]) * weight);
        }
        previous = next;
    }
    packed(previous.color)
}
fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}
