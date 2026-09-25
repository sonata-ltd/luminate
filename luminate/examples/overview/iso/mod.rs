//! Isometric page icons.
//!
//! An icon is a short list of flat rounded shapes lying in one ground plane,
//! seen through a fixed isometric camera. There is no extrusion and no
//! shading: depth is carried by elevation, overlap and tone alone. The
//! projection, the colour and the fit to the canvas are computed, so an icon
//! is authored as data rather than drawn.
//!
//! Authoring rule: [`Scene::depth_order`] sorts whole shapes along the view
//! direction, so two shapes that need a particular stacking must differ in
//! that ordering rather than merely overlap.

pub(crate) mod project;
// `scenes.rs` sits next to this file holding the extruded ancestors of the
// set. It is intentionally not declared here, so it is never compiled.
pub(crate) mod scenes_flat;
mod shade;
mod shape;

use iced_luminate::iced::widget::canvas::{Canvas, Frame, Geometry, Path, Program};
use iced_luminate::iced::{Color, Length, Point, Rectangle, Size, mouse};
use iced_luminate::{Element, Renderer, Theme};

use project::{Fit, project};
use shape::footprint;

pub(crate) use project::Vec3;
pub(crate) use shade::Tone;

/// The square an icon draws into.
pub(crate) const ICON_SIZE: f32 = 128.0;

/// The outline of a shape.
///
/// Every variant is a rounded rectangle. The names carry the author's intent,
/// not different geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    /// A rounded rectangle with an explicit corner radius.
    Slab {
        /// The corner radius, clamped to half the shorter side.
        radius: f32,
    },
    /// A rounded rectangle whose radius saturates the clamp: a stadium.
    Pill,
    /// A slab the author means as a thin line.
    Bar {
        /// The corner radius.
        radius: f32,
    },
}

impl Shape {
    /// The corner radius this shape asks for. [`Shape::Pill`] asks for more
    /// than the clamp allows, which is how it becomes a stadium.
    const fn radius(self) -> f32 {
        match self {
            Self::Slab { radius } | Self::Bar { radius } => radius,
            Self::Pill => f32::MAX,
        }
    }
}

/// One shape in a scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Element3 {
    /// The outline kind.
    pub(crate) shape: Shape,
    /// Where the shape sits. Its `z` is elevation above the ground plane,
    /// which the camera turns into an offset up the screen.
    pub(crate) at: Vec3,
    /// The shape's full extent along `x` and along `y`.
    pub(crate) footprint: (f32, f32),
    /// The colour role.
    pub(crate) tone: Tone,
    /// Scales the colour's alpha.
    pub(crate) alpha: f32,
}

impl Element3 {
    /// An opaque shape.
    pub(crate) const fn new(shape: Shape, at: Vec3, footprint: (f32, f32), tone: Tone) -> Self {
        Self {
            shape,
            at,
            footprint,
            tone,
            alpha: 1.0,
        }
    }

    /// The same shape at a lower alpha: a trail or an after-image rather than
    /// a solid.
    pub(crate) const fn ghost(mut self, alpha: f32) -> Self {
        self.alpha = alpha;
        self
    }

    /// The shape's projected outline.
    fn face(&self) -> Vec<Point> {
        footprint(self.at, self.footprint, self.shape.radius())
            .iter()
            .copied()
            .map(project)
            .collect()
    }

    /// The sort key: elevation first, then distance along the view direction.
    ///
    /// Elevation wins so that a shape laid on another always draws over it,
    /// wherever the two sit in the plane. Summing the three axes instead
    /// would let a shape placed towards the back sink under its own base.
    fn layer(&self) -> (f32, f32) {
        (self.at.z, self.at.x + self.at.y)
    }
}

/// A whole icon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Scene {
    /// The shapes, in declaration order. Draw order is by depth.
    pub(crate) elements: &'static [Element3],
}

impl Scene {
    /// Every projected point the scene paints. This is what the canvas is
    /// fitted to, so nothing the scene draws can fall outside it.
    fn outline(&self) -> Vec<Point> {
        self.elements.iter().flat_map(Element3::face).collect()
    }

    /// The elements' indices, farthest first. `sort_by` is stable, so ties
    /// keep declaration order.
    fn depth_order(&self) -> Vec<usize> {
        let mut order: Vec<usize> = (0..self.elements.len()).collect();

        order.sort_by(|a, b| {
            self.elements[*a]
                .layer()
                .partial_cmp(&self.elements[*b].layer())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        order
    }

    /// Paints the scene into `frame`, fitted to `size`.
    fn draw(&self, frame: &mut Frame<Renderer>, theme: &Theme, size: Size) {
        let fit = Fit::new(&self.outline(), size);

        for index in self.depth_order() {
            let element = &self.elements[index];

            fill(
                frame,
                &element.face(),
                &fit,
                element.tone.color(theme, element.alpha),
            );
        }
    }
}

/// Fills a projected polygon after fitting it to the canvas.
fn fill(frame: &mut Frame<Renderer>, polygon: &[Point], fit: &Fit, color: Color) {
    if polygon.len() < 3 {
        return;
    }

    let path = Path::new(|builder| {
        let mut points = polygon.iter().map(|point| fit.apply(*point));

        if let Some(first) = points.next() {
            builder.move_to(first);

            for point in points {
                builder.line_to(point);
            }
        }

        builder.close();
    });

    frame.fill(&path, color);
}

/// Draws one [`Scene`]. It holds no state and emits no messages.
#[derive(Debug)]
struct IconProgram {
    /// The scene to paint.
    scene: &'static Scene,
}

impl<Message> Program<Message, Theme, Renderer> for IconProgram {
    type State = ();

    fn draw(
        &self,
        _state: &(),
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry<Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());

        self.scene.draw(&mut frame, theme, bounds.size());

        vec![frame.into_geometry()]
    }
}

/// The page icon for `scene`, drawn into a fixed square.
pub(crate) fn icon<'a, Message: 'a>(scene: &'static Scene) -> Element<'a, Message> {
    Canvas::new(IconProgram { scene })
        .width(Length::Fixed(ICON_SIZE))
        .height(Length::Fixed(ICON_SIZE))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three shapes declared out of depth order.
    const ORDERED: Scene = Scene {
        elements: &[
            Element3::new(
                Shape::Pill,
                Vec3::new(0.0, 0.0, 0.0),
                (10.0, 6.0),
                Tone::Neutral,
            ),
            Element3::new(
                Shape::Pill,
                Vec3::new(20.0, 20.0, 0.0),
                (10.0, 6.0),
                Tone::Accent,
            ),
            Element3::new(
                Shape::Pill,
                Vec3::new(-20.0, -20.0, 0.0),
                (10.0, 6.0),
                Tone::Muted,
            ),
        ],
    };

    #[test]
    fn elements_draw_from_far_to_near() {
        assert_eq!(ORDERED.depth_order(), vec![2, 0, 1]);
    }

    #[test]
    fn elevation_counts_towards_depth() {
        const LIFTED: Scene = Scene {
            elements: &[
                Element3::new(
                    Shape::Pill,
                    Vec3::new(0.0, 0.0, 5.0),
                    (10.0, 6.0),
                    Tone::Neutral,
                ),
                Element3::new(
                    Shape::Pill,
                    Vec3::new(0.0, 0.0, 0.0),
                    (10.0, 6.0),
                    Tone::Accent,
                ),
            ],
        };

        assert_eq!(LIFTED.depth_order(), vec![1, 0]);
    }

    #[test]
    fn ties_keep_declaration_order() {
        const TIED: Scene = Scene {
            elements: &[
                Element3::new(
                    Shape::Pill,
                    Vec3::new(1.0, 1.0, 0.0),
                    (10.0, 6.0),
                    Tone::Neutral,
                ),
                Element3::new(
                    Shape::Pill,
                    Vec3::new(2.0, 0.0, 0.0),
                    (10.0, 6.0),
                    Tone::Accent,
                ),
            ],
        };

        assert_eq!(TIED.depth_order(), vec![0, 1]);
    }

    #[test]
    fn a_pill_saturates_its_radius() {
        assert!(Shape::Pill.radius() > 1000.0);
    }

    #[test]
    fn a_ghost_is_translucent() {
        let ghost = Element3::new(
            Shape::Pill,
            Vec3::new(0.0, 0.0, 0.0),
            (10.0, 6.0),
            Tone::Neutral,
        )
        .ghost(0.4);

        assert_eq!(ghost.alpha, 0.4);
    }

    #[test]
    fn every_scene_fits_inside_its_canvas() {
        let size = Size::new(ICON_SIZE, ICON_SIZE);

        for scene in scenes_flat::ALL {
            let fit = Fit::new(&scene.outline(), size);

            for point in scene.outline() {
                let fitted = fit.apply(point);

                assert!(fitted.x >= -0.001 && fitted.x <= ICON_SIZE + 0.001);
                assert!(fitted.y >= -0.001 && fitted.y <= ICON_SIZE + 0.001);
            }
        }
    }

    #[test]
    fn no_scene_leads_with_more_than_one_accent() {
        for scene in scenes_flat::ALL {
            let accents = scene
                .elements
                .iter()
                .filter(|element| element.tone == Tone::Accent && element.alpha == 1.0)
                .count();

            assert!(accents <= 1);
        }
    }
}
