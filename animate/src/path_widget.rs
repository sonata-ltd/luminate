//! A vector path whose stroke, fill and drawn range are resolved in `draw`.
//!
//! See [`path()`].

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use iced_core::widget::{Tree, tree};
use iced_core::{Color, Element, Length, Rectangle, Size, Vector};
use iced_core::{Layout, Widget, layout, mouse, renderer};
use iced_graphics::geometry::{self, Fill, Frame, Stroke, Style};

pub use iced_graphics::geometry::fill::Rule as FillRule;
pub use iced_graphics::geometry::{LineCap, LineJoin};

use crate::path::{ArcLength, DrawRange, Fit, PathData, Placement, Subpath};
use crate::shape::clamped_color;
use crate::{Anim, AnimLength, Tier};

/// Creates a widget that draws `source`, a [`PathData`].
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
    /// bounds of the path being drawn.
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

    /// `true` while any of the path's values is in motion.
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
    }

    /// Every animated value, read once for this frame.
    fn resolve(&self) -> Resolved {
        Resolved {
            range: self.draw.get().clamped(),
            stroke: self
                .stroke
                .as_ref()
                .map(|(color, width)| (clamped_color(color.get()), width.get().max(0.0))),
            fill: self.fill.as_ref().map(|fill| clamped_color(fill.get())),
        }
    }

    /// The identity of what is drawn, for the geometry cache.
    fn source_id(&self) -> usize {
        match &self.source {
            Source::Static(data) => Arc::as_ptr(data) as usize,
        }
    }

    /// Paints `shape` into `frame`. `table` measures `shape` when a partial
    /// range has to be trimmed out of it.
    fn paint<Renderer: geometry::Renderer>(
        &self,
        frame: &mut Frame<Renderer>,
        shape: &PathData,
        table: impl FnOnce() -> ArcLength,
        resolved: &Resolved,
    ) {
        let view_box = self.view_box.unwrap_or(shape.bounds());
        let placement = self.fit.placement(view_box, frame.size());
        let range = resolved.range;

        if let Some(color) = resolved.fill
            && (range.is_full() || !self.fill_follows_draw)
        {
            frame.fill(
                &to_geometry(&shape.subpaths, placement),
                Fill {
                    style: Style::Solid(color),
                    rule: self.fill_rule,
                },
            );
        }

        let Some((color, width)) = resolved.stroke else {
            return;
        };
        if width <= 0.0 || range.is_empty() {
            return;
        }

        let stroke = Stroke {
            style: Style::Solid(color),
            width,
            line_cap: self.line_cap,
            line_join: self.line_join,
            ..Stroke::default()
        };

        if range.is_full() {
            frame.stroke(&to_geometry(&shape.subpaths, placement), stroke);
            return;
        }

        let trimmed = table().trim(shape, range.start, range.end);
        if !trimmed.is_empty() {
            frame.stroke(&to_geometry(&trimmed, placement), stroke);
        }
    }
}

/// The values one frame draws with.
#[derive(Debug, Clone, Copy)]
struct Resolved {
    range: DrawRange,
    stroke: Option<(Color, f32)>,
    fill: Option<Color>,
}

/// How many `f32` values [`Resolved::bits`] packs.
const VALUES: usize = 16;

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
        ];

        let mut bits = [0; VALUES];
        for (slot, value) in bits.iter_mut().zip(values) {
            *slot = value.to_bits();
        }
        bits
    }
}

/// What the cached geometry was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    size: [u32; 2],
    source: usize,
    values: [u32; VALUES],
}

struct State<Renderer: geometry::Renderer> {
    cache: geometry::Cache<Renderer>,
    key: Cell<Option<Key>>,
    /// Keeps the drawn path alive while its address is in `key`, so the
    /// address cannot be reused by another path the cache would mistake for it.
    held: RefCell<Option<Arc<PathData>>>,
    /// The length table of the static path, reused while it is the same `Arc`.
    table: RefCell<Option<(Arc<PathData>, ArcLength)>>,
}

impl<Renderer: geometry::Renderer> State<Renderer> {
    fn new() -> Self {
        Self {
            cache: geometry::Cache::new(),
            key: Cell::new(None),
            held: RefCell::new(None),
            table: RefCell::new(None),
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

/// `subpaths` placed into widget pixels, as an iced path.
fn to_geometry(subpaths: &[Subpath], placement: Placement) -> geometry::Path {
    geometry::Path::new(|builder| {
        for subpath in subpaths {
            let Some(first) = subpath.segments.first() else {
                continue;
            };
            builder.move_to(placement.point(first.p0));
            for segment in &subpath.segments {
                builder.bezier_curve_to(
                    placement.point(segment.p1),
                    placement.point(segment.p2),
                    placement.point(segment.p3),
                );
            }
            if subpath.closed {
                builder.close();
            }
        }
    })
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for PathShape
where
    Renderer: geometry::Renderer + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State<Renderer>>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::<Renderer>::new())
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
        let resolved = self.resolve();
        let key = Key {
            size: [bounds.width.to_bits(), bounds.height.to_bits()],
            source: self.source_id(),
            values: resolved.bits(),
        };
        if state.key.replace(Some(key)) != Some(key) {
            state.cache.clear();
        }

        let Source::Static(data) = &self.source;
        *state.held.borrow_mut() = Some(Arc::clone(data));

        let geometry = state.cache.draw(renderer, bounds.size(), |frame| {
            crate::testing::note_path_geometry_build();
            self.paint(frame, data, || state.table_for(data), &resolved);
        });

        renderer.with_translation(Vector::new(bounds.x, bounds.y), |renderer| {
            renderer.draw_geometry(geometry);
        });
    }
}

impl<'a, Message, Theme, Renderer> From<PathShape> for Element<'a, Message, Theme, Renderer>
where
    Renderer: geometry::Renderer + 'static,
    Message: 'a,
    Theme: 'a,
{
    fn from(shape: PathShape) -> Self {
        Element::new(shape)
    }
}
