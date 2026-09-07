//! A rectangle whose paint is resolved every frame.
//!
//! [`Sized`] covers the layout tier and a compositor-tier widget (such as
//! `Cached` in `iced_texture_cache`) covers the compositor tier; this is the
//! tier in between. Stock iced widgets take their colours
//! from a style closure that runs while the view is being built, so a colour
//! written there is a snapshot and freezes until the next rebuild, the same
//! trap [`Sized`] exists to avoid for sizes.
//!
//! [`Shape`] reads its [`Anim`] values inside `draw`:
//!
//! ```
//! use iced::border::Radius;
//! use iced::Color;
//! use iced_animate::widget::shape;
//! use iced_animate::{curves::SMOOTH, key, Motion};
//!
//! let m = Motion::new();
//! let hot = true;
//! let _ = shape()
//!     .width(48)
//!     .height(48)
//!     .fill(m.to(key!(), SMOOTH, if hot { Color::WHITE } else { Color::BLACK }))
//!     .radius(m.to(key!(), SMOOTH, Radius::from(if hot { 24.0 } else { 6.0 })));
//! ```
//!
//! It is deliberately just a rectangle. Anything richer is a component, and a
//! component can compose this with [`Sized`] and a compositor-tier widget.
//!
//! [`Sized`]: crate::widget::Sized

use std::cell::Cell;

use iced_core::border::Radius;
use iced_core::widget::{Tree, tree};
use iced_core::{Background, Border, Color, Element, Length, Rectangle, Shadow, Size};
use iced_core::{Layout, Widget, layout, mouse, renderer};

use crate::{Anim, AnimLength, Tier};

/// A bouncy spring may push a channel outside `0.0..=1.0`; the renderer's
/// blending is undefined there.
fn clamped_color(color: Color) -> Color {
    Color {
        r: color.r.clamp(0.0, 1.0),
        g: color.g.clamp(0.0, 1.0),
        b: color.b.clamp(0.0, 1.0),
        a: color.a.clamp(0.0, 1.0),
    }
}

/// A corner radius cannot be negative.
fn non_negative_radius(radius: Radius) -> Radius {
    Radius {
        top_left: radius.top_left.max(0.0),
        top_right: radius.top_right.max(0.0),
        bottom_right: radius.bottom_right.max(0.0),
        bottom_left: radius.bottom_left.max(0.0),
    }
}

/// Creates a rectangle whose paint can be animated.
///
/// A shape has no intrinsic size: give it a `width` and `height`, or it
/// measures 0 × 0 like `Space::new()`.
///
/// See the [`widget`](crate::widget) module for why a style closure cannot
/// do this.
#[must_use]
pub fn shape() -> Shape {
    Shape::new()
}

/// A rectangle whose fill, border and corner radius are resolved in `draw`.
#[derive(Debug)]
pub struct Shape {
    width: AnimLength,
    height: AnimLength,
    fill: Anim<Color>,
    radius: Anim<Radius>,
    border_color: Anim<Color>,
    border_width: Anim<f32>,
    pixel_snap: PixelSnap,
}

impl Default for Shape {
    fn default() -> Self {
        Self::new()
    }
}

impl Shape {
    /// Creates a transparent, borderless rectangle that takes no space until
    /// given a [`width`](Self::width) and [`height`](Self::height).
    #[must_use]
    pub fn new() -> Self {
        Self {
            width: AnimLength::Shrink,
            height: AnimLength::Shrink,
            fill: Anim::constant(Color::TRANSPARENT),
            radius: Anim::constant(Radius::default()),
            border_color: Anim::constant(Color::TRANSPARENT),
            border_width: Anim::constant(0.0),
            pixel_snap: PixelSnap::default(),
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

    /// Sets the fill colour, which may be animated.
    ///
    /// Interpolation is component-wise in sRGB, including alpha, the same
    /// thing CSS does, so a transition between two saturated hues passes
    /// through a duller middle. Fade a whole subtree with a compositor-tier
    /// opacity instead of animating alpha here when that is what you mean.
    #[must_use]
    pub fn fill(mut self, color: impl Into<Anim<Color>>) -> Self {
        self.fill = color.into();
        self
    }

    /// Sets the corner radius, which may be animated.
    #[must_use]
    pub fn radius(mut self, radius: impl Into<Anim<Radius>>) -> Self {
        self.radius = radius.into();
        self
    }

    /// Sets the border colour, which may be animated.
    #[must_use]
    pub fn border_color(mut self, color: impl Into<Anim<Color>>) -> Self {
        self.border_color = color.into();
        self
    }

    /// Sets the border width, which may be animated.
    #[must_use]
    pub fn border_width(mut self, width: impl Into<Anim<f32>>) -> Self {
        self.border_width = width.into();
        self
    }

    /// Sets when the quad is rounded to the device pixel grid.
    ///
    /// The default, [`PixelSnap::Auto`], already declines to snap a shape
    /// that animates its own size. Reach for [`PixelSnap::Never`] when the
    /// shape is *moved* by something above it — a card re-centring as a page
    /// stack interpolates its height — which `Auto` cannot detect finely
    /// enough to keep off the grid on the frame the motion ends.
    #[must_use]
    pub fn pixel_snap(mut self, pixel_snap: PixelSnap) -> Self {
        self.pixel_snap = pixel_snap;
        self
    }

    /// `true` while any of the shape's values is in motion.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        self.width.is_animating()
            || self.height.is_animating()
            || self.fill.is_animating()
            || self.radius.is_animating()
            || self.border_color.is_animating()
            || self.border_width.is_animating()
    }

    /// Flags the paint values as needing a redraw, and the sizes a relayout.
    ///
    /// The distinction is the whole point of the tier system: a moving colour
    /// asks the host for another frame, a moving width also invalidates the
    /// layout.
    fn mark_tiers(&self) {
        self.width.mark_layout_tier();
        self.height.mark_layout_tier();

        self.fill.mark_tier(Tier::Paint);
        self.border_color.mark_tier(Tier::Paint);
        self.radius.mark_tier(Tier::Paint);
        self.border_width.mark_tier(Tier::Paint);
    }
}

/// What the previous frame drew.
///
/// A shape whose own values are constants can still be moving: an ancestor
/// may be animating the space it is laid out in, as [`Sized`](crate::widget::Sized)
/// does. Comparing bounds between frames is how it finds out.
#[derive(Debug, Default)]
struct State {
    last_bounds: Cell<Option<Rectangle>>,
}

/// When a [`Shape`] rounds its quad to the device pixel grid.
///
/// Snapping is geometry, not sharpness: the renderer rounds *both* the
/// origin and the far edge of the quad, so switching it on or off displaces
/// the shape by up to a pixel. That makes *when* it changes as visible as
/// whether it is on — which is why this is a choice and not a constant.
///
/// Whichever variant is chosen, nothing snaps unless iced's `crisp` feature
/// is on; it decides whether snapping is on the table at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum PixelSnap {
    /// Snap while the bounds stand still, leave them alone while they move:
    /// crisp at rest, smooth in motion. The default.
    ///
    /// A shape whose *size* is bound to a track never snaps, settled or not,
    /// because the alternative is a step onto the grid on the one frame the
    /// motion ends — the moment it is most visible. A shape moved by an
    /// *ancestor* still pays that step: this widget can see its own tracks,
    /// but only that its bounds changed, never by how much of a device pixel,
    /// so it cannot separate the travel from the grid the way
    /// [`Cached`](https://docs.rs/iced_texture_cache) can. Reach for
    /// [`Never`](PixelSnap::Never) there.
    #[default]
    Auto,
    /// Snap every frame. A resting edge is always crisp and nothing steps at
    /// the end of an animation, but a moving edge travels in whole-pixel
    /// lurches.
    Always,
    /// Never snap. Nothing ever steps, at the cost of a soft edge whenever
    /// the shape does not land on the grid by itself.
    Never,
}

impl Shape {
    /// Whether this frame's quad should be snapped to the pixel grid.
    ///
    /// Records `bounds` as it goes, so the answer covers motion this widget
    /// cannot see in its own values.
    fn snaps(&self, state: &State, bounds: Rectangle) -> bool {
        let moved = state.last_bounds.replace(Some(bounds)) != Some(bounds);
        // Bound, not merely moving. A size that is going to animate again must
        // not snap on the frame its track settles: the quad would step up to
        // half a device pixel just as the eye is told the motion is over,
        // which is the most noticeable moment it could pick. A shape whose box
        // is a plain number has no such frame coming and snaps as before.
        let box_is_bound = self.width.is_live() || self.height.is_live();

        renderer::Quad::default().snap
            && match self.pixel_snap {
                PixelSnap::Auto => !moved && !box_is_bound,
                PixelSnap::Always => true,
                PixelSnap::Never => false,
            }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Shape
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width.resolve(), self.height.resolve())
    }

    fn size_hint(&self) -> Size<Length> {
        // An animated axis passing through zero must not be advertised as
        // `Fixed(0.0)`, that is the void hint, and the parent deletes the
        // widget outright. See [`AnimLength::size_hint`].
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
        // Asked before the visibility test, so the record stays current for a
        // shape that scrolls back into view.
        let snap = self.snaps(tree.state.downcast_ref::<State>(), bounds);

        if bounds.intersection(viewport).is_none() {
            return;
        }

        // Every one of these is read *here*, not in the view. That is the
        // whole contract: the value is fetched at the moment it is used, so
        // the frame the engine just ticked is the frame that gets painted.
        //
        // Snapping to the pixel grid keeps a resting 1 px border as crisp as
        // the container next to it (when iced's `crisp` feature is on), but it
        // rounds *both* edges of the quad, so when the box moves it changes size
        // in whole-pixel lurches. Hence `snaps`: at rest, crisp; when the box is
        // moving, in whatever place the frame actually asks for.
        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: Border {
                    color: clamped_color(self.border_color.get()),
                    width: self.border_width.get().max(0.0),
                    radius: non_negative_radius(self.radius.get()),
                },
                shadow: Shadow::default(),
                snap,
            },
            Background::Color(clamped_color(self.fill.get())),
        );
    }
}

impl<'a, Message, Theme, Renderer> From<Shape> for Element<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
    Message: 'a,
    Theme: 'a,
{
    fn from(shape: Shape) -> Self {
        Element::new(shape)
    }
}

// `iced_core` implements `Renderer for ()` only in debug builds.
#[cfg(all(test, debug_assertions))]
mod tests {
    use iced_core::border::Radius;
    use iced_core::{Color, Length, Point, Rectangle, Size, Widget};

    use super::{Shape, State, clamped_color, non_negative_radius, shape};
    use crate::testing::FrameClock;
    use crate::{Motion, curves::BOUNCY, key};

    #[test]
    fn a_shape_moved_by_an_ancestor_does_not_snap_to_the_pixel_grid() {
        // `crisp` decides whether snapping is on the table at all; when it is
        // off nothing here can snap and there is nothing to check.
        if !iced_core::renderer::Quad::default().snap {
            return;
        }

        let widget: Shape = shape();
        let state = State::default();
        let at = |x: f32, side: f32| Rectangle::new(Point::new(x, 4.0), Size::new(side, side));

        assert!(
            !widget.snaps(&state, at(2.0, 40.0)),
            "nothing to compare against on the first frame"
        );
        assert!(
            widget.snaps(&state, at(2.0, 40.0)),
            "standing still: snapped, so a resting edge stays crisp"
        );
        // The shape's own values are all constants here — this is exactly the
        // `size (layout)` case, where a `Sized` ancestor grows the bounds.
        assert!(
            !widget.snaps(&state, at(2.0, 41.3)),
            "grown by an ancestor: not snapped, or the growth steps by whole pixels"
        );
        assert!(
            !widget.snaps(&state, at(3.7, 41.3)),
            "moved by an ancestor: likewise"
        );
        assert!(
            widget.snaps(&state, at(3.7, 41.3)),
            "settled again: snapped"
        );
    }

    #[test]
    fn a_shape_animating_its_own_paint_does_not_snap_either() {
        if !iced_core::renderer::Quad::default().snap {
            return;
        }

        let motion = Motion::new();
        let mut clock = FrameClock::new(&motion);
        let k = key!();
        let _ = motion.to(k, BOUNCY, Color::BLACK);

        let widget: Shape = shape().fill(motion.to(k, BOUNCY, Color::WHITE));
        let state = State::default();
        let bounds = Rectangle::new(Point::new(2.0, 4.0), Size::new(40.0, 40.0));

        assert!(
            !widget.snaps(&state, bounds),
            "nothing to compare against yet"
        );

        let _ = clock.run(3);
        assert!(
            widget.snaps(&state, bounds),
            "the fill moves, the box does not"
        );

        let _ = clock.run_until_settled();
        assert!(
            widget.snaps(&state, bounds),
            "and settling changes nothing: no step for the eye to catch"
        );
    }

    #[test]
    fn a_shape_animating_its_own_box_never_snaps_not_even_once_it_settles() {
        // The flip is the artifact, not the snapping. A box that glides
        // sub-pixel and then lands on the grid changes position by up to half
        // a device pixel in the one frame the eye has just been told the
        // motion is over, which is exactly where it is most noticeable. A
        // shape that animates its own size can see that coming, so it declines
        // to snap for as long as the track is live *and* on the frame it
        // settles.
        let motion = Motion::new();
        let mut clock = FrameClock::new(&motion);
        let k = key!();
        let _ = motion.to(k, BOUNCY, 40.0);

        let widget: Shape = shape().width(motion.to(k, BOUNCY, 90.0)).height(40.0);
        let state = State::default();
        let at = |side: f32| Rectangle::new(Point::new(2.0, 4.0), Size::new(side, 40.0));

        let _ = clock.run(3);
        assert!(!widget.snaps(&state, at(52.3)), "the box is still growing");

        let _ = clock.run_until_settled();
        assert!(
            !widget.snaps(&state, at(90.0)),
            "settled, and still unsnapped: the settling frame must not step"
        );
        assert!(
            !widget.snaps(&state, at(90.0)),
            "and it stays that way while the track is bound"
        );
    }

    #[test]
    fn always_snaps_even_while_the_bounds_move() {
        if !iced_core::renderer::Quad::default().snap {
            return;
        }

        let widget: Shape = shape().pixel_snap(super::PixelSnap::Always);
        let state = State::default();
        let at = |x: f32, side: f32| Rectangle::new(Point::new(x, 4.0), Size::new(side, side));

        assert!(
            widget.snaps(&state, at(2.0, 40.0)),
            "the first frame snaps too: `Always` never consults the bounds"
        );
        assert!(
            widget.snaps(&state, at(3.7, 41.3)),
            "moved and grown, and it still snaps"
        );
        assert!(
            widget.snaps(&state, at(3.7, 41.3)),
            "settled, and nothing changed - which is the whole point"
        );
    }

    #[test]
    fn never_snaps_even_at_rest() {
        let widget: Shape = shape().pixel_snap(super::PixelSnap::Never);
        let state = State::default();
        let at = |x: f32, side: f32| Rectangle::new(Point::new(x, 4.0), Size::new(side, side));

        assert!(!widget.snaps(&state, at(2.0, 40.0)));
        assert!(
            !widget.snaps(&state, at(2.0, 40.0)),
            "standing still is exactly where `Auto` would snap, and this does not"
        );
        assert!(!widget.snaps(&state, at(3.7, 41.3)));
    }

    #[test]
    fn a_shape_without_a_size_measures_nothing() {
        let widget: Shape = shape();
        assert_eq!(
            Widget::<(), (), ()>::size(&widget),
            Size::new(Length::Shrink, Length::Shrink)
        );
    }

    #[test]
    fn paint_is_clamped_to_what_the_renderer_accepts() {
        // Struct literals: `Color::from_rgba` debug-asserts its ranges.
        let wild = Color {
            r: 1.35,
            g: -0.1,
            b: 0.5,
            a: 2.0,
        };
        assert_eq!(
            clamped_color(wild),
            Color {
                r: 1.0,
                g: 0.0,
                b: 0.5,
                a: 1.0,
            }
        );

        let negative = Radius {
            top_left: -2.0,
            top_right: 1.0,
            bottom_right: -0.5,
            bottom_left: 0.0,
        };
        assert_eq!(
            non_negative_radius(negative),
            Radius {
                top_left: 0.0,
                top_right: 1.0,
                bottom_right: 0.0,
                bottom_left: 0.0,
            }
        );
    }

    #[test]
    fn a_bouncy_fill_overshoots_raw_but_never_after_clamping() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let _ = m.to(key, BOUNCY, Color::BLACK);
        let fill = m.to(key, BOUNCY, Color::WHITE);

        let mut overshot = false;
        for _ in 0..200 {
            let _ = clock.run(1);
            let raw = fill.get();
            overshot |= raw.r > 1.0;
            let safe = clamped_color(raw);
            assert!((0.0..=1.0).contains(&safe.r) && (0.0..=1.0).contains(&safe.a));
        }
        assert!(
            overshot,
            "BOUNCY should overshoot at least once, or the clamp is untested"
        );
    }

    #[test]
    fn a_shape_knows_when_any_of_its_values_moves() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, BOUNCY, 0.0_f32);
        let width = m.to(key, BOUNCY, 10.0_f32);

        assert!(shape().border_width(width).is_animating());
        assert!(!shape().fill(Color::WHITE).is_animating());
    }
}

#[cfg(test)]
/// Tests that drive the widget through `iced_test`.
mod simulator_tests {
    use std::time::Duration;

    use iced::time::Instant;

    use crate::{Curve, Motion, SpringParams, Tier, key};

    /// A fast spring for tests; not the shipped `curves::SMOOTH`.
    const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

    /// The tier is what keeps a cheap animation cheap, and it is set by whichever
    /// widget binds the value, so it is worth checking against the real widgets
    /// rather than by calling `mark_tier` by hand.
    #[test]
    fn a_shape_marks_its_paint_and_its_layout_apart() {
        use crate::widget::shape;
        use iced::Color;

        let m = Motion::new();

        let fill_key = key!();
        let size_key = key!();
        let _ = m.to(fill_key, FAST, Color::BLACK);
        let _ = m.to(size_key, FAST, 40.0_f32);

        let fill = m.to(fill_key, FAST, Color::WHITE);
        let side = m.to(size_key, FAST, 80.0_f32);

        let element: iced::Element<'_, ()> = shape()
            .width(side.clone())
            .height(40.0)
            .fill(fill.clone())
            .into();

        // Building the simulator lays the interface out (`UserInterface::build`
        // computes the root layout), which is when a widget declares what it
        // reads and where.
        let _ui: iced_test::Simulator<'_, ()> = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(200.0, 200.0),
            element,
        );

        assert_eq!(
            fill.tier(),
            Some(Tier::Paint),
            "a colour is read in `draw`, so it costs a redraw and nothing more"
        );
        assert_eq!(
            side.tier(),
            Some(Tier::Layout),
            "a width is read in `layout`, so it costs a relayout too"
        );

        let start = Instant::now();
        let _ = m.tick(start);
        let status = m.tick(start + Duration::from_millis(16));

        assert!(status.animating);
        assert!(
            status.layout_invalid,
            "the width is what makes this frame need a relayout"
        );
    }
}
