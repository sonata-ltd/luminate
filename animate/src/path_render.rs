//! How a path widget's frame is drawn: a plan in the widget's own flat
//! pixels, carried out either as iced geometry (projected on the CPU, every
//! frame the plan or the perspective changes) or as a triangle mesh that is
//! tessellated once in the flat and only has its vertices projected per frame.
//!
//! The mesh path is what makes a scene sway in perspective cheaply: lyon's
//! tessellators are the expensive part of drawing a path, and a projection
//! maps straight lines to straight lines, so the flat triangles stay valid
//! triangles once their corners are projected. Only a renderer that draws
//! meshes takes it — iced's software renderer ignores meshes — so the
//! renderer says so through [`MeshSupport`].

use iced_core::{Color, Point, Vector};
use iced_graphics::geometry::{self, Fill, Frame, LineCap, LineJoin, Stroke, Style};

pub(crate) use iced_graphics::geometry::fill::Rule as FillRule;

use crate::path::{Cubic, Projector, Subpath};

/// Whether a renderer draws triangle meshes, and so can take a path
/// widget's cheap perspective path.
///
/// Implemented for iced's renderers (behind this crate's `wgpu` and
/// `tiny-skia` features, which name the backend crates) and for iced's
/// fallback pair of them, which answers for whichever half is active. A
/// renderer crate wrapping its own backends implements it for them; see
/// `iced_texture_cache`.
pub trait MeshSupport {
    /// `true` when meshes handed to `mesh::Renderer::draw_mesh` are drawn.
    fn draws_meshes(&self) -> bool;
}

impl<A: MeshSupport, B: MeshSupport> MeshSupport for iced_renderer::fallback::Renderer<A, B> {
    fn draws_meshes(&self) -> bool {
        match self {
            Self::Primary(renderer) => renderer.draws_meshes(),
            Self::Secondary(renderer) => renderer.draws_meshes(),
        }
    }
}

#[cfg(feature = "wgpu")]
impl MeshSupport for iced_wgpu::Renderer {
    fn draws_meshes(&self) -> bool {
        true
    }
}

#[cfg(feature = "tiny-skia")]
impl MeshSupport for iced_tiny_skia::Renderer {
    fn draws_meshes(&self) -> bool {
        // `iced_tiny_skia` implements `mesh::Renderer` as a no-op.
        false
    }
}

/// A stroke's paint and shape.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pen {
    pub(crate) color: Color,
    pub(crate) width: f32,
    pub(crate) cap: LineCap,
    pub(crate) join: LineJoin,
}

/// One thing to draw, in the widget's flat pixels (fit and pose applied,
/// perspective not yet).
#[derive(Debug, Clone)]
pub(crate) enum Op {
    Fill {
        subpaths: Vec<Subpath>,
        color: Color,
        rule: FillRule,
    },
    Stroke {
        subpaths: Vec<Subpath>,
        pen: Pen,
    },
}

/// Carries `ops` out into `frame` as iced geometry, projecting every point
/// through `projector` when there is one.
pub(crate) fn draw_geometry<Renderer: geometry::Renderer>(
    frame: &mut Frame<Renderer>,
    ops: &[Op],
    projector: Option<&Projector>,
) {
    for op in ops {
        match op {
            Op::Fill {
                subpaths,
                color,
                rule,
            } => frame.fill(
                &to_geometry(subpaths, projector),
                Fill {
                    style: Style::Solid(*color),
                    rule: *rule,
                },
            ),
            Op::Stroke { subpaths, pen } => frame.stroke(
                &to_geometry(subpaths, projector),
                Stroke {
                    style: Style::Solid(pen.color),
                    width: pen.width,
                    line_cap: pen.cap,
                    line_join: pen.join,
                    ..Stroke::default()
                },
            ),
        }
    }
}

/// `subpaths` as an iced path, projected through `projector` if given.
///
/// Straight segments go in as lines: a projection keeps them straight, so
/// they need no splitting, and a line costs the tessellator nothing to
/// flatten. A curve is split in quarters before projecting, since a
/// projected cubic is no longer a cubic but a quarter of one is close.
fn to_geometry(subpaths: &[Subpath], projector: Option<&Projector>) -> geometry::Path {
    let map = |p: Point| projector.map_or(p, |projector| projector.project(p));

    geometry::Path::new(|builder| {
        for subpath in subpaths {
            let Some(first) = subpath.segments.first() else {
                continue;
            };
            builder.move_to(map(first.p0));
            for segment in &subpath.segments {
                if segment.is_line() {
                    builder.line_to(map(segment.p3));
                } else if projector.is_some() {
                    for part in segment.quarters() {
                        builder.bezier_curve_to(map(part.p1), map(part.p2), map(part.p3));
                    }
                } else {
                    builder.bezier_curve_to(segment.p1, segment.p2, segment.p3);
                }
            }
            if subpath.closed {
                builder.close();
            }
        }
    })
}

/// The two ends of an open run, each with the unit direction pointing away
/// from the run.
pub(crate) fn ends(subpath: &Subpath) -> [(Point, Vector); 2] {
    let unit = |v: Vector| {
        let length = (v.x * v.x + v.y * v.y).sqrt();
        (length > 1e-6).then(|| Vector::new(v.x / length, v.y / length))
    };

    let first = subpath.segments[0];
    let last = subpath.segments[subpath.segments.len() - 1];
    let into = unit(first.derivative(0.0))
        .or_else(|| unit(first.p3 - first.p0))
        .unwrap_or(Vector::new(1.0, 0.0));
    let out = unit(last.derivative(1.0))
        .or_else(|| unit(last.p3 - last.p0))
        .unwrap_or(Vector::new(1.0, 0.0));

    [(first.p0, Vector::new(-into.x, -into.y)), (last.p3, out)]
}

/// A cap reaching `depth` beyond a square-cut end at `at`, `half` wide on
/// each side, as a closed subpath to fill. At `depth == half` a round cap is
/// the stroke's own semicircle; shallower, it flattens into a half-ellipse,
/// down to nothing at zero.
pub(crate) fn cap(kind: LineCap, at: Point, outward: Vector, half: f32, depth: f32) -> Subpath {
    // Bézier handle length for a quarter of an ellipse.
    const K: f32 = 0.552_284_8;
    let across = Vector::new(-outward.y, outward.x);
    let a = at + across * half;
    let c = at - across * half;
    let tip = at + outward * depth;

    let segments = if let LineCap::Square = kind {
        vec![
            Cubic::line(a, a + outward * depth),
            Cubic::line(a + outward * depth, c + outward * depth),
            Cubic::line(c + outward * depth, c),
            Cubic::line(c, a),
        ]
    } else {
        vec![
            Cubic::new(a, a + outward * (K * depth), tip + across * (K * half), tip),
            Cubic::new(tip, tip - across * (K * half), c + outward * (K * depth), c),
            Cubic::line(c, a),
        ]
    };

    Subpath {
        segments,
        closed: true,
    }
}

/// The mesh path: tessellation in the flat, projection per frame.
#[cfg(feature = "wgpu")]
pub(crate) mod mesh {
    use iced_core::{Point, Rectangle, Size, Transformation};
    use iced_graphics::color;
    use iced_graphics::mesh::{Indexed, Mesh, SolidVertex2D};
    use lyon_tessellation::path::Path as LyonPath;
    use lyon_tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, FillVertex, FillVertexConstructor,
        StrokeOptions, StrokeTessellator, StrokeVertex, StrokeVertexConstructor, VertexBuffers,
    };

    use super::{FillRule, LineCap, LineJoin, Op};
    use crate::path::{Projector, Subpath};

    /// `ops` as triangles in the flat, in drawing order, every vertex
    /// carrying its op's colour. Built once and reused while the plan does
    /// not change, however the perspective moves.
    pub(crate) fn tessellate(ops: &[Op]) -> Indexed<SolidVertex2D> {
        let mut buffers: VertexBuffers<SolidVertex2D, u32> = VertexBuffers::new();
        let mut fill = FillTessellator::new();
        let mut stroke = StrokeTessellator::new();

        for op in ops {
            match op {
                Op::Fill {
                    subpaths,
                    color,
                    rule,
                } => {
                    let options = FillOptions::default().with_fill_rule(match rule {
                        FillRule::NonZero => lyon_tessellation::FillRule::NonZero,
                        FillRule::EvenOdd => lyon_tessellation::FillRule::EvenOdd,
                    });
                    let colour = Colour(color::pack(*color));
                    // A path that does not tessellate draws nothing, as it
                    // would through iced's own frame.
                    let _ = fill.tessellate_path(
                        &to_lyon(subpaths),
                        &options,
                        &mut BuffersBuilder::new(&mut buffers, colour),
                    );
                }
                Op::Stroke { subpaths, pen } => {
                    let mut options = StrokeOptions::default();
                    options.line_width = pen.width;
                    options.start_cap = cap(pen.cap);
                    options.end_cap = cap(pen.cap);
                    options.line_join = join(pen.join);
                    let colour = Colour(color::pack(pen.color));
                    let _ = stroke.tessellate_path(
                        &to_lyon(subpaths),
                        &options,
                        &mut BuffersBuilder::new(&mut buffers, colour),
                    );
                }
            }
        }

        Indexed {
            vertices: buffers.vertices,
            indices: buffers.indices,
        }
    }

    /// The flat mesh with every vertex projected, ready to draw at the
    /// widget's origin. `None` for a mesh with nothing in it, which iced
    /// asks never to be drawn.
    pub(crate) fn project(
        flat: &Indexed<SolidVertex2D>,
        projector: &Projector,
        size: Size,
    ) -> Option<Mesh> {
        if flat.indices.is_empty() {
            return None;
        }

        let vertices = flat
            .vertices
            .iter()
            .map(|vertex| {
                let [x, y] = vertex.position;
                let p = projector.project(Point::new(x, y));
                SolidVertex2D {
                    position: [p.x, p.y],
                    color: vertex.color,
                }
            })
            .collect();

        // The near side of a tilted plane grows past the widget's box; a
        // margin of the box's own size on every side keeps it unclipped.
        let clip_bounds = Rectangle::new(
            Point::new(-size.width, -size.height),
            Size::new(size.width * 3.0, size.height * 3.0),
        );

        Some(Mesh::Solid {
            buffers: Indexed {
                vertices,
                indices: flat.indices.clone(),
            },
            transformation: Transformation::IDENTITY,
            clip_bounds,
        })
    }

    fn to_lyon(subpaths: &[Subpath]) -> LyonPath {
        let point = |p: Point| lyon_tessellation::math::point(p.x, p.y);
        let mut builder = LyonPath::builder();
        for subpath in subpaths {
            let Some(first) = subpath.segments.first() else {
                continue;
            };
            builder.begin(point(first.p0));
            for segment in &subpath.segments {
                if segment.is_line() {
                    builder.line_to(point(segment.p3));
                } else {
                    builder.cubic_bezier_to(
                        point(segment.p1),
                        point(segment.p2),
                        point(segment.p3),
                    );
                }
            }
            builder.end(subpath.closed);
        }
        builder.build()
    }

    fn cap(cap: LineCap) -> lyon_tessellation::LineCap {
        match cap {
            LineCap::Butt => lyon_tessellation::LineCap::Butt,
            LineCap::Square => lyon_tessellation::LineCap::Square,
            LineCap::Round => lyon_tessellation::LineCap::Round,
        }
    }

    fn join(join: LineJoin) -> lyon_tessellation::LineJoin {
        match join {
            LineJoin::Miter => lyon_tessellation::LineJoin::Miter,
            LineJoin::Round => lyon_tessellation::LineJoin::Round,
            LineJoin::Bevel => lyon_tessellation::LineJoin::Bevel,
        }
    }

    /// Gives every vertex of one op the op's colour.
    #[derive(Clone, Copy)]
    struct Colour(color::Packed);

    impl FillVertexConstructor<SolidVertex2D> for Colour {
        fn new_vertex(&mut self, vertex: FillVertex<'_>) -> SolidVertex2D {
            let p = vertex.position();
            SolidVertex2D {
                position: [p.x, p.y],
                color: self.0,
            }
        }
    }

    impl StrokeVertexConstructor<SolidVertex2D> for Colour {
        fn new_vertex(&mut self, vertex: StrokeVertex<'_, '_>) -> SolidVertex2D {
            let p = vertex.position();
            SolidVertex2D {
                position: [p.x, p.y],
                color: self.0,
            }
        }
    }
}
