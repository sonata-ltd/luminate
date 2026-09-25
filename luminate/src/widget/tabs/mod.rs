//! Simple tabs widget

use iced::{
    Border, Color, Event, Length, Padding, Rectangle, Shadow, Size, Vector,
    advanced::{
        Layout, Widget, layout, mouse, overlay,
        renderer::{self, Quad},
        widget::{Tree, tree},
    },
    border::Radius,
    touch,
};
use iced_animate::{Anim, Motion, MotionKey, curves::QUICK};

use crate::widget::tabs::separator::SeparatorState;

mod separator;

const PADDING: f32 = 4.0;

/// Width of the line drawn between two tabs.
const SEPARATOR_WIDTH: f32 = 1.0;

/// How far a separator stops short of the bar's top and bottom edges.
const SEPARATOR_INSET: f32 = 7.0;

/// Tab struct
pub struct Tabs<'a, Message, Theme = iced::Theme, Renderer = crate::Renderer> {
    children: Vec<iced::Element<'a, Message, Theme, Renderer>>,
    width: Length,
    height: Length,
    active_index: usize,
    on_select: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    motion: Option<Motion>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Tabs<'_, Message, Theme, Renderer> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tabs")
            .field("children", &self.children.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("motion", &self.motion)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct State {
    backing_pos_key: MotionKey,
    backing_pos: Anim<f32>,

    backing_width_key: MotionKey,
    backing_width: Anim<f32>,

    backing_target_position: Option<f32>,
    backing_target_width: Option<f32>,

    separators: Vec<SeparatorState>,
}

impl State {
    fn new() -> Self {
        Self {
            backing_pos_key: MotionKey::unique(),
            backing_pos: Anim::constant(0.0),

            backing_width_key: MotionKey::unique(),
            backing_width: Anim::constant(0.0),

            backing_target_position: None,
            backing_target_width: None,

            separators: Vec::new(),
        }
    }

    fn retarget(&mut self, motion: Option<&Motion>, pos: f32, width: f32) {
        let pos = pos.clamp(0.0, 1.0);
        let width = width.clamp(0.0, 1.0);

        let position_changed = self
            .backing_target_position
            .is_none_or(|target| (target - pos).abs() > f32::EPSILON);

        let width_changed = self
            .backing_target_width
            .is_none_or(|target| (target - width).abs() > f32::EPSILON);

        if !position_changed && !width_changed {
            return;
        }

        self.backing_target_position = Some(pos);
        self.backing_target_width = Some(width);

        if let Some(motion) = motion {
            self.backing_pos = motion.to(self.backing_pos_key, QUICK, pos);
            self.backing_width = motion.to(self.backing_width_key, QUICK, width);
        } else {
            self.backing_pos = Anim::constant(pos);
            self.backing_width = Anim::constant(width);
        }
    }

    fn position(&self) -> f32 {
        self.backing_pos.get().clamp(0.0, 1.0)
    }

    fn width(&self) -> f32 {
        self.backing_width.get().clamp(0.0, 1.0)
    }

    fn sync_separators(&mut self, motion: Option<&Motion>, tabs_len: usize, active_index: usize) {
        let count = tabs_len.saturating_sub(1);

        if self.separators.len() != count {
            self.separators.resize_with(count, SeparatorState::new);
        }

        for (i, separator) in self.separators.iter_mut().enumerate() {
            let neighbor_active = active_index == i || active_index == i + 1;
            let target = if neighbor_active { 0.0 } else { 1.0 };
            separator.retarget(motion, target);
        }
    }
}

impl<'a, Message, Theme, Renderer> Tabs<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    /// Creates an empty [`Tabs`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_children(Vec::new())
    }

    /// Creates a [`Tabs`] with the given tabs.
    #[must_use]
    pub fn with_children(
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            children: Vec::new(),
            width: Length::Fill,
            height: Length::Shrink,
            active_index: 0,
            motion: None,
            on_select: None,
        }
        .extend(children)
    }

    /// Adds a tab. A tab whose size hint is void is dropped.
    #[must_use]
    pub fn push(mut self, child: impl Into<iced::Element<'a, Message, Theme, Renderer>>) -> Self {
        let child = child.into();
        let size = child.as_widget().size_hint();

        if !size.is_void() {
            self.height = self.height.enclose(size.height);
            self.children.push(child);
        }

        self
    }

    /// Adds every tab in `children`, as [`push`](Self::push) does.
    #[must_use]
    pub fn extend(
        self,
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        children.into_iter().fold(self, Self::push)
    }

    /// Selects the tab at index `i`, clamped to the last tab added so far:
    /// call it after the tabs are in.
    #[must_use]
    pub fn active(mut self, i: usize) -> Self {
        self.active_index = i.min(self.children.len().saturating_sub(1));
        self
    }

    /// The message produced when a tab is pressed, given its index.
    #[must_use]
    pub fn on_select(mut self, on_select: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }

    /// Animates the selection backing and the separators through `motion`.
    /// Without one they jump.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    fn backing_geometry(tabs: iced::Rectangle, tab: iced::Rectangle) -> (f32, f32) {
        if tabs.width <= 0.0 {
            return (0.0, 0.0);
        }

        let pos = (tab.x - tabs.x) / tabs.width;
        let width = tab.width / tabs.width;

        (pos.clamp(0.0, 1.0), width.clamp(0.0, 1.0))
    }
}

impl<Message, Theme, Renderer> Default for Tabs<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Tabs<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> iced::Size<Length> {
        Size::new(self.width, self.height)
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new())
    }

    fn children(&self) -> Vec<tree::Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.children);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: iced::advanced::Layout<'_>,
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

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        // A finger selects a tab as a click does: the children are usually
        // plain text, with no press handling of their own to fall back on.
        let pressed_at = match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => cursor.position(),
            Event::Touch(touch::Event::FingerPressed { position, .. }) => Some(*position),
            _ => None,
        };

        if let Some(at) = pressed_at
            && let Some(on_select) = &self.on_select
        {
            for (index, child_layout) in layout.children().enumerate() {
                if child_layout.bounds().contains(at) {
                    shell.capture_event();
                    shell.publish(on_select(index));
                }
            }
        }

        for ((child, tree), child_layout) in self
            .children
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            child.as_widget_mut().update(
                tree,
                event,
                child_layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let node = layout::flex::resolve(
            layout::flex::Axis::Horizontal,
            renderer,
            limits,
            self.width,
            self.height,
            Padding::from(PADDING),
            5.0,
            iced::Alignment::Center,
            &mut self.children,
            &mut tree.children,
        );

        let bounds = Rectangle {
            x: 0.0,
            y: 0.0,
            width: node.size().width,
            height: node.size().height,
        };

        if let Some(active_layout) = node.children().get(self.active_index) {
            let tab = active_layout.bounds();

            let (position, width) = Self::backing_geometry(bounds, tab);

            tree.state
                .downcast_mut::<State>()
                .retarget(self.motion.as_ref(), position, width);
        }

        tree.state.downcast_mut::<State>().sync_separators(
            self.motion.as_ref(),
            self.children.len(),
            self.active_index,
        );

        node
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: layout::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };

        let state = tree.state.downcast_ref::<State>();

        let position = state.position();
        let width = state.width();

        let background = Rectangle {
            x: bounds.x,
            y: bounds.y,
            width: bounds.width,
            height: bounds.height,
        };

        // Draw background
        renderer.fill_quad(
            Quad {
                bounds: background,
                border: Border {
                    color: Color::from_rgb8(233, 234, 235),
                    width: 1.0,
                    radius: Radius::from(8),
                },
                snap: false,
                ..Default::default()
            },
            Color::from_rgb8(250, 250, 250),
        );

        let backing = Rectangle {
            x: bounds.x + position * bounds.width,
            y: bounds.y + PADDING,
            width: width * bounds.width,
            height: bounds.height - PADDING * 2.0,
        };

        // Draw separators
        let child_bounds: Vec<Rectangle> = layout.children().map(|l| l.bounds()).collect();

        for (i, pair) in child_bounds.windows(2).enumerate() {
            let opacity = state.separators.get(i).map_or(0.0, SeparatorState::value);

            if opacity <= 0.0 {
                continue;
            }

            let [left, right] = pair else { continue };
            let center_x = (left.x + left.width + right.x) / 2.0;

            let separator = Rectangle {
                x: center_x - SEPARATOR_WIDTH / 2.0,
                y: bounds.y + SEPARATOR_INSET,
                width: SEPARATOR_WIDTH,
                height: (bounds.height - SEPARATOR_INSET * 2.0).max(0.0),
            };

            renderer.fill_quad(
                Quad {
                    bounds: separator,
                    border: Border::default(),
                    snap: false,
                    ..Default::default()
                },
                Color::from_rgba8(232, 232, 232, opacity),
            );
        }

        // Draw the backing first
        renderer.fill_quad(
            Quad {
                bounds: backing,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: Radius::from(5),
                },
                shadow: Shadow {
                    color: Color::from_rgba8(0, 0, 0, 0.15),
                    offset: Vector::ZERO,
                    blur_radius: 4.0,
                },
                snap: false,
            },
            Color::WHITE,
        );

        for ((child, child_tree), child_layout) in self
            .children
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            if !child_layout.bounds().intersects(&visible) {
                continue;
            }

            child.as_widget().draw(
                child_tree,
                renderer,
                theme,
                style,
                child_layout,
                cursor,
                &visible,
            );
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

    fn overlay<'b>(
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
}

impl<'a, Message, Theme, Renderer> From<Tabs<'a, Message, Theme, Renderer>>
    for iced::Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(tabs: Tabs<'a, Message, Theme, Renderer>) -> Self {
        Self::new(tabs)
    }
}

/// Creates a [`Tabs`] widget.
#[must_use]
pub fn tabs<'a, Message, Theme, Renderer>(
    children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
) -> Tabs<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    Tabs::with_children(children)
}

#[cfg(test)]
mod tests {
    use iced::widget::text;
    use iced::{Point, Size};
    use iced_test::Simulator;

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Select(usize);

    fn bar() -> iced::Element<'static, Select, iced::Theme, crate::Renderer> {
        tabs([text("One").into(), text("Two").into()])
            .on_select(Select)
            .into()
    }

    /// The centre of tab `index` in a bar laid out at 300 x 60.
    fn centre_of(index: usize) -> Point {
        let mut ui = Simulator::with_size(iced::Settings::default(), Size::new(300.0, 60.0), bar());
        let label = if index == 0 { "One" } else { "Two" };
        let target = ui.find(label).expect("the tab is laid out");
        target.bounds().center()
    }

    #[test]
    fn a_click_selects_the_tab_under_it() {
        let at = centre_of(1);
        let mut ui = Simulator::with_size(iced::Settings::default(), Size::new(300.0, 60.0), bar());
        ui.point_at(at);
        let _ = ui.simulate(iced_test::simulator::click());
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Select(1)]);
    }

    /// Only mouse presses used to select: the tabs are plain text, so a
    /// finger had nothing at all to press.
    #[test]
    fn a_touch_selects_the_tab_under_it() {
        let at = centre_of(1);
        let mut ui = Simulator::with_size(iced::Settings::default(), Size::new(300.0, 60.0), bar());
        let _ = ui.simulate([Event::Touch(touch::Event::FingerPressed {
            id: touch::Finger(0),
            position: at,
        })]);
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Select(1)]);
    }
}
