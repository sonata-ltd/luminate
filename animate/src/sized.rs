//! Animated sizing for any child, including stock iced widgets.
//!
//! Most iced widgets take their `Length` and `Padding` when they are
//! *constructed*, which means those values are captured while the view is
//! being built. An animated value read there is a snapshot: it freezes until
//! the application next rebuilds the view, which is exactly what the motion
//! engine exists to avoid.
//!
//! [`Sized`] closes that gap. It owns the layout for its child and resolves
//! its own [`AnimLength`] values inside `layout`, so the child is re-measured
//! against fresh numbers every frame:
//!
//! ```
//! use iced::widget::text;
//! use iced::Padding;
//! use iced_animate::widget::sized;
//! use iced_animate::{curves::SMOOTH, key, motion_set, Motion};
//!
//! motion_set! {
//!     struct RowStyle -> RowStyleAnim {
//!         row_height: f32,
//!         row_pad: Padding,
//!     }
//! }
//! const OPEN: RowStyle = RowStyle { row_height: 48.0, row_pad: Padding::new(8.0) };
//! const CLOSED: RowStyle = RowStyle { row_height: 24.0, row_pad: Padding::ZERO };
//!
//! let m = Motion::new();
//! let open = true;
//! let s = m.to_set(key!(), SMOOTH, if open { OPEN } else { CLOSED });
//!
//! let _: iced::Element<'_, ()> = sized(text("row"))
//!     .height(s.row_height)
//!     .padding(s.row_pad)
//!     .into();
//! ```
//!
//! The child needs to know nothing about animation.

use std::ops::Sub;

use iced_core::widget::{Tree, tree};
use iced_core::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced_core::{Element, Event, Length, Padding, Rectangle, Size, Vector};

use crate::{Anim, AnimLength, Tier, scale_factor, snap_between};

/// A bouncy spring may overshoot below zero; padding cannot.
fn non_negative(padding: Padding) -> Padding {
    Padding {
        top: padding.top.max(0.0),
        right: padding.right.max(0.0),
        bottom: padding.bottom.max(0.0),
        left: padding.left.max(0.0),
    }
}

/// Where each value a snapped box animates last rested: the start of the
/// motion it is on now, which the snapping grid passes through as well as
/// the target (see [`snap_between`](crate::snap_between)).
#[derive(Debug, Default, Clone, Copy)]
struct Rest {
    width: Option<f32>,
    height: Option<f32>,
    padding: Option<Padding>,
    collapsed: Option<f32>,
}

/// The length a box takes around content that asks for `length`: a fill,
/// portion and all, is inherited; a shrink or a fixed size shrinks around
/// the content.
///
/// `iced`'s `Container` widens a `FillPortion` to `Fill`, which silently
/// evens out a row of wrapped portions; keeping it keeps the split.
fn inherited(length: Length) -> Length {
    match length {
        Length::Fill | Length::FillPortion(_) => length,
        Length::Shrink | Length::Fixed(_) => Length::Shrink,
    }
}

/// Wraps `content` so its size and padding can be animated.
#[must_use]
pub fn sized<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> Sized<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    Sized::new(content)
}

/// The paint-only offset of the content, one track handle per axis so the
/// axes can be set independently.
#[derive(Debug)]
struct OffsetValue {
    x: Anim<f32>,
    y: Anim<f32>,
}

impl OffsetValue {
    fn new() -> Self {
        Self {
            x: Anim::constant(0.0),
            y: Anim::constant(0.0),
        }
    }

    fn from_vector(vec: &Anim<Vector>) -> Self {
        Self {
            x: vec.map(|v| v.x),
            y: vec.map(|v| v.y),
        }
    }

    fn set_x(&mut self, x: Anim<f32>) -> &mut Self {
        self.x = x;
        self
    }

    fn set_y(&mut self, y: Anim<f32>) -> &mut Self {
        self.y = y;
        self
    }

    fn get_vector(&self) -> Vector {
        let finite = |value: f32| if value.is_finite() { value } else { 0.0 };
        Vector::new(finite(self.x.get()), finite(self.y.get()))
    }
}

/// A container whose width, height and padding are resolved every frame, and
/// whose content can be offset without a relayout.
///
/// See the [`widget`](crate::widget) module for why this is needed at all.
pub struct Sized<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    width: AnimLength,
    height: AnimLength,
    padding: Anim<Padding>,
    collapse: Anim<f32>,
    offset: OffsetValue,
    pixel_snap: bool,
    content: Element<'a, Message, Theme, Renderer>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Sized<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sized")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("padding", &self.padding)
            .field("collapse", &self.collapse)
            .field("pixel_snap", &self.pixel_snap)
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl<'a, Message, Theme, Renderer> Sized<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Wraps `content`, as fluid as it: filling an axis the content fills,
    /// and shrinking to fit it otherwise.
    ///
    /// This is what an iced `Container` does, and for the same reason, except
    /// that a `FillPortion` keeps its portion where `Container` widens it to
    /// `Fill`. A box
    /// that shrank around a filling content would lay it out compressed, and
    /// a fill with no size of its own, a progress bar, compresses to
    /// nothing. A content's *fixed* size is deliberately not inherited: a box
    /// fixed at the content's size would have no room left for the
    /// [`padding`](Self::padding) and squash the content into it.
    ///
    /// [`width`](Self::width) and [`height`](Self::height) override either
    /// axis.
    #[must_use]
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        let content = content.into();
        let content_size = content.as_widget().size_hint();

        Self {
            width: inherited(content_size.width).into(),
            height: inherited(content_size.height).into(),
            padding: Anim::constant(Padding::ZERO),
            offset: OffsetValue::new(),
            collapse: Anim::constant(1.0),
            pixel_snap: false,
            content,
        }
    }

    /// Sets the width, which may be an animated value.
    #[must_use]
    pub fn width(mut self, width: impl Into<AnimLength>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height, which may be an animated value.
    #[must_use]
    pub fn height(mut self, height: impl Into<AnimLength>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets the padding, which may be an animated value.
    #[must_use]
    pub fn padding(mut self, padding: impl Into<Anim<Padding>>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Moves the content by `offset` where it is drawn, leaving the layout
    /// alone.
    ///
    /// The offset is paint-only: this widget reports the same size, and its
    /// siblings stay where they are, so an animated offset costs a redraw per
    /// frame and never a relayout (`Tier::Paint`). Hand it a track of its
    /// own; a track that also feeds a size or a padding is `Tier::Layout`
    /// for all its readers.
    ///
    /// Pointer input follows the content: it is hovered and clicked where it
    /// is drawn, and overlays such as a pick list's menu open next to it.
    /// Three things keep the original position:
    ///
    /// - Parents route input by *layout* bounds. A `Stack` or a `Scrollable`
    ///   may never pass a click to content drawn far outside this box.
    /// - [`operate`](Widget::operate), so focus and scroll-to operations see
    ///   the content where it is laid out.
    /// - Touch events, whose position travels in the event and not in the
    ///   cursor.
    ///
    /// Under a [`collapse`](Self::collapse) below `1.0` the clip stays with
    /// the box and the content moves inside it, so an offset slides content
    /// in from behind an edge rather than past it.
    ///
    /// A non-finite component is drawn as `0.0`. This replaces both axes;
    /// [`offset_x`](Self::offset_x) and [`offset_y`](Self::offset_y) replace
    /// one.
    ///
    /// ```
    /// use iced::Vector;
    /// use iced::widget::text;
    /// use iced_animate::widget::sized;
    /// use iced_animate::{curves::QUICK, key, Motion};
    ///
    /// let m = Motion::new();
    /// let shown = true;
    /// // A label that drops into place from 8 px above once it is shown,
    /// // without moving anything around it.
    /// let target = if shown { Vector::ZERO } else { Vector::new(0.0, -8.0) };
    /// let drop_in = m.to(key!(), QUICK, target);
    ///
    /// let _: iced::Element<'_, ()> = sized(text("label")).offset(drop_in).into();
    /// ```
    #[must_use]
    pub fn offset(mut self, offset: impl Into<Anim<Vector>>) -> Self {
        self.offset = OffsetValue::from_vector(&offset.into());
        self
    }

    /// Sets the horizontal content offset, which may be an animated value,
    /// and keeps the vertical one.
    ///
    /// See [`offset`](Self::offset) for what an offset does and does not
    /// move.
    #[must_use]
    pub fn offset_x(mut self, offset: impl Into<Anim<f32>>) -> Self {
        self.offset.set_x(offset.into());
        self
    }

    /// Sets the vertical content offset, which may be an animated value, and
    /// keeps the horizontal one.
    ///
    /// See [`offset`](Self::offset) for what an offset does and does not
    /// move.
    #[must_use]
    pub fn offset_y(mut self, offset: impl Into<Anim<f32>>) -> Self {
        self.offset.set_y(offset.into());
        self
    }

    /// Scales the height this widget *reports*, from its measured content
    /// height down to nothing.
    ///
    /// This is the "collapse to zero" that a plain height cannot express: the
    /// natural height of a piece of content is only known after it has been
    /// laid out, so it cannot be written down in the view as an animation
    /// target. `collapse` animates the factor instead, `1.0` is the content's
    /// own height, `0.0` is nothing, and the content keeps its natural
    /// layout, so text does not reflow on the way out.
    ///
    /// The content is anchored at the top and clipped, so the box shrinks
    /// upward and whatever follows it slides up. That is the point: unlike a
    /// fade, a collapse moves its siblings, which is why it necessarily costs
    /// a relayout per frame.
    ///
    /// Pointer input is clipped along with the paint, clipped-away content
    /// cannot be clicked or hovered, and a fully collapsed box (factor `0.0`
    /// or below) shows no overlays, a pick list's menu stays closed.
    /// Keyboard focus is not: a text input inside a fully collapsed box is
    /// still reachable by tab, so take it out of the view once its exit has
    /// finished.
    ///
    /// ```
    /// use iced::widget::text;
    /// use iced_animate::widget::sized;
    /// use iced_animate::{curves::FADE, key, Motion};
    ///
    /// let m = Motion::new();
    /// // A list row that collapses out (pair with a compositor-tier opacity
    /// // to fade it at the same time).
    /// let leaving = m.retire(key!(), FADE, 0.0_f32);
    ///
    /// let _: iced::Element<'_, ()> = sized(text("row")).collapse(leaving).into();
    /// ```
    #[must_use]
    pub fn collapse(mut self, factor: impl Into<Anim<f32>>) -> Self {
        self.collapse = factor.into();
        self
    }

    /// Moves this box in whole device pixels while its size, padding or
    /// collapse animates.
    ///
    /// Text in iced sits on whole device pixels vertically, and everything
    /// else is drawn wherever the layout puts it. A box that glides by
    /// fractions of a pixel moves whatever is laid out after it smoothly and
    /// that content's text in whole-pixel jumps, out of step with it: the
    /// text snaps against its own background, most visibly in the slow tail
    /// of a spring. Snapped, every frame differs from the last by whole
    /// device pixels, the steps the text takes anyway, and everything moves
    /// together.
    ///
    /// Each value is snapped on a grid through where it last rested for the
    /// first half of its way and through its target for the second, so the
    /// box leaves exactly from its resting size and rests exactly on the one
    /// it was animated to, and the one step that is not a whole pixel falls
    /// in the middle of the motion, where it is fastest (see
    /// [`snap_between`](crate::snap_between)). Only animating values are
    /// snapped, and only while they move. How large a device pixel is comes
    /// from [`scale_factor`](crate::scale_factor), which
    /// `iced_texture_cache`'s renderer keeps up to date.
    ///
    /// Reach for it where a box moves text *vertically* — a row collapsing
    /// above a list, a panel growing under a heading. Horizontally text is
    /// placed in quarter-pixel steps, so a width that glides does not leave
    /// its text behind, and snapping it only makes the motion coarser.
    ///
    /// ```
    /// use iced::widget::text;
    /// use iced_animate::widget::sized;
    /// use iced_animate::{curves::SMOOTH, key, Motion};
    ///
    /// let m = Motion::new();
    /// // A banner collapsing above a list: the list below moves a whole
    /// // device pixel at a time, text and backgrounds together.
    /// let height = m.to(key!(), SMOOTH, 0.0_f32);
    ///
    /// let _: iced::Element<'_, ()> = sized(text("banner"))
    ///     .height(height)
    ///     .pixel_snap(true)
    ///     .into();
    /// ```
    #[must_use]
    pub fn pixel_snap(mut self, snap: bool) -> Self {
        self.pixel_snap = snap;
        self
    }

    /// `length` for this frame. While it animates under
    /// [`pixel_snap`](Self::pixel_snap) it is snapped to whole device pixels
    /// between where it rested (`rest`) and its target; at rest it is
    /// recorded there, as the start of the next motion.
    fn resolve(&self, length: &AnimLength, rest: &mut Option<f32>) -> Length {
        let AnimLength::Fixed(value) = length else {
            *rest = None;
            return length.resolve();
        };

        if !value.is_animating() {
            let resting = value.get().max(0.0);
            *rest = Some(resting);
            return Length::Fixed(resting);
        }

        if !self.pixel_snap {
            return length.resolve();
        }

        let from = rest.unwrap_or(f32::NAN);

        Length::Fixed(snap_between(value.get(), from, value.target(), scale_factor()).max(0.0))
    }

    /// The padding for this frame, snapped and recorded like
    /// [`resolve`](Self::resolve).
    fn padding_now(&self, rest: &mut Option<Padding>) -> Padding {
        let padding = self.padding.get();

        if !self.padding.is_animating() {
            *rest = Some(padding);
            return padding;
        }

        if !self.pixel_snap {
            return padding;
        }

        let (target, scale) = (self.padding.target(), scale_factor());
        // Never rested: nothing to leave from, so every side snaps to its
        // target (see `snap_between`).
        let from = rest.unwrap_or(Padding::new(f32::NAN));

        Padding {
            top: snap_between(padding.top, from.top, target.top, scale),
            right: snap_between(padding.right, from.right, target.right, scale),
            bottom: snap_between(padding.bottom, from.bottom, target.bottom, scale),
            left: snap_between(padding.left, from.left, target.left, scale),
        }
    }

    /// The height a collapse at `factor` leaves of `height`, snapped and
    /// recorded like [`resolve`](Self::resolve).
    fn collapsed(&self, height: f32, factor: f32, rest: &mut Option<f32>) -> f32 {
        let collapsing = factor.is_finite() && factor < 1.0;
        let collapsed = if collapsing {
            height * factor.max(0.0)
        } else {
            height
        };

        if !self.collapse.is_animating() {
            *rest = Some(collapsed);
            return collapsed;
        }

        if !(self.pixel_snap && collapsing) {
            return collapsed;
        }

        let target = height * self.collapse.target().clamp(0.0, 1.0);
        let from = rest.unwrap_or(f32::NAN);

        snap_between(collapsed, from, target, scale_factor()).max(0.0)
    }

    /// Flags every animated value here as one the layout depends on.
    fn mark_tiers(&self) {
        self.width.mark_layout_tier();
        self.height.mark_layout_tier();

        self.padding.mark_tier(Tier::Layout);
        self.collapse.mark_tier(Tier::Layout);
    }

    /// The cursor as the content is allowed to see it.
    ///
    /// A collapsed box is smaller than the content laid out inside it, and
    /// that content deliberately keeps its original layout nodes, that is
    /// what lets it slide up behind the clip instead of being squashed. Its
    /// *hit-boxes* keep those positions too, so without this a list row that
    /// has finished leaving still sits over whatever moved up into its place
    /// and swallows the click meant for its neighbour.
    ///
    /// `levitate` is the iced idiom for "clipped away": the position survives,
    /// so a widget already mid-press still receives its release, but every
    /// `is_over` / `position_over` test reports nothing.
    fn clipped_cursor(&self, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Cursor {
        if self.collapse.get() >= 1.0 || cursor.is_over(bounds) {
            cursor
        } else {
            cursor.levitate()
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Sized<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<Rest>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(Rest::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width.resolve(), self.height.resolve())
    }

    fn size_hint(&self) -> Size<Length> {
        // Not the same as `size`: an animated axis must never be advertised as
        // `Fixed(0.0)`, or the parent container drops this widget outright.
        // See [`AnimLength::size_hint`].
        // A collapsing height is derived from the content, so there is no
        // number to advertise, and at factor 0 a concrete one would be the
        // void hint that gets this widget deleted.
        let height = if self.collapse.is_live() {
            Length::Shrink
        } else {
            self.height.size_hint()
        };

        Size::new(self.width.size_hint(), height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.mark_tiers();

        let mut rest = *tree.state.downcast_ref::<Rest>();

        let width = self.resolve(&self.width, &mut rest.width);
        let height = self.resolve(&self.height, &mut rest.height);
        let padding = non_negative(self.padding_now(&mut rest.padding));

        // `layout::padded`, inlined: the collapsed box is then built in the
        // same pass instead of cloning the content's subtree out of a
        // finished node every frame of a collapse.
        let limits = limits.width(width).height(height);
        let content = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &limits.shrink(padding),
        );
        let padding = padding.fit(content.size(), limits.max());
        let size = limits
            .shrink(padding)
            .resolve(width, height, content.size())
            .expand(padding);

        // Keep the content exactly where it is and shrink only the box
        // around it, so it slides up behind the clip rather than being
        // squashed. A non-finite factor (only possible from a constant; the
        // engine sanitises its tracks) means "not collapsed".
        let factor = self.collapse.get();
        let size = Size::new(
            size.width,
            self.collapsed(size.height, factor, &mut rest.collapsed),
        );

        *tree.state.downcast_mut::<Rest>() = rest;

        layout::Node::with_children(size, vec![content.move_to((padding.left, padding.top))])
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let Some(content_layout) = layout.children().next() else {
            return;
        };

        let offset = self.offset.get_vector();

        let translated_cursor = self.clipped_cursor(layout.bounds(), cursor).sub(offset);
        let translated_viewport = (*viewport) - offset;

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content_layout,
            translated_cursor,
            renderer,
            clipboard,
            shell,
            &translated_viewport,
        );
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let Some(content_layout) = layout.children().next() else {
            return;
        };

        let offset = self.offset.get_vector();
        self.offset.x.mark_tier(Tier::Paint);
        self.offset.y.mark_tier(Tier::Paint);

        // Clipped-away content must not paint itself hovered either.
        let cursor = self.clipped_cursor(layout.bounds(), cursor);

        let draw = |renderer: &mut Renderer| {
            if offset == Vector::ZERO {
                self.content.as_widget().draw(
                    &tree.children[0],
                    renderer,
                    theme,
                    style,
                    content_layout,
                    cursor,
                    viewport,
                );
            } else {
                let translated_cursor = cursor.sub(offset);
                let translated_viewport = *viewport - offset;

                renderer.with_translation(offset, |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        content_layout,
                        translated_cursor,
                        &translated_viewport,
                    );
                });
            }
        };

        // A collapsed box is smaller than its content, so the overflow has to
        // be clipped or it would spill over whatever comes next.
        if self.collapse.get() < 1.0 {
            renderer.with_layer(layout.bounds(), draw);
        } else {
            draw(renderer);
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced_core::widget::Operation,
    ) {
        let Some(content_layout) = layout.children().next() else {
            return;
        };

        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            content_layout,
            renderer,
            operation,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let offset = self.offset.get_vector();

        let translated_cursor = self.clipped_cursor(layout.bounds(), cursor).sub(offset);
        let translated_viewport = (*viewport) - offset;

        layout
            .children()
            .next()
            .map_or_else(mouse::Interaction::default, |content_layout| {
                self.content.as_widget().mouse_interaction(
                    &tree.children[0],
                    content_layout,
                    translated_cursor,
                    &translated_viewport,
                    renderer,
                )
            })
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        // Nothing of a fully collapsed box is visible; its overlays must not
        // float above whatever slid up into its place.
        if self.collapse.get() <= 0.0 {
            return None;
        }

        let offset = self.offset.get_vector();

        let content_layout = layout.children().next()?;

        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            content_layout,
            renderer,
            viewport,
            translation + offset,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<Sized<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(sized: Sized<'a, Message, Theme, Renderer>) -> Self {
        Element::new(sized)
    }
}

// `iced_core` implements `Renderer for ()` only in debug builds.
#[cfg(all(test, debug_assertions))]
mod tests {
    use iced_core::widget::Tree;
    use iced_core::{Length, Padding, Point, Rectangle, Size, Vector, Widget, layout};

    use super::{Sized, sized};
    use crate::testing::FrameClock;
    use crate::widget::shape;
    use crate::{Anim, Motion, curves::QUICK, key};

    type Boxed<'a> = Sized<'a, (), (), ()>;

    fn layout_of(widget: &mut Boxed<'_>) -> layout::Node {
        let mut tree = Tree::new(&*widget as &dyn Widget<(), (), ()>);
        Widget::layout(
            widget,
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 400.0)),
        )
    }

    fn content() -> Boxed<'static> {
        sized(shape().width(100.0).height(40.0)).padding(Padding::new(10.0))
    }

    #[test]
    fn a_collapse_factor_shrinks_the_box_and_leaves_the_content_in_place() {
        let mut full = content();
        let node = layout_of(&mut full);
        assert_eq!(node.size(), Size::new(120.0, 60.0));

        let mut half = content().collapse(0.5);
        let node = layout_of(&mut half);
        assert_eq!(
            node.size(),
            Size::new(120.0, 30.0),
            "half the padded height"
        );
        assert_eq!(
            node.children()[0].bounds(),
            Rectangle::new(Point::new(10.0, 10.0), Size::new(100.0, 40.0)),
            "the content keeps its natural layout behind the clip"
        );
    }

    #[test]
    fn a_non_finite_collapse_factor_is_ignored() {
        let mut nan = content().collapse(f32::NAN);
        assert_eq!(layout_of(&mut nan).size(), Size::new(120.0, 60.0));
        let mut negative = content().collapse(-1.0);
        assert_eq!(layout_of(&mut negative).size(), Size::new(120.0, 0.0));
    }

    #[test]
    fn negative_padding_is_clamped_to_zero() {
        let mut widget = sized(shape().width(100.0).height(40.0)).padding(Padding::new(-5.0));
        let node = layout_of(&mut widget);
        assert_eq!(node.size(), Size::new(100.0, 40.0));
        assert_eq!(node.children()[0].bounds().x, 0.0);
    }

    #[test]
    fn a_live_collapse_hints_shrink_but_a_constant_height_hints_itself() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, QUICK, 1.0_f32);
        let live = m.to(key, QUICK, 0.0_f32);

        let collapsing: Boxed<'_> = sized(shape()).height(40.0).collapse(live);
        assert_eq!(
            Widget::<(), (), ()>::size_hint(&collapsing).height,
            Length::Shrink
        );

        let steady: Boxed<'_> = sized(shape()).height(40.0).collapse(Anim::constant(0.5));
        assert_eq!(
            Widget::<(), (), ()>::size_hint(&steady).height,
            Length::Fixed(40.0)
        );
    }

    #[test]
    fn a_filling_content_makes_the_box_fill() {
        let mut bar = sized(shape().width(Length::Fill).height(5.0));
        assert_eq!(
            Widget::<(), (), ()>::size(&bar),
            Size::new(Length::Fill, Length::Shrink),
            "a fixed height is not copied, only the fill is"
        );

        let node = layout_of(&mut bar);
        assert_eq!(node.size(), Size::new(400.0, 5.0));
        assert_eq!(node.children()[0].bounds().width, 400.0);
    }

    /// A box that shrank around a filling content would lay it out
    /// compressed, and a fill with nothing intrinsic compresses to nothing:
    /// a progress bar wrapped in `sized` used to be zero pixels wide.
    #[test]
    fn a_filling_content_fills_its_column() {
        let mut column: iced::widget::Column<'_, (), (), ()> =
            iced::widget::Column::new().push(sized(shape().width(Length::Fill).height(5.0)));

        let mut tree = Tree::new(&column as &dyn Widget<(), (), ()>);
        let node = Widget::layout(
            &mut column,
            &mut tree,
            &(),
            &layout::Limits::new(Size::ZERO, Size::new(400.0, 400.0)),
        );

        assert_eq!(node.children()[0].bounds().width, 400.0);
    }

    #[test]
    fn a_filling_content_is_inset_by_the_padding() {
        let mut bar = sized(shape().width(Length::Fill).height(5.0)).padding(Padding::new(10.0));
        let node = layout_of(&mut bar);

        assert_eq!(node.size(), Size::new(400.0, 25.0));
        assert_eq!(
            node.children()[0].bounds(),
            Rectangle::new(Point::new(10.0, 10.0), Size::new(380.0, 5.0))
        );
    }

    /// A fixed size is not inherited: a box fixed at the content's size would
    /// have no room left for the padding, and squash the content into it.
    #[test]
    fn a_fixed_content_leaves_the_box_shrinking() {
        let fixed = content();
        assert_eq!(
            Widget::<(), (), ()>::size(&fixed),
            Size::new(Length::Shrink, Length::Shrink)
        );
    }

    #[test]
    fn an_explicit_width_overrides_the_inherited_fill() {
        let mut narrow = sized(shape().width(Length::Fill).height(5.0)).width(50.0);
        let node = layout_of(&mut narrow);

        assert_eq!(node.size(), Size::new(50.0, 5.0));
        assert_eq!(node.children()[0].bounds().width, 50.0);
    }

    /// Every frame of a snapped motion is a whole number of device pixels
    /// from where it is going, and it comes to rest exactly there; unsnapped,
    /// the same motion passes through fractions.
    ///
    /// One test for every scale factor and every snapped value: the factor
    /// is process-wide, and nothing else in this binary snaps.
    #[test]
    fn a_pixel_snapped_box_moves_in_whole_device_pixels_and_rests_on_its_target() {
        use crate::curves::SMOOTH;

        let heights = |snap: bool| {
            let m = Motion::new();
            let key = key!();
            let _ = m.to(key, SMOOTH, 40.0_f32);
            let height = m.to(key, SMOOTH, 18.3_f32);
            let mut clock = FrameClock::new(&m);

            let mut frames = Vec::new();
            while height.is_animating() {
                let _ = clock.run(1);
                let mut widget = sized(shape().width(10.0).height(4.0))
                    .height(height.clone())
                    .pixel_snap(snap);
                frames.push(layout_of(&mut widget).size().height);
            }
            frames
        };

        for scale in [1.0_f32, 1.25, 2.0] {
            crate::set_scale_factor(scale);

            let snapped = heights(true);
            let (moving, rest) = snapped.split_at(snapped.len() - 1);
            for height in moving {
                let steps = (height - 18.3) * scale;
                assert!(
                    (steps - steps.round()).abs() < 1e-3,
                    "{scale}x: {height} is {steps} device px from the target"
                );
            }
            assert_eq!(rest, [18.3], "{scale}x: it rests on its target");

            let glided = heights(false);
            padding_and_collapse_snap_at(scale);
            leaves_its_rest_on_its_grid_at(scale);

            assert!(
                glided.iter().any(|height| {
                    let steps = (height - 18.3) * scale;
                    (steps - steps.round()).abs() > 0.05
                }),
                "{scale}x: unsnapped, the motion passes through fractions"
            );
        }

        crate::set_scale_factor(1.0);
    }

    /// A snapped box that has been resting leaves exactly from there: the
    /// first half of its motion is whole device pixels from where it rested,
    /// the second from where it is going. Anchored at the target alone, its
    /// first frame would jump by the fraction the distance is off the grid.
    fn leaves_its_rest_on_its_grid_at(scale: f32) {
        use crate::curves::SMOOTH;

        let m = Motion::new();
        let key = key!();
        let resting = m.to(key, SMOOTH, 40.0_f32);

        let snapped = |height: &Anim<f32>| -> Boxed<'static> {
            sized(shape().width(10.0).height(4.0))
                .height(height.clone())
                .pixel_snap(true)
        };

        // One tree for every frame, as an application keeps it: the frame at
        // rest is what records where the motion starts.
        let first = snapped(&resting);
        let mut tree = Tree::new(&first as &dyn Widget<(), (), ()>);
        let limits = layout::Limits::new(Size::ZERO, Size::new(400.0, 400.0));
        let mut lay = |mut widget: Boxed<'static>| {
            Widget::diff(&widget, &mut tree);
            Widget::layout(&mut widget, &mut tree, &(), &limits)
                .size()
                .height
        };

        assert_eq!(lay(snapped(&resting)), 40.0, "{scale}x: at rest");

        let height = m.to(key, SMOOTH, 18.3_f32);
        let mut clock = FrameClock::new(&m);
        let mut moving = Vec::new();
        let _ = clock.run(1);
        while height.is_animating() {
            moving.push(lay(snapped(&height)));
            let _ = clock.run(1);
        }
        assert_eq!(
            lay(snapped(&height)),
            18.3,
            "{scale}x: it rests on its target"
        );

        for h in &moving {
            let anchor = if (h - 40.0).abs() < (h - 18.3).abs() {
                40.0
            } else {
                18.3
            };
            let steps = (h - anchor) * scale;
            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "{scale}x: {h} is {steps} device px from {anchor}"
            );
        }
        assert!(
            moving.iter().any(|h| (h - 40.0).abs() < (h - 18.3).abs()),
            "{scale}x: some frames belong to the start's half"
        );
    }

    #[test]
    fn a_fill_portion_is_kept() {
        let portioned = sized(shape().width(Length::FillPortion(3)).height(5.0));

        assert_eq!(
            Widget::<(), (), ()>::size(&portioned).width,
            Length::FillPortion(3),
            "the portion survives the wrap"
        );
    }

    /// The padding and the collapse of a snapped box move in whole device
    /// pixels at `scale`; run from the test that owns the scale factor.
    fn padding_and_collapse_snap_at(scale: f32) {
        use crate::curves::SMOOTH;

        let m = Motion::new();
        let (pad, fold) = (key!(), key!());
        let _ = m.to(pad, SMOOTH, Padding::new(20.0));
        let _ = m.to(fold, SMOOTH, 1.0_f32);
        let padding = m.to(pad, SMOOTH, Padding::new(3.7));
        let factor = m.to(fold, SMOOTH, 0.0_f32);
        let mut clock = FrameClock::new(&m);

        for _ in 0..6 {
            let _ = clock.run(1);

            let mut padded = sized(shape().width(10.0).height(10.0))
                .padding(padding.clone())
                .pixel_snap(true);
            let node = layout_of(&mut padded);
            let top = node.children()[0].bounds().y;
            let steps = (top - 3.7) * scale;
            assert!(
                (steps - steps.round()).abs() < 1e-3,
                "{scale}x: the padding moved to {top}"
            );

            let mut folding = sized(shape().width(10.0).height(40.0))
                .collapse(factor.clone())
                .pixel_snap(true);
            let height = layout_of(&mut folding).size().height * scale;
            assert!(
                (height - height.round()).abs() < 1e-3,
                "{scale}x: the collapse left {height} device px"
            );
        }
    }

    #[test]
    fn an_offset_leaves_the_layout_alone() {
        let mut plain = content();
        let mut moved = content().offset(Vector::new(30.0, -15.0));

        let (plain_node, moved_node) = (layout_of(&mut plain), layout_of(&mut moved));
        assert_eq!(moved_node.size(), plain_node.size());
        assert_eq!(
            moved_node.children()[0].bounds(),
            plain_node.children()[0].bounds(),
            "the content is moved where it is drawn, not where it is laid out"
        );
        assert_eq!(
            Widget::<(), (), ()>::size_hint(&moved),
            Widget::<(), (), ()>::size_hint(&plain)
        );
    }

    #[test]
    fn offset_axes_are_set_independently() {
        let both = content().offset_x(3.0).offset_y(4.0);
        assert_eq!(both.offset.get_vector(), Vector::new(3.0, 4.0));

        let x_after = content().offset(Vector::new(1.0, 2.0)).offset_x(5.0);
        assert_eq!(
            x_after.offset.get_vector(),
            Vector::new(5.0, 2.0),
            "an axis setter keeps the other axis"
        );

        let vector_after = content().offset_x(5.0).offset(Vector::new(1.0, 2.0));
        assert_eq!(
            vector_after.offset.get_vector(),
            Vector::new(1.0, 2.0),
            "`offset` replaces both axes"
        );
    }

    #[test]
    fn a_non_finite_offset_reads_as_zero() {
        let widget = content().offset_x(f32::NAN).offset_y(f32::INFINITY);
        assert_eq!(widget.offset.get_vector(), Vector::ZERO);

        let partly = content().offset(Vector::new(f32::NEG_INFINITY, 7.0));
        assert_eq!(partly.offset.get_vector(), Vector::new(0.0, 7.0));
    }

    /// The view is built once and the engine then ticks without rebuilding
    /// it: an offset that snapshotted its value while the view was built
    /// would stay at the start forever.
    #[test]
    fn an_animated_offset_moves_without_a_rebuild() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, QUICK, Vector::ZERO);
        let live = m.to(key, QUICK, Vector::new(20.0, 100.0));

        let mut widget = content().offset(live);
        assert_eq!(widget.offset.get_vector(), Vector::ZERO);

        // The layout marks the tiers of everything it reads; the offset must
        // not be among them.
        let _ = layout_of(&mut widget);

        let mut clock = FrameClock::new(&m);
        let status = clock.run(3);
        assert!(status.animating);
        assert!(
            !status.layout_invalid,
            "an offset-only track asked for a relayout"
        );

        let midway = widget.offset.get_vector();
        assert!(
            midway.y > 0.0 && midway.y < 100.0,
            "the offset did not move: {midway:?}"
        );

        let _ = clock.run_until_settled();
        assert_eq!(widget.offset.get_vector(), Vector::new(20.0, 100.0));
    }

    #[test]
    fn an_animated_axis_offset_moves_without_a_rebuild() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, QUICK, 0.0_f32);
        let live = m.to(key, QUICK, -40.0_f32);

        let widget = content().offset_y(live);

        let mut clock = FrameClock::new(&m);
        let _ = clock.run(3);
        let midway = widget.offset.get_vector().y;
        assert!(
            midway < 0.0 && midway > -40.0,
            "the offset did not move: {midway}"
        );

        let _ = clock.run_until_settled();
        assert_eq!(widget.offset.get_vector(), Vector::new(0.0, -40.0));
    }
}

#[cfg(test)]
/// Tests that drive the widget through `iced_test`.
mod simulator_tests {
    use std::time::Duration;

    use crate::{Curve, Motion, SpringParams, key};

    /// A fast spring for tests; not the shipped `curves::SMOOTH`.
    const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

    /// A row collapsed to nothing keeps its children's layout nodes exactly where
    /// they were, that is what lets the content slide up behind the clip instead
    /// of being squashed. It must not keep their hit-boxes too: a list row that
    /// has finished leaving sits on top of whatever moved up into its place, and
    /// would swallow the click meant for its neighbour.
    #[test]
    fn a_collapsed_row_does_not_intercept_clicks() {
        use iced::widget::{button, column};

        #[derive(Debug, Clone, PartialEq)]
        enum Message {
            Ghost,
            Live,
        }

        let m = Motion::new();

        // The gone row: collapsed to zero, still built because nothing has
        // rebuilt the view since its exit finished.
        let gone: iced::Element<'_, Message> =
            crate::widget::sized(button("gone").on_press(Message::Ghost))
                .collapse(m.to(key!(), FAST, 0.0_f32))
                .into();

        let live: iced::Element<'_, Message> =
            crate::widget::sized(button("live").on_press(Message::Live)).into();

        let mut ui: iced_test::Simulator<'_, Message> = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(400.0, 200.0),
            column![gone, live].width(iced::Length::Fill),
        );

        let _ = ui.click("live").expect("the live button is on screen");

        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![Message::Live],
            "the click landed on the collapsed row above instead"
        );
    }

    /// Pointer input follows the content to where it is drawn: the spot it
    /// is laid out at is empty.
    #[test]
    fn an_offset_button_is_clicked_where_it_is_drawn() {
        use iced::widget::{button, column};

        #[derive(Debug, Clone, PartialEq)]
        struct Pressed;

        // A fresh interface per click, so each position answers for itself.
        let clicks_at = |y: f32| {
            // Laid out at y 0..40, drawn at y 100..140.
            let moved: iced::Element<'_, Pressed> =
                crate::widget::sized(button("moved").on_press(Pressed).width(100).height(40))
                    .offset_y(100.0)
                    .into();

            let mut ui: iced_test::Simulator<'_, Pressed> = iced_test::Simulator::with_size(
                iced::Settings::default(),
                iced::Size::new(400.0, 300.0),
                column![moved].width(iced::Length::Fill),
            );

            ui.point_at(iced::Point::new(50.0, y));
            let _ = ui.simulate(iced_test::simulator::click());
            ui.into_messages().count()
        };

        assert_eq!(clicks_at(120.0), 1, "the drawn button missed its click");
        assert_eq!(
            clicks_at(20.0),
            0,
            "the spot it is laid out at took a click"
        );
    }

    /// Under a collapse the clip stays with the box: the part of the offset
    /// content that hangs below it is invisible, and so cannot be clicked.
    #[test]
    fn a_collapsed_box_clips_input_to_the_box_not_to_the_offset_content() {
        use iced::widget::{button, column};

        #[derive(Debug, Clone, PartialEq)]
        struct Pressed;

        // Box y 0..20 (half of 40), button drawn at y 10..50.
        let clipped: iced::Element<'_, Pressed> =
            crate::widget::sized(button("clipped").on_press(Pressed).width(100).height(40))
                .collapse(0.5)
                .offset_y(10.0)
                .into();

        let mut ui: iced_test::Simulator<'_, Pressed> = iced_test::Simulator::with_size(
            iced::Settings::default(),
            iced::Size::new(400.0, 300.0),
            column![clipped].width(iced::Length::Fill),
        );

        // Drawn there, but below the box.
        ui.point_at(iced::Point::new(50.0, 35.0));
        let _ = ui.simulate(iced_test::simulator::click());
        // Drawn there and inside the box.
        ui.point_at(iced::Point::new(50.0, 15.0));
        let _ = ui.simulate(iced_test::simulator::click());

        assert_eq!(
            ui.into_messages().collect::<Vec<_>>(),
            vec![Pressed],
            "only the visible part of the button counts"
        );
    }
}
