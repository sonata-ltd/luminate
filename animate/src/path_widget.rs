//! A vector path whose stroke, fill and drawn range are resolved in `draw`.
//!
//! See [`path()`].

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use iced_core::time::Instant;
use iced_core::widget::{Tree, tree};
use iced_core::{
    Clipboard, Color, Element, Event, Length, Point, Rectangle, Shell, Size, Vector, window,
};
use iced_core::{Layout, Widget, layout, mouse, renderer};
use iced_graphics::geometry;
use iced_graphics::mesh;

pub use iced_graphics::geometry::fill::Rule as FillRule;
pub use iced_graphics::geometry::{LineCap, LineJoin};

use crate::SpringParams;
use crate::path::{
    ArcLength, DrawRange, Fit, Morph, MorphDriver, PathData, Perspective, Placement, Pose, Subpath,
};
use crate::path_render::{self, MeshSupport, Op, Pen};
use crate::shape::clamped_color;
use crate::{Anim, AnimLength, Tier};

/// Creates a widget that draws `source`, a [`PathData`], or a prepared
/// [`Morph`] driven by [`progress`](PathShape::progress).
///
/// The widget has no intrinsic size, like [`shape()`](crate::widget::shape):
/// give it a `width` and a `height`. It draws nothing until it has a
/// [`stroke`](PathShape::stroke) or a [`fill`](PathShape::fill).
///
/// ```ignore
/// path(&CHECK)
///     .width(24)
///     .height(24)
///     .stroke(color, 2.0)
///     .draw(m.to(key!(), SMOOTH, if done { DrawRange::FULL } else { DrawRange::EMPTY }))
/// ```
#[must_use]
pub fn path(source: impl Into<PathSource>) -> PathShape {
    PathShape::new(source.into())
}

/// What a [`PathShape`] draws.
#[derive(Debug, Clone)]
pub struct PathSource(Source);

#[derive(Debug, Clone)]
enum Source {
    Static(Arc<PathData>),
    Morph {
        morph: Arc<Morph>,
        progress: Anim<f32>,
    },
}

impl From<Arc<PathData>> for PathSource {
    fn from(data: Arc<PathData>) -> Self {
        Self(Source::Static(data))
    }
}

impl From<&Arc<PathData>> for PathSource {
    fn from(data: &Arc<PathData>) -> Self {
        Self(Source::Static(Arc::clone(data)))
    }
}

impl From<PathData> for PathSource {
    fn from(data: PathData) -> Self {
        Self(Source::Static(Arc::new(data)))
    }
}

impl From<Arc<Morph>> for PathSource {
    fn from(morph: Arc<Morph>) -> Self {
        Self(Source::Morph {
            morph,
            progress: Anim::constant(0.0),
        })
    }
}

impl From<&Arc<Morph>> for PathSource {
    fn from(morph: &Arc<Morph>) -> Self {
        Self::from(Arc::clone(morph))
    }
}

impl From<Morph> for PathSource {
    fn from(morph: Morph) -> Self {
        Self::from(Arc::new(morph))
    }
}

/// A vector path whose stroke, fill and drawn range are resolved in `draw`.
///
/// Every value is read on the frame being painted, and the tessellated
/// geometry is cached against those values: a path at rest costs what a
/// cached canvas costs, a moving one rebuilds its geometry once per frame.
///
/// The stroke width is in the widget's logical pixels whatever the
/// [`Fit`] scale, like SVG's `vector-effect: non-scaling-stroke`, so an
/// icon keeps its line weight at any size. The widget does not clip: half a
/// stroke width, and any overshoot of a morph, may paint outside its bounds.
#[derive(Debug)]
pub struct PathShape {
    source: Source,
    width: AnimLength,
    height: AnimLength,
    view_box: Option<Rectangle>,
    fit: Fit,
    stroke: Option<(Anim<Color>, Anim<f32>)>,
    line_cap: LineCap,
    line_join: LineJoin,
    fill: Option<Anim<Color>>,
    fill_rule: FillRule,
    draw: Anim<DrawRange>,
    fill_follows_draw: bool,
    pose: Option<Anim<Pose>>,
    perspective: Option<Anim<Perspective>>,
    clamp_progress: bool,
    morph_to: Option<(SpringParams, bool)>,
}

impl PathShape {
    fn new(source: PathSource) -> Self {
        Self {
            source: source.0,
            width: AnimLength::Shrink,
            height: AnimLength::Shrink,
            view_box: None,
            fit: Fit::default(),
            stroke: None,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            fill: None,
            fill_rule: FillRule::NonZero,
            draw: Anim::constant(DrawRange::FULL),
            fill_follows_draw: true,
            pose: None,
            perspective: None,
            clamp_progress: false,
            morph_to: None,
        }
    }

    /// Sets the width, which may be animated.
    #[must_use]
    pub fn width(mut self, width: impl Into<AnimLength>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height, which may be animated.
    #[must_use]
    pub fn height(mut self, height: impl Into<AnimLength>) -> Self {
        self.height = height.into();
        self
    }

    /// The part of path space fitted into the widget. Defaults to the
    /// bounds of the path being drawn — for a [`Morph`] source or a
    /// [`morph`](Self::morph) target, the union of both ends' bounds, so the
    /// box does not shift as the in-between shape grows and shrinks.
    #[must_use]
    pub fn view_box(mut self, view_box: Rectangle) -> Self {
        self.view_box = Some(view_box);
        self
    }

    /// How the view box is fitted into the widget; [`Fit::Contain`] by default.
    #[must_use]
    pub fn fit(mut self, fit: Fit) -> Self {
        self.fit = fit;
        self
    }

    /// Strokes the path in `color`, `width` logical pixels wide. Both may be
    /// animated.
    #[must_use]
    pub fn stroke(mut self, color: impl Into<Anim<Color>>, width: impl Into<Anim<f32>>) -> Self {
        self.stroke = Some((color.into(), width.into()));
        self
    }

    /// The shape of the stroke's ends; [`LineCap::Round`] by default.
    #[must_use]
    pub fn line_cap(mut self, cap: LineCap) -> Self {
        self.line_cap = cap;
        self
    }

    /// The shape of the stroke's corners; [`LineJoin::Round`] by default.
    #[must_use]
    pub fn line_join(mut self, join: LineJoin) -> Self {
        self.line_join = join;
        self
    }

    /// Fills the path in `color`, which may be animated.
    #[must_use]
    pub fn fill(mut self, color: impl Into<Anim<Color>>) -> Self {
        self.fill = Some(color.into());
        self
    }

    /// Which regions a self-crossing fill covers; [`FillRule::NonZero`] by default.
    #[must_use]
    pub fn fill_rule(mut self, rule: FillRule) -> Self {
        self.fill_rule = rule;
        self
    }

    /// The part of the stroke to draw, which may be animated: the drawable
    /// of anime.js. Clamped to `[0, 1]` where it is drawn.
    #[must_use]
    pub fn draw(mut self, range: impl Into<Anim<DrawRange>>) -> Self {
        self.draw = range.into();
        self
    }

    /// Whether the fill waits for the whole stroke to be drawn (the
    /// default) or is painted whatever the drawn range.
    #[must_use]
    pub fn fill_follows_draw(mut self, follows: bool) -> Self {
        self.fill_follows_draw = follows;
        self
    }

    /// Places the path by `pose`, which may be animated: the path's own
    /// origin goes to `pose.position` (the widget's logical pixels) and the
    /// path turns by `pose.angle` about it.
    ///
    /// A posed path ignores [`fit`](Self::fit) and
    /// [`view_box`](Self::view_box): one path unit is one pixel, so a marker
    /// drawn round its origin and an orbit drawn with [`Fit::None`] share one
    /// coordinate system. Derive the pose from a progress with
    /// [`MotionPath::pose_at`](crate::path::MotionPath::pose_at).
    #[must_use]
    pub fn pose(mut self, pose: impl Into<Anim<Pose>>) -> Self {
        self.pose = Some(pose.into());
        self
    }

    /// Projects the drawing through `perspective`, which may be animated:
    /// the plane tilts about the widget's centre and is seen from
    /// [`Perspective::distance`](crate::path::Perspective::distance).
    ///
    /// It applies last, after [`fit`](Self::fit) and [`pose`](Self::pose),
    /// so layers stacked in one box with one shared perspective tilt as one
    /// scene. Stroke widths are not foreshortened. Curves are projected by
    /// their control points after splitting each segment in four, which is
    /// exact for a flat plane and close for the tilts a scene uses.
    #[must_use]
    pub fn perspective(mut self, perspective: impl Into<Anim<Perspective>>) -> Self {
        self.perspective = Some(perspective.into());
        self
    }

    /// How far a [`Morph`] source has got, which may be animated: `0` draws
    /// its `from`, `1` its `to`. Values outside `[0, 1]` extrapolate unless
    /// [`clamp_progress`](Self::clamp_progress) is set. Ignored for a plain
    /// path.
    #[must_use]
    pub fn progress(mut self, progress: impl Into<Anim<f32>>) -> Self {
        if let Source::Morph { progress: slot, .. } = &mut self.source {
            *slot = progress.into();
        }
        self
    }

    /// Clamps a morph's progress to `[0, 1]`, so an overshooting spring
    /// stops at the end shapes instead of exaggerating them.
    #[must_use]
    pub fn clamp_progress(mut self, clamp: bool) -> Self {
        self.clamp_progress = clamp;
        self
    }

    /// Morphs to this path whenever it changes, with a spring of `params`.
    ///
    /// The widget remembers the path it last drew. When a rebuild hands it
    /// a different one (compared by `Arc`, then by value) it morphs from the
    /// shape on screen — mid-flight included — to the new one, and asks for
    /// frames until it arrives. The first path a widget is built with is
    /// drawn as it is. Only for a plain path source.
    #[must_use]
    pub fn morph(mut self, params: SpringParams) -> Self {
        let carry = self.morph_to.is_some_and(|(_, carry)| carry);
        self.morph_to = Some((params, carry));
        self
    }

    /// Whether a retarget mid-morph keeps the shape's points moving at the
    /// speed they had (`true`) or starts the new morph from rest (`false`,
    /// the default). Only has an effect together with [`morph`](Self::morph).
    ///
    /// Calling this without [`morph`](Self::morph) turns morphing on with
    /// [`SpringParams::default`].
    #[must_use]
    pub fn carry_velocity(mut self, carry: bool) -> Self {
        let params = self
            .morph_to
            .map_or(SpringParams::default(), |(params, _)| params);
        self.morph_to = Some((params, carry));
        self
    }

    /// `true` while any of the path's values is in motion.
    ///
    /// A [`morph`](Self::morph) retarget in flight is invisible to this: the
    /// spring driving it lives in the widget's own tree state, not in any
    /// `Anim` this struct holds, so a settled view built with a fresh target
    /// still reports `false` here even though the widget keeps asking for
    /// frames until it arrives.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.width.is_animating()
            || self.height.is_animating()
            || self.draw.is_animating()
            || self.fill.as_ref().is_some_and(Anim::is_animating)
            || self
                .stroke
                .as_ref()
                .is_some_and(|(color, width)| color.is_animating() || width.is_animating())
            || self.pose.as_ref().is_some_and(Anim::is_animating)
            || self.perspective.as_ref().is_some_and(Anim::is_animating)
            || matches!(&self.source, Source::Morph { progress, .. } if progress.is_animating())
    }

    fn mark_tiers(&self) {
        self.width.mark_layout_tier();
        self.height.mark_layout_tier();

        self.draw.mark_tier(Tier::Paint);
        if let Some(fill) = &self.fill {
            fill.mark_tier(Tier::Paint);
        }
        if let Some((color, width)) = &self.stroke {
            color.mark_tier(Tier::Paint);
            width.mark_tier(Tier::Paint);
        }
        if let Some(pose) = &self.pose {
            pose.mark_tier(Tier::Paint);
        }
        if let Some(perspective) = &self.perspective {
            perspective.mark_tier(Tier::Paint);
        }
        if let Source::Morph { progress, .. } = &self.source {
            progress.mark_tier(Tier::Paint);
        }
    }

    /// Every animated value, read once for this frame. `driver` is the
    /// widget-owned morph, when the source follows a target rather than a
    /// fixed [`Morph`].
    fn resolve(&self, driver: Option<&MorphDriver>) -> Resolved {
        let progress = match &self.source {
            Source::Morph { progress, .. } => {
                let t = progress.get();
                if self.clamp_progress {
                    t.clamp(0.0, 1.0)
                } else {
                    t
                }
            }
            Source::Static(_) => 0.0,
        };

        Resolved {
            range: self.draw.get(),
            stroke: self
                .stroke
                .as_ref()
                .map(|(color, width)| (clamped_color(color.get()), width.get().max(0.0))),
            fill: self.fill.as_ref().map(|fill| clamped_color(fill.get())),
            pose: self.pose.as_ref().map(Anim::get),
            perspective: self
                .perspective
                .as_ref()
                .map(Anim::get)
                .filter(|perspective| !perspective.is_flat()),
            progress,
            generation: driver.map_or(0, MorphDriver::generation),
        }
    }

    /// What the cached geometry of a frame of `size` drawn with `resolved`
    /// depends on.
    fn key(&self, size: Size, resolved: &Resolved) -> Key {
        Key {
            size: [size.width.to_bits(), size.height.to_bits()],
            source: self.source_id(),
            values: resolved.bits(),
            generation: resolved.generation,
            view_box: self.view_box.map(|r| {
                [
                    r.x.to_bits(),
                    r.y.to_bits(),
                    r.width.to_bits(),
                    r.height.to_bits(),
                ]
            }),
            fit: self.fit,
            line_cap: line_cap_key(self.line_cap),
            line_join: line_join_key(self.line_join),
            fill_rule: self.fill_rule,
            fill_follows_draw: self.fill_follows_draw,
        }
    }

    /// The identity of what is drawn, for the geometry cache.
    fn source_id(&self) -> usize {
        match &self.source {
            Source::Static(data) => Arc::as_ptr(data) as usize,
            Source::Morph { morph, .. } => Arc::as_ptr(morph) as usize,
        }
    }

    /// The driver a fresh state should start with: `Some` only when a plain
    /// path source asks to be morphed to.
    fn initial_driver(&self) -> Option<MorphDriver> {
        match (&self.source, self.morph_to) {
            (Source::Static(target), Some((params, carry))) => {
                Some(MorphDriver::new(Arc::clone(target), params, carry))
            }
            _ => None,
        }
    }

    /// What to draw for `shape` in a widget of `size`, in the widget's flat
    /// pixels: fit and pose applied, perspective left to whoever carries the
    /// plan out. `table` measures `shape` when a partial range has to be
    /// trimmed out of it. `default_view_box` is used when no
    /// [`view_box`](Self::view_box) was set: the caller works it out from the
    /// source rather than `shape.bounds()`, because a morph's in-between
    /// shape shrinks and grows every frame and would otherwise re-fit the
    /// view box to it on every frame too.
    fn plan(
        &self,
        size: Size,
        shape: &PathData,
        table: impl FnOnce() -> ArcLength,
        default_view_box: Rectangle,
        resolved: &Resolved,
    ) -> Vec<Op> {
        let placement = if resolved.pose.is_some() {
            Placement::IDENTITY
        } else {
            let view_box = self.view_box.unwrap_or(default_view_box);
            self.fit.placement(view_box, size)
        };
        let pose = resolved.pose.map(|pose| (pose, pose.angle.0.sin_cos()));
        // Fit and pose are affine, so mapping the control points is exact.
        let flat = |p: Point| {
            let q = placement.point(p);
            match pose {
                Some((pose, (sin, cos))) => Point::new(
                    pose.position.x + q.x * cos - q.y * sin,
                    pose.position.y + q.x * sin + q.y * cos,
                ),
                None => q,
            }
        };
        let place = |subpaths: &[Subpath]| -> Vec<Subpath> {
            subpaths
                .iter()
                .map(|subpath| Subpath {
                    segments: subpath.segments.iter().map(|c| c.map(flat)).collect(),
                    closed: subpath.closed,
                })
                .collect()
        };

        // A range reaching back before zero on a single closed loop wraps
        // across the loop's start instead of being clamped there. An end of
        // exactly zero still wraps: a trail whose head lands on the loop's
        // start keeps its tail rather than blinking out for that frame.
        let raw = resolved.range;
        let wraps = shape.is_closed_loop()
            && raw.start < 0.0
            && raw.end >= 0.0
            && raw.end - raw.start < 1.0;
        let range = raw.clamped();
        let (start, end) = if wraps {
            (raw.start, raw.end)
        } else {
            (range.start, range.end)
        };
        let full = !wraps && range.is_full();
        let mut ops = Vec::new();

        if let Some(color) = resolved.fill
            && (full || !self.fill_follows_draw)
        {
            ops.push(Op::Fill {
                subpaths: place(&shape.subpaths),
                color,
                rule: self.fill_rule,
            });
        }

        let Some((color, width)) = resolved.stroke else {
            return ops;
        };
        if width <= 0.0 || end <= start {
            return ops;
        }

        let pen = Pen {
            color,
            width,
            cap: self.line_cap,
            join: self.line_join,
        };

        if full {
            ops.push(Op::Stroke {
                subpaths: place(&shape.subpaths),
                pen,
            });
            return ops;
        }

        plan_trimmed(
            &mut ops,
            shape,
            &table(),
            (start, end),
            pen,
            placement,
            &place,
        );
        ops
    }
}

/// The part of [`PathShape::plan`] for a partial range: the runs of
/// `shape` between `start` and `end`, stroked with `pen`.
fn plan_trimmed(
    ops: &mut Vec<Op>,
    shape: &PathData,
    table: &ArcLength,
    (start, end): (f32, f32),
    pen: Pen,
    placement: Placement,
    place: &dyn Fn(&[Subpath]) -> Vec<Subpath>,
) {
    let pieces = table.trim_around(shape, start, end);
    if pieces.is_empty() {
        return;
    }
    let (color, width) = (pen.color, pen.width);

    // A range shorter than the stroke is wide leaves only the cap: a dot
    // that would vanish in one frame when the range hits zero. Below one
    // stroke width of drawn length the stroke thins to that length and
    // fades in proportion, so drawing off (and on) ends at nothing.
    let scale = f32::midpoint(placement.scale.x.abs(), placement.scale.y.abs());
    let drawn = table.length() * (end - start) * scale;
    let pen = if drawn < width {
        Pen {
            color: Color {
                a: color.a * (drawn / width),
                ..color
            },
            width: drawn,
            ..pen
        }
    } else {
        pen
    };

    let mut open = Vec::with_capacity(pieces.len());
    for piece in pieces {
        // The two ends of a run of a closed loop face each other across
        // the part left out. Once that gap is narrower than the stroke,
        // full caps would lap over each other (and blend twice where a
        // renderer tessellates). Instead the ends are cut square and
        // capped with bumps of half the gap each, which meet exactly as
        // the gap closes and hand over to the closed loop without a jump.
        match piece.gap.map(|gap| gap * scale) {
            Some(gap) if gap < pen.width && !matches!(pen.cap, LineCap::Butt) => {
                let run = place(std::slice::from_ref(&piece.subpath));
                let caps = run
                    .iter()
                    .flat_map(path_render::ends)
                    .map(|(at, outward)| {
                        path_render::cap(pen.cap, at, outward, pen.width / 2.0, gap / 2.0)
                    })
                    .collect();
                ops.push(Op::Stroke {
                    subpaths: run,
                    pen: Pen {
                        cap: LineCap::Butt,
                        ..pen
                    },
                });
                ops.push(Op::Fill {
                    subpaths: caps,
                    color: pen.color,
                    rule: FillRule::NonZero,
                });
            }
            _ => open.push(piece.subpath),
        }
    }

    if !open.is_empty() {
        ops.push(Op::Stroke {
            subpaths: place(&open),
            pen,
        });
    }
}

/// The values one frame draws with.
#[derive(Debug, Clone, Copy)]
struct Resolved {
    range: DrawRange,
    stroke: Option<(Color, f32)>,
    fill: Option<Color>,
    pose: Option<Pose>,
    perspective: Option<Perspective>,
    /// A morph's progress: `0` at `from`, `1` at `to`. `0` for a plain path.
    progress: f32,
    /// The driver's generation, for the geometry cache key; `0` without one.
    generation: u64,
}

/// How many `f32` values [`Resolved::bits`] packs.
const VALUES: usize = 20;

impl Resolved {
    fn bits(&self) -> [u32; VALUES] {
        let (stroke, width) = self.stroke.unwrap_or((Color::TRANSPARENT, -1.0));
        let fill = self.fill.unwrap_or(Color::TRANSPARENT);
        let values = [
            self.range.start,
            self.range.end,
            stroke.r,
            stroke.g,
            stroke.b,
            stroke.a,
            width,
            fill.r,
            fill.g,
            fill.b,
            fill.a,
            if self.fill.is_some() { 1.0 } else { 0.0 },
            self.pose.map_or(0.0, |p| p.position.x),
            self.pose.map_or(0.0, |p| p.position.y),
            self.pose.map_or(f32::NAN, |p| p.angle.0),
            self.progress,
            self.perspective.map_or(0.0, |p| p.tilt_x.0),
            self.perspective.map_or(0.0, |p| p.tilt_y.0),
            self.perspective.map_or(f32::NAN, |p| p.distance),
        ];

        let mut bits = [0; VALUES];
        for (slot, value) in bits.iter_mut().zip(values) {
            *slot = value.to_bits();
        }
        bits
    }
}

impl Resolved {
    /// [`bits`](Self::bits) with the perspective left out: what the flat
    /// triangles of the mesh path depend on.
    #[cfg(feature = "wgpu")]
    fn flat_bits(&self) -> [u32; VALUES] {
        Self {
            perspective: None,
            ..*self
        }
        .bits()
    }
}

/// What the cached geometry was built from.
///
/// Besides the resolved animated values, this must cover every non-animated
/// setting `paint` reads: `view_box`, `fit`, the line cap and join, the fill
/// rule and whether the fill follows the drawn range. Leaving one out means
/// a rebuild that only changes it would draw the old geometry unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    size: [u32; 2],
    source: usize,
    values: [u32; VALUES],
    /// The driver's generation: a morph mid-flight changes shape every frame
    /// without any of `values` moving, since progress lives in the spring,
    /// not in a value the view rebuilds with.
    generation: u64,
    /// `view_box`'s bits, or `None` for the source's own bounds. `f32` is not
    /// `Eq`, so the rectangle is packed the way `Resolved::bits` packs colours.
    view_box: Option<[u32; 4]>,
    fit: Fit,
    /// `LineCap` has no `PartialEq`, so its variant is packed as a small
    /// discriminant instead.
    line_cap: u8,
    /// `LineJoin` has no `PartialEq`; see `line_cap`.
    line_join: u8,
    fill_rule: FillRule,
    fill_follows_draw: bool,
}

/// A `LineCap`'s variant, since the type itself has no `PartialEq`.
fn line_cap_key(cap: LineCap) -> u8 {
    match cap {
        LineCap::Butt => 0,
        LineCap::Square => 1,
        LineCap::Round => 2,
    }
}

/// A `LineJoin`'s variant, since the type itself has no `PartialEq`.
fn line_join_key(join: LineJoin) -> u8 {
    match join {
        LineJoin::Miter => 0,
        LineJoin::Round => 1,
        LineJoin::Bevel => 2,
    }
}

struct State<Renderer: geometry::Renderer> {
    cache: geometry::Cache<Renderer>,
    key: Cell<Option<Key>>,
    /// Keeps the drawn path alive while its address is in `key`, so the
    /// address cannot be reused by another path the cache would mistake for it.
    held: RefCell<Option<Arc<PathData>>>,
    /// Keeps a morph source alive while its address is in `key`.
    held_morph: RefCell<Option<Arc<Morph>>>,
    /// The length table of the static path, reused while it is the same `Arc`.
    table: RefCell<Option<(Arc<PathData>, ArcLength)>>,
    /// The high-level morph, when the widget follows a target.
    driver: RefCell<Option<MorphDriver>>,
    /// The frame the driver was last ticked on; `None` at rest, so the
    /// stretch before a retarget is not charged to the morph.
    last_frame: Cell<Option<Instant>>,
    /// The flat triangles of the last plan drawn in perspective on a
    /// renderer that draws meshes, and the key they were built for.
    #[cfg(feature = "wgpu")]
    mesh: RefCell<Option<(Key, mesh::Indexed<mesh::SolidVertex2D>)>>,
}

impl<Renderer: geometry::Renderer> State<Renderer> {
    fn new(driver: Option<MorphDriver>) -> Self {
        Self {
            cache: geometry::Cache::new(),
            key: Cell::new(None),
            held: RefCell::new(None),
            held_morph: RefCell::new(None),
            table: RefCell::new(None),
            driver: RefCell::new(driver),
            last_frame: Cell::new(None),
            #[cfg(feature = "wgpu")]
            mesh: RefCell::new(None),
        }
    }

    fn table_for(&self, data: &Arc<PathData>) -> ArcLength {
        let mut table = self.table.borrow_mut();
        if !table
            .as_ref()
            .is_some_and(|(held, _)| Arc::ptr_eq(held, data))
        {
            *table = Some((Arc::clone(data), ArcLength::new(data)));
        }
        table
            .as_ref()
            .map(|(_, table)| table.clone())
            .expect("filled above")
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for PathShape
where
    Renderer: geometry::Renderer + mesh::Renderer + MeshSupport + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer>::new(self.initial_driver()))
    }

    fn diff(&self, tree: &mut Tree) {
        let state = tree.state.downcast_mut::<State<Renderer>>();
        let driver = state.driver.get_mut();

        match (&self.source, self.morph_to) {
            (Source::Static(target), Some((params, carry))) => match driver {
                Some(driver) => {
                    driver.set_tuning(params, carry);
                    driver.retarget(Arc::clone(target));
                }
                None => *driver = Some(MorphDriver::new(Arc::clone(target), params, carry)),
            },
            _ => *driver = None,
        }
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width.resolve(), self.height.resolve())
    }

    fn size_hint(&self) -> Size<Length> {
        Size::new(self.width.size_hint(), self.height.size_hint())
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.mark_tiers();

        layout::atomic(limits, self.width.resolve(), self.height.resolve())
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return;
        };
        let state = tree.state.downcast_mut::<State<Renderer>>();
        let Some(driver) = state.driver.get_mut() else {
            return;
        };

        if driver.is_settled() {
            state.last_frame.set(None);
            return;
        }

        let dt = state.last_frame.replace(Some(*now)).map_or(0.0, |last| {
            now.saturating_duration_since(last)
                .as_secs_f32()
                .min(crate::engine::MAX_FRAME)
        });
        driver.tick(dt);
        shell.request_redraw();
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.width < 1.0 || bounds.height < 1.0 || bounds.intersection(viewport).is_none() {
            return;
        }

        let state = tree.state.downcast_ref::<State<Renderer>>();
        let driver = state.driver.borrow();
        let resolved = self.resolve(driver.as_ref());
        let key = self.key(bounds.size(), &resolved);
        let size = bounds.size();
        let projector = resolved
            .perspective
            .map(|p| p.projector(Point::new(size.width / 2.0, size.height / 2.0)));

        // The plan is only worked out when a cache misses.
        let plan = || match (&self.source, driver.as_ref()) {
            (_, Some(driver)) => {
                let shape = driver.shape();
                self.plan(
                    size,
                    &shape,
                    || ArcLength::new(&shape),
                    driver.view_box(),
                    &resolved,
                )
            }
            (Source::Static(data), None) => {
                *state.held.borrow_mut() = Some(Arc::clone(data));
                self.plan(
                    size,
                    data,
                    || state.table_for(data),
                    data.bounds(),
                    &resolved,
                )
            }
            (Source::Morph { morph, .. }, None) => {
                *state.held_morph.borrow_mut() = Some(Arc::clone(morph));
                let shape = morph.at(resolved.progress);
                self.plan(
                    size,
                    &shape,
                    || ArcLength::new(&shape),
                    morph.bounds(),
                    &resolved,
                )
            }
        };

        // In perspective on a renderer that draws meshes: triangles
        // tessellated once in the flat, keyed without the perspective, and
        // only their corners projected on each frame the sway moves.
        #[cfg(feature = "wgpu")]
        if let Some(projector) = projector.as_ref()
            && renderer.draws_meshes()
        {
            let flat_key = Key {
                values: resolved.flat_bits(),
                ..key
            };
            let mut mesh = state.mesh.borrow_mut();
            if mesh.as_ref().map(|(key, _)| *key) != Some(flat_key) {
                crate::testing::note_path_geometry_build();
                *mesh = Some((flat_key, path_render::mesh::tessellate(&plan())));
            }
            if let Some((_, flat)) = mesh.as_ref()
                && let Some(projected) = path_render::mesh::project(flat, projector, size)
            {
                renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
                    renderer.draw_mesh(projected);
                });
            }
            return;
        }

        if state.key.replace(Some(key)) != Some(key) {
            state.cache.clear();
        }

        // The near side of a tilted plane grows past the widget's box; give
        // a projected frame a margin of the box's own size to grow into.
        let frame_bounds = if projector.is_some() {
            Rectangle::new(
                Point::new(-size.width, -size.height),
                Size::new(size.width * 3.0, size.height * 3.0),
            )
        } else {
            Rectangle::with_size(size)
        };
        let geometry = state
            .cache
            .draw_with_bounds(renderer, frame_bounds, |frame| {
                crate::testing::note_path_geometry_build();
                path_render::draw_geometry(frame, &plan(), projector.as_ref());
            });

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }
}

impl<'a, Message, Theme, Renderer> From<PathShape> for Element<'a, Message, Theme, Renderer>
where
    Renderer: geometry::Renderer + mesh::Renderer + MeshSupport + 'static,
    Message: 'a,
    Theme: 'a,
{
    fn from(shape: PathShape) -> Self {
        Element::new(shape)
    }
}
