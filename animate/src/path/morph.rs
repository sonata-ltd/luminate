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

/// The most velocity, in morph progress per second, a retarget carries into
/// the next morph. A retarget onto a shape almost where the current one is
/// would otherwise turn a gentle motion into a lurch.
#[cfg(any(test, feature = "geometry"))]
pub(crate) const MAX_CARRIED_VELOCITY: f32 = 8.0;

/// How many points along the paths compare the old motion with the new.
#[cfg(any(test, feature = "geometry"))]
const VELOCITY_SAMPLES: usize = 64;

/// A morph that follows a target: the high-level API of the path widget.
///
/// It rests at its target. A new target starts a morph from the shape on
/// screen right now — mid-flight included — so a retarget never jumps back.
/// Its progress is a widget-owned [`Spring`](crate::Spring) from `0` to `1`.
#[cfg(any(test, feature = "geometry"))]
#[derive(Debug, Clone)]
pub(crate) struct MorphDriver {
    params: crate::SpringParams,
    carry: bool,
    target: std::sync::Arc<PathData>,
    /// `None` at rest on `target`.
    morph: Option<Morph>,
    spring: crate::Spring,
    /// Bumped on every change of shape, for the widget's geometry cache.
    generation: u64,
}

#[cfg(any(test, feature = "geometry"))]
impl MorphDriver {
    pub(crate) fn new(
        target: std::sync::Arc<PathData>,
        params: crate::SpringParams,
        carry: bool,
    ) -> Self {
        Self {
            params,
            carry,
            target,
            morph: None,
            spring: crate::Spring::new(params, 1.0),
            generation: 0,
        }
    }

    /// Takes the view's current tuning; applies from the next retarget.
    #[allow(dead_code)] // only called by the path widget (feature `geometry`)
    pub(crate) fn set_tuning(&mut self, params: crate::SpringParams, carry: bool) {
        self.params = params;
        self.carry = carry;
    }

    pub(crate) fn retarget(&mut self, target: std::sync::Arc<PathData>) {
        if std::sync::Arc::ptr_eq(&target, &self.target) || *target == *self.target {
            return;
        }

        let snapshot = self.shape();
        let velocity = if self.carry {
            self.carried_velocity(&snapshot, &target)
        } else {
            0.0
        };

        self.morph = Some(Morph::new(&snapshot, &target));
        self.spring = crate::Spring::new(self.params, 0.0).with_velocity(velocity);
        self.spring.set_target(1.0);
        self.target = target;
        self.generation += 1;
    }

    pub(crate) fn tick(&mut self, dt: f32) {
        if self.morph.is_none() {
            return;
        }

        self.spring.tick(dt);
        if self.spring.is_settled() {
            self.spring.snap();
            self.morph = None;
        }
        self.generation += 1;
    }

    pub(crate) fn is_settled(&self) -> bool {
        self.morph.is_none()
    }

    pub(crate) fn shape(&self) -> PathData {
        match &self.morph {
            Some(morph) => morph.at(self.spring.position()),
            None => (*self.target).clone(),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    /// The progress velocity of a new morph that keeps the shape's points
    /// moving as they were.
    ///
    /// The old progress and the new one measure different displacements
    /// (`to_old − from_old` against `target − snapshot`), and their nodes no
    /// longer correspond after the new matching, so both are sampled at
    /// equal fractions of arc length and the old point velocities are
    /// projected onto the new displacement by least squares.
    fn carried_velocity(&self, snapshot: &PathData, target: &PathData) -> f32 {
        let Some(morph) = &self.morph else {
            return 0.0;
        };

        let old = displacements(&morph.at(0.0), &morph.at(1.0));
        let new = displacements(snapshot, target);
        let v = self.spring.velocity();

        let along: f32 = old
            .iter()
            .zip(&new)
            .map(|(a, b)| v * (a.x * b.x + a.y * b.y))
            .sum();
        let norm: f32 = new.iter().map(|b| b.x * b.x + b.y * b.y).sum();

        if norm <= f32::EPSILON {
            return 0.0;
        }

        (along / norm).clamp(0.0, MAX_CARRIED_VELOCITY)
    }
}

/// Point by point, at equal fractions of each path's length, how far `to`
/// is from `from`.
#[cfg(any(test, feature = "geometry"))]
fn displacements(from: &PathData, to: &PathData) -> Vec<iced_core::Vector> {
    let (a, b) = (super::ArcLength::new(from), super::ArcLength::new(to));
    (0..VELOCITY_SAMPLES)
        .map(|k| {
            let u = k as f32 / (VELOCITY_SAMPLES - 1) as f32;
            b.point_at(to, u) - a.point_at(from, u)
        })
        .collect()
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

    use std::sync::Arc;

    use super::{MAX_CARRIED_VELOCITY, MorphDriver};
    use crate::SpringParams;

    const DT: f32 = 1.0 / 60.0;

    fn centred_square(side: f32) -> Arc<PathData> {
        let h = side / 2.0;
        Arc::new(polygon(&[(-h, -h), (h, -h), (h, h), (-h, h)], true))
    }

    fn width(driver: &MorphDriver) -> f32 {
        driver.shape().bounds().width
    }

    /// How fast the shape grows in the frame before a retarget, against its
    /// instantaneous speed right after.
    ///
    /// "After" is sampled over a 1 ms tick rather than a full `DT` frame: a
    /// 400 ms spring's velocity changes fast enough that a 16.67 ms average
    /// blurs the very thing being measured (the speed the instant the new
    /// morph starts), so the reported speed for "after" is not what a
    /// same-length "before" average would suggest.
    fn growth_around_a_retarget(carry: bool) -> (f32, f32) {
        const INSTANT: f32 = 0.001;

        let params = SpringParams::new(0.0, std::time::Duration::from_millis(400));
        let mut driver = MorphDriver::new(centred_square(10.0), params, carry);
        driver.retarget(centred_square(20.0));
        for _ in 0..9 {
            driver.tick(DT);
        }
        let before = width(&driver);
        driver.tick(DT);
        let speed_before = (width(&driver) - before) / DT;

        driver.retarget(centred_square(30.0));
        let at_retarget = width(&driver);
        driver.tick(INSTANT);
        let speed_after = (width(&driver) - at_retarget) / INSTANT;

        (speed_before, speed_after)
    }

    #[test]
    fn carrying_velocity_keeps_the_shape_moving_through_a_retarget() {
        let (before, after) = growth_around_a_retarget(true);
        assert!(before > 1.0, "mid-flight: {before}");
        assert!(
            after > 0.6 * before && after < 1.6 * before,
            "carried: {before} then {after}"
        );
    }

    #[test]
    fn without_carrying_a_retarget_starts_from_rest() {
        let (before, after) = growth_around_a_retarget(false);
        assert!(after < 0.35 * before, "from rest: {before} then {after}");
    }

    #[test]
    fn a_retarget_does_not_jump_and_the_same_target_changes_nothing() {
        let params = SpringParams::default();
        let mut driver = MorphDriver::new(centred_square(10.0), params, false);
        driver.retarget(centred_square(20.0));
        for _ in 0..6 {
            driver.tick(DT);
        }
        let shape = driver.shape();
        driver.retarget(centred_square(30.0));
        assert!((driver.shape().bounds().width - shape.bounds().width).abs() < 1e-3);

        let generation = driver.generation();
        driver.retarget(centred_square(30.0));
        assert_eq!(
            driver.generation(),
            generation,
            "an equal target is not a retarget"
        );

        for _ in 0..600 {
            driver.tick(DT);
        }
        assert!(driver.is_settled());
        assert_eq!(driver.shape(), *centred_square(30.0));
    }

    #[test]
    fn a_carried_velocity_is_bounded() {
        const {
            assert!(
                MAX_CARRIED_VELOCITY > 3.0,
                "room for the mid-flight speed above"
            );
        };
    }
}
