//! Shared state, layout plumbing and event handling for collapsible bars.

use iced::advanced::renderer::Quad;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, layout, mouse, overlay, renderer, svg};
use iced::border::Radius;
use iced::gradient::Linear;
use iced::{
    Color, Degrees, Event, Gradient, Length, Padding, Rectangle, Shadow, Size, Vector, touch,
};
use iced_animate::curves::STRUCTURAL;
use iced_animate::{Anim, AnimLength, Motion, MotionKey, Tier};

use crate::descriptor::Axis;

use super::{compute_layout, icons};

/// Default extent along the collapse axis when collapsed.
pub(super) const DEFAULT_COLLAPSED_SIZE: f32 = 50.0;
/// Default extent of the toggle header along the flex main axis.
pub(super) const DEFAULT_HEADER_SIZE: f32 = 44.0;
/// Default side length of the toggle chevron.
pub(super) const DEFAULT_ICON_SIZE: f32 = 20.0;
/// Default padding around the children.
pub(super) const DEFAULT_PADDING: Padding = Padding::new(10.0);
/// Default gap between children.
pub(super) const DEFAULT_SPACING: f32 = 5.0;
/// Width of the band used by the sidebar's inner edge shadow.
const EDGE_SHADOW_WIDTH: f32 = 6.0;

/// Paint resolved by the public sidebar or header catalog.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Paint {
    pub background: Color,
    pub hover_overlay: Color,
    pub icon: Color,
    pub outer_shadow: Shadow,
    pub inner_edge_shadow: Option<Color>,
}

/// The mechanics shared by [`Sidebar`](super::sidebar::Sidebar) and
/// [`Header`](super::header::Header).
pub(super) struct Core<'a, Message, Theme, Renderer> {
    pub children: Vec<iced::Element<'a, Message, Theme, Renderer>>,
    pub width: AnimLength,
    pub height: AnimLength,
    pub collapsed_size: f32,
    pub header_size: f32,
    pub icon_size: f32,
    pub padding: Padding,
    pub spacing: f32,
    pub collapsed: bool,
    pub show_toggle: bool,
    pub axis: Axis,
    pub on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
    /// Engine driving the collapse. Without one the bar collapses in a
    /// single frame.
    pub motion: Option<Motion>,
    /// Corners of the background, each its own radius. Square by default.
    pub border_radius: Radius,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Core<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Core")
            .field("children", &self.children.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("collapsed_size", &self.collapsed_size)
            .field("header_size", &self.header_size)
            .field("icon_size", &self.icon_size)
            .field("padding", &self.padding)
            .field("spacing", &self.spacing)
            .field("collapsed", &self.collapsed)
            .field("show_toggle", &self.show_toggle)
            .field("axis", &self.axis)
            .field("has_on_toggle", &self.on_toggle.is_some())
            .field("motion", &self.motion)
            .field("border_radius", &self.border_radius)
            .finish()
    }
}

#[derive(Debug)]
pub(super) struct State {
    /// Identity of the collapse track: it lives and dies with this state.
    key: MotionKey,
    /// `0.0` expanded, `1.0` collapsed.
    collapse: Anim<f32>,
    /// Whether the pointer was over the toggle at the last event.
    is_toggle_hovered: bool,
}

fn target(collapsed: bool) -> f32 {
    if collapsed { 1.0 } else { 0.0 }
}

impl State {
    fn new(motion: Option<&Motion>, collapsed: bool) -> Self {
        let mut state = Self {
            key: MotionKey::unique(),
            collapse: Anim::constant(target(collapsed)),
            is_toggle_hovered: false,
        };
        state.retarget(motion, collapsed);
        state
    }

    fn retarget(&mut self, motion: Option<&Motion>, collapsed: bool) {
        match motion {
            Some(motion) => {
                self.collapse = motion.to(self.key, STRUCTURAL, target(collapsed));
                self.collapse.mark_tier(Tier::Layout);
            }
            None => self.collapse = Anim::constant(target(collapsed)),
        }
    }

    /// `0.0` expanded … `1.0` collapsed.
    pub(super) fn progress(&self) -> f32 {
        self.collapse.get().clamp(0.0, 1.0)
    }
}

impl<'a, Message, Theme, Renderer> Core<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    pub(super) fn with_children(
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
        axis: Axis,
    ) -> Self {
        Self {
            children: Vec::new(),
            width: AnimLength::Shrink,
            height: AnimLength::Shrink,
            collapsed_size: DEFAULT_COLLAPSED_SIZE,
            header_size: DEFAULT_HEADER_SIZE,
            icon_size: DEFAULT_ICON_SIZE,
            padding: DEFAULT_PADDING,
            spacing: DEFAULT_SPACING,
            collapsed: false,
            show_toggle: false,
            axis,
            on_toggle: None,
            motion: None,
            border_radius: Radius::default(),
        }
        .extend(children)
    }

    /// Appends a child (void-sized elements are dropped).
    pub(super) fn push(
        mut self,
        child: impl Into<iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        let child = child.into();
        let size = child.as_widget().size_hint();

        if !size.is_void() {
            self.width = self.width.enclose(size.width);
            self.height = self.height.enclose(size.height);
            self.children.push(child);
        }

        self
    }

    /// Appends every child.
    pub(super) fn extend(
        self,
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        children.into_iter().fold(self, Self::push)
    }

    pub(super) fn size(&self) -> Size<Length> {
        // A live length is advertised as `Shrink`, never `Fixed(0.0)`: the
        // void hint would make a parent drop the widget.
        Size::new(self.width.size_hint(), self.height.size_hint())
    }

    pub(super) fn tag() -> tree::Tag {
        tree::Tag::of::<State>()
    }

    pub(super) fn state(&self) -> tree::State {
        tree::State::new(State::new(self.motion.as_ref(), self.collapsed))
    }

    pub(super) fn trees(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    pub(super) fn diff(&self, tree: &mut Tree) {
        // Idempotent; touches the track every build so it is never collected.
        tree.state
            .downcast_mut::<State>()
            .retarget(self.motion.as_ref(), self.collapsed);

        tree.diff_children(&self.children);
    }

    pub(super) fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((child, tree), layout) in self
                .children
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(tree, layout, renderer, operation);
            }
        });
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn update(
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
        let toggle = self
            .show_toggle
            .then(|| self.toggle_bounds(layout.bounds()));
        let is_press = matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                | Event::Touch(touch::Event::FingerPressed { .. })
        );

        if let Some(toggle) = toggle
            && is_press
            && cursor.is_over(toggle)
        {
            // Publishing rebuilds and redraws; no explicit redraw request.
            shell.capture_event();

            if let Some(on_toggle) = &self.on_toggle {
                shell.publish(on_toggle(!self.collapsed));
            }

            return;
        }

        for ((child, tree), layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }

        // The toggle highlight follows the pointer, and iced only redraws
        // on request.
        let hovered = toggle.is_some_and(|toggle| cursor.is_over(toggle));
        let state = tree.state.downcast_mut::<State>();

        if state.is_toggle_hovered != hovered {
            state.is_toggle_hovered = hovered;
            shell.request_redraw();
        }
    }

    pub(super) fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        // A moving length changes the layout, not just the paint.
        self.width.mark_layout_tier();
        self.height.mark_layout_tier();

        compute_layout::resolve(self, tree, renderer, limits)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        paint: Paint,
    ) where
        Renderer: svg::Renderer,
    {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };

        let state = tree.state.downcast_ref::<State>();

        renderer.fill_quad(
            Quad {
                bounds,
                border: iced::Border {
                    radius: self.border_radius,
                    ..Default::default()
                },
                shadow: paint.outer_shadow,
                ..Default::default()
            },
            paint.background,
        );

        if let Some(shadow) = paint.inner_edge_shadow {
            // A narrow band on the inner edge, transparent → shadow across
            // its width: 90° runs left → right (right edge of a column),
            // 180° top → bottom (bottom edge of a row).
            let (angle, strip) = match self.axis {
                Axis::Vertical => (
                    Degrees(90.0),
                    Rectangle {
                        x: bounds.x + (bounds.width - EDGE_SHADOW_WIDTH).max(0.0),
                        width: EDGE_SHADOW_WIDTH.min(bounds.width),
                        ..bounds
                    },
                ),
                Axis::Horizontal => (
                    Degrees(180.0),
                    Rectangle {
                        y: bounds.y + (bounds.height - EDGE_SHADOW_WIDTH).max(0.0),
                        height: EDGE_SHADOW_WIDTH.min(bounds.height),
                        ..bounds
                    },
                ),
            };
            renderer.fill_quad(
                Quad {
                    bounds: strip,
                    border: iced::Border {
                        radius: self.edge_shadow_radius(),
                        ..Default::default()
                    },
                    ..Default::default()
                },
                Gradient::Linear(
                    Linear::new(angle)
                        .add_stop(0.0, Color { a: 0.0, ..shadow })
                        .add_stop(1.0, shadow),
                ),
            );
        }

        if self.show_toggle {
            let toggle = self.toggle_bounds(bounds);

            if state.is_toggle_hovered {
                renderer.fill_quad(
                    Quad {
                        bounds: toggle,
                        border: iced::Border {
                            radius: 4.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                    paint.hover_overlay,
                );
            }

            renderer.draw_svg(
                svg::Svg {
                    handle: icons::chevron(self.axis, self.collapsed),
                    color: Some(paint.icon),
                    rotation: 0.0.into(),
                    opacity: 1.0,
                },
                toggle,
                visible,
            );
        }

        let draw_children = |renderer: &mut Renderer| {
            for ((child, tree), layout) in self
                .children
                .iter()
                .zip(&tree.children)
                .zip(layout.children())
                .filter(|(_, layout)| layout.bounds().intersects(&visible))
            {
                child
                    .as_widget()
                    .draw(tree, renderer, theme, style, layout, cursor, &visible);
            }
        };

        // While the collapse axis is shorter than the children's natural
        // size they must be clipped to the bar; at rest the parent's viewport
        // is enough and a layer would cost a render pass.
        if state.progress() > 0.0 || state.collapse.is_animating() {
            renderer.with_layer(visible, draw_children);
        } else {
            draw_children(renderer);
        }
    }

    pub(super) fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        if self.show_toggle && cursor.is_over(self.toggle_bounds(layout.bounds())) {
            return mouse::Interaction::Pointer;
        }

        self.children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, tree), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(tree, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default()
    }

    pub(super) fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        overlay::from_children(
            &mut self.children,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }

    /// Header extent along the flex main axis (0 when the toggle is hidden).
    pub(super) fn header_extent(&self) -> f32 {
        if self.show_toggle {
            self.header_size
        } else {
            0.0
        }
    }

    /// The toggle square, centred in the header's leading corner.
    fn toggle_bounds(&self, bounds: Rectangle) -> Rectangle {
        let pad = ((self.header_size - self.icon_size) / 2.0).max(0.0);

        Rectangle {
            x: bounds.x + pad,
            y: bounds.y + pad,
            width: self.icon_size,
            height: self.icon_size,
        }
    }

    /// The corners of the sidebar's inner edge-shadow band.
    fn edge_shadow_radius(&self) -> Radius {
        let r = self.border_radius;

        match self.axis {
            Axis::Vertical => Radius::default()
                .top_right(r.top_right)
                .bottom_right(r.bottom_right),
            Axis::Horizontal => Radius::default()
                .bottom_left(r.bottom_left)
                .bottom_right(r.bottom_right),
        }
    }
}
