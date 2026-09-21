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

/// Collapse at which the stretch is complete.
const STRETCH_END: f32 = 0.5;

/// Collapse at which the squash begins. Before [`STRETCH_END`], so the two
/// overlap: run end to end they read as two animations rather than one.
const SQUASH_START: f32 = 0.4;

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

    /// Whether it is anywhere but fully open, and so has something to draw
    /// differently.
    #[must_use]
    pub fn is_live(self) -> bool {
        self.progress < 1.0
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
        self.anchor.flips()
    }

    /// Where a source point lands, or `None` once it has been drawn through
    /// the anchor. The shader never needs this — it runs
    /// [`source`](Self::source) — but the forward direction is what makes
    /// the inverse testable.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn destination(self, u: f32, v: f32) -> Option<(f32, f32)> {
        // `u`, `v`, `x`, `y`, `k`, `s` are the map's own notation, shared
        // with the spec and the shader. Renaming them here would break that
        // correspondence for no gain.
        let (flip_x, flip_y) = self.flips();
        let (u, v) = (flip(u, flip_x), flip(v, flip_y));
        let (k, s) = self.phases();

        let y = v - s;
        if y < 0.0 {
            return None;
        }

        let w = width(y, k);
        if w <= CLOSED {
            return None;
        }

        Some((flip(u * w, flip_x), flip(y, flip_y)))
    }

    /// Which source point a destination point shows, or `None` where the
    /// collapsed shape does not reach.
    #[must_use]
    #[allow(clippy::many_single_char_names)]
    pub fn source(self, x: f32, y: f32) -> Option<(f32, f32)> {
        let (flip_x, flip_y) = self.flips();
        let (x, y) = (flip(x, flip_x), flip(y, flip_y));
        let (k, s) = self.phases();

        // The width comes straight from the destination row, which is what
        // keeps the inverse closed-form.
        let w = width(y, k);
        if w <= CLOSED {
            return None;
        }

        let u = x / w;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }

        let v = y + s;
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

impl From<Genie> for Warp {
    fn from(genie: Genie) -> Self {
        Self::Genie(genie)
    }
}

/// The width of the row drawn at `y`, stretched by `k`.
///
/// `1 - y` is the row's position along the travel path, and cubing it is the
/// shape curve both reference implementations settled on. Rows near the
/// anchor are pinched; rows still far from it keep their width.
fn width(y: f32, k: f32) -> f32 {
    let along = (1.0 - y).clamp(0.0, 1.0);

    1.0 - k * along * along * along
}

/// Mirrors a normalised coordinate when the anchor is on the far side.
fn flip(value: f32, flip: bool) -> f32 {
    if flip { 1.0 - value } else { value }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every progress a test sweeps, including both ends.
    const PROGRESSES: [f32; 7] = [0.0, 0.05, 0.25, 0.5, 0.75, 0.95, 1.0];

    fn grid() -> impl Iterator<Item = (f32, f32)> {
        (0..=10).flat_map(|i| (0..=10).map(move |j| (i as f32 / 10.0, j as f32 / 10.0)))
    }

    #[test]
    fn fully_open_is_the_identity() {
        let genie = Genie::new(1.0, Corner::TopLeft);
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
            let genie = Genie::new(t, Corner::TopLeft);
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
            let genie = Genie::new(t, Corner::TopLeft);
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
        // The property the first version of this map failed, and the reason
        // it read as a squeeze rather than a suction. The row's width is the
        // destination of its far edge, `u = 1`.
        let genie = Genie::new(0.5, Corner::TopLeft);
        let (near, _) = genie.destination(1.0, 0.3).expect("still drawn");
        let (far, _) = genie.destination(1.0, 1.0).expect("still drawn");
        assert!(near < far, "row at 0.3 is {near} wide, row at 1.0 is {far}");
    }

    #[test]
    fn the_phases_overlap_as_researched() {
        // Open: neither phase has started.
        assert_eq!(Genie::new(1.0, Corner::TopLeft).phases(), (0.0, 0.0));

        // Stretch is complete by half-collapsed...
        let (stretch, _) = Genie::new(0.5, Corner::TopLeft).phases();
        assert!(
            (stretch - 1.0).abs() < 1e-6,
            "stretch {stretch} != 1 at c = 0.5"
        );

        // ...and squash has not started at c = 0.4.
        let (_, squash) = Genie::new(0.6, Corner::TopLeft).phases();
        assert!(squash.abs() < 1e-6, "squash {squash} != 0 at c = 0.4");

        // Between the two they run together, which is the whole point.
        let (stretch, squash) = Genie::new(0.55, Corner::TopLeft).phases();
        assert!(
            stretch > 0.0 && stretch < 1.0,
            "stretch {stretch} is not mid-flight"
        );
        assert!(squash > 0.0, "squash {squash} has not started");
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
        for (corner, toward) in [
            (Corner::TopLeft, (0.0, 0.0)),
            (Corner::TopRight, (1.0, 0.0)),
            (Corner::BottomLeft, (0.0, 1.0)),
            (Corner::BottomRight, (1.0, 1.0)),
        ] {
            let (x, y) = Genie::new(0.5, corner)
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
        let (x, y) = Genie::new(0.5, Corner::TopLeft)
            .destination(0.7, 0.4)
            .expect("drawn");
        let (mx, my) = Genie::new(0.5, Corner::TopRight)
            .destination(0.3, 0.4)
            .expect("drawn");
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

    #[test]
    fn the_affine_fallback_scales_toward_the_anchor() {
        // Fully open, the fallback is the identity.
        let open = Warp::Genie(Genie::new(1.0, Corner::TopLeft));
        assert_eq!(open.affine_fallback(), Some((1.0, Corner::TopLeft)));

        // Half collapsed, it is a half scale about the same corner.
        let half = Warp::Genie(Genie::new(0.5, Corner::BottomRight));
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
