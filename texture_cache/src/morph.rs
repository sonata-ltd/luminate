//! A composite that carries its content into another rectangle, the way an
//! app's screen closes into its icon on iOS.
//!
//! The rectangle the content is shown in travels from the content's own
//! bounds to a target, changing its proportions on the way, and its corner
//! radius runs from one value to another. The content does not stretch with
//! it: it is scaled uniformly by whatever makes it *cover* the rectangle,
//! centred on it, and the rectangle cuts away what overhangs. That is a
//! transform, a crop and a rounded mask on the composite — the texture is
//! neither re-recorded nor laid out again, however the rectangle moves.
//!
//! Where the rectangle goes on the way is a [`MorphPath`]: [`Morph`] is the
//! straight one, and an application choreographing its own transition —
//! position, size and corners each on their own schedule, along a curve —
//! implements the trait. [`MorphFrame::cover`] is the fitting, whichever
//! path asked for the rectangle.
//!
//! The arithmetic lives here, apart from the widget, so that a caller
//! drawing something alongside the content — a shadow that has to follow
//! the same rectangle — can ask for the very same frame.

use iced_core::{Point, Rectangle, Size, Transformation, border};

use crate::geometry::SCALE_EPS;

/// Progress within this of `1.0` is the content at rest in its own bounds:
/// the frame differs from them by far less than a pixel, and treating it as
/// identity is what hands the layer back to a plain composite.
const AT_REST: f32 = 1e-4;

/// The way a [`Cached::morph`](crate::Cached::morph) carries its content:
/// the rectangle the content is shown in, and its corner radius, at each
/// progress.
///
/// Progress `1` is the content at rest in its own bounds; what `0` means is
/// the path's to say. Both methods are called on every frame of the morph,
/// inside `draw` and for the pointer: keep them cheap.
pub trait MorphPath: 'static {
    /// Whether the content is at rest in its own bounds at `progress`, so
    /// that the layer draws as it would without a morph. Must agree with
    /// [`shape`](Self::shape) returning `None`, and must not depend on the
    /// bounds: it is asked where they are not at hand.
    fn at_rest(&self, progress: f32) -> bool;

    /// The rectangle the content is shown in at `progress`, in the same
    /// coordinates as `bounds`, and the rectangle's corner radius; `None`
    /// at rest.
    fn shape(&self, bounds: Rectangle, progress: f32) -> Option<(Rectangle, f32)>;
}

/// The straight [`MorphPath`]: the content's rectangle interpolated toward
/// another one, corners included.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Morph {
    /// The rectangle the content ends in at progress `0`, in the same
    /// coordinates as the widget's layout bounds.
    pub target: Rectangle,
    /// The corner radius of the content's own shape, at progress `1`.
    ///
    /// Give it the radius the content is already drawn with: the mask takes
    /// over the content's corners the moment it leaves its bounds, and a
    /// different radius there would show as a jump.
    pub radius: f32,
    /// The corner radius at `target`, at progress `0`.
    pub target_radius: f32,
}

/// One frame of a [`Morph`]: the rectangle the content is shown in and how
/// the content is fitted into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MorphFrame {
    /// The rectangle the content is shown in, in layout coordinates.
    pub frame: Rectangle,
    /// The frame's corner radius, in the same logical pixels.
    pub radius: f32,
    /// The uniform scale the content is drawn at: the smallest that leaves
    /// no part of `frame` uncovered.
    pub scale: f32,
    /// The part of the content's bounds that `frame` shows, in layout
    /// coordinates: `frame`'s proportions, centred on the content.
    pub crop: Rectangle,
}

impl Morph {
    /// The frame at `progress` for content laid out at `bounds`: `1.0` is
    /// the content in its own bounds, `0.0` is the content at `target`.
    ///
    /// `None` at rest — within a hair of `1.0` — and for degenerate bounds:
    /// there is nothing to morph, and the layer draws as it would without
    /// one. Progress is clamped to `0..=1`, since past either end the frame
    /// would leave the segment between the two rectangles and the content
    /// would be magnified beyond what was recorded; drive it with a curve
    /// that does not overshoot. A non-finite progress is treated as rest.
    #[must_use]
    pub fn at(self, bounds: Rectangle, progress: f32) -> Option<MorphFrame> {
        let (frame, radius) = self.shape(bounds, progress)?;
        MorphFrame::cover(bounds, frame, radius)
    }

    /// Whether `progress` is anywhere but at rest: within a hair of `1.0`,
    /// or not a number at all, it is not.
    #[must_use]
    pub fn runs(progress: f32) -> bool {
        progress.is_finite() && progress < 1.0 - AT_REST
    }
}

impl MorphPath for Morph {
    fn at_rest(&self, progress: f32) -> bool {
        !Self::runs(progress)
    }

    fn shape(&self, bounds: Rectangle, progress: f32) -> Option<(Rectangle, f32)> {
        if !Self::runs(progress) {
            return None;
        }

        let t = progress.max(0.0);
        let lerp = |to: f32, from: f32| to + (from - to) * t;

        let frame = Rectangle {
            x: lerp(self.target.x, bounds.x),
            y: lerp(self.target.y, bounds.y),
            width: lerp(self.target.width, bounds.width),
            height: lerp(self.target.height, bounds.height),
        };

        Some((frame, lerp(self.target_radius, self.radius)))
    }
}

impl MorphFrame {
    /// Fits content laid out at `bounds` into `frame`, rounded to `radius`:
    /// scaled uniformly by the smallest factor that covers the frame,
    /// centred on it. `None` for degenerate bounds, which have nothing to
    /// show.
    #[must_use]
    pub fn cover(bounds: Rectangle, frame: Rectangle, radius: f32) -> Option<Self> {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return None;
        }

        let frame = Rectangle {
            width: frame.width.max(0.0),
            height: frame.height.max(0.0),
            ..frame
        };
        let scale = (frame.width / bounds.width).max(frame.height / bounds.height);
        let crop = if scale > SCALE_EPS {
            let size = Size::new(frame.width / scale, frame.height / scale);
            let centre = bounds.center();
            Rectangle::new(
                Point::new(centre.x - size.width / 2.0, centre.y - size.height / 2.0),
                size,
            )
        } else {
            Rectangle::new(bounds.center(), Size::ZERO)
        };

        Some(Self {
            frame,
            radius: radius.max(0.0),
            scale,
            crop,
        })
    }

    /// Whether the frame has shrunk to nothing, so that nothing is drawn
    /// and nothing can be hit.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.scale <= SCALE_EPS || self.frame.width <= 0.0 || self.frame.height <= 0.0
    }

    /// The transform that carries `crop` onto `frame`, in layout
    /// coordinates. Right-most applies first.
    #[must_use]
    pub fn transform(self) -> Transformation {
        Transformation::translate(self.frame.x, self.frame.y)
            * Transformation::scale(self.scale)
            * Transformation::translate(-self.crop.x, -self.crop.y)
    }

    /// Where a composite of this frame is clipped on screen, the content
    /// laid out under `transform`; `None` when none of it is in `viewport`.
    ///
    /// The crop is cut by this clip, and on wgpu also by the rounded mask
    /// [`corners`](Self::corners) puts on it, which smooths its straight
    /// edges too; the clip then stands a pixel off so as not to step them.
    /// A mask without corners masks nothing, so a square frame keeps the
    /// exact clip.
    pub(crate) fn clip(self, transform: Transformation, viewport: &Rectangle) -> Option<Rectangle> {
        let frame = self.frame * transform;
        let frame = if self.radius > 0.0 {
            frame.expand(1.0)
        } else {
            frame
        };

        frame.intersection(viewport)
    }

    /// The frame's corners in the content's own units, for a composite under
    /// [`transform`](Self::transform): it scales them back to `radius`.
    pub(crate) fn corners(self) -> border::Radius {
        border::Radius::from(self.radius / self.scale)
    }

    /// The point of the content shown at `point` of the frame, or `None`
    /// where the frame does not reach.
    #[must_use]
    pub fn source_point(self, point: Point) -> Option<Point> {
        if self.is_empty() || !self.frame.contains(point) {
            return None;
        }

        Some(Point::new(
            self.crop.x + (point.x - self.frame.x) / self.scale,
            self.crop.y + (point.y - self.frame.y) / self.scale,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOUNDS: Rectangle = Rectangle {
        x: 100.0,
        y: 200.0,
        width: 400.0,
        height: 300.0,
    };

    const ICON: Morph = Morph {
        target: Rectangle {
            x: 900.0,
            y: 10.0,
            width: 30.0,
            height: 30.0,
        },
        radius: 16.0,
        target_radius: 8.0,
    };

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn at_rest_there_is_nothing_to_morph() {
        assert_eq!(ICON.at(BOUNDS, 1.0), None);
        assert_eq!(ICON.at(BOUNDS, 1.0 - AT_REST / 2.0), None);
        assert_eq!(ICON.at(BOUNDS, f32::NAN), None);
        assert_eq!(ICON.at(Rectangle::with_size(Size::ZERO), 0.5), None);
    }

    #[test]
    fn at_the_target_the_frame_is_the_target() {
        let at = ICON.at(BOUNDS, 0.0).unwrap();

        assert_eq!(at.frame, ICON.target);
        assert!(close(at.radius, 8.0));
    }

    #[test]
    fn the_content_covers_the_frame_without_stretching() {
        // A square frame over 4:3 content: the height decides the scale, and
        // the sides of the content overhang it.
        let at = ICON.at(BOUNDS, 0.0).unwrap();

        assert!(close(at.scale, 30.0 / 300.0));
        assert!(close(at.crop.width, 300.0));
        assert!(close(at.crop.height, 300.0));
        assert!(close(at.crop.center_x(), BOUNDS.center_x()));
        assert!(close(at.crop.center_y(), BOUNDS.center_y()));
    }

    #[test]
    fn the_rectangle_and_the_radius_travel_together() {
        let at = ICON.at(BOUNDS, 0.5).unwrap();

        assert!(close(at.frame.x, 500.0));
        assert!(close(at.frame.y, 105.0));
        assert!(close(at.frame.width, 215.0));
        assert!(close(at.frame.height, 165.0));
        assert!(close(at.radius, 12.0));
    }

    #[test]
    fn progress_is_clamped_at_the_target() {
        assert_eq!(ICON.at(BOUNDS, -1.0), ICON.at(BOUNDS, 0.0));
    }

    #[test]
    fn the_transform_carries_the_crop_onto_the_frame() {
        let at = ICON.at(BOUNDS, 0.3).unwrap();
        let moved = at.crop * at.transform();

        assert!(close(moved.x, at.frame.x));
        assert!(close(moved.y, at.frame.y));
        assert!(close(moved.width, at.frame.width));
        assert!(close(moved.height, at.frame.height));
    }

    #[test]
    fn a_point_in_the_frame_maps_back_onto_the_content() {
        let at = ICON.at(BOUNDS, 0.0).unwrap();

        assert_eq!(at.source_point(at.frame.center()), Some(BOUNDS.center()));
        assert_eq!(at.source_point(Point::new(0.0, 0.0)), None);
    }

    #[test]
    fn a_target_of_no_size_draws_nothing_at_the_end() {
        let point = Morph {
            target: Rectangle::new(Point::new(10.0, 10.0), Size::ZERO),
            ..ICON
        };

        let at = point.at(BOUNDS, 0.0).unwrap();
        assert!(at.is_empty());
        assert_eq!(at.source_point(Point::new(10.0, 10.0)), None);
    }
}
