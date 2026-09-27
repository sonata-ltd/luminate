//! One cubic Bézier segment, the only kind of segment a [`PathData`] holds.
//!
//! [`PathData`]: super::PathData

use iced_core::{Point, Vector};

/// How far, in path units, a flattened polyline may stray from its curve.
pub(crate) const TOLERANCE: f32 = 0.05;

/// The most straight pieces one segment is flattened into, whatever its size.
const MAX_STEPS: u32 = 256;

/// `a·(1 − t) + b·t`. Exact at both ends, unlike `a + (b − a)·t`, so a morph
/// at `t = 1` lands on its target bit for bit.
pub(crate) fn lerp(a: Point, b: Point, t: f32) -> Point {
    Point::new(a.x * (1.0 - t) + b.x * t, a.y * (1.0 - t) + b.y * t)
}

/// A cubic Bézier from `p0` to `p3` with controls `p1` and `p2`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Cubic {
    pub(crate) p0: Point,
    pub(crate) p1: Point,
    pub(crate) p2: Point,
    pub(crate) p3: Point,
}

impl Cubic {
    pub(crate) const fn new(p0: Point, p1: Point, p2: Point, p3: Point) -> Self {
        Self { p0, p1, p2, p3 }
    }

    /// A straight line, controls at a third and two thirds, so the parameter
    /// runs at constant speed along it.
    pub(crate) fn line(from: Point, to: Point) -> Self {
        Self::new(
            from,
            lerp(from, to, 1.0 / 3.0),
            lerp(from, to, 2.0 / 3.0),
            to,
        )
    }

    /// A quadratic raised to a cubic. Degree elevation is exact: the curve
    /// does not change.
    pub(crate) fn quad(from: Point, control: Point, to: Point) -> Self {
        Self::new(
            from,
            lerp(from, control, 2.0 / 3.0),
            lerp(to, control, 2.0 / 3.0),
            to,
        )
    }

    /// A segment sitting at one point: what a subpath collapses into when a
    /// morph has no partner for it.
    pub(crate) const fn point(at: Point) -> Self {
        Self::new(at, at, at, at)
    }

    pub(crate) fn is_finite(&self) -> bool {
        [self.p0, self.p1, self.p2, self.p3]
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite())
    }

    /// The point at parameter `t`.
    pub(crate) fn eval(&self, t: f32) -> Point {
        self.split(t).0.p3
    }

    /// The derivative at parameter `t`.
    pub(crate) fn derivative(&self, t: f32) -> Vector {
        let u = 1.0 - t;
        let d0 = self.p1 - self.p0;
        let d1 = self.p2 - self.p1;
        let d2 = self.p3 - self.p2;

        (d0 * (u * u) + d1 * (2.0 * u * t) + d2 * (t * t)) * 3.0
    }

    /// Splits at `t` (de Casteljau) into the part before and the part after.
    pub(crate) fn split(&self, t: f32) -> (Self, Self) {
        let a = lerp(self.p0, self.p1, t);
        let b = lerp(self.p1, self.p2, t);
        let c = lerp(self.p2, self.p3, t);
        let ab = lerp(a, b, t);
        let bc = lerp(b, c, t);
        let abc = lerp(ab, bc, t);

        (
            Self::new(self.p0, a, ab, abc),
            Self::new(abc, bc, c, self.p3),
        )
    }

    /// The part between parameters `t0 ≤ t1`.
    #[allow(dead_code)] // used by `trim`, called by the path widget (feature "geometry")
    pub(crate) fn subsegment(&self, t0: f32, t1: f32) -> Self {
        if t1 <= 0.0 {
            return Self::point(self.p0);
        }

        let (before, _) = self.split(t1);
        before.split(t0 / t1).1
    }

    /// The curve in four equal-parameter quarters, from three splits rather
    /// than four independent sub-segments.
    #[allow(dead_code)] // used by the path widget (feature "geometry")
    pub(crate) fn quarters(&self) -> [Self; 4] {
        let (half_a, half_b) = self.split(0.5);
        let (a, b) = half_a.split(0.5);
        let (c, d) = half_b.split(0.5);
        [a, b, c, d]
    }

    /// Whether the segment is a straight line: both controls lie on the
    /// chord (within a hundredth of a unit), as every `line_to` leaves them.
    /// A projection maps a straight line to a straight line, so such a
    /// segment needs neither splitting nor flattening.
    #[allow(dead_code)] // used by the path widget (feature "geometry")
    pub(crate) fn is_line(&self) -> bool {
        const SLACK: f32 = 0.01;
        let chord = self.p3 - self.p0;
        let length = (chord.x * chord.x + chord.y * chord.y).sqrt();
        let off = |p: Point| {
            let d = p - self.p0;
            if length > f32::EPSILON {
                (d.x * chord.y - d.y * chord.x).abs() / length
            } else {
                (d.x * d.x + d.y * d.y).sqrt()
            }
        };
        off(self.p1) <= SLACK && off(self.p2) <= SLACK
    }

    /// The same curve, traversed from the other end.
    pub(crate) const fn reversed(&self) -> Self {
        Self::new(self.p3, self.p2, self.p1, self.p0)
    }

    /// Every control point moved by `f`; exact for an affine map.
    pub(crate) fn map(&self, f: impl Fn(Point) -> Point) -> Self {
        Self::new(f(self.p0), f(self.p1), f(self.p2), f(self.p3))
    }

    /// The segment between `a` and `b` at `t`, control point by control
    /// point. Not clamped: `t` outside `[0, 1]` extrapolates.
    pub(crate) fn lerp(a: &Self, b: &Self, t: f32) -> Self {
        Self::new(
            lerp(a.p0, b.p0, t),
            lerp(a.p1, b.p1, t),
            lerp(a.p2, b.p2, t),
            lerp(a.p3, b.p3, t),
        )
    }

    /// How many uniform straight pieces keep a polyline within
    /// [`TOLERANCE`] of the curve.
    ///
    /// `n` pieces stray at most `M / (8 n²)`, where `M = 6 d` bounds the
    /// second derivative and `d` is the larger second difference of the
    /// control points; solving for `n` gives `√(0.75 d / tolerance)`.
    pub(crate) fn steps(&self) -> u32 {
        let second = |a: Point, b: Point, c: Point| {
            let x = a.x - 2.0 * b.x + c.x;
            let y = a.y - 2.0 * b.y + c.y;
            (x * x + y * y).sqrt()
        };
        let d = second(self.p0, self.p1, self.p2).max(second(self.p1, self.p2, self.p3));
        let steps = (0.75 * d / TOLERANCE).sqrt().ceil();

        if steps.is_finite() {
            (steps as u32).clamp(1, MAX_STEPS)
        } else {
            MAX_STEPS
        }
    }

    /// Length of the flattened curve.
    pub(crate) fn length(&self) -> f32 {
        let n = self.steps();
        let mut previous = self.p0;
        let mut total = 0.0;

        for step in 1..=n {
            let point = self.eval(step as f32 / n as f32);
            total += previous.distance(point);
            previous = point;
        }

        total
    }

    /// The parameter at which half the length has been covered: where a
    /// morph splits a segment so the new nodes fall evenly.
    pub(crate) fn half_length_t(&self) -> f32 {
        let n = self.steps().max(8);
        let points: Vec<Point> = (0..=n)
            .map(|step| self.eval(step as f32 / n as f32))
            .collect();
        let pieces: Vec<f32> = points.windows(2).map(|w| w[0].distance(w[1])).collect();
        let half = pieces.iter().sum::<f32>() / 2.0;

        if half <= 0.0 {
            return 0.5;
        }

        let mut covered = 0.0;
        for (index, piece) in pieces.iter().enumerate() {
            if covered + piece >= half {
                let within = if *piece > 0.0 {
                    (half - covered) / piece
                } else {
                    0.0
                };
                return (index as f32 + within) / n as f32;
            }
            covered += piece;
        }

        1.0
    }
}

#[cfg(test)]
mod tests {
    use iced_core::Point;

    use super::Cubic;

    fn close_to(a: Point, b: Point) -> bool {
        a.distance(b) < 1e-4
    }

    #[test]
    fn a_line_is_straight_and_evenly_parameterised() {
        let line = Cubic::line(Point::new(0.0, 0.0), Point::new(9.0, 3.0));
        assert!(close_to(line.eval(0.5), Point::new(4.5, 1.5)));
        assert!(close_to(line.eval(1.0 / 3.0), Point::new(3.0, 1.0)));
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn a_raised_quadratic_is_the_same_curve() {
        let (a, c, b) = (
            Point::new(0.0, 0.0),
            Point::new(5.0, 10.0),
            Point::new(10.0, 0.0),
        );
        let cubic = Cubic::quad(a, c, b);
        for step in 0..=10 {
            let t = step as f32 / 10.0;
            let u = 1.0 - t;
            let quadratic = Point::new(
                u * u * a.x + 2.0 * u * t * c.x + t * t * b.x,
                u * u * a.y + 2.0 * u * t * c.y + t * t * b.y,
            );
            assert!(close_to(cubic.eval(t), quadratic), "t = {t}");
        }
    }

    #[test]
    fn the_halves_of_a_split_meet_and_the_whole_is_kept_exactly() {
        let curve = Cubic::new(
            Point::new(0.0, 0.0),
            Point::new(1.0, 7.0),
            Point::new(8.0, 9.0),
            Point::new(10.0, 0.0),
        );
        let (left, right) = curve.split(0.3);
        assert_eq!(left.p3, right.p0);
        assert!(close_to(left.p3, curve.eval(0.3)));
        assert_eq!(curve.subsegment(0.0, 1.0), curve, "no rounding at the ends");
        assert!(close_to(
            curve.subsegment(0.25, 0.75).eval(0.5),
            curve.eval(0.5)
        ));
    }

    #[test]
    fn a_reversed_curve_runs_backwards() {
        let curve = Cubic::new(
            Point::new(0.0, 0.0),
            Point::new(1.0, 7.0),
            Point::new(8.0, 9.0),
            Point::new(10.0, 0.0),
        );
        assert!(close_to(curve.reversed().eval(0.2), curve.eval(0.8)));
    }

    #[test]
    fn a_curve_is_flattened_finely_enough_and_a_line_in_one_piece() {
        assert_eq!(
            Cubic::line(Point::ORIGIN, Point::new(100.0, 0.0)).steps(),
            1
        );
        let bend = Cubic::new(
            Point::ORIGIN,
            Point::new(0.0, 100.0),
            Point::new(100.0, 100.0),
            Point::new(100.0, 0.0),
        );
        assert!(bend.steps() > 10);
        assert!(Cubic::point(Point::new(f32::NAN, 0.0)).steps() >= 1);
    }

    #[test]
    fn half_the_length_is_found_on_a_line() {
        let line = Cubic::line(Point::ORIGIN, Point::new(10.0, 0.0));
        assert!((line.half_length_t() - 0.5).abs() < 1e-3);
        assert!((line.length() - 10.0).abs() < 1e-4);
        assert_eq!(Cubic::point(Point::ORIGIN).half_length_t(), 0.5);
    }

    #[test]
    fn lines_are_told_from_curves() {
        assert!(Cubic::line(Point::new(1.0, 2.0), Point::new(40.0, -3.0)).is_line());
        assert!(Cubic::point(Point::new(5.0, 5.0)).is_line());
        let bend = Cubic::new(
            Point::ORIGIN,
            Point::new(0.0, 10.0),
            Point::new(10.0, 10.0),
            Point::new(10.0, 0.0),
        );
        assert!(!bend.is_line());
    }

    #[test]
    fn quarters_are_the_four_sub_segments() {
        let curve = Cubic::new(
            Point::new(0.0, 0.0),
            Point::new(1.0, 7.0),
            Point::new(8.0, 9.0),
            Point::new(10.0, 0.0),
        );
        for (i, quarter) in curve.quarters().iter().enumerate() {
            let expected = curve.subsegment(i as f32 / 4.0, (i + 1) as f32 / 4.0);
            for (a, b) in [
                (quarter.p0, expected.p0),
                (quarter.p1, expected.p1),
                (quarter.p2, expected.p2),
                (quarter.p3, expected.p3),
            ] {
                assert!(close_to(a, b), "quarter {i}: {a:?} vs {b:?}");
            }
        }
    }
}
