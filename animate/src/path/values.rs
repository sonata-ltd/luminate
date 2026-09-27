//! The scalars a path animation actually moves.

use iced_core::{Point, Radians};

use crate::{Animatable, MAX_COMPONENTS};

/// Which part of a path's stroke is drawn, as fractions of its length.
///
/// Animatable as two components, so one curve can draw a line on
/// (`EMPTY → FULL`), wipe it off (`FULL → DrawRange::new(1.0, 1.0)`) or run a
/// dash along it. Values are not clamped while they move; the widget clamps
/// them to `[0, 1]` where it draws, so an overshooting spring stops at the
/// ends of the path.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrawRange {
    /// Where the drawn part starts, from `0.0` (the path's start).
    pub start: f32,
    /// Where it ends, up to `1.0` (the path's end).
    pub end: f32,
}

impl DrawRange {
    /// The whole stroke.
    pub const FULL: Self = Self::new(0.0, 1.0);

    /// Nothing, collapsed at the start.
    pub const EMPTY: Self = Self::new(0.0, 0.0);

    /// A range from `start` to `end`.
    #[must_use]
    pub const fn new(start: f32, end: f32) -> Self {
        Self { start, end }
    }

    /// Both ends clamped to `[0, 1]`; a NaN end counts as `0`.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self::new(unit(self.start), unit(self.end))
    }

    /// `true` when the clamped range covers the whole path.
    #[must_use]
    pub fn is_full(self) -> bool {
        let range = self.clamped();
        range.start <= 0.0 && range.end >= 1.0
    }

    /// `true` when the clamped range draws nothing.
    #[must_use]
    pub fn is_empty(self) -> bool {
        let range = self.clamped();
        range.end <= range.start
    }
}

impl Default for DrawRange {
    fn default() -> Self {
        Self::FULL
    }
}

fn unit(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

impl Animatable for DrawRange {
    const COMPONENTS: usize = 2;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.start;
        out[1] = self.end;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Self::new(src[0], src[1])
    }
}

/// A position and a heading: where something on a path is and which way it
/// faces. Animatable as three components.
///
/// It is usually derived from a progress track with
/// [`Anim::map`](crate::Anim::map) and
/// [`MotionPath::pose_at`](crate::path::MotionPath::pose_at), not animated on
/// its own: a track interpolating the angle directly would turn the long way
/// round across `±π`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Where the origin of what is posed goes.
    pub position: Point,
    /// Its rotation, clockwise on screen (y points down).
    pub angle: Radians,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Point::ORIGIN,
            angle: Radians(0.0),
        }
    }
}

impl Animatable for Pose {
    const COMPONENTS: usize = 3;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.position.x;
        out[1] = self.position.y;
        out[2] = self.angle.0;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Self {
            position: Point::new(src[0], src[1]),
            angle: Radians(src[2]),
        }
    }
}

/// A tilt of the drawing plane, seen from `distance`: the perspective a path
/// widget projects its drawing through. Animatable as three components.
///
/// The plane turns about the widget's centre: first by `tilt_x` about the
/// horizontal axis (a positive tilt leans the top edge away), then by
/// `tilt_y` about the vertical one (a positive tilt leans the right edge
/// away), and is then seen in perspective from `distance` logical pixels in
/// front of it. The far side shrinks toward the centre, the near side grows.
///
/// Give every layer of a scene the same perspective (one handle, cloned) and
/// the layers tilt together, poses included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Perspective {
    /// Rotation about the horizontal axis through the centre.
    pub tilt_x: Radians,
    /// Rotation about the vertical axis through the centre.
    pub tilt_y: Radians,
    /// How far the eye is from the plane, in logical pixels. Smaller is a
    /// stronger perspective; a few times the widget's size looks natural.
    pub distance: f32,
}

impl Perspective {
    /// No tilt: the drawing as it is.
    pub const NONE: Self = Self::new(0.0, 0.0, 1000.0);

    /// A tilt of `tilt_x` and `tilt_y` radians, seen from `distance`.
    #[must_use]
    pub const fn new(tilt_x: f32, tilt_y: f32, distance: f32) -> Self {
        Self {
            tilt_x: Radians(tilt_x),
            tilt_y: Radians(tilt_y),
            distance,
        }
    }

    /// `true` when the plane is not tilted, so projecting changes nothing.
    #[must_use]
    pub fn is_flat(self) -> bool {
        self.tilt_x.0 == 0.0 && self.tilt_y.0 == 0.0
    }

    /// Where `point` of the plane appears once the plane is tilted about
    /// `centre` and seen in perspective.
    ///
    /// A distance that is not a positive finite number, or a point that
    /// would pass behind the eye, is held just in front of it, so the result
    /// is always finite. To project many points, take a
    /// [`projector`](Self::projector) once instead: it works the tilt's sines
    /// and cosines out a single time.
    #[must_use]
    pub fn project(self, point: Point, centre: Point) -> Point {
        self.projector(centre).project(point)
    }

    /// This perspective about `centre`, ready to project many points.
    #[must_use]
    pub fn projector(self, centre: Point) -> Projector {
        let (sin_x, cos_x) = self.tilt_x.0.sin_cos();
        let (sin_y, cos_y) = self.tilt_y.0.sin_cos();
        let distance = if self.distance.is_finite() && self.distance > 1.0 {
            self.distance
        } else {
            1.0
        };

        Projector {
            centre,
            sin_x,
            cos_x,
            sin_y,
            cos_y,
            distance,
        }
    }
}

/// A [`Perspective`] about a fixed centre, with its trigonometry done: what
/// a frame's worth of points is projected through.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projector {
    centre: Point,
    sin_x: f32,
    cos_x: f32,
    sin_y: f32,
    cos_y: f32,
    distance: f32,
}

impl Projector {
    /// Where `point` appears. See [`Perspective::project`].
    #[must_use]
    pub fn project(&self, point: Point) -> Point {
        let (x, y) = (point.x - self.centre.x, point.y - self.centre.y);

        // About the horizontal axis: the top (y < 0) goes away (z > 0).
        let y1 = y * self.cos_x;
        let z1 = -y * self.sin_x;
        // About the vertical axis: the right (x > 0) goes away.
        let x2 = x * self.cos_y - z1 * self.sin_y;
        let z2 = x * self.sin_y + z1 * self.cos_y;

        let scale = self.distance / (self.distance + z2).max(self.distance * 0.05);

        Point::new(self.centre.x + x2 * scale, self.centre.y + y1 * scale)
    }
}

impl Default for Perspective {
    fn default() -> Self {
        Self::NONE
    }
}

impl Animatable for Perspective {
    const COMPONENTS: usize = 3;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.tilt_x.0;
        out[1] = self.tilt_y.0;
        out[2] = self.distance;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Self::new(src[0], src[1], src[2])
    }
}

#[cfg(test)]
mod tests {
    use iced_core::{Point, Radians};

    use super::{DrawRange, Pose};
    use crate::{Animatable, MAX_COMPONENTS};

    fn round_trip<T: Animatable + PartialEq + std::fmt::Debug>(value: T) {
        let mut slots = [f32::NAN; MAX_COMPONENTS];
        value.write(&mut slots);
        assert!(slots[..T::COMPONENTS].iter().all(|v| v.is_finite()));
        assert_eq!(T::read(&slots), value);
    }

    #[test]
    fn both_types_round_trip_through_the_track_slots() {
        round_trip(DrawRange::new(0.25, 0.75));
        round_trip(Pose {
            position: Point::new(3.0, -4.0),
            angle: Radians(1.25),
        });
    }

    #[test]
    fn a_range_is_clamped_to_the_path_and_empty_when_it_turns_back() {
        assert_eq!(DrawRange::new(-0.2, 1.3).clamped(), DrawRange::FULL);
        assert!(DrawRange::new(-0.2, 1.3).is_full());
        assert!(DrawRange::new(0.6, 0.4).is_empty());
        assert!(DrawRange::EMPTY.is_empty());
        assert_eq!(DrawRange::new(f32::NAN, 0.5).clamped().start, 0.0);
        assert!(!DrawRange::new(0.0, 0.5).is_full());
    }

    #[test]
    fn a_flat_perspective_leaves_points_alone() {
        let centre = Point::new(50.0, 50.0);
        let p = Point::new(12.0, 80.0);
        assert_eq!(super::Perspective::NONE.project(p, centre), p);
        round_trip(super::Perspective::new(0.3, -0.2, 800.0));
    }

    #[test]
    fn a_tilt_shrinks_the_far_side_and_grows_the_near_one() {
        let centre = Point::new(0.0, 0.0);
        // Top edge away: a point above the centre moves in, one below out.
        let tilt = super::Perspective::new(0.5, 0.0, 400.0);
        let top = tilt.project(Point::new(100.0, -100.0), centre);
        let bottom = tilt.project(Point::new(100.0, 100.0), centre);
        assert!(top.x < 100.0 && bottom.x > 100.0, "{top:?} {bottom:?}");
        assert!(
            top.y > -100.0 && bottom.y < 100.0,
            "both foreshortened vertically"
        );

        // Right edge away.
        let turn = super::Perspective::new(0.0, 0.5, 400.0);
        let right = turn.project(Point::new(100.0, 50.0), centre);
        let left = turn.project(Point::new(-100.0, 50.0), centre);
        assert!(right.y < 50.0 && left.y > 50.0, "{right:?} {left:?}");
    }

    #[test]
    fn a_degenerate_perspective_stays_finite() {
        let centre = Point::new(0.0, 0.0);
        for perspective in [
            super::Perspective::new(1.5, 1.5, 10.0),
            super::Perspective::new(0.4, 0.0, 0.0),
            super::Perspective::new(0.4, 0.0, f32::NAN),
        ] {
            let p = perspective.project(Point::new(500.0, -500.0), centre);
            assert!(p.x.is_finite() && p.y.is_finite(), "{perspective:?}: {p:?}");
        }
    }
}
