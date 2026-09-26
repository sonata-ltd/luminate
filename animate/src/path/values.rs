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
/// It is usually derived from a progress track with `MotionPath::pose_at`, not animated
/// on its own: a track interpolating the angle directly would turn the long
/// way round across `±π`.
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
}
