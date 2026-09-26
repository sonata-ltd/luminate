//! A path normalised into absolute cubic segments.

use std::fmt;

use iced_core::{Point, Rectangle, Size};

use super::cubic::Cubic;

/// A path normalised into absolute cubic Bézier segments.
///
/// Every command a path can be written in — lines, quadratics, cubics,
/// arcs, their relative and shorthand forms — is reduced to cubics once,
/// when the path is built. Length, trimming, morphing and drawing then deal
/// with one kind of segment.
///
/// Build one with [`PathData::builder`], or parse SVG path data with
/// `PathData::parse` (feature `svg-path`).
#[derive(Debug, Clone, PartialEq)]
pub struct PathData {
    pub(crate) subpaths: Vec<Subpath>,
    bounds: Rectangle,
}

/// A run of connected segments: what one move starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Subpath {
    /// Never empty: a subpath without segments is dropped while building.
    pub(crate) segments: Vec<Cubic>,
    /// Whether the subpath ends with a close. The segment back to the start
    /// is already in `segments`; this only decides the line join there.
    pub(crate) closed: bool,
}

impl Subpath {
    #[allow(dead_code)] // only used by this module's own tests
    pub(crate) fn start(&self) -> Point {
        self.segments[0].p0
    }
}

impl PathData {
    /// Starts building a path.
    #[must_use]
    pub fn builder() -> PathBuilder {
        PathBuilder::default()
    }

    /// Validates `subpaths`: at least one, every point finite.
    pub(crate) fn from_subpaths(subpaths: Vec<Subpath>) -> Result<Self, PathError> {
        if subpaths.is_empty() {
            return Err(PathError::Empty);
        }

        if !subpaths
            .iter()
            .flat_map(|s| &s.segments)
            .all(Cubic::is_finite)
        {
            return Err(PathError::NonFinite);
        }

        Ok(Self::from_subpaths_unchecked(subpaths))
    }

    /// Wraps subpaths already known to be sound, such as a morph's.
    pub(crate) fn from_subpaths_unchecked(subpaths: Vec<Subpath>) -> Self {
        let bounds = bounds_of(&subpaths);
        Self { subpaths, bounds }
    }

    /// The smallest rectangle holding the curve itself, not its control
    /// points. The default view box of a path widget.
    #[must_use]
    pub fn bounds(&self) -> Rectangle {
        self.bounds
    }

    /// How many subpaths (runs started by a move) the path has.
    #[must_use]
    pub fn subpath_count(&self) -> usize {
        self.subpaths.len()
    }

    /// How many cubic segments the path has, across all subpaths.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        self.subpaths.iter().map(|s| s.segments.len()).sum()
    }

    /// `true` for a single closed loop: the kind of path a motion can go
    /// round and round.
    #[must_use]
    pub fn is_closed_loop(&self) -> bool {
        self.subpaths.len() == 1 && self.subpaths[0].closed
    }

    /// The path with every point moved by `f`. Exact for an affine map
    /// (scale, rotation, translation), which is what it is for.
    #[must_use]
    pub fn map(&self, f: impl Fn(Point) -> Point) -> Self {
        Self::from_subpaths_unchecked(
            self.subpaths
                .iter()
                .map(|subpath| Subpath {
                    segments: subpath.segments.iter().map(|c| c.map(&f)).collect(),
                    closed: subpath.closed,
                })
                .collect(),
        )
    }
}

/// Bounds of the flattened curve: tight to within the flattening tolerance.
fn bounds_of(subpaths: &[Subpath]) -> Rectangle {
    let mut min = Point::new(f32::INFINITY, f32::INFINITY);
    let mut max = Point::new(f32::NEG_INFINITY, f32::NEG_INFINITY);

    for segment in subpaths.iter().flat_map(|s| &s.segments) {
        let n = segment.steps();
        for step in 0..=n {
            let p = segment.eval(step as f32 / n as f32);
            min = Point::new(min.x.min(p.x), min.y.min(p.y));
            max = Point::new(max.x.max(p.x), max.y.max(p.y));
        }
    }

    if min.x > max.x {
        return Rectangle::default();
    }

    Rectangle::new(min, Size::new(max.x - min.x, max.y - min.y))
}

/// Why a path could not be built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PathError {
    /// Nothing to draw: no segment at all, or only moves.
    Empty,
    /// A coordinate is NaN or infinite, or overflowed `f32` when parsed.
    NonFinite,
    /// The SVG path data does not parse.
    Syntax,
}

impl fmt::Display for PathError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "the path draws nothing",
            Self::NonFinite => "the path has a coordinate that is not a finite number",
            Self::Syntax => "the path data does not parse",
        })
    }
}

impl std::error::Error for PathError {}

/// Builds a [`PathData`] command by command, the way SVG path data reads.
///
/// A drawing command before any [`move_to`](Self::move_to) starts at the
/// origin. After [`close`](Self::close) the pen is back at the start of the
/// subpath, as in SVG.
#[derive(Debug, Default)]
pub struct PathBuilder {
    done: Vec<Subpath>,
    open: Vec<Cubic>,
    start: Point,
    pen: Point,
}

impl PathBuilder {
    /// Starts a new subpath at `to`.
    #[must_use]
    pub fn move_to(mut self, to: Point) -> Self {
        self.finish(false);
        self.start = to;
        self.pen = to;
        self
    }

    /// A straight line from the pen to `to`.
    #[must_use]
    pub fn line_to(self, to: Point) -> Self {
        let from = self.pen;
        self.push(Cubic::line(from, to))
    }

    /// A quadratic Bézier from the pen to `to`.
    #[must_use]
    pub fn quad_to(self, control: Point, to: Point) -> Self {
        let from = self.pen;
        self.push(Cubic::quad(from, control, to))
    }

    /// A cubic Bézier from the pen to `to`.
    #[must_use]
    pub fn cubic_to(self, control_a: Point, control_b: Point, to: Point) -> Self {
        let from = self.pen;
        self.push(Cubic::new(from, control_a, control_b, to))
    }

    /// Closes the subpath with a straight line back to its start, unless the
    /// pen is there already.
    #[must_use]
    pub fn close(mut self) -> Self {
        if self.open.is_empty() {
            return self;
        }

        if self.pen != self.start {
            let (pen, start) = (self.pen, self.start);
            self = self.push(Cubic::line(pen, start));
        }

        self.finish(true);
        self.pen = self.start;
        self
    }

    /// Finishes the path.
    ///
    /// # Errors
    ///
    /// [`PathError::Empty`] if nothing was drawn, [`PathError::NonFinite`] if
    /// any coordinate is NaN or infinite.
    pub fn build(mut self) -> Result<PathData, PathError> {
        self.finish(false);
        PathData::from_subpaths(self.done)
    }

    fn push(mut self, segment: Cubic) -> Self {
        self.pen = segment.p3;
        self.open.push(segment);
        self
    }

    fn finish(&mut self, closed: bool) {
        if !self.open.is_empty() {
            self.done.push(Subpath {
                segments: std::mem::take(&mut self.open),
                closed,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use iced_core::Point;

    use super::{PathData, PathError};

    #[test]
    fn lines_become_cubics_and_close_adds_the_way_back() {
        let open = PathData::builder()
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(10.0, 10.0))
            .build()
            .unwrap();
        assert_eq!(open.segment_count(), 2);
        assert!(!open.is_closed_loop());

        let closed = PathData::builder()
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(10.0, 10.0))
            .close()
            .build()
            .unwrap();
        assert_eq!(closed.segment_count(), 3, "the closing line is a segment");
        assert!(closed.is_closed_loop());

        let already_home = PathData::builder()
            .move_to(Point::new(0.0, 0.0))
            .line_to(Point::new(10.0, 0.0))
            .line_to(Point::new(0.0, 0.0))
            .close()
            .build()
            .unwrap();
        assert_eq!(
            already_home.segment_count(),
            2,
            "no zero-length closing line"
        );
    }

    #[test]
    fn nothing_to_draw_and_non_finite_points_are_errors() {
        assert_eq!(PathData::builder().build(), Err(PathError::Empty));
        assert_eq!(
            PathData::builder()
                .move_to(Point::new(1.0, 1.0))
                .move_to(Point::new(2.0, 2.0))
                .build(),
            Err(PathError::Empty)
        );
        assert_eq!(
            PathData::builder()
                .move_to(Point::ORIGIN)
                .line_to(Point::new(f32::NAN, 0.0))
                .build(),
            Err(PathError::NonFinite)
        );
    }

    #[test]
    fn a_command_after_close_starts_where_the_subpath_began() {
        let path = PathData::builder()
            .move_to(Point::new(5.0, 5.0))
            .line_to(Point::new(10.0, 5.0))
            .close()
            .line_to(Point::new(5.0, 20.0))
            .build()
            .unwrap();
        assert_eq!(path.subpath_count(), 2);
        assert_eq!(path.subpaths[1].start(), Point::new(5.0, 5.0));
    }

    #[test]
    fn bounds_follow_the_curve_not_its_controls() {
        // The controls reach y = 100; the curve itself peaks at y = 75.
        let arch = PathData::builder()
            .move_to(Point::ORIGIN)
            .cubic_to(
                Point::new(0.0, 100.0),
                Point::new(100.0, 100.0),
                Point::new(100.0, 0.0),
            )
            .build()
            .unwrap();
        let bounds = arch.bounds();
        assert!((bounds.height - 75.0).abs() < 0.1, "{bounds:?}");
        assert!((bounds.width - 100.0).abs() < 1e-3);
    }

    #[test]
    fn map_moves_every_point() {
        let path = PathData::builder()
            .move_to(Point::ORIGIN)
            .line_to(Point::new(10.0, 0.0))
            .build()
            .unwrap();
        let moved = path.map(|p| Point::new(p.x * 2.0, p.y + 3.0));
        assert_eq!(moved.bounds().x, 0.0);
        assert_eq!(moved.bounds().y, 3.0);
        assert_eq!(moved.bounds().width, 20.0);
    }
}
