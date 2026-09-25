//! The camera: scene space to the drawing plane, and fitting a projected
//! scene into a box.

use iced_luminate::iced::{Point, Size};

/// The camera angle. Every icon in the set shares it, so changing it here
/// restyles all of them at once.
pub(crate) const CAMERA_ANGLE: f32 = std::f32::consts::FRAC_PI_6;

/// The inset between a scene's bounding box and the edge of its canvas.
pub(crate) const PADDING: f32 = 12.0;

/// A point in scene space: `x` runs right and down, `y` runs left and down,
/// `z` runs up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Vec3 {
    /// The right-and-down axis.
    pub(crate) x: f32,
    /// The left-and-down axis.
    pub(crate) y: f32,
    /// The up axis.
    pub(crate) z: f32,
}

impl Vec3 {
    /// A point in scene space.
    pub(crate) const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

/// Projects a scene point onto the drawing plane.
///
/// Screen `y` grows downward, so a larger `z` moves a point up. A change in
/// `z` moves a point along screen `y` alone, which is what lets
/// [`sweep`](super::shape::sweep) extrude a footprint with a straight shift.
pub(crate) fn project(point: Vec3) -> Point {
    Point::new(
        (point.x - point.y) * CAMERA_ANGLE.cos(),
        (point.x + point.y) * CAMERA_ANGLE.sin() - point.z,
    )
}

/// The uniform transform that centres a projected scene in a canvas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Fit {
    /// The top-left of the projected bounding box.
    min: Point,
    /// The uniform scale onto the canvas.
    scale: f32,
    /// Where the scaled bounding box starts on the canvas.
    origin: Point,
}

impl Fit {
    /// Fits the bounding box of `points` into `size` inset by [`PADDING`],
    /// preserving the aspect ratio and centring the result.
    ///
    /// An empty or degenerate set of points yields an unscaled transform, so
    /// a malformed scene draws small rather than panicking.
    pub(crate) fn new(points: &[Point], size: Size) -> Self {
        let mut min = Point::new(f32::MAX, f32::MAX);
        let mut max = Point::new(f32::MIN, f32::MIN);

        for point in points {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }

        if points.is_empty() {
            min = Point::ORIGIN;
            max = Point::ORIGIN;
        }

        let extent = Size::new(max.x - min.x, max.y - min.y);
        let available = Size::new(
            (size.width - PADDING * 2.0).max(0.0),
            (size.height - PADDING * 2.0).max(0.0),
        );

        let scale = if extent.width > 0.0 && extent.height > 0.0 {
            (available.width / extent.width).min(available.height / extent.height)
        } else {
            1.0
        };

        let origin = Point::new(
            (size.width - extent.width * scale) / 2.0,
            (size.height - extent.height * scale) / 2.0,
        );

        Self { min, scale, origin }
    }

    /// Maps a projected point into canvas coordinates.
    pub(crate) fn apply(&self, point: Point) -> Point {
        Point::new(
            self.origin.x + (point.x - self.min.x) * self.scale,
            self.origin.y + (point.y - self.min.y) * self.scale,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projects_the_three_axes() {
        let cos = CAMERA_ANGLE.cos();
        let sin = CAMERA_ANGLE.sin();

        assert_eq!(project(Vec3::new(0.0, 0.0, 0.0)), Point::ORIGIN);
        assert_eq!(project(Vec3::new(1.0, 0.0, 0.0)), Point::new(cos, sin));
        assert_eq!(project(Vec3::new(0.0, 1.0, 0.0)), Point::new(-cos, sin));
        assert_eq!(project(Vec3::new(0.0, 0.0, 1.0)), Point::new(0.0, -1.0));
    }

    #[test]
    fn height_moves_a_point_up_the_screen() {
        let ground = project(Vec3::new(2.0, 3.0, 0.0));
        let raised = project(Vec3::new(2.0, 3.0, 5.0));

        assert_eq!(raised.x, ground.x);
        assert_eq!(raised.y, ground.y - 5.0);
    }

    #[test]
    fn fit_insets_by_the_padding_on_the_binding_axis() {
        let points = [Point::new(0.0, 0.0), Point::new(10.0, 5.0)];
        let fit = Fit::new(&points, Size::new(128.0, 128.0));
        let scale = (128.0 - PADDING * 2.0) / 10.0;

        assert_eq!(fit.apply(Point::new(0.0, 0.0)).x, PADDING);
        assert_eq!(fit.apply(Point::new(10.0, 5.0)).x, PADDING + 10.0 * scale);
    }

    #[test]
    fn fit_centres_the_shorter_axis() {
        let points = [Point::new(0.0, 0.0), Point::new(10.0, 5.0)];
        let fit = Fit::new(&points, Size::new(128.0, 128.0));

        let top = fit.apply(Point::new(0.0, 0.0)).y;
        let bottom = fit.apply(Point::new(10.0, 5.0)).y;

        assert!((top - (128.0 - bottom)).abs() < 0.001);
    }

    #[test]
    fn fit_of_nothing_does_not_divide_by_zero() {
        let fit = Fit::new(&[], Size::new(128.0, 128.0));

        assert!(fit.apply(Point::ORIGIN).x.is_finite());
        assert!(fit.apply(Point::ORIGIN).y.is_finite());
    }
}
