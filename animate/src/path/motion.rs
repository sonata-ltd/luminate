//! Following a path: a point and a heading for every fraction of its length.

use std::sync::Arc;

use iced_core::{Point, Radians, Vector};

use super::{ArcLength, PathData, Placement, Pose};

/// A path to move along.
///
/// Built once, since it measures the path; then [`point_at`](Self::point_at),
/// [`angle_at`](Self::angle_at) and [`pose_at`](Self::pose_at) map a fraction
/// of the length to where something on the path is. Feed them a progress
/// track through [`Anim::map`](crate::Anim::map):
///
/// ```ignore
/// let lap = m.play(key!(), Curve::ease(Easing::Linear, secs(4)), 0.0, 1.0);
/// let orbit = Arc::new(MotionPath::new(ORBIT.clone()));
/// let dot = cached(cache, content).translate(lap.map({
///     let orbit = Arc::clone(&orbit);
///     move |u| orbit.offset_at(u)
/// }));
/// ```
///
/// A single closed loop wraps: a progress of `1.25` is a quarter into the
/// second lap, and a progress running `0 → 1` over and over (a repeating
/// curve) goes round without a jump, since `1` and `0` are the same point.
/// Any other path clamps to its ends. A progress that is not a number reads
/// as the start.
#[derive(Debug, Clone)]
pub struct MotionPath {
    data: Arc<PathData>,
    table: ArcLength,
    wraps: bool,
}

impl MotionPath {
    /// Measures `data` for following.
    #[must_use]
    pub fn new(data: impl Into<Arc<PathData>>) -> Self {
        let data = data.into();
        let table = ArcLength::new(&data);
        let wraps = data.is_closed_loop();

        Self { data, table, wraps }
    }

    /// The same path placed into a widget: what to follow when the path is
    /// drawn with a [`Fit`](crate::path::Fit) and something else must move
    /// over the drawing.
    #[must_use]
    pub fn scaled(&self, placement: Placement) -> Self {
        Self::new(self.data.map(|p| placement.point(p)))
    }

    /// The path being followed.
    #[must_use]
    pub fn path(&self) -> &Arc<PathData> {
        &self.data
    }

    /// The path's length in its own units.
    #[must_use]
    pub fn length(&self) -> f32 {
        self.table.length()
    }

    fn fraction(&self, u: f32) -> f32 {
        if !u.is_finite() {
            0.0
        } else if self.wraps {
            u.rem_euclid(1.0)
        } else {
            u.clamp(0.0, 1.0)
        }
    }

    /// Where on the path fraction `u` of its length falls.
    #[must_use]
    pub fn point_at(&self, u: f32) -> Point {
        self.table.point_at(&self.data, self.fraction(u))
    }

    /// How far fraction `u` is from the start: the translation that moves
    /// something laid out at the start of the path to `u`.
    #[must_use]
    pub fn offset_at(&self, u: f32) -> Vector {
        self.point_at(u) - self.point_at(0.0)
    }

    /// The heading at fraction `u`, clockwise from the x axis.
    #[must_use]
    pub fn angle_at(&self, u: f32) -> Radians {
        let tangent = self.table.tangent_at(&self.data, self.fraction(u));
        Radians(tangent.y.atan2(tangent.x))
    }

    /// Point and heading at fraction `u`.
    #[must_use]
    pub fn pose_at(&self, u: f32) -> Pose {
        Pose {
            position: self.point_at(u),
            angle: self.angle_at(u),
        }
    }
}

#[cfg(test)]
mod tests {
    use iced_core::Point;

    use super::MotionPath;
    use crate::path::{PathData, Placement};

    fn circle(r: f32) -> PathData {
        const K: f32 = 0.552_284_8;
        let p = Point::new;
        PathData::builder()
            .move_to(p(r, 0.0))
            .cubic_to(p(r, K * r), p(K * r, r), p(0.0, r))
            .cubic_to(p(-K * r, r), p(-r, K * r), p(-r, 0.0))
            .cubic_to(p(-r, -K * r), p(-K * r, -r), p(0.0, -r))
            .cubic_to(p(K * r, -r), p(r, -K * r), p(r, 0.0))
            .close()
            .build()
            .unwrap()
    }

    fn near(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-3
    }

    #[test]
    fn on_a_circle_the_heading_is_square_to_the_radius() {
        let orbit = MotionPath::new(circle(10.0));
        for k in 0..8 {
            let u = k as f32 / 8.0;
            let p = orbit.point_at(u);
            let angle = orbit.angle_at(u).0;
            let dot = p.x * angle.cos() + p.y * angle.sin();
            assert!(dot.abs() < 0.05, "u = {u}: radius · heading = {dot}");
        }
    }

    #[test]
    fn progress_outside_the_unit_range_wraps_or_clamps() {
        // Review Focus 5.
        let orbit = MotionPath::new(circle(10.0));
        assert!(near(orbit.point_at(1.25), orbit.point_at(0.25)));
        assert!(near(orbit.point_at(-0.25), orbit.point_at(0.75)));
        assert!(near(orbit.point_at(1.0), orbit.point_at(0.0)));
        assert!(near(orbit.point_at(f32::NAN), orbit.point_at(0.0)));

        let line = MotionPath::new(
            PathData::builder()
                .move_to(Point::ORIGIN)
                .line_to(Point::new(10.0, 0.0))
                .build()
                .unwrap(),
        );
        assert!(near(line.point_at(-0.25), Point::ORIGIN));
        assert!(near(line.point_at(1.5), Point::new(10.0, 0.0)));
    }

    #[test]
    fn an_offset_is_measured_from_the_start_and_scaling_moves_everything() {
        let orbit = MotionPath::new(circle(10.0));
        assert!(orbit.offset_at(0.0).x.abs() < 1e-6);
        let half = orbit.offset_at(0.5);
        assert!(
            (half.x + 20.0).abs() < 1e-2 && half.y.abs() < 1e-2,
            "{half:?}"
        );

        let doubled = orbit.scaled(Placement {
            scale: iced_core::Vector::new(2.0, 2.0),
            offset: iced_core::Vector::new(5.0, 0.0),
        });
        assert!(near(doubled.point_at(0.0), Point::new(25.0, 0.0)));
        assert!((doubled.length() - 2.0 * orbit.length()).abs() < 0.1);
    }
}
