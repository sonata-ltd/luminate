//! Analytic radial fills, independent of texture recording.

use iced_animate::{Anim, Tier};
use iced_core::widget::{Tree, Widget, tree};
use iced_core::{
    Color, Element, Event, Length, Point, Rectangle, Shell, Size, Vector, border, layout, mouse,
    renderer,
};
use std::cell::RefCell;

#[cfg(feature = "wgpu")]
pub(crate) mod gpu;
#[cfg(feature = "tiny-skia")]
mod software;

/// A circular or elliptical radial fill with up to eight colour stops.
///
/// The centre and radii are fractions of the painted rectangle's width and
/// height. `(0.5, 0.5)` is its centre; radii `(0.5, 0.5)` reach its four
/// sides. A non-square rectangle therefore produces an ellipse. The centre
/// may lie outside the rectangle. Stops interpolate **premultiplied** colours
/// in iced's active colour space (`web-colors` or linear RGB); outside the
/// stops, the nearest endpoint is extended (pad).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radial {
    center: Point,
    radii: Vector,
    stops: [Option<iced_core::gradient::ColorStop>; 8],
}

impl Radial {
    /// An empty fill centred in the rectangle, with half-width/height radii.
    /// No stops paints nothing; one stop paints a solid colour.
    #[must_use]
    pub fn new() -> Self {
        Self {
            center: Point::new(0.5, 0.5),
            radii: Vector::new(0.5, 0.5),
            stops: [None; 8],
        }
    }

    /// The centre, as fractions of the rectangle's width and height.
    /// Non-finite coordinates make the fill invisible.
    #[must_use]
    pub fn center(mut self, center: impl Into<Point>) -> Self {
        self.center = center.into();
        self
    }

    /// The horizontal and vertical radii, as fractions of width and height.
    /// Non-positive or non-finite radii make the fill invisible.
    #[must_use]
    pub fn radii(mut self, radii: impl Into<Vector>) -> Self {
        self.radii = radii.into();
        self
    }

    /// Adds a stop in `0..=1`, sorted by offset. Equal offsets replace the
    /// previous stop; invalid offsets/colours and stops beyond eight are ignored.
    /// Channels of finite colours are clamped to `0..=1`.
    #[must_use]
    pub fn add_stop(mut self, offset: f32, color: Color) -> Self {
        if !offset.is_finite()
            || !(0.0..=1.0).contains(&offset)
            || ![color.r, color.g, color.b, color.a]
                .iter()
                .all(|c| c.is_finite())
        {
            return self;
        }
        let stop = iced_core::gradient::ColorStop {
            offset,
            color: Color {
                r: color.r.clamp(0.0, 1.0),
                g: color.g.clamp(0.0, 1.0),
                b: color.b.clamp(0.0, 1.0),
                a: color.a.clamp(0.0, 1.0),
            },
        };
        if let Some(slot) = self
            .stops
            .iter_mut()
            .find(|s| s.is_some_and(|s| s.offset == offset))
        {
            *slot = Some(stop);
            return self;
        }
        if let Some(slot) = self.stops.iter_mut().find(|s| s.is_none()) {
            *slot = Some(stop);
            self.stops.sort_by(|a, b| match (a, b) {
                (Some(a), Some(b)) => a.offset.total_cmp(&b.offset),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            });
        }
        self
    }

    pub(crate) fn valid(self) -> bool {
        self.center.x.is_finite()
            && self.center.y.is_finite()
            && self.radii.x.is_finite()
            && self.radii.y.is_finite()
            && self.radii.x > 0.0
            && self.radii.y > 0.0
            && self.stops[0].is_some()
    }
}

impl Default for Radial {
    fn default() -> Self {
        Self::new()
    }
}

/// Backend operations required by [`RadialGradient`]. Separate from
/// [`crate::TextureRenderer`]: a radial fill does not record a texture on GPU.
/// Third-party renderer wrappers can delegate this operation.
pub trait RadialRenderer: renderer::Renderer {
    /// Draws `fill` over the **whole** `bounds`, preserving the renderer's
    /// current clipping and transformations. `corners` are logical pixels;
    /// `opacity` is group opacity. Invalid fills/bounds paint nothing.
    /// `cache` belongs to this widget instance and may retain a software raster.
    fn draw_radial(
        &mut self,
        bounds: Rectangle,
        fill: Radial,
        corners: border::Radius,
        opacity: f32,
        cache: &mut RadialCache,
    );
}

/// Per-widget raster storage for [`RadialRenderer`] implementations.
/// GPU drawing leaves it empty. A software renderer retains at most one image;
/// moving or fading the widget does not regenerate that image.
#[derive(Debug, Default)]
pub struct RadialCache {
    #[cfg(feature = "tiny-skia")]
    raster: Option<software::Raster>,
}

/// A decorative, non-interactive radial fill. Compose it with `stack` rather
/// than wrapping content or coupling a paint operation to [`crate::Cached`].
/// It defaults to filling available space; in a `stack`, put size-defining
/// content first and this fill second to overlay it. For a background, put a
/// sized fill first and transparent content second.
///
/// Animation values are read in `draw`, marked [`Tier::Paint`], and request
/// frames only while moving. Inside `cached`, changing paint invalidates the
/// enclosing recording. For a cheap fade of an entire subtree use
/// [`crate::Cached::opacity`] outside it instead.
#[derive(Debug, Clone)]
pub struct RadialGradient {
    fill: Radial,
    center: Anim<Point>,
    radii: Anim<Vector>,
    opacity: Anim<f32>,
    corners: border::Radius,
    width: Length,
    height: Length,
}

/// Creates a radial paint layer. No events are captured and no messages emitted.
///
/// ```no_run
/// use iced::{Color, widget::{stack, text, container}};
/// use iced_texture_cache::{Radial, radial_gradient};
/// let _: iced_texture_cache::Element<'_, ()> = stack![
///     radial_gradient(Radial::new()
///         .add_stop(0.0, Color::from_rgb(0.1, 0.8, 0.3))
///         .add_stop(1.0, Color::TRANSPARENT))
///         .width(320).height(180).border_radius(20.0),
///     container(text("Ready to play")).padding(24),
/// ].into();
/// ```
#[must_use]
pub fn radial_gradient(fill: Radial) -> RadialGradient {
    RadialGradient {
        fill,
        center: fill.center.into(),
        radii: fill.radii.into(),
        opacity: 1.0.into(),
        corners: border::Radius::default(),
        width: Length::Fill,
        height: Length::Fill,
    }
}

impl RadialGradient {
    /// Animated centre, in fractions of width and height.
    #[must_use]
    pub fn center(mut self, center: impl Into<Anim<Point>>) -> Self {
        self.center = center.into();
        self.center.mark_tier(Tier::Paint);
        self
    }
    /// Animated radii, in fractions of width and height.
    #[must_use]
    pub fn radii(mut self, radii: impl Into<Anim<Vector>>) -> Self {
        self.radii = radii.into();
        self.radii.mark_tier(Tier::Paint);
        self
    }
    /// Animated opacity. Non-finite or non-positive opacity paints nothing.
    #[must_use]
    pub fn opacity(mut self, opacity: impl Into<Anim<f32>>) -> Self {
        self.opacity = opacity.into();
        self.opacity.mark_tier(Tier::Paint);
        self
    }
    /// The fill's corner radii in logical pixels. Does not clip sibling content.
    #[must_use]
    pub fn border_radius(mut self, radius: impl Into<border::Radius>) -> Self {
        self.corners = radius.into();
        self
    }
    /// Width of the layer; defaults to [`Length::Fill`].
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }
    /// Height of the layer; defaults to [`Length::Fill`].
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
    fn resolved(&self) -> Paint {
        let fill = Radial {
            center: self.center.get(),
            radii: self.radii.get(),
            ..self.fill
        };
        let opacity = self.opacity.get();
        if !fill.valid() || !opacity.is_finite() || opacity <= 0.0 {
            // Canonicalise invalid values: NaN must not look like changing
            // paint and invalidate an enclosing cache on every idle frame.
            return Paint {
                fill: Radial::new(),
                opacity: 0.0,
                corners: border::Radius::default(),
            };
        }
        let finite = |v: f32| if v.is_finite() { v.max(0.0) } else { 0.0 };
        Paint {
            fill,
            opacity: opacity.min(1.0),
            corners: border::Radius {
                top_left: finite(self.corners.top_left),
                top_right: finite(self.corners.top_right),
                bottom_right: finite(self.corners.bottom_right),
                bottom_left: finite(self.corners.bottom_left),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Paint {
    fill: Radial,
    corners: border::Radius,
    opacity: f32,
}
#[derive(Debug, Default)]
struct State {
    raster: RefCell<RadialCache>,
    paint: Option<Paint>,
}

impl<Message, Theme, Renderer: RadialRenderer> Widget<Message, Theme, Renderer> for RadialGradient {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }
    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }
    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }
    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }
    fn update(
        &mut self,
        tree: &mut Tree,
        _: &Event,
        _: layout::Layout<'_>,
        _: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn iced_core::Clipboard,
        shell: &mut Shell<'_, Message>,
        _: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let paint = self.resolved();
        if state.paint != Some(paint) {
            state.paint = Some(paint);
            crate::ancestors::invalidate_ancestors();
            shell.request_redraw();
        }
        if self.center.is_animating() || self.radii.is_animating() || self.opacity.is_animating() {
            shell.request_redraw();
        }
    }
    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        _: &Theme,
        _: &renderer::Style,
        layout: layout::Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        if bounds.intersection(viewport).is_none() {
            return;
        }
        // The enclosing renderer already owns the clip. Starting another layer
        // with the widget's bounds could replace an ancestor's scissor in iced.
        let paint = self.resolved();
        renderer.draw_radial(
            bounds,
            paint.fill,
            paint.corners,
            paint.opacity,
            &mut tree.state.downcast_ref::<State>().raster.borrow_mut(),
        );
    }
}

impl<'a, Message: 'a, Theme: 'a, Renderer: RadialRenderer + 'a> From<RadialGradient>
    for Element<'a, Message, Theme, Renderer>
{
    fn from(fill: RadialGradient) -> Self {
        Self::new(fill)
    }
}

pub(crate) fn drawable(bounds: Rectangle, fill: Radial, opacity: f32) -> bool {
    fill.valid()
        && opacity.is_finite()
        && opacity > 0.0
        && [bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .all(|x| x.is_finite())
        && bounds.width > 0.0
        && bounds.height > 0.0
}

pub(crate) fn radii(size: Size, corners: border::Radius) -> [f32; 4] {
    crate::geometry::clamp_radii(
        size,
        [
            corners.top_left,
            corners.top_right,
            corners.bottom_right,
            corners.bottom_left,
        ]
        .map(|v| if v.is_finite() { v.max(0.0) } else { 0.0 }),
    )
}

pub(crate) fn packed(color: Color) -> [f32; 4] {
    let [r, g, b, a] = iced_graphics::color::pack(color).components();
    [r * a, g * a, b * a, a]
}

#[cfg(test)]
mod tests;
