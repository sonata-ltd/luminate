//! Fitting a path's view box into a widget.

use iced_core::{Point, Rectangle, Size, Vector};

/// How a path's view box is fitted into a widget, like SVG's
/// `preserveAspectRatio`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum Fit {
    /// Scaled uniformly to fit inside, centred: `xMidYMid meet`.
    #[default]
    Contain,
    /// Stretched to fill both axes: `none`.
    Fill,
    /// Not scaled: one path unit is one logical pixel, and the view box's
    /// corner sits on the widget's.
    None,
}

/// The map from path units to a widget's logical pixels: a scale per axis,
/// then an offset.
///
/// Built by [`Fit::placement`]. Positions derived from a path outside the
/// widget (a [`MotionPath`](crate::path) that moves another widget over the
/// drawn one) go through the same placement to land where the path is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Path units to pixels, per axis.
    pub scale: Vector,
    /// Added after scaling.
    pub offset: Vector,
}

impl Placement {
    /// Path units are pixels, nothing moves.
    pub const IDENTITY: Self = Self {
        scale: Vector { x: 1.0, y: 1.0 },
        offset: Vector { x: 0.0, y: 0.0 },
    };

    /// Where a point of the path lands.
    #[must_use]
    pub fn point(self, p: Point) -> Point {
        Point::new(
            p.x * self.scale.x + self.offset.x,
            p.y * self.scale.y + self.offset.y,
        )
    }

    /// How long a displacement in path units is on screen.
    #[must_use]
    pub fn vector(self, v: Vector) -> Vector {
        Vector::new(v.x * self.scale.x, v.y * self.scale.y)
    }
}

impl Fit {
    /// Where `view_box` lands in a widget of `size`.
    ///
    /// An axis the view box has no extent on (the box of a straight
    /// horizontal line) borrows the other axis's scale, or `1` when both are
    /// flat, so the placement is always finite.
    #[must_use]
    pub fn placement(self, view_box: Rectangle, size: Size) -> Placement {
        let ratio = |space: f32, extent: f32| (extent > f32::EPSILON).then(|| space / extent);
        let sx = ratio(size.width, view_box.width);
        let sy = ratio(size.height, view_box.height);

        let scale = match self {
            Fit::None => Vector::new(1.0, 1.0),
            Fit::Fill => {
                let fallback = sx.or(sy).unwrap_or(1.0);
                Vector::new(sx.unwrap_or(fallback), sy.unwrap_or(fallback))
            }
            Fit::Contain => {
                let s = match (sx, sy) {
                    (Some(x), Some(y)) => x.min(y),
                    (Some(s), None) | (None, Some(s)) => s,
                    (None, None) => 1.0,
                };
                Vector::new(s, s)
            }
        };

        let offset = match self {
            Fit::None => Vector::new(-view_box.x, -view_box.y),
            Fit::Fill | Fit::Contain => Vector::new(
                (size.width - view_box.width * scale.x) / 2.0 - view_box.x * scale.x,
                (size.height - view_box.height * scale.y) / 2.0 - view_box.y * scale.y,
            ),
        };

        Placement { scale, offset }
    }
}

#[cfg(test)]
mod tests {
    use iced_core::{Point, Rectangle, Size, Vector};

    use super::Fit;

    const BOX: Rectangle = Rectangle {
        x: 10.0,
        y: 10.0,
        width: 20.0,
        height: 10.0,
    };

    #[test]
    fn contain_scales_uniformly_and_centres() {
        let place = Fit::Contain.placement(BOX, Size::new(100.0, 100.0));
        assert_eq!(place.scale, Vector::new(5.0, 5.0));
        assert_eq!(place.point(Point::new(10.0, 10.0)), Point::new(0.0, 25.0));
        assert_eq!(place.point(Point::new(30.0, 20.0)), Point::new(100.0, 75.0));
    }

    #[test]
    fn fill_stretches_and_none_only_moves_the_corner() {
        let fill = Fit::Fill.placement(BOX, Size::new(100.0, 100.0));
        assert_eq!(fill.point(Point::new(30.0, 20.0)), Point::new(100.0, 100.0));

        let none = Fit::None.placement(BOX, Size::new(100.0, 100.0));
        assert_eq!(none.scale, Vector::new(1.0, 1.0));
        assert_eq!(none.point(Point::new(10.0, 10.0)), Point::ORIGIN);
    }

    #[test]
    fn a_flat_view_box_places_finitely() {
        // Review Focus 4: the view box of a horizontal line has no height.
        let flat = Rectangle::new(Point::new(0.0, 5.0), Size::new(40.0, 0.0));
        for fit in [Fit::Contain, Fit::Fill, Fit::None] {
            for size in [Size::new(80.0, 20.0), Size::new(0.0, 0.0)] {
                let place = fit.placement(flat, size);
                let p = place.point(Point::new(40.0, 5.0));
                assert!(
                    p.x.is_finite() && p.y.is_finite(),
                    "{fit:?} {size:?}: {p:?}"
                );
            }
        }
        let dot = Rectangle::new(Point::new(3.0, 3.0), Size::ZERO);
        let place = Fit::Contain.placement(dot, Size::new(10.0, 10.0));
        assert_eq!(place.point(Point::new(3.0, 3.0)), Point::new(5.0, 5.0));
    }
}
