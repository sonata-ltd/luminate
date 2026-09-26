//! Arc length: where along a path a given fraction of its length falls.

use iced_core::{Point, Vector};

use super::cubic::Cubic;
use super::data::{PathData, Subpath};

/// One flattened point: the parameter of its segment and the length covered
/// from the start of the path.
#[derive(Debug, Clone, Copy)]
struct Sample {
    t: f32,
    s: f32,
}

/// The path's length, tabulated once and searched per frame.
///
/// Every segment is flattened to [`TOLERANCE`](super::cubic::TOLERANCE) and
/// the running length is stored at each point. A lookup is a binary search
/// plus an interpolation between two samples, so a fraction of the length
/// maps to a parameter that moves at constant speed along the curve rather
/// than at the curve's own uneven pace.
#[derive(Debug, Clone)]
pub(crate) struct ArcLength {
    /// `(subpath, segment)` for every segment, in drawing order.
    segments: Vec<(usize, usize)>,
    /// Index into `samples` of each segment's `t = 0` sample.
    first: Vec<usize>,
    samples: Vec<Sample>,
    length: f32,
}

impl ArcLength {
    /// Builds an arc length table for a path.
    pub(crate) fn new(path: &PathData) -> Self {
        let mut table = Self {
            segments: Vec::new(),
            first: Vec::new(),
            samples: Vec::new(),
            length: 0.0,
        };

        for (sub, subpath) in path.subpaths.iter().enumerate() {
            for (index, segment) in subpath.segments.iter().enumerate() {
                table.segments.push((sub, index));
                table.first.push(table.samples.len());
                table.samples.push(Sample {
                    t: 0.0,
                    s: table.length,
                });

                let n = segment.steps();
                let mut previous = segment.p0;
                for step in 1..=n {
                    let t = step as f32 / n as f32;
                    let point = segment.eval(t);
                    table.length += previous.distance(point);
                    previous = point;
                    table.samples.push(Sample { t, s: table.length });
                }
            }
        }

        table
    }

    /// The whole length. Moves between subpaths add nothing.
    pub(crate) fn length(&self) -> f32 {
        self.length
    }

    /// The segment (index into `segments`) and parameter at length `s`.
    ///
    /// At a boundary between segments the later one wins, at `t = 0`, which
    /// is what lets a trim starting there skip the earlier segment whole.
    #[allow(clippy::many_single_char_names)]
    fn locate(&self, s: f32) -> (usize, f32) {
        let s = s.clamp(0.0, self.length);
        let k = self
            .samples
            .partition_point(|sample| sample.s <= s)
            .saturating_sub(1);
        let segment = self
            .first
            .partition_point(|&first| first <= k)
            .saturating_sub(1);
        let end = self
            .first
            .get(segment + 1)
            .copied()
            .unwrap_or(self.samples.len());

        if k + 1 >= end {
            return (segment, self.samples[k].t);
        }

        let (a, b) = (self.samples[k], self.samples[k + 1]);
        let t = if b.s > a.s {
            a.t + (b.t - a.t) * (s - a.s) / (b.s - a.s)
        } else {
            a.t
        };

        (segment, t)
    }

    fn cubic<'a>(&self, path: &'a PathData, segment: usize) -> &'a Cubic {
        let (sub, index) = self.segments[segment];
        &path.subpaths[sub].segments[index]
    }

    /// The point at fraction `u` of the length, `u` clamped to `[0, 1]`.
    pub(crate) fn point_at(&self, path: &PathData, u: f32) -> Point {
        let (segment, t) = self.locate(u.clamp(0.0, 1.0) * self.length);
        self.cubic(path, segment).eval(t)
    }

    /// The unit direction of travel at fraction `u`.
    ///
    /// Where the curve stops dead (a cusp, a segment of no length) the chord
    /// of the segment stands in, and failing that the x axis, so the result
    /// is always a unit vector.
    pub(crate) fn tangent_at(&self, path: &PathData, u: f32) -> Vector {
        let (segment, t) = self.locate(u.clamp(0.0, 1.0) * self.length);
        let cubic = self.cubic(path, segment);

        [cubic.derivative(t), cubic.p3 - cubic.p0]
            .into_iter()
            .find_map(unit)
            .unwrap_or(Vector::new(1.0, 0.0))
    }

    /// The part of the path between fractions `u0` and `u1` of its length,
    /// as open subpaths: a trimmed loop no longer joins at its start.
    ///
    /// Empty when `u1 ≤ u0` after clamping to `[0, 1]`, or when the path has
    /// no length — so a drawn range of zero draws nothing, not even the dot
    /// a round cap would leave.
    #[allow(dead_code)] // used by the path widget (feature `geometry`, task 5)
    pub(crate) fn trim(&self, path: &PathData, u0: f32, u1: f32) -> Vec<Subpath> {
        let (u0, u1) = (u0.clamp(0.0, 1.0), u1.clamp(0.0, 1.0));
        if u1 <= u0 || self.length <= 0.0 {
            return Vec::new();
        }

        let (first, t0) = self.locate(u0 * self.length);
        let (last, t1) = self.locate(u1 * self.length);
        let mut out: Vec<Subpath> = Vec::new();
        let mut current = None;

        for segment in first..=last {
            let from = if segment == first { t0 } else { 0.0 };
            let to = if segment == last { t1 } else { 1.0 };
            if to <= from {
                continue;
            }

            let piece = self.cubic(path, segment).subsegment(from, to);
            let sub = self.segments[segment].0;
            if current != Some(sub) {
                out.push(Subpath {
                    segments: Vec::new(),
                    closed: false,
                });
                current = Some(sub);
            }
            if let Some(open) = out.last_mut() {
                open.segments.push(piece);
            }
        }

        out
    }
}

fn unit(v: Vector) -> Option<Vector> {
    let length = (v.x * v.x + v.y * v.y).sqrt();
    (length > 1e-6).then(|| Vector::new(v.x / length, v.y / length))
}

#[cfg(test)]
mod tests {
    use iced_core::Point;

    use super::ArcLength;
    use crate::path::PathData;

    /// A circle of four cubic quarter arcs, the usual κ approximation.
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

    fn line(from: Point, to: Point) -> PathData {
        PathData::builder()
            .move_to(from)
            .line_to(to)
            .build()
            .unwrap()
    }

    #[test]
    fn a_line_measures_exactly_and_a_circle_to_its_circumference() {
        let straight = line(Point::ORIGIN, Point::new(10.0, 0.0));
        assert_eq!(ArcLength::new(&straight).length(), 10.0);

        let round = ArcLength::new(&circle(10.0)).length();
        assert!(
            (round - std::f32::consts::TAU * 10.0).abs() < 0.1,
            "{round}"
        );
    }

    #[test]
    #[allow(clippy::many_single_char_names)]
    fn a_quadratic_measures_as_its_raised_cubic() {
        let (a, c, b) = (Point::ORIGIN, Point::new(30.0, 60.0), Point::new(60.0, 0.0));
        let quad = PathData::builder()
            .move_to(a)
            .quad_to(c, b)
            .build()
            .unwrap();
        let cubic = PathData::builder()
            .move_to(a)
            .cubic_to(Point::new(20.0, 40.0), Point::new(40.0, 40.0), b)
            .build()
            .unwrap();
        let (q, k) = (
            ArcLength::new(&quad).length(),
            ArcLength::new(&cubic).length(),
        );
        assert!((q - k).abs() < 1e-3, "{q} vs {k}");
    }

    #[test]
    fn equal_fractions_of_length_are_equal_distances_along_the_curve() {
        // Controls bunched at the start: the parameter crawls there, races later.
        let path = PathData::builder()
            .move_to(Point::ORIGIN)
            .cubic_to(
                Point::new(1.0, 0.0),
                Point::new(2.0, 0.0),
                Point::new(100.0, 50.0),
            )
            .build()
            .unwrap();
        let table = ArcLength::new(&path);
        let cubic = path.subpaths[0].segments[0];

        let dense: Vec<Point> = (0..=20_000)
            .map(|i| cubic.eval(i as f32 / 20_000.0))
            .collect();
        let mut along = vec![0.0_f32];
        for pair in dense.windows(2) {
            let last = *along.last().unwrap();
            along.push(last + pair[0].distance(pair[1]));
        }
        let total = *along.last().unwrap();
        let arc_of = |p: Point| {
            let (index, _) = dense
                .iter()
                .enumerate()
                .min_by(|a, b| a.1.distance(p).total_cmp(&b.1.distance(p)))
                .unwrap();
            along[index]
        };

        for k in 0..=10 {
            let u = k as f32 / 10.0;
            let s = arc_of(table.point_at(&path, u));
            assert!(
                (s - u * total).abs() < 0.01 * total,
                "u = {u}: {s} of {total}"
            );
        }
    }

    #[test]
    fn a_trim_is_as_long_as_its_share_and_a_full_trim_is_the_path() {
        let round = circle(10.0);
        let table = ArcLength::new(&round);

        let part = PathData::from_subpaths(table.trim(&round, 0.2, 0.7)).unwrap();
        let share = ArcLength::new(&part).length();
        assert!((share - 0.5 * table.length()).abs() < 0.05, "{share}");

        let whole = table.trim(&round, 0.0, 1.0);
        assert_eq!(whole.len(), 1);
        assert_eq!(whole[0].segments, round.subpaths[0].segments);
        assert!(!whole[0].closed, "a trim is always open");
    }

    #[test]
    fn a_trim_across_two_subpaths_gives_two_and_a_backwards_one_nothing() {
        let two = PathData::builder()
            .move_to(Point::ORIGIN)
            .line_to(Point::new(10.0, 0.0))
            .move_to(Point::new(0.0, 5.0))
            .line_to(Point::new(10.0, 5.0))
            .build()
            .unwrap();
        let table = ArcLength::new(&two);
        assert_eq!(table.trim(&two, 0.25, 0.75).len(), 2);
        assert!(table.trim(&two, 0.6, 0.4).is_empty());
        assert!(table.trim(&two, 0.5, 0.5).is_empty());
        assert!(
            table.trim(&two, -3.0, 4.0).len() == 2,
            "clamped to the whole path"
        );
    }

    #[test]
    fn a_line_heads_along_itself() {
        let straight = line(Point::ORIGIN, Point::new(0.0, 10.0));
        let tangent = ArcLength::new(&straight).tangent_at(&straight, 0.5);
        assert!(tangent.x.abs() < 1e-6 && (tangent.y - 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_path_of_no_length_measures_and_trims_without_nan() {
        // Review Focus 2.
        let dot = line(Point::new(5.0, 5.0), Point::new(5.0, 5.0));
        let table = ArcLength::new(&dot);
        assert_eq!(table.length(), 0.0);
        assert_eq!(table.point_at(&dot, 0.5), Point::new(5.0, 5.0));
        let tangent = table.tangent_at(&dot, 0.5);
        assert!(tangent.x.is_finite() && tangent.y.is_finite());
        assert!(table.trim(&dot, 0.0, 0.5).is_empty());
    }
}
