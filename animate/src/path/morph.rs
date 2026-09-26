//! Morphing one path into another: the two are matched once, then every
//! frame is a lerp of control points.

use iced_core::{Point, Rectangle, Size};

use super::cubic::Cubic;
use super::data::{PathData, Subpath};

/// Closed subpaths with more nodes than this try rotations on a stride, so
/// matching stays near-linear in the size of the path.
const ALIGN_BUDGET: usize = 256;

/// Two paths brought to one structure, ready to be blended at any `t`.
///
/// Building a morph does the work: subpaths are paired, each pair is given
/// the same number of segments by splitting the longest ones, and each
/// closed pair is rotated and, if need be, reversed so that corresponding
/// nodes are as close as they can be. [`at`](Self::at) is then a lerp of
/// control points, cheap enough for every frame.
///
/// Because the matching splits cubics rather than resampling points, curves
/// stay curves at every `t` and every scale.
#[derive(Debug, Clone, PartialEq)]
pub struct Morph {
    pairs: Vec<Pair>,
}

#[derive(Debug, Clone, PartialEq)]
struct Pair {
    from: Vec<Cubic>,
    to: Vec<Cubic>,
    from_closed: bool,
    to_closed: bool,
}

impl Morph {
    /// Matches `from` against `to`.
    #[must_use]
    pub fn new(from: &PathData, to: &PathData) -> Self {
        let pairs = pair_subpaths(&from.subpaths, &to.subpaths)
            .into_iter()
            .map(|(a, b)| match_pair(&a, &b))
            .collect();

        Self { pairs }
    }

    /// The shape at `t`: `from` at `0`, `to` at `1`.
    ///
    /// `t` is not clamped. Outside `[0, 1]` the shape extrapolates, which is
    /// what a spring's overshoot looks like on a morph. A subpath is closed
    /// if the nearer end has it closed.
    #[must_use]
    pub fn at(&self, t: f32) -> PathData {
        PathData::from_subpaths_unchecked(
            self.pairs
                .iter()
                .map(|pair| Subpath {
                    segments: pair
                        .from
                        .iter()
                        .zip(&pair.to)
                        .map(|(a, b)| Cubic::lerp(a, b, t))
                        .collect(),
                    closed: if t < 0.5 {
                        pair.from_closed
                    } else {
                        pair.to_closed
                    },
                })
                .collect(),
        )
    }
}

/// Bounds of the control points: cheap, and good enough to rank and pair.
fn hull(subpath: &Subpath) -> Rectangle {
    let mut min = Point::new(f32::INFINITY, f32::INFINITY);
    let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);
    for c in &subpath.segments {
        for p in [c.p0, c.p1, c.p2, c.p3] {
            min = Point::new(min.x.min(p.x), min.y.min(p.y));
            max = Point::new(max.x.max(p.x), max.y.max(p.y));
        }
    }
    Rectangle::new(min, Size::new(max.x - min.x, max.y - min.y))
}

fn area(subpath: &Subpath) -> f32 {
    let hull = hull(subpath);
    hull.width * hull.height
}

/// The same structure as `subpath`, every point at its centre: what a
/// subpath with no partner grows out of or shrinks into.
fn collapsed(subpath: &Subpath) -> Subpath {
    let centre = hull(subpath).center();
    Subpath {
        segments: vec![Cubic::point(centre); subpath.segments.len()],
        closed: subpath.closed,
    }
}

/// Pairs the largest subpaths first, each with the nearest one left on the
/// other side; the rest pair with a collapsed copy of themselves.
fn pair_subpaths(from: &[Subpath], to: &[Subpath]) -> Vec<(Subpath, Subpath)> {
    // A `fn` item, not a closure: a closure's parameter type is fixed to one
    // concrete lifetime on first use, which `from` and `to` do not share.
    fn by_area(subpaths: &[Subpath]) -> Vec<&Subpath> {
        let mut sorted: Vec<&Subpath> = subpaths.iter().collect();
        sorted.sort_by(|a, b| area(b).total_cmp(&area(a)));
        sorted
    }
    let from = by_area(from);
    let mut left: Vec<Option<&Subpath>> = by_area(to).into_iter().map(Some).collect();
    let mut pairs = Vec::with_capacity(from.len().max(left.len()));

    for a in from {
        let centre = hull(a).center();
        let nearest = left
            .iter()
            .enumerate()
            .filter_map(|(i, b)| b.map(|b| (i, hull(b).center().distance(centre))))
            .min_by(|x, y| x.1.total_cmp(&y.1));

        match nearest.and_then(|(i, _)| left[i].take()) {
            Some(b) => pairs.push((a.clone(), b.clone())),
            None => pairs.push((a.clone(), collapsed(a))),
        }
    }

    for b in left.into_iter().flatten() {
        pairs.push((collapsed(b), b.clone()));
    }

    pairs
}

fn match_pair(a: &Subpath, b: &Subpath) -> Pair {
    let n = a.segments.len().max(b.segments.len());
    let from = subdivide_to(&a.segments, n);
    let mut to = subdivide_to(&b.segments, n);

    if a.closed && b.closed {
        to = align_loop(&from, to);
    }

    Pair {
        from,
        to,
        from_closed: a.closed,
        to_closed: b.closed,
    }
}

/// Splits the longest segment in half by length until there are `n`.
fn subdivide_to(segments: &[Cubic], n: usize) -> Vec<Cubic> {
    let mut out = segments.to_vec();
    let mut lengths: Vec<f32> = out.iter().map(Cubic::length).collect();

    while out.len() < n {
        let (longest, _) = lengths
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .expect("a subpath has at least one segment");
        let (left, right) = out[longest].split(out[longest].half_length_t());

        out[longest] = left;
        out.insert(longest + 1, right);
        lengths[longest] = left.length();
        lengths.insert(longest + 1, right.length());
    }

    out
}

/// The rotation, and direction, of the loop `to` whose nodes sit closest to
/// those of `from`.
fn align_loop(from: &[Cubic], to: Vec<Cubic>) -> Vec<Cubic> {
    let n = to.len();
    let backwards: Vec<Cubic> = to.iter().rev().map(Cubic::reversed).collect();
    let stride = n.div_ceil(ALIGN_BUDGET).max(1);

    let cost = |candidate: &[Cubic], shift: usize| -> f32 {
        (0..n)
            .map(|i| {
                let d = from[i].p0 - candidate[(i + shift) % n].p0;
                d.x * d.x + d.y * d.y
            })
            .sum()
    };

    let mut best = (f32::INFINITY, 0, false);
    for (candidate, reversed) in [(&to, false), (&backwards, true)] {
        for shift in (0..n).step_by(stride) {
            let c = cost(candidate, shift);
            if c < best.0 {
                best = (c, shift, reversed);
            }
        }
    }

    let (_, shift, reversed) = best;
    let source = if reversed { backwards } else { to };
    (0..n).map(|i| source[(i + shift) % n]).collect()
}

#[cfg(test)]
mod tests {
    use iced_core::Point;

    use super::Morph;
    use crate::path::PathData;

    fn polygon(points: &[(f32, f32)], closed: bool) -> PathData {
        let mut builder = PathData::builder().move_to(Point::new(points[0].0, points[0].1));
        for &(x, y) in &points[1..] {
            builder = builder.line_to(Point::new(x, y));
        }
        if closed {
            builder = builder.close();
        }
        builder.build().unwrap()
    }

    fn square(side: f32) -> PathData {
        polygon(&[(0.0, 0.0), (side, 0.0), (side, side), (0.0, side)], true)
    }

    fn nodes(path: &PathData) -> Vec<Point> {
        path.subpaths[0].segments.iter().map(|s| s.p0).collect()
    }

    fn near(a: &[Point], b: &[Point]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.distance(*b) < 1e-3)
    }

    #[test]
    fn the_ends_of_a_morph_are_its_paths_exactly() {
        let (small, large) = (square(10.0), square(20.0));
        let morph = Morph::new(&small, &large);
        assert_eq!(morph.at(0.0), small);
        assert_eq!(morph.at(1.0), large);
    }

    #[test]
    fn a_path_morphed_into_itself_stays_put() {
        let path = square(10.0);
        let morph = Morph::new(&path, &path);
        for t in [0.1, 0.37, 0.5, 0.93] {
            assert!(near(&nodes(&morph.at(t)), &nodes(&path)), "t = {t}");
        }
    }

    #[test]
    fn a_loop_started_elsewhere_or_run_backwards_is_aligned() {
        let path = square(10.0);
        let shifted = polygon(&[(10.0, 0.0), (10.0, 10.0), (0.0, 10.0), (0.0, 0.0)], true);
        let backwards = polygon(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0), (10.0, 0.0)], true);

        for other in [shifted, backwards] {
            let morph = Morph::new(&path, &other);
            assert!(
                near(&nodes(&morph.at(0.5)), &nodes(&path)),
                "halfway between one square and the same square is that square"
            );
        }
    }

    #[test]
    fn unlike_structures_are_equalised() {
        let triangle = polygon(&[(0.0, 0.0), (10.0, 0.0), (5.0, 8.0)], true);
        let morph = Morph::new(&triangle, &square(10.0));
        assert_eq!(morph.at(0.3).segment_count(), 4);

        let one = square(10.0);
        let two = PathData::builder()
            .move_to(Point::ORIGIN)
            .line_to(Point::new(10.0, 0.0))
            .move_to(Point::new(40.0, 40.0))
            .line_to(Point::new(50.0, 40.0))
            .build()
            .unwrap();
        let grown = Morph::new(&one, &two);
        let start = grown.at(0.0);
        assert_eq!(start.subpath_count(), 2);
        let seed = &start.subpaths[1];
        assert!(
            seed.segments
                .iter()
                .all(|s| s.p0 == s.p3 && s.p0 == seed.segments[0].p0),
            "the subpath with no partner grows out of a point"
        );
    }

    #[test]
    fn morphs_between_unlike_paths_stay_finite() {
        // Review Focus 3: one open segment into a closed fifty-gon, and far
        // outside the unit range, as an overshooting spring asks for.
        let line = polygon(&[(0.0, 0.0), (10.0, 0.0)], false);
        let ring: Vec<(f32, f32)> = (0..50)
            .map(|k| {
                let a = k as f32 / 50.0 * std::f32::consts::TAU;
                (20.0 * a.cos(), 20.0 * a.sin())
            })
            .collect();
        let ring = polygon(&ring, true);

        let morph = Morph::new(&line, &ring);
        for t in [-0.5, 0.0, 0.5, 1.0, 1.5] {
            let shape = morph.at(t);
            assert_eq!(shape.segment_count(), 50);
            assert!(
                shape
                    .subpaths
                    .iter()
                    .flat_map(|s| &s.segments)
                    .all(super::Cubic::is_finite),
                "t = {t}"
            );
        }
        assert!(
            !morph.at(0.2).is_closed_loop(),
            "the open end wins below one half"
        );
        assert!(morph.at(0.8).is_closed_loop(), "the closed end wins above");
    }
}
