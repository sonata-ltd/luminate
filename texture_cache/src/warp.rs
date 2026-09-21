//! A non-affine warp applied when a cached texture is composited.
//!
//! Affine motion — translate, scale — rides on the `Transformation` the
//! compositor already applies to the destination rectangle. A warp cannot:
//! it is evaluated per destination pixel, as an *inverse* map that answers
//! "which source texel does this pixel show?". The shader runs it; this
//! module is the same arithmetic in Rust, which is what makes it testable
//! and what the software backend approximates.

/// How much of the content is drawn into the neck, as a fraction of the
/// collapsed height. Below it a row is pinched toward the anchor; above it
/// the row keeps its full width. This is the number that makes the effect
/// read as a genie rather than as a scale, and it is tuned in
/// `examples/genie.rs`.
pub(crate) const NECK: f32 = 0.6;

/// The corner a [`Genie`] collapses into.
///
/// A corner of the *content*, not a point in the window. Callers that think
/// in terms of where a popup was anchored map that onto a corner themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Corner {
    /// The top-left corner of the content.
    TopLeft,
    /// The top-right corner of the content.
    TopRight,
    /// The bottom-left corner of the content.
    BottomLeft,
    /// The bottom-right corner of the content.
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
}

/// A collapse into one corner, in the manner of a window minimising into a
/// dock icon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Genie {
    progress: f32,
    anchor: Corner,
}

impl Genie {
    /// `progress` is `1.0` fully open and `0.0` fully collapsed, clamped to
    /// that range: the map only stays inside the content's own rectangle
    /// while it is, and a curve that overshoots would otherwise tear the
    /// image out of its texture. A non-finite progress is treated as open.
    #[must_use]
    pub fn new(progress: f32, anchor: Corner) -> Self {
        let progress = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            1.0
        };

        Self { progress, anchor }
    }

    /// The clamped progress this genie will actually draw at.
    #[must_use]
    pub const fn progress(self) -> f32 {
        self.progress
    }

    /// The corner it collapses into.
    #[must_use]
    pub const fn anchor(self) -> Corner {
        self.anchor
    }

    /// The axis mirrors that put this genie's anchor at the origin.
    pub(crate) const fn flips(self) -> (bool, bool) {
        self.anchor.flips()
    }

    /// Whether it is anywhere but fully open, and so has something to draw
    /// differently.
    #[must_use]
    pub fn is_live(self) -> bool {
        self.progress < 1.0
    }

    /// Where a source point lands. The shader never needs this — it runs
    /// [`source`](Self::source) — but the forward direction is what makes
    /// the inverse testable.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn destination(self, u: f32, v: f32) -> (f32, f32) {
        let (flip_x, flip_y) = self.anchor.flips();
        let (u, v) = (flip(u, flip_x), flip(v, flip_y));

        let t = self.progress;
        let y = v * t;
        let x = u * width(v, t);

        (flip(x, flip_x), flip(y, flip_y))
    }

    /// Which source point a destination point shows, or `None` where the
    /// collapsed shape does not reach.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn source(self, x: f32, y: f32) -> Option<(f32, f32)> {
        let t = self.progress;
        if t <= 0.0 {
            return None;
        }

        let (flip_x, flip_y) = self.anchor.flips();
        let (x, y) = (flip(x, flip_x), flip(y, flip_y));

        // The row first: the width below depends on it, which is what keeps
        // the inverse closed-form.
        let v = y / t;
        if !(0.0..=1.0).contains(&v) {
            return None;
        }

        let u = x / width(v, t);
        if !(0.0..=1.0).contains(&u) {
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
    /// A collapse into one of the content's own corners.
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
}

impl From<Genie> for Warp {
    fn from(genie: Genie) -> Self {
        Self::Genie(genie)
    }
}

/// The width of source row `v` once collapsed to `t`: pinched to `t` at the
/// anchor, full past [`NECK`], swept smoothly between.
fn width(v: f32, t: f32) -> f32 {
    t + (1.0 - t) * smoothstep(v / NECK)
}

/// The standard cubic smoothstep, on an already-normalised input.
fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Mirrors a normalised coordinate when the anchor is on the far side.
fn flip(value: f32, flip: bool) -> f32 {
    if flip { 1.0 - value } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `t` a test sweeps, including both ends.
    const PROGRESSES: [f32; 7] = [0.0, 0.05, 0.25, 0.5, 0.75, 0.95, 1.0];

    fn grid() -> impl Iterator<Item = (f32, f32)> {
        (0..=10).flat_map(|i| (0..=10).map(move |j| (i as f32 / 10.0, j as f32 / 10.0)))
    }

    #[test]
    fn fully_open_is_the_identity() {
        for (x, y) in grid() {
            let genie = Genie::new(1.0, Corner::TopLeft);
            let (u, v) = genie.source(x, y).expect("nothing is clipped when open");
            assert!((u - x).abs() < 1e-6, "u {u} != x {x}");
            assert!((v - y).abs() < 1e-6, "v {v} != y {y}");
        }
    }

    #[test]
    fn the_map_round_trips() {
        for t in PROGRESSES {
            if t <= 0.0 {
                continue;
            }
            let genie = Genie::new(t, Corner::TopLeft);
            for (u, v) in grid() {
                let (x, y) = genie.destination(u, v);
                let (u2, v2) = genie
                    .source(x, y)
                    .expect("a point the forward map produced is inside the shape");
                assert!((u2 - u).abs() < 1e-4, "t {t}: u {u} -> {x},{y} -> {u2}");
                assert!((v2 - v).abs() < 1e-4, "t {t}: v {v} -> {x},{y} -> {v2}");
            }
        }
    }

    #[test]
    fn the_map_never_leaves_the_source_rectangle() {
        for t in PROGRESSES {
            let genie = Genie::new(t, Corner::TopLeft);
            for (u, v) in grid() {
                let (x, y) = genie.destination(u, v);
                assert!(
                    (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y),
                    "t {t}: ({u},{v}) left the rectangle at ({x},{y})"
                );
            }
        }
    }

    #[test]
    fn a_point_outside_the_collapsed_shape_is_clipped() {
        // Half collapsed, the far corner of the destination shows nothing.
        let genie = Genie::new(0.5, Corner::TopLeft);
        assert!(genie.source(0.99, 0.99).is_none());
    }

    #[test]
    fn a_collapsed_genie_shows_nothing() {
        let genie = Genie::new(0.0, Corner::TopLeft);
        for (x, y) in grid() {
            assert!(genie.source(x, y).is_none(), "({x},{y}) survived collapse");
        }
    }

    #[test]
    fn each_corner_collapses_toward_the_corner_it_names() {
        // The centre of the content moves toward the named corner.
        for (corner, toward) in [
            (Corner::TopLeft, (0.0, 0.0)),
            (Corner::TopRight, (1.0, 0.0)),
            (Corner::BottomLeft, (0.0, 1.0)),
            (Corner::BottomRight, (1.0, 1.0)),
        ] {
            let (x, y) = Genie::new(0.25, corner).destination(0.5, 0.5);
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
        let (x, y) = Genie::new(0.3, Corner::TopLeft).destination(0.7, 0.4);
        let (mx, my) = Genie::new(0.3, Corner::TopRight).destination(0.3, 0.4);
        assert!(
            (x - (1.0 - mx)).abs() < 1e-6,
            "{x} is not the mirror of {mx}"
        );
        assert!((y - my).abs() < 1e-6, "{y} != {my}");
    }

    #[test]
    fn progress_is_clamped_so_a_bouncy_curve_cannot_expand_it() {
        let over = Genie::new(1.4, Corner::TopLeft);
        assert!((over.progress() - 1.0).abs() < f32::EPSILON);
        let under = Genie::new(-0.3, Corner::TopLeft);
        assert!(under.progress().abs() < f32::EPSILON);
    }

    #[test]
    fn a_non_finite_progress_is_treated_as_open() {
        assert!((Genie::new(f32::NAN, Corner::TopLeft).progress() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn only_a_running_genie_is_live() {
        assert!(!Warp::None.is_live());
        assert!(!Warp::Genie(Genie::new(1.0, Corner::TopLeft)).is_live());
        assert!(Warp::Genie(Genie::new(0.4, Corner::TopLeft)).is_live());
    }
}
