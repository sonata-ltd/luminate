//! A virtualized vertical list.
//!
//! Only the visible items, plus an overscan, exist: no `Element`, no `Tree`, no
//! layout node for the rest. Items are built in `layout`, which has `&mut self`;
//! `update` decides which ones are needed from the viewport and invalidates the
//! layout when the built set no longer covers the visible one.
//!
//! The trees of the built items live in the widget state under the key of the
//! item, not in `tree.children`: the built set changes between layouts, and the
//! items are not even built yet during `diff`. With [`VirtualList::key`] the
//! state of an item follows its data when the items are reordered.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Range;

use iced::advanced::widget::operation::Scrollable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::widget::operation::AbsoluteOffset;
use iced::{Event, Length, Padding, Rectangle, Size, Vector};

use crate::widget::fading_scrollable::Glide;
use crate::{Element, Renderer, Theme};

/// The first layout comes before the first `update`, when the visible range is unknown.
const INITIAL_ROWS: usize = 32;

/// A vertical list of equal-height items that builds only the visible ones.
pub struct VirtualList<'a, Message> {
    count: usize,
    item_height: f32,
    padding: Padding,
    overscan: Option<f32>,
    width: Length,
    view: Box<dyn Fn(usize) -> Element<'a, Message> + 'a>,
    key: Option<Box<dyn Fn(usize) -> u64 + 'a>>,
    built: Vec<Built<'a, Message>>,
}

struct Built<'a, Message> {
    key: u64,
    element: Element<'a, Message>,
}

#[derive(Debug, Default)]
struct State {
    wanted: Option<Range<usize>>,
    trees: HashMap<u64, Tree>,
}

impl<'a, Message> VirtualList<'a, Message> {
    /// A list of `count` items of `item_height`; `view(i)` builds item `i`.
    #[must_use]
    pub fn new(
        count: usize,
        item_height: f32,
        view: impl Fn(usize) -> Element<'a, Message> + 'a,
    ) -> Self {
        Self {
            count,
            item_height: item_height.max(1.0),
            padding: Padding::default(),
            overscan: None,
            width: Length::Fill,
            view: Box::new(view),
            key: None,
            built: Vec::new(),
        }
    }

    /// Padding of the list.
    #[must_use]
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Key of an item, so that its state follows the data when items are reordered. Defaults to the index.
    #[must_use]
    pub fn key(mut self, key: impl Fn(usize) -> u64 + 'a) -> Self {
        self.key = Some(Box::new(key));
        self
    }

    /// Extra pixels built above and below the visible area. Defaults to half of its height.
    #[must_use]
    pub fn overscan(mut self, pixels: f32) -> Self {
        self.overscan = Some(pixels.max(0.0));
        self
    }

    /// Width of the list.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    fn key_of(&self, index: usize) -> u64 {
        self.key.as_ref().map_or(index as u64, |key| key(index))
    }

    fn rows_between(&self, top: f32, bottom: f32) -> Range<usize> {
        let first = ((top - self.padding.top) / self.item_height)
            .floor()
            .max(0.0) as usize;
        let last = (((bottom - self.padding.top) / self.item_height)
            .ceil()
            .max(0.0) as usize)
            .min(self.count);

        first.min(last)..last
    }
}

fn covers(built: &Range<usize>, needed: &Range<usize>) -> bool {
    needed.is_empty() || (built.start <= needed.start && needed.end <= built.end)
}

impl<Message> Widget<Message, Theme, Renderer> for VirtualList<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        Vec::new()
    }

    fn diff(&self, _tree: &mut Tree) {}

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let state = tree.state.downcast_mut::<State>();

        // `f32` holds integers exactly up to 2^24: about 500 thousand items of 32 px.
        let padding = self.padding;
        let full_height = self.count as f32 * self.item_height + padding.top + padding.bottom;
        let size = limits.resolve(self.width, Length::Shrink, Size::new(0.0, full_height));

        let wanted = state.wanted.clone().unwrap_or(0..INITIAL_ROWS);
        let wanted = wanted.start.min(self.count)..wanted.end.min(self.count);

        let item = Size::new(size.width - padding.right - padding.left, self.item_height);
        let item_limits = layout::Limits::new(item, item);

        self.built.clear();
        let mut nodes = Vec::with_capacity(wanted.len());

        for index in wanted {
            let mut element = (self.view)(index);
            let key = self.key_of(index);

            let child = state
                .trees
                .entry(key)
                .or_insert_with(|| Tree::new(element.as_widget()));
            child.diff(element.as_widget());

            let node = element
                .as_widget_mut()
                .layout(child, renderer, &item_limits)
                .move_to((padding.left, index as f32 * self.item_height + padding.top));

            nodes.push(node);
            self.built.push(Built { key, element });
        }

        let alive: HashSet<u64> = self.built.iter().map(|built| built.key).collect();
        state.trees.retain(|key, _| alive.contains(key));

        debug_assert_eq!(
            alive.len(),
            self.built.len(),
            "VirtualList: row keys must be unique"
        );

        layout::Node::with_children(size, nodes)
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
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();

        let top = viewport.y - bounds.y;
        let bottom = top + viewport.height;
        let overscan = self.overscan.unwrap_or(viewport.height / 2.0);

        let visible = self.rows_between(top, bottom);
        let built = state.wanted.clone().unwrap_or(0..0);

        // Rebuild only when the visible items leave the built set, and build
        // with overscan so that the layout does not change at every item boundary.
        if !covers(&built, &visible) {
            state.wanted = Some(self.rows_between(top - overscan, bottom + overscan));
            shell.invalidate_layout();
            shell.request_redraw();
        } else if state.wanted.is_none() {
            state.wanted = Some(built);
        }

        for (built, layout) in self.built.iter_mut().zip(layout.children()) {
            if let Some(child) = state.trees.get_mut(&built.key) {
                built.element.as_widget_mut().update(
                    child, event, layout, cursor, renderer, clipboard, shell, viewport,
                );
            }
        }
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
        let state = tree.state.downcast_ref::<State>();

        for (built, layout) in self.built.iter().zip(layout.children()) {
            if !layout.bounds().intersects(viewport) {
                continue;
            }

            if let Some(child) = state.trees.get(&built.key) {
                built
                    .element
                    .as_widget()
                    .draw(child, renderer, theme, style, layout, cursor, viewport);
            }
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

        self.built
            .iter()
            .zip(layout.children())
            .filter_map(|(built, layout)| {
                let child = state.trees.get(&built.key)?;
                Some(
                    built
                        .element
                        .as_widget()
                        .mouse_interaction(child, layout, cursor, viewport, renderer),
                )
            })
            .max()
            .unwrap_or_default()
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let state = tree.state.downcast_mut::<State>();

        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for (built, layout) in self.built.iter_mut().zip(layout.children()) {
                if let Some(child) = state.trees.get_mut(&built.key) {
                    built
                        .element
                        .as_widget_mut()
                        .operate(child, layout, renderer, operation);
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
        let state = tree.state.downcast_mut::<State>();

        // One `&mut` into a `HashMap` per item: split it into independent ones.
        let mut trees: HashMap<u64, &'b mut Tree> = state
            .trees
            .iter_mut()
            .map(|(key, tree)| (*key, tree))
            .collect();

        let children: Vec<_> = self
            .built
            .iter_mut()
            .zip(layout.children())
            .filter_map(|(built, layout)| {
                let child = trees.remove(&built.key)?;
                built.element.as_widget_mut().overlay(
                    child,
                    layout,
                    renderer,
                    viewport,
                    translation,
                )
            })
            .collect();

        (!children.is_empty()).then(|| overlay::Group::with_children(children).overlay())
    }
}

impl<Message> fmt::Debug for VirtualList<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VirtualList")
            .field("count", &self.count)
            .field("item_height", &self.item_height)
            .field("built", &self.built.len())
            .finish_non_exhaustive()
    }
}

impl<'a, Message: 'a> From<VirtualList<'a, Message>> for Element<'a, Message> {
    fn from(list: VirtualList<'a, Message>) -> Self {
        Element::new(list)
    }
}

/// A list of `count` items of `item_height`; `view(i)` builds item `i`.
pub fn virtual_list<'a, Message>(
    count: usize,
    item_height: f32,
    view: impl Fn(usize) -> Element<'a, Message> + 'a,
) -> VirtualList<'a, Message> {
    VirtualList::new(count, item_height, view)
}

/// Scrolls the first scrollable inside `content` so that the band `[top, bottom]`
/// is visible, gliding if it is a `FadingScrollable`.
///
/// Takes two passes: a scroll can change only during `Operation::scrollable`,
/// while the decision depends on the current offset.
pub(crate) fn scroll_into_view<Message>(
    content: &mut Element<'_, Message>,
    tree: &mut Tree,
    layout: Layout<'_>,
    renderer: &Renderer,
    top: f32,
    bottom: f32,
) {
    let mut probe = Probe::default();
    content
        .as_widget_mut()
        .operate(tree, layout, renderer, &mut probe);

    let Some((viewport, offset)) = probe.found else {
        return;
    };

    let target = if top < offset {
        top
    } else if bottom > offset + viewport {
        bottom - viewport
    } else {
        return;
    };

    let mut apply = Apply {
        y: target,
        glided: false,
        done: false,
    };
    content
        .as_widget_mut()
        .operate(tree, layout, renderer, &mut apply);
}

#[derive(Default)]
struct Probe {
    found: Option<(f32, f32)>,
}

impl Operation for Probe {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        if self.found.is_none() {
            operate(self);
        }
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        bounds: Rectangle,
        _content_bounds: Rectangle,
        translation: Vector,
        _state: &mut dyn Scrollable,
    ) {
        if self.found.is_none() {
            self.found = Some((bounds.height, translation.y));
        }
    }
}

struct Apply {
    y: f32,
    glided: bool,
    done: bool,
}

impl Operation for Apply {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
        if !self.done {
            operate(self);
        }
    }

    fn custom(&mut self, _id: Option<&Id>, _bounds: Rectangle, state: &mut dyn std::any::Any) {
        if !self.done
            && let Some(glide) = state.downcast_mut::<Glide>()
        {
            glide.to(AbsoluteOffset {
                x: None,
                y: Some(self.y),
            });
            self.glided = true;
        }
    }

    fn scrollable(
        &mut self,
        _id: Option<&Id>,
        _bounds: Rectangle,
        _content_bounds: Rectangle,
        _translation: Vector,
        state: &mut dyn Scrollable,
    ) {
        if self.done {
            return;
        }

        if !self.glided {
            state.scroll_to(AbsoluteOffset {
                x: None,
                y: Some(self.y),
            });
        }

        self.done = true;
    }
}
