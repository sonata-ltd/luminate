//! A non-affine warp applied when a cached texture is composited.
//!
//! Affine motion — translate, scale — rides on the `Transformation` the
//! compositor already applies to the destination rectangle. A warp cannot:
//! it is evaluated per destination pixel, as an *inverse* map that answers
//! "which source texel does this pixel show?". The shader runs it; this
//! module is the same arithmetic in Rust, which is what makes it testable
//! and what the software backend approximates.
//!
//! The shape is taken from two reference implementations rather than
//! invented — `KWin`'s magic lamp effects and a published recreation of the
//! macOS original — and two of their properties are what make it read as a
//! suction rather than a squeeze: the content *travels* along the axis
//! toward the anchor, and the neck's shape curve is argued by a row's
//! position along that travel path rather than within the content, so rows
//! enter the neck one after another instead of narrowing all together.

use iced_core::{Point, Rectangle, Transformation, mouse};

/// Collapse at which the stretch is complete.
const STRETCH_END: f32 = 0.4;

/// Collapse at which the squash begins. Well before [`STRETCH_END`], so the
/// two overlap heavily: run end to end they read as two animations rather
/// than one. The references disagree here and these are `GenieWarpMesh`'s,
/// which is the one calibrated against the real effect.
const SQUASH_START: f32 = 0.15;

/// The largest lag the map stays invertible at.
///
/// `Genie::row` is monotonic — and so has an inverse for the shader to run —
/// only while `stretch_power * stretch < e`. Past that it folds over itself,
/// and a fragment shader has no way to report that: the image simply
/// corrupts. So [`GenieShape::stretch_power`] is clamped to this on
/// construction rather than trusted to the caller.
pub const MAX_STRETCH_POWER: f32 = 2.7;

/// The lag `GenieShape::default` uses: `GenieWarpMesh`'s value.
const DEFAULT_STRETCH_POWER: f32 = 2.0;

/// The target width `GenieShape::default` uses.
const DEFAULT_TARGET_WIDTH: f32 = 0.12;

/// The side-curve controls `GenieShape::default` uses: the pair that makes
/// [`bend`] the smoothstep, which is `GenieWarpMesh`'s own side curve.
const DEFAULT_CURVE_IN: f32 = 0.0;
const DEFAULT_CURVE_OUT: f32 = 1.0;

/// How far the corner mask's edge is spread, in device pixels, so it is not
/// a hard staircase.
#[cfg(test)]
const CORNER_FEATHER: f32 = 1.0;

/// A row narrower than this has closed. Dividing by it would turn rounding
/// into a visible streak across the rest of the row.
const CLOSED: f32 = 1e-4;

/// The corner a [`Genie`] collapses into.
///
/// A corner of the *content*, not a point in the window. Callers that think
/// in terms of where a popup was anchored map that onto a corner themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// The content's top-left corner.
    TopLeft,
    /// The content's top-right corner.
    TopRight,
    /// The content's bottom-left corner.
    BottomLeft,
    /// The content's bottom-right corner.
    BottomRight,
}

impl Corner {
    /// Whether the normalised axes are mirrored to put this corner at the
    /// origin.
    const fn flips(self) -> (bool, bool) {
        match self {
            Self::TopLeft => (false, false),
            Self::TopRight => (true, false),
            Self::BottomLeft => (false, true),
            Self::BottomRight => (true, true),
        }
    }

    /// Where this corner sits in `bounds`: the point a scale about the
    /// corner holds still.
    pub(crate) fn fixed_point(self, bounds: iced_core::Rectangle) -> (f32, f32) {
        let (right, bottom) = match self {
            Self::TopLeft => (false, false),
            Self::TopRight => (true, false),
            Self::BottomLeft => (false, true),
            Self::BottomRight => (true, true),
        };

        (
            if right {
                bounds.x + bounds.width
            } else {
                bounds.x
            },
            if bottom {
                bounds.y + bounds.height
            } else {
                bounds.y
            },
        )
    }
}

/// Everything about a genie except how far along it is.
///
/// A struct rather than a row of arguments: the shape is tuned by eye, and
/// this way another knob costs no signature change anywhere downstream.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GenieShape {
    /// The corner the content collapses into.
    pub anchor: Corner,
    /// How wide the band it collapses into is, as a fraction of the
    /// content's own width. Clamped to `0..=1`.
    pub target_width: f32,
    /// How sharply a row's travel is delayed by its distance from the
    /// anchor, clamped to `0..=`[`MAX_STRETCH_POWER`].
    ///
    /// The shift is `squash^(1 + stretch_power * stretch * y)`, so the row at
    /// the anchor moves by the whole squash and the row at the far edge
    /// barely moves at all. That is what keeps the wide end of the S on
    /// screen. At `0` every row travels together, the far edge leaves as
    /// fast as the near one, and all that is left to see is the concave half
    /// — an inward arch.
    pub stretch_power: f32,
    /// The side curve's control value at the wide end, clamped to `0..=1`.
    ///
    /// `bend`'s slope there is `3 * curve_in`, so `0` leaves the side
    /// exactly parallel to the travel — a flat run before anything happens —
    /// and raising it bends the side sooner.
    pub curve_in: f32,
    /// The side curve's control value at the neck end, clamped to `0..=1`.
    ///
    /// `bend`'s slope there is `3 * (1 - curve_out)`, so `1` arrives
    /// parallel to the travel and lowering it bends the side later.
    pub curve_out: f32,
    /// The radius, in logical pixels, that the collapsing shape's corners
    /// keep however far it has been squeezed. `0` leaves them alone.
    ///
    /// A rounded corner recorded into the texture is *pixels*, so squeezing
    /// a row to `target_width` squeezes its corner with it: an 8px radius at
    /// a target of 0.12 is drawn about 1px wide and reads as a straight cut.
    /// Rounding here instead, in destination space, keeps the radius on
    /// screen whatever the row's width. Clamped per row to half the shape,
    /// so a narrow end becomes a stadium rather than growing corners bigger
    /// than itself.
    ///
    /// Only applied while the warp is live. At rest the content's own
    /// rounding is left exactly as recorded, and the handover is continuous
    /// because the warp starts at full width.
    pub corner_radius: f32,
}

impl Default for GenieShape {
    fn default() -> Self {
        Self {
            anchor: Corner::TopLeft,
            target_width: DEFAULT_TARGET_WIDTH,
            stretch_power: DEFAULT_STRETCH_POWER,
            curve_in: DEFAULT_CURVE_IN,
            curve_out: DEFAULT_CURVE_OUT,
            corner_radius: 0.0,
        }
    }
}

/// A collapse into one corner, in the manner of a window minimising into a
/// dock icon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Genie {
    progress: f32,
    shape: GenieShape,
}

impl Genie {
    /// `progress` is `1.0` fully open and `0.0` fully collapsed, clamped to
    /// that range: the map only stays inside the content's own rectangle
    /// while it is, and a curve that overshoots would otherwise tear the
    /// image out of its texture. A non-finite progress is treated as open.
    ///
    /// `shape` is clamped here rather than on the caller's behalf
    /// elsewhere: `target_width` into `0..=1`, and `stretch_power` into
    /// `0..=`[`MAX_STRETCH_POWER`], past which the map would fold over
    /// itself and corrupt the image with nothing to report it. A non-finite
    /// field falls back to its default.
    #[must_use]
    pub fn new(progress: f32, shape: GenieShape) -> Self {
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            1.0
        };

        let shape = GenieShape {
            anchor: shape.anchor,
            target_width: if shape.target_width.is_finite() {
                shape.target_width.clamp(0.0, 1.0)
            } else {
                DEFAULT_TARGET_WIDTH
            },
            stretch_power: if shape.stretch_power.is_finite() {
                shape.stretch_power.clamp(0.0, MAX_STRETCH_POWER)
            } else {
                DEFAULT_STRETCH_POWER
            },
            // Inside `0..=1` the Bézier stays inside its own convex hull,
            // which is what keeps the width from exceeding the content's
            // rectangle and being clipped at its edge.
            curve_in: if shape.curve_in.is_finite() {
                shape.curve_in.clamp(0.0, 1.0)
            } else {
                DEFAULT_CURVE_IN
            },
            curve_out: if shape.curve_out.is_finite() {
                shape.curve_out.clamp(0.0, 1.0)
            } else {
                DEFAULT_CURVE_OUT
            },
            corner_radius: if shape.corner_radius.is_finite() {
                shape.corner_radius.max(0.0)
            } else {
                0.0
            },
        };

        Self { progress, shape }
    }

    /// The shape it collapses with, as clamped.
    #[must_use]
    pub const fn shape(self) -> GenieShape {
        self.shape
    }

    /// The width of the band the rows converge on, as a fraction of the
    /// content's own width.
    #[must_use]
    pub const fn target_width(self) -> f32 {
        self.shape.target_width
    }

    /// The radius its corners keep however far it is squeezed.
    #[must_use]
    pub const fn corner_radius(self) -> f32 {
        self.shape.corner_radius
    }

    /// The clamped progress this genie will actually draw at.
    #[must_use]
    pub const fn progress(self) -> f32 {
        self.progress
    }

    /// The corner it collapses into.
    #[must_use]
    pub const fn anchor(self) -> Corner {
        self.shape.anchor
    }

    /// Whether it is anywhere but fully open, and so has something to draw
    /// differently.
    #[must_use]
    pub fn is_live(self) -> bool {
        self.progress < 1.0
    }

    /// How far from the anchor the last row is drawn, as a fraction of the
    /// content's height: the shape's visible far edge in destination space.
    /// `1.0` until the squash begins, falling to `0.0` when the whole
    /// content has been drawn through the anchor.
    ///
    /// The corner mask measures a pixel's distance to this edge, and the
    /// distance has to be taken here, in the space the pixel is drawn in.
    /// `1 - v` is the same distance in *source* rows, and the squash makes
    /// a source row longer than the destination row that shows it, so a
    /// mask that used it would round the far corners to a radius that
    /// shrinks as the collapse runs.
    ///
    /// The edge is the same for every column, so it is solved once here
    /// rather than per pixel in the shader.
    #[must_use]
    pub fn far_edge(self) -> f32 {
        let (_, s) = self.phases();
        if s <= 0.0 {
            return 1.0;
        }

        // `row` is strictly increasing, `row(0) = s <= 1` and `row(1) > 1`,
        // so a bisection finds the one `y` whose row is the last one.
        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        for _ in 0..40 {
            let mid = f32::midpoint(lo, hi);
            if self.row(mid) < 1.0 {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        f32::midpoint(lo, hi)
    }

    /// How far the stretch and the squash have each run, derived from one
    /// animated progress so a caller has one number to drive.
    pub(crate) fn phases(self) -> (f32, f32) {
        let collapse = 1.0 - self.progress;
        let stretch = (collapse / STRETCH_END).clamp(0.0, 1.0);
        let squash = ((collapse - SQUASH_START) / (1.0 - SQUASH_START)).clamp(0.0, 1.0);

        (stretch, squash)
    }

    /// The axis mirrors that put this genie's anchor at the origin.
    pub(crate) const fn flips(self) -> (bool, bool) {
        self.shape.anchor.flips()
    }

    /// Where a source point lands, or `None` once it has been drawn through
    /// the anchor. The shader never needs this — it runs
    /// [`source`](Self::source) — but the forward direction is what makes
    /// the inverse testable.
    ///
    /// Solved by bisection. Lagging each row by its *destination* position
    /// is what keeps the inverse closed-form, and it is the inverse the
    /// shader runs every frame; this direction is implicit as a result, and
    /// nothing but a test ever calls it.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn destination(self, u: f32, v: f32) -> Option<(f32, f32)> {
        // `u`, `v`, `x`, `y`, `k`, `s` are the map's own notation, shared
        // with the spec and the shader. Renaming them here would break that
        // correspondence for no gain.
        let (flip_x, flip_y) = self.flips();
        let (u, v) = (flip(u, flip_x), flip(v, flip_y));
        let (k, _) = self.phases();

        // `row(y)` is strictly increasing, so a bisection converges on the
        // one `y` that shows this row. Below `row(0)` the row has already
        // been drawn through the anchor.
        if v < self.row(0.0) {
            return None;
        }

        let mut lo = 0.0_f32;
        let mut hi = 1.0_f32;
        for _ in 0..40 {
            let mid = f32::midpoint(lo, hi);
            if self.row(mid) < v {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let y = f32::midpoint(lo, hi);

        let w = width(y, k, self.shape);
        if w <= CLOSED {
            return None;
        }

        Some((flip(u * w, flip_x), flip(y, flip_y)))
    }

    /// Which source row the row drawn at `y` shows, before the `0..=1` test.
    ///
    /// The exponent is the lag: `1` at the anchor, rising with distance from
    /// it, so a far row's shift is a high power of a number below one and is
    /// therefore small.
    fn row(self, y: f32) -> f32 {
        let (k, s) = self.phases();

        y + s.powf(1.0 + self.shape.stretch_power * k * y)
    }

    /// Which source point a destination point shows, or `None` where the
    /// collapsed shape does not reach.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn source(self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (flip_x, flip_y) = self.flips();
        let (x, y) = (flip(x, flip_x), flip(y, flip_y));
        let (k, s) = self.phases();

        // Fully collapsed, everything has been drawn through the anchor;
        // the one row the arithmetic still admits, at `y = 0`, is a line of
        // no height. The shader makes the same cut.
        if s >= 1.0 {
            return None;
        }

        // The width comes straight from the destination row, which is what
        // keeps the inverse closed-form.
        let w = width(y, k, self.shape);
        if w <= CLOSED {
            return None;
        }

        let u = x / w;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }

        let v = self.row(y);
        if !(0.0..=1.0).contains(&v) {
            return None;
        }

        Some((flip(u, flip_x), flip(v, flip_y)))
    }
}

/// What a warp is, if it is anything.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Warp {
    /// No warp: the composite is the affine one it has always been.
    #[default]
    None,
    /// A collapse into one corner; see [`Genie`].
    Genie(Genie),
}

impl Warp {
    /// Whether the warp is doing anything this frame. A warp that is not
    /// live composites exactly as `None` does.
    #[must_use]
    pub fn is_live(self) -> bool {
        match self {
            Self::None => false,
            Self::Genie(genie) => genie.is_live(),
        }
    }

    /// The affine approximation as a transform of `content`, the rectangle
    /// the warp is defined on: a scale about its anchor corner by the
    /// progress, composed *after* whatever transform the content is drawn
    /// under. `None` once the content has closed to nothing, which a scale
    /// of zero cannot express (its inverse is not finite) and which draws
    /// nothing anyway.
    #[must_use]
    pub(crate) fn affine_transform(self, content: Rectangle) -> Option<Transformation> {
        let Some((progress, anchor)) = self.affine_fallback() else {
            return Some(Transformation::IDENTITY);
        };
        if progress <= CLOSED {
            return None;
        }
        let (fixed_x, fixed_y) = anchor.fixed_point(content);
        Some(
            Transformation::translate(fixed_x, fixed_y)
                * Transformation::scale(progress)
                * Transformation::translate(-fixed_x, -fixed_y),
        )
    }

    /// The affine approximation a backend without shaders composites
    /// instead: a scale about the anchor corner, by the same progress. It
    /// loses the neck and keeps the timing, which is the right trade on a
    /// software renderer.
    #[must_use]
    pub(crate) fn affine_fallback(self) -> Option<(f32, Corner)> {
        match self {
            Self::None => None,
            Self::Genie(genie) => Some((genie.progress(), genie.anchor())),
        }
    }
}

impl Warp {
    /// Which point of the content a pointer at `point` is over, or `None`
    /// where the warped image does not reach: the inverse of what the
    /// backend draws, so a widget's hit-testing agrees with its pixels.
    ///
    /// `bounds` is the content's rectangle, the one the warp is defined on.
    /// `exact` says the shader is drawing the genie itself, so the inverse
    /// is [`Genie::source`]; otherwise what is drawn is the affine
    /// approximation of [`affine_fallback`](Self::affine_fallback) — on the
    /// software backend, or for content too large for a texture — and its
    /// inverse is a scale about the same anchor.
    pub(crate) fn source_point(
        self,
        exact: bool,
        bounds: Rectangle,
        point: Point,
    ) -> Option<Point> {
        let Self::Genie(genie) = self else {
            return Some(point);
        };
        if !genie.is_live() {
            return Some(point);
        }
        // Fully collapsed there is nothing on screen. The inverse map still
        // admits the one row drawn *at* the anchor, a line of no height
        // that no pixel centre ever lands on; a pointer must not either.
        if genie.progress() <= CLOSED || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }

        if exact {
            let x = (point.x - bounds.x) / bounds.width;
            let y = (point.y - bounds.y) / bounds.height;
            if !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
                return None;
            }
            let (u, v) = genie.source(x, y)?;
            Some(Point::new(
                bounds.x + u * bounds.width,
                bounds.y + v * bounds.height,
            ))
        } else {
            let (progress, anchor) = self.affine_fallback()?;
            let (fixed_x, fixed_y) = anchor.fixed_point(bounds);
            let source = Point::new(
                fixed_x + (point.x - fixed_x) / progress,
                fixed_y + (point.y - fixed_y) / progress,
            );
            bounds.contains(source).then_some(source)
        }
    }

    /// [`source_point`](Self::source_point) applied to a cursor: a cursor
    /// over nothing becomes [`mouse::Cursor::Unavailable`], so collapsed
    /// content is neither clickable nor hovered where it no longer is.
    pub(crate) fn map_cursor(
        self,
        exact: bool,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Cursor {
        let map = |point| self.source_point(exact, bounds, point);
        match cursor {
            mouse::Cursor::Available(point) => {
                map(point).map_or(mouse::Cursor::Unavailable, mouse::Cursor::Available)
            }
            mouse::Cursor::Levitating(point) => {
                map(point).map_or(mouse::Cursor::Unavailable, mouse::Cursor::Levitating)
            }
            mouse::Cursor::Unavailable => mouse::Cursor::Unavailable,
        }
    }
}

impl From<Genie> for Warp {
    fn from(genie: Genie) -> Self {
        Self::Genie(genie)
    }
}

/// The width of the row drawn at `y`, stretched by `k`, with `shape`'s
/// target width and side curve.
///
/// `1 - y` is the row's position along the travel path, which is what makes
/// the neck sweep.
fn width(y: f32, k: f32, shape: GenieShape) -> f32 {
    let along = (1.0 - y).clamp(0.0, 1.0);

    1.0 - k * bend(along, shape.curve_in, shape.curve_out) * (1.0 - shape.target_width)
}

/// The side curve: the cubic Bézier through `(0, c_in, c_out, 1)`.
///
/// `GenieWarpMesh` builds each side as a Bézier whose control points sit on
/// its endpoints' cross-axis coordinates, which is this with `c_in = 0` and
/// `c_out = 1` — and that pair is exactly `3t² - 2t³`, the smoothstep. So
/// the default is the reference's own curve, and the two controls open up
/// the shape either side of it: the slope is `3·c_in` at the wide end and
/// `3·(1 - c_out)` at the neck.
fn bend(t: f32, c_in: f32, c_out: f32) -> f32 {
    let u = 1.0 - t;

    3.0 * u * u * t * c_in + 3.0 * u * t * t * c_out + t * t * t
}

/// How much of a pixel survives the corner mask.
///
/// `dl`, `dr`, `dt`, `db` are its distances to the shape's four edges and
/// `r` the corner radius, all in device pixels. Away from a corner this is
/// `1`; inside one it is the coverage of a circle of radius `r` centred `r`
/// in from both edges, feathered over [`CORNER_FEATHER`].
///
/// Measuring from the edges rather than from a rectangle's corners is what
/// lets it follow a shape whose right edge is curved: each row is rounded
/// against its own width.
///
/// The mask that actually runs is `corner_alpha` in `composite.wgsl`; this
/// is its mirror, and exists so the arithmetic can be tested without a GPU.
/// Nothing but the tests calls it, and the two must be kept in step by hand.
#[cfg(test)]
pub(crate) fn corner_alpha(dl: f32, dr: f32, dt: f32, db: f32, r: f32) -> f32 {
    if r <= 0.0 {
        return 1.0;
    }

    let dx = dl.min(dr);
    let dy = dt.min(db);
    if dx >= r || dy >= r {
        return 1.0;
    }

    let reach = ((r - dx).powi(2) + (r - dy).powi(2)).sqrt();

    ((r - reach) / CORNER_FEATHER + 0.5).clamp(0.0, 1.0)
}

/// Mirrors a normalised coordinate when the anchor is on the far side.
fn flip(value: f32, flip: bool) -> f32 {
    if flip { 1.0 - value } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_core::Size;

    /// Every progress a test sweeps, including both ends.
    const PROGRESSES: [f32; 7] = [0.0, 0.05, 0.25, 0.5, 0.75, 0.95, 1.0];

    /// A shape with the default lag, so a test says only what it is about.
    fn shape(anchor: Corner, target_width: f32) -> GenieShape {
        GenieShape {
            anchor,
            target_width,
            ..GenieShape::default()
        }
    }

    fn grid() -> impl Iterator<Item = (f32, f32)> {
        (0..=10).flat_map(|i| (0..=10).map(move |j| (i as f32 / 10.0, j as f32 / 10.0)))
    }

    #[test]
    fn fully_open_is_the_identity() {
        let genie = Genie::new(1.0, shape(Corner::TopLeft, 0.0));
        for (x, y) in grid() {
            let (u, v) = genie.source(x, y).expect("nothing is clipped when open");
            assert!((u - x).abs() < 1e-6, "u {u} != x {x}");
            assert!((v - y).abs() < 1e-6, "v {v} != y {y}");
        }
    }

    #[test]
    fn the_map_round_trips() {
        // Tested destination-first. The other direction divides by the row
        // width, which magnifies error by `1 / w` and says nothing useful
        // about a row that has closed to a point.
        for t in PROGRESSES {
            let genie = Genie::new(t, shape(Corner::TopLeft, 0.0));
            for (x, y) in grid() {
                let Some((u, v)) = genie.source(x, y) else {
                    continue;
                };
                let (x2, y2) = genie
                    .destination(u, v)
                    .expect("a source the inverse found is drawn somewhere");
                assert!((x2 - x).abs() < 1e-4, "t {t}: x {x} -> {u},{v} -> {x2}");
                assert!((y2 - y).abs() < 1e-4, "t {t}: y {y} -> {u},{v} -> {y2}");
            }
        }
    }

    #[test]
    fn the_map_never_leaves_the_source_rectangle() {
        for t in PROGRESSES {
            let genie = Genie::new(t, shape(Corner::TopLeft, 0.0));
            for (u, v) in grid() {
                // `None` is content drawn through the anchor: consumed, not
                // drawn outside, which is the property under test.
                let Some((x, y)) = genie.destination(u, v) else {
                    continue;
                };
                assert!(
                    (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
                    "t {t}: ({u},{v}) left the rectangle at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn the_neck_sweeps_so_rows_nearer_the_anchor_are_narrower() {
        // The row's width is the destination of its far edge, `u = 1`.
        let genie = Genie::new(0.5, shape(Corner::TopLeft, 0.0));
        let (near, _) = genie.destination(1.0, 0.5).expect("still drawn");
        let (far, _) = genie.destination(1.0, 1.0).expect("still drawn");
        assert!(near < far, "row at 0.5 is {near} wide, row at 1.0 is {far}");
    }

    #[test]
    fn the_side_profile_is_a_wave_not_an_arch() {
        // Flat at both ends, steepest in the middle. An arch is steepest at
        // one end and passes every other test in this module, so this is
        // asserted directly: it is the shape that was shipped and rejected.
        let flat = shape(Corner::TopLeft, 0.0);
        let slope = |a: f32, b: f32| (width(b, 1.0, flat) - width(a, 1.0, flat)).abs() / (b - a);

        let at_anchor = slope(0.0, 0.05);
        let middle = slope(0.475, 0.525);
        let far = slope(0.95, 1.0);

        assert!(
            middle > at_anchor * 4.0,
            "middle {middle} vs anchor {at_anchor}"
        );
        assert!(middle > far * 4.0, "middle {middle} vs far {far}");
    }

    #[test]
    fn the_far_edge_is_where_the_last_row_is_drawn() {
        for t in PROGRESSES {
            let genie = Genie::new(t, shape(Corner::TopLeft, 0.12));
            let far = genie.far_edge();
            assert!((0.0..=1.0).contains(&far), "t {t}: far edge {far}");
            let (_, s) = genie.phases();
            if s <= 0.0 {
                assert!((far - 1.0).abs() < f32::EPSILON, "t {t}: no squash yet");
            } else {
                // The last source row, `v = 1`, is the one drawn there.
                let v = genie.row(far);
                assert!((v - 1.0).abs() < 1e-5, "t {t}: row({far}) = {v}");
            }
            // Nothing is drawn past it.
            assert!(
                genie.source(0.0, (far + 1e-3).min(1.0)).is_none() || far >= 1.0 - 1e-3,
                "t {t}: something is drawn beyond the far edge {far}"
            );
        }
    }

    #[test]
    fn the_far_edge_is_measured_in_destination_rows() {
        // The reviewer's case: a quarter of the way in, a point eight
        // pixels short of the visible far edge is eight pixels short of it,
        // not the five that `1 - v` (in source rows) would say.
        let genie = Genie::new(0.25, GenieShape::default());
        let height = 200.0;
        let far = genie.far_edge();
        let y = far - 8.0 / height;
        let (_, v) = genie.source(0.0, y).expect("still drawn");
        let in_source = (1.0 - v) * height;
        let in_destination = (far - y) * height;
        assert!((in_destination - 8.0).abs() < 1e-3, "{in_destination}");
        assert!(in_source < 6.0, "source rows are longer: {in_source}");
    }

    #[test]
    fn a_pointer_follows_the_shader_on_wgpu() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        let open = Warp::Genie(Genie::new(1.0, GenieShape::default()));
        let point = Point::new(60.0, 45.0);
        assert_eq!(open.source_point(true, bounds, point), Some(point));

        // Half way in, the rows past the visible far edge have been drawn
        // through a top-left anchor: nothing is there to click.
        let genie = Genie::new(0.5, GenieShape::default());
        let half = Warp::Genie(genie);
        let far = genie.far_edge();
        assert!(far < 1.0, "the squash has begun by half way");
        assert_eq!(
            half.source_point(true, bounds, Point::new(20.0, 20.0 + (far + 0.01) * 50.0)),
            None
        );
        assert!(
            half.source_point(true, bounds, Point::new(20.0, 20.0 + (far - 0.01) * 50.0))
                .is_some()
        );
        // What is drawn maps back onto the content, exactly as the shader
        // samples it.
        let (u, v) = genie.source(0.1, 0.1).expect("drawn");
        assert_eq!(
            half.source_point(true, bounds, Point::new(20.0, 25.0)),
            Some(Point::new(10.0 + u * 100.0, 20.0 + v * 50.0))
        );
        // Outside the content there is nothing, whatever the map would say.
        assert_eq!(half.source_point(true, bounds, Point::new(5.0, 25.0)), None);
    }

    #[test]
    fn a_pointer_follows_the_affine_fallback_on_tiny_skia() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        let half = Warp::Genie(Genie::new(0.5, GenieShape::default()));
        // The software backend scales about the anchor, so the content's
        // centre is drawn a quarter of the way in from the top-left.
        assert_eq!(
            half.source_point(false, bounds, Point::new(35.0, 32.5)),
            Some(Point::new(60.0, 45.0))
        );
        // Where the content used to be, and is no longer drawn.
        assert_eq!(
            half.source_point(false, bounds, Point::new(90.0, 60.0)),
            None
        );
        // The anchor is the content's corner, not a padded one.
        let bottom_right = Warp::Genie(Genie::new(
            0.5,
            GenieShape {
                anchor: Corner::BottomRight,
                ..GenieShape::default()
            },
        ));
        assert_eq!(
            bottom_right.source_point(false, bounds, Point::new(109.0, 69.0)),
            Some(Point::new(108.0, 68.0))
        );
    }

    #[test]
    fn a_collapsed_genie_draws_nothing_not_even_the_anchor_row() {
        // At progress 0 the arithmetic still admits `y = 0`, whose source
        // is the last row: a line of no height that a pixel centre in the
        // texture's padding can land on exactly.
        let gone = Genie::new(0.0, GenieShape::default());
        assert_eq!(gone.source(0.0, 0.0), None);
        assert_eq!(gone.source(0.05, 0.0), None);
    }

    #[test]
    fn the_affine_transform_scales_about_the_contents_anchor() {
        let content = Rectangle::new(Point::new(10.0, 20.0), Size::new(100.0, 50.0));
        let open = Warp::Genie(Genie::new(1.0, GenieShape::default()));
        assert_eq!(
            open.affine_transform(content),
            Some(Transformation::IDENTITY)
        );
        assert_eq!(
            Warp::None.affine_transform(content),
            Some(Transformation::IDENTITY)
        );

        let half = Warp::Genie(Genie::new(
            0.5,
            GenieShape {
                anchor: Corner::BottomRight,
                ..GenieShape::default()
            },
        ));
        let transform = half.affine_transform(content).expect("still drawn");
        // The anchor corner holds still; the opposite corner travels half
        // way towards it.
        let corner = Point::new(110.0, 70.0);
        let far = Point::new(10.0, 20.0);
        assert_eq!(corner * transform, corner);
        assert_eq!(far * transform, Point::new(60.0, 45.0));

        let gone = Warp::Genie(Genie::new(0.0, GenieShape::default()));
        assert_eq!(gone.affine_transform(content), None);
    }

    #[test]
    fn a_collapsed_genie_takes_no_pointer_at_all() {
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 50.0));
        let gone = Warp::Genie(Genie::new(0.0, GenieShape::default()));
        for exact in [true, false] {
            for (x, y) in grid() {
                let point = Point::new(x * 100.0, y * 50.0);
                assert_eq!(
                    gone.map_cursor(exact, bounds, mouse::Cursor::Available(point)),
                    mouse::Cursor::Unavailable,
                    "exact {exact}: {point:?}"
                );
            }
        }
        assert_eq!(
            Warp::None.map_cursor(true, bounds, mouse::Cursor::Available(Point::ORIGIN)),
            mouse::Cursor::Available(Point::ORIGIN)
        );
    }

    #[test]
    fn the_far_edge_lingers_while_the_near_edge_travels() {
        // The wide end of the S has to stay on screen, or all that is left
        // is its concave half and the silhouette reads as an inward arch.
        // Without the per-row lag this row was down to about 0.68 by here.
        let genie = Genie::new(0.5, shape(Corner::TopLeft, 0.12));
        let (far, _) = genie
            .destination(1.0, 1.0)
            .expect("the far edge is still drawn half way through");

        assert!(far > 0.9, "far edge is {far} wide, so the S has left frame");
    }

    #[test]
    // The bound is the point: asserting it is what stops a future edit of
    // `MAX_STRETCH_POWER` from folding the map over itself silently.
    #[allow(clippy::assertions_on_constants)]
    fn the_lag_keeps_the_map_invertible() {
        // `row` folds over itself once stretch_power * stretch reaches `e`,
        // which would corrupt the image rather than fail loudly. The clamp
        // is what keeps a caller on the right side of that.
        assert!(
            MAX_STRETCH_POWER < std::f32::consts::E,
            "MAX_STRETCH_POWER {MAX_STRETCH_POWER} is past the monotonic bound"
        );

        for power in [0.0, 1.0, DEFAULT_STRETCH_POWER, MAX_STRETCH_POWER] {
            for step in 0u8..=20 {
                let genie = Genie::new(
                    f32::from(step) / 20.0,
                    GenieShape {
                        anchor: Corner::TopLeft,
                        target_width: 0.0,
                        stretch_power: power,
                        ..GenieShape::default()
                    },
                );

                let mut previous = f32::NEG_INFINITY;
                for row in 0u8..=20 {
                    let current = genie.row(f32::from(row) / 20.0);
                    assert!(current > previous, "row {row} went backwards at {genie:?}");
                    previous = current;
                }
            }
        }
    }

    #[test]
    fn the_default_bend_is_the_smoothstep_it_replaces() {
        // Generalising the side curve has to be a refactor, not a change of
        // shape: at its defaults the Bézier is the reference's own curve.
        for step in 0u8..=20 {
            let t = f32::from(step) / 20.0;
            let smoothstep = t * t * (3.0 - 2.0 * t);
            let bend = bend(t, DEFAULT_CURVE_IN, DEFAULT_CURVE_OUT);
            assert!(
                (bend - smoothstep).abs() < 1e-6,
                "at {t}: {bend} != {smoothstep}"
            );
        }
    }

    #[test]
    fn the_curve_controls_move_the_ends_they_name() {
        // `curve_in` bends the wide end sooner...
        let flat = bend(0.15, 0.0, DEFAULT_CURVE_OUT);
        let bent = bend(0.15, 0.6, DEFAULT_CURVE_OUT);
        assert!(
            bent > flat,
            "curve_in did not bend the wide end: {bent} vs {flat}"
        );

        // ...and `curve_out` bends the neck end later.
        let arrives_flat = bend(0.85, DEFAULT_CURVE_IN, 1.0);
        let arrives_late = bend(0.85, DEFAULT_CURVE_IN, 0.4);
        assert!(
            arrives_late < arrives_flat,
            "curve_out did not bend the neck end: {arrives_late} vs {arrives_flat}"
        );
    }

    #[test]
    fn the_curve_controls_keep_the_width_inside_the_rectangle() {
        // A Bézier lies inside the convex hull of its control values, so a
        // clamped pair can never widen a row past the content itself. That
        // is what the whole no-inflation design rests on.
        for control in 0u8..=10 {
            let c = f32::from(control) / 10.0;
            for step in 0u8..=20 {
                let t = f32::from(step) / 20.0;
                for (c_in, c_out) in [(c, DEFAULT_CURVE_OUT), (DEFAULT_CURVE_IN, c)] {
                    let b = bend(t, c_in, c_out);
                    assert!(
                        (0.0..=1.0).contains(&b),
                        "bend({t}, {c_in}, {c_out}) = {b} left the unit range"
                    );
                }
            }
        }
    }

    #[test]
    fn a_curve_control_outside_the_unit_range_is_clamped() {
        let over = Genie::new(
            0.5,
            GenieShape {
                curve_in: 4.0,
                curve_out: 9.0,
                ..GenieShape::default()
            },
        );
        assert!((over.shape().curve_in - 1.0).abs() < f32::EPSILON);
        assert!((over.shape().curve_out - 1.0).abs() < f32::EPSILON);

        let under = Genie::new(
            0.5,
            GenieShape {
                curve_in: -2.0,
                curve_out: -2.0,
                ..GenieShape::default()
            },
        );
        assert!(under.shape().curve_in.abs() < f32::EPSILON);
        assert!(under.shape().curve_out.abs() < f32::EPSILON);
    }

    #[test]
    fn a_stretch_power_past_the_bound_is_clamped() {
        let over = Genie::new(
            0.5,
            GenieShape {
                stretch_power: 9.0,
                ..GenieShape::default()
            },
        );
        assert!((over.shape().stretch_power - MAX_STRETCH_POWER).abs() < f32::EPSILON);

        let under = Genie::new(
            0.5,
            GenieShape {
                stretch_power: -4.0,
                ..GenieShape::default()
            },
        );
        assert!(under.shape().stretch_power.abs() < f32::EPSILON);
    }

    #[test]
    fn no_lag_moves_every_row_together() {
        // `stretch_power` of zero is the un-lagged map: one shift for all.
        let genie = Genie::new(
            0.5,
            GenieShape {
                anchor: Corner::TopLeft,
                target_width: 0.0,
                stretch_power: 0.0,
                ..GenieShape::default()
            },
        );
        let (_, squash) = genie.phases();

        for row in 0u8..=10 {
            let y = f32::from(row) / 10.0;
            assert!(
                (genie.row(y) - (y + squash)).abs() < 1e-6,
                "row {y} was lagged"
            );
        }
    }

    #[test]
    fn the_rows_converge_on_a_band_not_a_point() {
        // Fully stretched, the row at the anchor is the target width.
        let band = shape(Corner::TopLeft, 0.25);
        assert!((width(0.0, 1.0, band) - 0.25).abs() < 1e-6);
        // A target width of zero still collapses to a point.
        assert!(width(0.0, 1.0, shape(Corner::TopLeft, 0.0)).abs() < 1e-6);
        // The far row keeps its width whatever the target is.
        assert!((width(1.0, 1.0, band) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_phases_overlap_as_researched() {
        // Open: neither phase has started.
        assert_eq!(
            Genie::new(1.0, shape(Corner::TopLeft, 0.0)).phases(),
            (0.0, 0.0)
        );

        // Stretch is complete by collapse 0.4...
        let (stretch, _) = Genie::new(0.6, shape(Corner::TopLeft, 0.0)).phases();
        assert!(
            (stretch - 1.0).abs() < 1e-6,
            "stretch {stretch} != 1 at c = 0.4"
        );

        // ...and squash has not started at collapse 0.15.
        let (_, squash) = Genie::new(0.85, shape(Corner::TopLeft, 0.0)).phases();
        assert!(squash.abs() < 1e-6, "squash {squash} != 0 at c = 0.15");

        // Between the two they run together, which is the whole point.
        let (stretch, squash) = Genie::new(0.7, shape(Corner::TopLeft, 0.0)).phases();
        assert!(
            stretch > 0.0 && stretch < 1.0,
            "stretch {stretch} is not mid-flight"
        );
        assert!(squash > 0.0, "squash {squash} has not started");
    }

    #[test]
    fn a_target_width_outside_the_unit_range_is_clamped() {
        let over = Genie::new(0.5, shape(Corner::TopLeft, 3.0));
        assert!((over.target_width() - 1.0).abs() < f32::EPSILON);

        let under = Genie::new(0.5, shape(Corner::TopLeft, -1.0));
        assert!(under.target_width().abs() < f32::EPSILON);
    }

    #[test]
    fn a_non_finite_field_falls_back_to_its_default() {
        // Not the same thing as clamping. A NaN is not a value out of range,
        // it is the absence of one, and the crate's own default is a better
        // answer to that than either end of the range. Zero in particular
        // would be the wrong guess: it collapses to a point, which is the
        // failure this shape exists to avoid. A caller who wants that still
        // passes `0.0`, which is finite and survives the clamp untouched.
        let width = Genie::new(0.5, shape(Corner::TopLeft, f32::NAN));
        assert!((width.target_width() - DEFAULT_TARGET_WIDTH).abs() < f32::EPSILON);

        let power = Genie::new(
            0.5,
            GenieShape {
                stretch_power: f32::NAN,
                ..GenieShape::default()
            },
        );
        assert!((power.shape().stretch_power - DEFAULT_STRETCH_POWER).abs() < f32::EPSILON);
    }

    #[test]
    fn a_point_outside_the_collapsed_shape_is_clipped() {
        // Half collapsed, the far corner of the destination shows nothing.
        let genie = Genie::new(0.5, shape(Corner::TopLeft, 0.0));
        assert!(genie.source(0.99, 0.99).is_none());
    }

    #[test]
    fn a_collapsed_genie_shows_nothing() {
        let genie = Genie::new(0.0, shape(Corner::TopLeft, 0.0));
        for (x, y) in grid() {
            assert!(genie.source(x, y).is_none(), "({x},{y}) survived collapse");
        }
    }

    #[test]
    fn each_corner_collapses_toward_the_corner_it_names() {
        for (corner, toward) in [
            (Corner::TopLeft, (0.0, 0.0)),
            (Corner::TopRight, (1.0, 0.0)),
            (Corner::BottomLeft, (0.0, 1.0)),
            (Corner::BottomRight, (1.0, 1.0)),
        ] {
            let (x, y) = Genie::new(0.5, shape(corner, 0.0))
                .destination(0.5, 0.5)
                .expect("the centre is still drawn half way");
            let before = (0.5_f32 - toward.0).hypot(0.5 - toward.1);
            let after = (x - toward.0).hypot(y - toward.1);
            assert!(
                after < before,
                "{corner:?}: {after} is no closer than {before}"
            );
        }
    }

    #[test]
    fn the_corners_are_mirror_images() {
        // Sampled well away from the anchor: by half collapsed the squash
        // has carried the nearer rows through it, and a row that is gone
        // has no mirror to compare.
        let (x, y) = Genie::new(0.5, shape(Corner::TopLeft, 0.0))
            .destination(0.7, 0.9)
            .expect("drawn");
        let (mx, my) = Genie::new(0.5, shape(Corner::TopRight, 0.0))
            .destination(0.3, 0.9)
            .expect("drawn");
        assert!(
            (x - (1.0 - mx)).abs() < 1e-6,
            "{x} is not the mirror of {mx}"
        );
        assert!((y - my).abs() < 1e-6, "{y} != {my}");
    }

    #[test]
    fn progress_is_clamped_so_a_bouncy_curve_cannot_expand_it() {
        let over = Genie::new(1.4, shape(Corner::TopLeft, 0.0));
        assert!((over.progress() - 1.0).abs() < f32::EPSILON);
        let under = Genie::new(-0.3, shape(Corner::TopLeft, 0.0));
        assert!(under.progress().abs() < f32::EPSILON);
    }

    #[test]
    fn a_corner_radius_of_zero_masks_nothing() {
        assert!((corner_alpha(0.0, 0.0, 0.0, 0.0, 0.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_mask_only_touches_the_corners() {
        // Further than the radius from either pair of edges is nowhere near
        // a corner and must be left alone.
        assert!((corner_alpha(40.0, 40.0, 2.0, 400.0, 8.0) - 1.0).abs() < f32::EPSILON);
        assert!((corner_alpha(2.0, 400.0, 40.0, 40.0, 8.0) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_masked_corner_is_round() {
        let r = 8.0;

        // The very corner falls outside the circle entirely.
        assert!(corner_alpha(0.0, 500.0, 0.0, 500.0, r) < f32::EPSILON);
        // Its centre is well inside.
        assert!((corner_alpha(r, 500.0, r, 500.0, r) - 1.0).abs() < f32::EPSILON);
        // And a point on the arc is partly covered, which is the curve.
        let on_arc = r - r / 2.0_f32.sqrt();
        let alpha = corner_alpha(on_arc, 500.0, on_arc, 500.0, r);
        assert!((0.2..=0.8).contains(&alpha), "arc coverage was {alpha}");
    }

    #[test]
    fn the_mask_does_not_care_which_corner() {
        // Distances are to the nearest edge on each axis, so all four
        // corners are masked alike.
        let a = corner_alpha(1.0, 90.0, 2.0, 90.0, 8.0);
        let b = corner_alpha(90.0, 1.0, 90.0, 2.0, 8.0);
        assert!((a - b).abs() < f32::EPSILON, "{a} != {b}");
    }

    #[test]
    fn a_non_finite_corner_radius_is_ignored() {
        let nan = GenieShape {
            corner_radius: f32::NAN,
            ..GenieShape::default()
        };
        assert!(Genie::new(0.5, nan).corner_radius().abs() < f32::EPSILON);

        let negative = GenieShape {
            corner_radius: -4.0,
            ..GenieShape::default()
        };
        assert!(Genie::new(0.5, negative).corner_radius().abs() < f32::EPSILON);
    }

    #[test]
    fn a_non_finite_progress_is_treated_as_open() {
        assert!(
            (Genie::new(f32::NAN, shape(Corner::TopLeft, 0.0)).progress() - 1.0).abs()
                < f32::EPSILON
        );
    }

    #[test]
    fn only_a_running_genie_is_live() {
        assert!(!Warp::None.is_live());
        assert!(!Warp::Genie(Genie::new(1.0, shape(Corner::TopLeft, 0.0))).is_live());
        assert!(Warp::Genie(Genie::new(0.4, shape(Corner::TopLeft, 0.0))).is_live());
    }

    #[test]
    fn the_affine_fallback_scales_toward_the_anchor() {
        // Fully open, the fallback is the identity.
        let open = Warp::Genie(Genie::new(1.0, shape(Corner::TopLeft, 0.0)));
        assert_eq!(open.affine_fallback(), Some((1.0, Corner::TopLeft)));

        // Half collapsed, it is a half scale about the same corner.
        let half = Warp::Genie(Genie::new(0.5, shape(Corner::BottomRight, 0.0)));
        assert_eq!(half.affine_fallback(), Some((0.5, Corner::BottomRight)));

        // No warp, nothing to apply.
        assert_eq!(Warp::None.affine_fallback(), None);
    }

    #[test]
    fn a_corner_fixes_its_own_position() {
        let bounds = iced_core::Rectangle {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };

        assert_eq!(Corner::TopLeft.fixed_point(bounds), (10.0, 20.0));
        assert_eq!(Corner::TopRight.fixed_point(bounds), (110.0, 20.0));
        assert_eq!(Corner::BottomLeft.fixed_point(bounds), (10.0, 70.0));
        assert_eq!(Corner::BottomRight.fixed_point(bounds), (110.0, 70.0));
    }
}
