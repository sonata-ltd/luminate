//! [`ContentStack`]: a sliding page stack that never moves its pages.
//!
//! The rationale lives on the type, which is what the crate re-exports.

use iced_animate::curves::STRUCTURAL;
use iced_animate::{Anim, Curve, Motion, MotionKey, Tier};
use iced_core::layout::{Layout, Limits, Node};
use iced_core::widget::{Id, Operation, Tree, tree};
use iced_core::{
    Clipboard, Element, Event, Length, Pixels, Point, Rectangle, Shell, Size, Transformation,
    Vector, Widget, mouse, overlay, renderer, window,
};

use crate::ancestors;
use crate::filter::FilterQuality;
use crate::geometry::{composite_geometry, lerp, snap_to_grid};
use crate::reaction::{Activity, observe};
use crate::record::{Record, TextureRenderer};
use crate::texture_cache::TextureCache;

/// Pages are recorded edge to edge: they are clipped to the stack anyway, and
/// a margin would put the texture's own grid at a fractional phase.
const BLEED: u32 = 0;

/// A sliding page stack whose pages hold still in the layout.
///
/// The same job as [`Pager`](crate::Pager), reached from the other side. A
/// `Pager` puts the slide in the layout: every page is laid out where it is
/// actually seen, so hit-testing, operations and overlays need no correction,
/// and the composited origin has to be split into the part that is moving and
/// the part that is not. A `ContentStack` puts the slide in the *composite*:
/// pages are laid out in a fixed row, `x = i * width`, and the offset is
/// applied when the textures are drawn.
///
/// That one difference is the whole point of this widget. Because a page's
/// layout box does not move for the length of a slide:
///
/// * the origin handed to the snap is constant, so there is nothing for the
///   device grid to quantise and no staircase to fall down;
/// * the record translation is constant, so a page re-recorded mid-slide is
///   byte-for-byte the frame before it;
/// * and — the part that is hard to get any other way — the resting frame is
///   computed by the *same expression* as the sliding one. `draw` snaps the
///   layout origin and subtracts the offset whether or not a texture is
///   involved, so the frame the stack stops compositing and starts drawing
///   its page directly puts every pixel exactly where the previous frame had
///   it. The handoff has nothing to give itself away with.
///
/// The price is paid where a `Pager` pays nothing. Everything that asks
/// "where is this page?" has to be told about the offset by hand: the cursor
/// is translated on the way in, overlays are opened against a shifted
/// translation, and an operation walking the tree sees pages in their
/// unshifted row. Pick this widget when the transition has to be flawless and
/// that correction is acceptable; pick [`Pager`](crate::Pager) when it is not.
///
/// While the slide runs, each visible page is recorded into its own texture
/// and composited under the offset; at rest the current page is drawn
/// directly at the same place, so the idle frame costs nothing extra. The
/// stack's height interpolates between the outgoing and the incoming page,
/// which is why the slide is a layout-tier animation, and each page is
/// centred vertically in the result.
///
/// # Examples
///
/// ```no_run
/// use iced::widget::text;
/// use iced_animate::Motion;
/// use iced_texture_cache::{ContentStack, Element};
///
/// let motion = Motion::new();
/// let stack: Element<'_, ()> = ContentStack::new([text("one"), text("two")])
///     .current(1)
///     .motion(motion.clone())
///     .into();
/// ```
pub struct ContentStack<'a, Message, Theme = iced_core::Theme, Renderer = crate::Renderer> {
    id: Option<Id>,
    children: Vec<Element<'a, Message, Theme, Renderer>>,
    /// `None` takes the width the parent offers.
    width: Option<f32>,
    max_height: f32,
    current: usize,
    /// Engine driving the slide. Without one the stack jumps straight to the
    /// requested page instead of animating against a clock nothing ticks.
    motion: Option<Motion>,
    curve: Curve,
    /// `None` inherits the renderer's tier; see [`Self::filter_quality`].
    filter: Option<FilterQuality>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for ContentStack<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContentStack")
            .field("id", &self.id)
            .field("pages", &self.children.len())
            .field("width", &self.width)
            .field("max_height", &self.max_height)
            .field("current", &self.current)
            .field("curve", &self.curve)
            .field("filter", &self.filter)
            .finish_non_exhaustive()
    }
}

/// One page's texture and the bookkeeping that keeps it fresh.
#[derive(Debug)]
struct Page {
    cache: TextureCache,
    activity: Activity,
    last_interaction: mouse::Interaction,
    propagated: u64,
}

impl Page {
    fn new() -> Self {
        Self {
            cache: TextureCache::new(),
            activity: Activity::default(),
            last_interaction: mouse::Interaction::None,
            propagated: 0,
        }
    }
}

/// Where the stack is and where it is heading.
#[derive(Debug)]
struct Switch {
    current: usize,
    pending: Option<usize>,
}

#[derive(Debug)]
struct State {
    switch: Switch,
    /// Identity of this stack's slide track: it has the lifetime of this
    /// `tree::State`, not an application-level identity.
    key: MotionKey,
    /// Position in page units: `1.5` is half way between pages 1 and 2.
    slide: Anim<f32>,
    pages: Vec<Page>,
    /// Pages laid out this frame, ascending, at most two. Layout children are
    /// emitted in this order, so index order is layout-child order.
    visible: Vec<usize>,
}

impl State {
    fn new(motion: Option<&Motion>, curve: Curve, page_count: usize, current: usize) -> Self {
        let mut state = Self {
            switch: Switch {
                current: current.min(page_count.saturating_sub(1)),
                pending: None,
            },
            key: MotionKey::unique(),
            slide: Anim::constant(0.0),
            pages: (0..page_count).map(|_| Page::new()).collect(),
            visible: Vec::new(),
        };

        state.retarget(motion, curve);
        state
    }

    /// Points the slide track at the page the stack is heading for.
    ///
    /// Idempotent, and called on every rebuild so the track is touched each
    /// build and never collected by the engine while the stack lives.
    fn retarget(&mut self, motion: Option<&Motion>, curve: Curve) {
        let target = self.switch.pending.unwrap_or(self.switch.current) as f32;

        let Some(motion) = motion else {
            self.slide = Anim::constant(target);
            return;
        };

        self.slide = motion.to(self.key, curve, target);

        // `layout` interpolates the stack's height from this value, so it is
        // a layout-tier animation.
        self.slide.mark_tier(Tier::Layout);
    }

    fn position(&self) -> f32 {
        self.slide.get()
    }

    fn is_sliding(&self) -> bool {
        self.slide.is_animating()
    }
}

/// The clamped slide position and the one or two pages under it.
#[derive(Debug, Clone, Copy)]
struct Camera {
    first: usize,
    second: usize,
    position: f32,
}

fn camera(position: f32, page_count: usize) -> Camera {
    let last = page_count.saturating_sub(1) as f32;
    let position = if position.is_nan() {
        0.0
    } else {
        position.clamp(0.0, last)
    };

    Camera {
        first: position.floor() as usize,
        second: position.ceil() as usize,
        position,
    }
}

/// The pages to lay out: the camera's, ascending and deduplicated.
fn visible_indices(camera: Camera, page_count: usize) -> Vec<usize> {
    let mut visible = Vec::with_capacity(2);

    for index in [camera.first, camera.second] {
        if index < page_count && !visible.contains(&index) {
            visible.push(index);
        }
    }
    visible.sort_unstable();
    visible
}

/// Pairs every laid-out page index with its layout.
fn visible_pages<'a>(
    visible: &'a [usize],
    layout: Layout<'a>,
) -> impl Iterator<Item = (usize, Layout<'a>)> {
    visible.iter().copied().zip(layout.children())
}

/// Moves the cursor into the pages' unshifted row.
///
/// The pages are laid out where they are *not* drawn, so everything that
/// hit-tests against a layout box has to be corrected by the same offset the
/// composite subtracts.
fn shift_cursor(cursor: mouse::Cursor, offset_x: f32) -> mouse::Cursor {
    let shift = |p: Point| Point::new(p.x + offset_x, p.y);

    match cursor {
        mouse::Cursor::Available(p) => mouse::Cursor::Available(shift(p)),
        mouse::Cursor::Levitating(p) => mouse::Cursor::Levitating(shift(p)),
        mouse::Cursor::Unavailable => mouse::Cursor::Unavailable,
    }
}

/// Where a page's content is drawn, in the enclosing layout space, whether it
/// goes through a texture or not.
///
/// The single source of truth for the two branches of [`Widget::draw`], and
/// the reason the handoff between them is invisible: the sliding frame and
/// the resting frame ask this same question and get the same answer. The
/// layout origin is put on the device grid — it does not move for the length
/// of a slide, so there is nothing there to quantise — and the offset is
/// subtracted afterwards, at whatever sub-pixel phase the frame asks for.
fn page_origin(page: Rectangle, offset_x: f32, scale: f32) -> Point {
    Point::new(
        snap_to_grid(page.x, scale) - offset_x,
        snap_to_grid(page.y, scale),
    )
}

impl<'a, Message, Theme, Renderer> ContentStack<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// A stack over `children`, showing page 0.
    #[must_use]
    pub fn new(
        children: impl IntoIterator<Item = impl Into<Element<'a, Message, Theme, Renderer>>>,
    ) -> Self {
        let iterator = children.into_iter();
        Self::with_capacity(iterator.size_hint().0).extend(iterator)
    }

    /// An empty stack with room for `capacity` pages.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            id: None,
            children: Vec::with_capacity(capacity),
            width: None,
            max_height: f32::INFINITY,
            current: 0,
            motion: None,
            curve: STRUCTURAL,
            filter: None,
        }
    }

    /// The page to show. Clamped to the last page.
    #[must_use]
    pub fn current(mut self, index: usize) -> Self {
        self.current = index;
        self
    }

    /// Binds this stack's slide animation to `motion`. Without it the stack
    /// still works, but switches pages in one frame. The stack must sit
    /// inside a [`Host`](iced_animate::widget::Host) for the engine to
    /// advance.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// The curve of the slide (default [`STRUCTURAL`]). Only used with
    /// [`motion`](Self::motion).
    #[must_use]
    pub fn curve(mut self, curve: Curve) -> Self {
        self.curve = curve;
        self
    }

    /// Appends a page. Pages are addressed by index, so every push counts,
    /// including zero-sized ones.
    #[must_use]
    pub fn push(mut self, child: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        self.children.push(child.into());
        self
    }

    /// Appends several pages.
    #[must_use]
    pub fn extend(
        self,
        children: impl IntoIterator<Item = impl Into<Element<'a, Message, Theme, Renderer>>>,
    ) -> Self {
        children.into_iter().fold(self, Self::push)
    }

    /// Sets the widget id used by operations.
    #[must_use]
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Caps the stack's height; pages are laid out within it and anything
    /// that overflows is clipped.
    #[must_use]
    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = max_height.into().0;
        self
    }

    /// Sets the width of every page exactly. Without it the stack takes the
    /// width the parent offers.
    ///
    /// A page's horizontal position is `index * width`, so this is also the
    /// distance one slide travels.
    #[must_use]
    pub fn width(mut self, width: impl Into<Pixels>) -> Self {
        self.width = Some(width.into().0);
        self
    }

    /// The reconstruction filter for the composited pages. `None` (the
    /// default) inherits the renderer's tier.
    ///
    /// [`FilterQuality::Snap`] is a poor fit here and this widget does not
    /// honour its geometry rule: it would put the slide itself on the device
    /// grid, which is the staircase this widget exists to avoid.
    #[must_use]
    pub fn filter_quality(mut self, filter: impl Into<Option<FilterQuality>>) -> Self {
        self.filter = filter.into();
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for ContentStack<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + TextureRenderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(
            self.motion.as_ref(),
            self.curve,
            self.children.len(),
            self.current,
        ))
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);

        let state = tree.state.downcast_mut::<State>();

        state.pages.resize_with(self.children.len(), Page::new);

        // Pages may have been removed since the indices were recorded.
        let last = self.children.len().saturating_sub(1);
        state.switch.current = state.switch.current.min(last);
        state.switch.pending = state.switch.pending.map(|pending| pending.min(last));

        let current = self.current.min(last);
        let heading_to = state.switch.pending.unwrap_or(state.switch.current);

        if heading_to != current {
            state.switch.pending = Some(current);

            // The pages the slide will pass over are recorded fresh.
            let from = state.switch.current;
            for page in &state.pages[from.min(current)..=from.max(current)] {
                page.cache.invalidate();
            }
        }

        state.retarget(self.motion.as_ref(), self.curve);
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: self.width.map_or(Length::Fill, Length::Fixed),
            height: Length::Shrink,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &Limits) -> Node {
        let width = self.width.unwrap_or_else(|| limits.max().width);
        let max_height = limits.max().height.min(self.max_height);

        let (camera, visible) = {
            let state = tree.state.downcast_ref::<State>();
            let camera = camera(state.position(), self.children.len());
            (camera, visible_indices(camera, self.children.len()))
        };

        let child_limits = Limits::new(Size::new(width, 0.0), Size::new(width, max_height));
        let mut measured: Vec<(usize, Node)> = Vec::with_capacity(visible.len());
        for &i in &visible {
            let node = self.children[i].as_widget_mut().layout(
                &mut tree.children[i],
                renderer,
                &child_limits,
            );
            measured.push((i, node));
        }

        let height_of = |index: usize| -> f32 {
            measured
                .iter()
                .find(|(i, _)| *i == index)
                .map_or(0.0, |(_, node)| node.size().height)
        };
        let height = lerp(
            height_of(camera.first),
            height_of(camera.second),
            camera.position.fract(),
        )
        .min(self.max_height);

        // The row the pages sit in, and never leave: page `i` is at
        // `i * width` for the whole slide. Only the composite moves.
        let children = measured
            .into_iter()
            .map(|(i, node)| {
                let page_height = node.size().height;
                node.move_to(Point::new(i as f32 * width, (height - page_height) / 2.0))
            })
            .collect();

        tree.state.downcast_mut::<State>().visible = visible;

        Node::with_children(Size { width, height }, children)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let visible = tree.state.downcast_ref::<State>().visible.clone();

        operation.container(self.id.as_ref(), layout.bounds());
        operation.traverse(&mut |operation| {
            for (i, child_layout) in visible_pages(&visible, layout) {
                self.children[i].as_widget_mut().operate(
                    &mut tree.children[i],
                    child_layout,
                    renderer,
                    operation,
                );
            }
        });
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
        let redraw_now = match event {
            Event::Window(window::Event::RedrawRequested(now)) => Some(*now),
            _ => None,
        };
        let width = layout.bounds().width;

        let Tree {
            state,
            children: trees,
            ..
        } = tree;
        let State {
            switch,
            slide,
            pages,
            visible,
            ..
        } = state.downcast_mut::<State>();

        // `iced_animate::widget::Host` has already advanced the clock for this
        // frame, so the arrival can simply be observed here.
        if redraw_now.is_some()
            && !slide.is_animating()
            && let Some(next) = switch.pending.take()
        {
            switch.current = next;
            shell.invalidate_layout();
            shell.request_redraw();
            // The resting page is drawn directly from now on: an enclosing
            // texture must pick up the final pose.
            ancestors::invalidate_ancestors();
        }

        let sliding = slide.is_animating();
        let offset_x = slide.get() * width;
        let cursor = shift_cursor(cursor, offset_x);

        // Mid-slide the page textures composite at a moving offset: the
        // stack's own image changes every frame even when no page cache does,
        // so an enclosing texture re-records each frame.
        if sliding && redraw_now.is_some() {
            ancestors::invalidate_ancestors();
        }

        let visible = visible.clone();
        for (i, child_layout) in visible_pages(&visible, layout) {
            let page = &mut pages[i];
            let child = &mut self.children[i];
            let child_tree = &mut trees[i];

            let mut local_messages = Vec::new();
            let mut local = Shell::new(&mut local_messages);

            // A `Cached` inside this page is baked into the page texture.
            ancestors::with_ancestor(&page.cache, || {
                child.as_widget_mut().update(
                    child_tree,
                    event,
                    child_layout,
                    cursor,
                    renderer,
                    clipboard,
                    &mut local,
                    viewport,
                );
            });

            if sliding {
                // A hover appearance change the page applies silently on
                // `RedrawRequested` (the page moving under a static cursor).
                let interaction_changed = redraw_now.is_some() && {
                    let interaction = child.as_widget().mouse_interaction(
                        child_tree,
                        child_layout,
                        cursor,
                        viewport,
                        renderer,
                    );
                    let changed = page.last_interaction != interaction;
                    page.last_interaction = interaction;
                    changed
                };

                if observe(&local, redraw_now, interaction_changed, &mut page.activity) {
                    page.cache.invalidate();
                }
            }

            // A page texture is baked into every enclosing texture.
            let _ = ancestors::propagate(&page.cache, &mut page.propagated);

            shell.merge(local, |m| m);
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<State>();
        let cursor = shift_cursor(cursor, state.position() * layout.bounds().width);

        visible_pages(&state.visible, layout)
            .map(|(i, child_layout)| {
                self.children[i].as_widget().mouse_interaction(
                    &tree.children[i],
                    child_layout,
                    cursor,
                    viewport,
                    renderer,
                )
            })
            .max()
            .unwrap_or_default()
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
        if self.children.is_empty() {
            return;
        }

        let state = tree.state.downcast_ref::<State>();
        let bounds = layout.bounds();
        let scale = renderer.scale_factor();
        let filter = self.filter.unwrap_or_else(|| renderer.filter_quality());

        let offset_x = state.position() * bounds.width;
        let cursor = shift_cursor(cursor, offset_x);
        let sliding = state.is_sliding();

        // A page mid-slide overhangs the stack's edges, and the stack itself
        // may be inside someone else's clip.
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };

        renderer.with_layer(clip, |renderer| {
            for (i, child_layout) in visible_pages(&state.visible, layout) {
                let page_bounds = child_layout.bounds();
                let origin = page_origin(page_bounds, offset_x, scale);

                // Where the page lands on screen. Off-screen pages are not
                // worth a record.
                let on_screen = Rectangle {
                    x: origin.x,
                    y: origin.y,
                    ..page_bounds
                };
                if on_screen.intersection(&clip).is_none() {
                    continue;
                }

                let child = &self.children[i];
                let child_tree = &tree.children[i];

                if !sliding {
                    // The resting page, drawn in place at exactly the origin
                    // the composite would have used.
                    renderer.with_translation(
                        Vector::new(origin.x - page_bounds.x, origin.y - page_bounds.y),
                        |renderer| {
                            child.as_widget().draw(
                                child_tree,
                                renderer,
                                theme,
                                style,
                                child_layout,
                                cursor,
                                viewport,
                            );
                        },
                    );
                    continue;
                }

                // The texture is recorded against the page's *layout* box,
                // which does not move for the length of the slide, and the
                // offset is applied to the composite instead. That is what
                // keeps the record identical frame to frame and the origin
                // free of any quantised motion.
                let composite = composite_geometry(
                    BLEED,
                    page_bounds,
                    scale,
                    1.0,
                    Some(page_bounds.position()),
                );
                let record_origin = Vector::new(-page_bounds.x, -page_bounds.y);

                let record = renderer.record(
                    &state.pages[i].cache,
                    composite.physical,
                    composite.texture_scale,
                    |r| {
                        r.with_translation(record_origin, |r| {
                            child.as_widget().draw(
                                child_tree,
                                r,
                                theme,
                                style,
                                child_layout,
                                cursor,
                                &page_bounds,
                            );
                        });
                    },
                );

                match record {
                    Record::Fresh | Record::Reused => renderer.draw_cached(
                        &state.pages[i].cache,
                        composite.cache_bounds,
                        clip,
                        Transformation::translate(-offset_x, 0.0),
                        1.0,
                        filter,
                    ),
                    // Too large for a texture: draw it where it belongs, by
                    // the same rule, so nothing jumps when it fits again.
                    Record::Uncacheable => renderer.with_translation(
                        Vector::new(origin.x - page_bounds.x, origin.y - page_bounds.y),
                        |renderer| {
                            child.as_widget().draw(
                                child_tree,
                                renderer,
                                theme,
                                style,
                                child_layout,
                                cursor,
                                &page_bounds,
                            );
                        },
                    ),
                }
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_ref::<State>();
        let visible = state.visible.clone();
        let bounds = layout.bounds();
        // The pages are laid out in their row, so an overlay opened by one
        // has to be pushed by the same offset the composite subtracts.
        let slide = Vector::new(-(state.position() * bounds.width), 0.0);

        let mut overlays = Vec::new();
        let mut layouts = layout.children();

        for (i, (child, child_tree)) in self.children.iter_mut().zip(&mut tree.children).enumerate()
        {
            if !visible.contains(&i) {
                continue;
            }

            let Some(child_layout) = layouts.next() else {
                break;
            };

            if let Some(overlay) = child.as_widget_mut().overlay(
                child_tree,
                child_layout,
                renderer,
                viewport,
                translation + slide,
            ) {
                overlays.push(overlay);
            }
        }

        if overlays.is_empty() {
            None
        } else {
            Some(overlay::Group::with_children(overlays).overlay())
        }
    }
}

impl<'a, Message, Theme, Renderer> From<ContentStack<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + TextureRenderer + 'a,
{
    fn from(stack: ContentStack<'a, Message, Theme, Renderer>) -> Self {
        Self::new(stack)
    }
}

/// A [`ContentStack`] over `children`. See the widget for the details.
#[must_use]
pub fn content_stack<'a, Message, Theme, Renderer>(
    children: impl IntoIterator<Item = impl Into<Element<'a, Message, Theme, Renderer>>>,
) -> ContentStack<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    ContentStack::new(children)
}

// The harness is software-only (see `test_support`).
#[cfg(all(test, feature = "tiny-skia"))]
mod tests {
    use super::*;
    use crate::test_support::Harness;
    use iced::widget::{button, column, text};
    use iced_core::clipboard;
    use iced_core::time::Instant;
    use iced_core::{Color, Theme};
    use std::time::Duration;

    #[derive(Debug, Clone, PartialEq)]
    enum Message {
        A,
        B,
    }

    fn two_pages<'a>(motion: Option<&Motion>, current: usize) -> crate::Element<'a, Message> {
        let mut stack = ContentStack::new([
            column![button("a").on_press(Message::A)],
            column![button("b").on_press(Message::B)],
        ])
        .current(current);
        if let Some(motion) = motion {
            stack = stack.motion(motion.clone());
        }
        match motion {
            Some(motion) => motion.host(stack).into(),
            None => stack.into(),
        }
    }

    /// Builds page 0, rebuilds with page 1 requested (which runs `diff`),
    /// drives `frames` redraws, then clicks where page 1's button is *drawn*
    /// and returns the messages produced.
    fn switch_then_click(motion: Option<&Motion>, frames: u64) -> Vec<Message> {
        let size = Size::new(300.0, 200.0);
        let mut harness = Harness::new(size, two_pages(motion, 0)).rebuild(two_pages(motion, 1));
        let start = Instant::now();
        for frame in 1..=frames {
            harness.redraw(start + Duration::from_millis(16 * frame));
        }
        harness.click_at(Point::new(12.0, 12.0));
        harness.into_messages()
    }

    #[test]
    fn without_an_engine_the_switch_is_immediate() {
        assert_eq!(switch_then_click(None, 1), vec![Message::B]);
    }

    #[test]
    fn the_cursor_is_corrected_so_clicks_land_where_pages_are_drawn() {
        let motion = Motion::new();
        // Settled on page 1: its button is drawn at the stack's top-left,
        // even though the page is laid out one width to the right.
        assert_eq!(switch_then_click(Some(&motion), 120), vec![Message::B]);
    }

    #[test]
    fn mid_slide_the_outgoing_page_still_takes_the_click_it_covers() {
        let motion = Motion::new();
        // Two frames in, page 0 has barely moved: it is still under the
        // cursor, and the correction has to find it there.
        assert_eq!(switch_then_click(Some(&motion), 2), vec![Message::A]);
    }

    type TestStack<'a> = ContentStack<'a, (), Theme, crate::Renderer>;

    fn pages<'a>(n: usize) -> Vec<crate::Element<'a, ()>> {
        (0..n).map(|i| text(i).into()).collect()
    }

    fn state(tree: &Tree) -> &State {
        tree.state.downcast_ref::<State>()
    }

    fn tree_of(stack: &TestStack<'_>) -> Tree {
        Tree::new(stack as &dyn Widget<(), Theme, crate::Renderer>)
    }

    fn limits(width: f32, height: f32) -> Limits {
        Limits::new(Size::ZERO, Size::new(width, height))
    }

    fn viewport() -> Rectangle {
        Rectangle::with_size(Size::new(300.0, 200.0))
    }

    #[test]
    fn the_snap_falls_on_the_layout_box_and_the_offset_rides_on_top() {
        // The rule the whole widget rests on. The layout origin — which does
        // not move for the length of a slide — is what gets rounded; the
        // offset is subtracted afterwards and keeps every fraction it has.
        let page = Rectangle::new(Point::new(10.3, 4.1), Size::new(100.0, 50.0));
        let scale = 2.0;

        let still = page_origin(page, 0.0, scale);
        assert_eq!(still.x, snap_to_grid(10.3, scale));
        assert_eq!(still.y, snap_to_grid(4.1, scale));

        // A sub-pixel offset moves the page by exactly that much: no step.
        let nudged = page_origin(page, 0.07, scale);
        assert!((still.x - nudged.x - 0.07).abs() < 1e-6);
        assert_eq!(nudged.y, still.y, "the offset is horizontal");

        // And it stays smooth however small the increment gets, which is
        // where a staircase would show first.
        for step in 1..40 {
            let offset = step as f32 * 0.013;
            let moved = page_origin(page, offset, scale);
            assert!(
                (still.x - moved.x - offset).abs() < 1e-5,
                "offset {offset} was quantised to {}",
                still.x - moved.x
            );
        }
    }

    #[test]
    fn pages_hold_still_in_the_layout_while_the_slide_runs() {
        let motion = Motion::new();
        let build = |current: usize| -> TestStack<'_> {
            ContentStack::new(pages(2))
                .current(current)
                .motion(motion.clone())
        };
        let renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&build(0));
        let mut stack = build(1);
        stack.diff(&mut tree);

        let start = Instant::now();
        let _ = motion.tick(start);
        let mut moved = false;
        for frame in 1..=60 {
            let _ = motion.tick(start + Duration::from_millis(16 * frame));
            let node = stack.layout(&mut tree, &renderer, &limits(300.0, 200.0));
            let width = node.size().width;
            let layout = Layout::new(&node);
            for (i, child) in visible_pages(&state(&tree).visible.clone(), layout) {
                assert!(
                    (child.bounds().x - i as f32 * width).abs() < 1e-4,
                    "page {i} left its row at frame {frame}: {}",
                    child.bounds().x
                );
            }
            if state(&tree).position() > 0.05 {
                moved = true;
            }
        }
        assert!(moved, "the slide never ran, so nothing was proven");
    }

    #[test]
    fn a_non_redraw_event_does_not_commit_the_switch() {
        let renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&ContentStack::new(pages(2)));
        let mut stack: TestStack<'_> = ContentStack::new(pages(2)).current(1);
        stack.diff(&mut tree);
        assert_eq!(state(&tree).switch.pending, Some(1));

        let node = stack.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        let mut messages: Vec<()> = Vec::new();
        let mut shell = Shell::new(&mut messages);
        stack.update(
            &mut tree,
            &Event::Mouse(mouse::Event::CursorMoved {
                position: Point::ORIGIN,
            }),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &renderer,
            &mut clipboard::Null,
            &mut shell,
            &viewport(),
        );
        assert_eq!(
            state(&tree).switch.pending,
            Some(1),
            "only a redraw commits the switch"
        );
    }

    #[test]
    fn a_slide_records_each_visible_page_once() {
        let motion = Motion::new();
        let build = |current: usize| -> TestStack<'_> {
            ContentStack::new(pages(2))
                .current(current)
                .motion(motion.clone())
        };
        let mut renderer = crate::testing::headless_tiny_skia();
        let mut tree = tree_of(&build(0));
        let mut stack = build(1);
        stack.diff(&mut tree);

        let start = Instant::now();
        let _ = motion.tick(start);
        let style = renderer::Style {
            text_color: Color::BLACK,
        };
        let mut frame = 0;
        loop {
            frame += 1;
            assert!(frame < 600, "the slide never settled");
            let now = start + Duration::from_millis(16 * frame);
            let _ = motion.tick(now);
            let node = stack.layout(&mut tree, &renderer, &limits(300.0, 200.0));
            let mut messages: Vec<()> = Vec::new();
            let mut shell = Shell::new(&mut messages);
            stack.update(
                &mut tree,
                &Event::Window(window::Event::RedrawRequested(now)),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard::Null,
                &mut shell,
                &viewport(),
            );
            iced_core::Renderer::reset(&mut renderer, viewport());
            stack.draw(
                &tree,
                &mut renderer,
                &Theme::Light,
                &style,
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &viewport(),
            );
            if !state(&tree).is_sliding() {
                break;
            }
        }
        let counts: Vec<u64> = state(&tree)
            .pages
            .iter()
            .map(|page| page.cache.record_count())
            .collect();
        assert_eq!(
            counts,
            vec![1, 1],
            "each page is recorded exactly once per slide"
        );
    }

    #[test]
    fn max_height_and_a_fixed_width_are_honoured() {
        let renderer = crate::testing::headless_tiny_skia();
        let mut stack: TestStack<'_> = ContentStack::new(pages(2)).max_height(5.0);
        let mut tree = tree_of(&stack);
        let node = stack.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert!(node.size().height <= 5.0, "clamped: {}", node.size().height);

        let mut stack: TestStack<'_> = ContentStack::new(pages(2)).width(120.0);
        let mut tree = tree_of(&stack);
        let node = stack.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert_eq!(node.size().width, 120.0);
    }

    #[test]
    fn an_empty_stack_lays_out_and_draws_nothing() {
        let mut renderer = crate::testing::headless_tiny_skia();
        let mut stack: TestStack<'_> = ContentStack::new(Vec::<crate::Element<'_, ()>>::new());
        let mut tree = tree_of(&stack);
        let node = stack.layout(&mut tree, &renderer, &limits(300.0, 200.0));
        assert_eq!(node.size().height, 0.0);
        iced_core::Renderer::reset(&mut renderer, viewport());
        stack.draw(
            &tree,
            &mut renderer,
            &Theme::Light,
            &renderer::Style {
                text_color: Color::BLACK,
            },
            Layout::new(&node),
            mouse::Cursor::Unavailable,
            &viewport(),
        );
    }
}
