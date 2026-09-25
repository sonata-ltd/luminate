//! A transparent wrapper that dictates the mouse cursor over its content.

use std::slice::from_ref;

use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Element, Event, Length, Rectangle, Size, Vector};

/// Reports a fixed [`mouse::Interaction`] over `content`, in place of the
/// one the content asks for.
///
/// Nothing else changes: layout, drawing, events, focus operations and
/// overlays all pass straight through, and the widget occupies the
/// content's layout node rather than a box of its own — wrapping an
/// element never moves it or changes what it does. Only the cursor is
/// overridden, so a wrapped button still presses.
///
/// The override holds only while the cursor is over that node; elsewhere
/// the widget reports [`mouse::Interaction::None`]. A parent takes the
/// strongest interaction among its children, so a wrapper that claimed one
/// unconditionally would hand it to the whole window.
///
/// The default is [`mouse::Interaction::None`], the weakest of them: it
/// drops the content's cursor without proposing another, which is what a
/// button that should not advertise itself as clickable wants.
///
/// # Example
///
/// ```
/// use iced_luminate::iced::widget::button;
/// use iced_luminate::iced::{Element, Theme};
/// use iced_luminate::widget::interaction_override::interaction_override;
///
/// // Still presses; no longer shows the pointer cursor.
/// let quiet: Element<'_, (), Theme, iced_luminate::Renderer> =
///     interaction_override(button("press me")).into();
/// ```
pub struct InteractionOverride<'a, Message, Theme = iced::Theme, Renderer = crate::Renderer> {
    content: Element<'a, Message, Theme, Renderer>,
    interaction: mouse::Interaction,
}

impl<Message, Theme, Renderer> std::fmt::Debug
    for InteractionOverride<'_, Message, Theme, Renderer>
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InteractionOverride")
            .field("interaction", &self.interaction)
            .finish_non_exhaustive()
    }
}

impl<'a, Message, Theme, Renderer> InteractionOverride<'a, Message, Theme, Renderer> {
    /// Wraps `content`, reporting [`mouse::Interaction::None`] over it.
    #[must_use]
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            interaction: mouse::Interaction::None,
        }
    }

    /// The interaction to report while the cursor is over the content.
    #[must_use]
    pub fn interaction(mut self, interaction: mouse::Interaction) -> Self {
        self.interaction = interaction;
        self
    }

    /// The interaction reported for a cursor against the content's `bounds`.
    fn interaction_at(&self, bounds: Rectangle, cursor: mouse::Cursor) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            self.interaction
        } else {
            mouse::Interaction::None
        }
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for InteractionOverride<'_, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer,
{
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(from_ref(&self.content));
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
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
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        _tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        self.interaction_at(layout.bounds(), cursor)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<InteractionOverride<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(value: InteractionOverride<'a, Message, Theme, Renderer>) -> Self {
        Element::new(value)
    }
}

/// Wraps `content` in an [`InteractionOverride`].
#[must_use]
pub fn interaction_override<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> InteractionOverride<'a, Message, Theme, Renderer>
where
    Renderer: renderer::Renderer + 'a,
{
    InteractionOverride::new(content)
}

#[cfg(test)]
mod tests {
    use iced::Point;
    use iced::advanced::widget::tree;
    use iced::widget::{button, text, text_input};
    use iced_test::Simulator;

    use super::*;

    type TestElement<'a> = Element<'a, Message, iced::Theme, crate::Renderer>;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct Message;

    fn simulator(root: TestElement<'_>) -> Simulator<'_, Message, iced::Theme, crate::Renderer> {
        Simulator::with_size(iced::Settings::default(), Size::new(200.0, 100.0), root)
    }

    fn over(bounds: Rectangle, x: f32, y: f32) -> mouse::Cursor {
        mouse::Cursor::Available(Point::new(bounds.x + x, bounds.y + y))
    }

    /// The wrapper claims no box of its own, so the content lands exactly
    /// where it would have unwrapped.
    #[test]
    fn wrapping_leaves_the_content_where_it_was() {
        let bare = {
            let root: TestElement<'_> = text("content").into();
            simulator(root).find("content").expect("on screen").bounds()
        };
        let wrapped = {
            let root: TestElement<'_> = interaction_override(text("content")).into();
            simulator(root).find("content").expect("on screen").bounds()
        };

        assert_eq!(bare, wrapped);
    }

    /// Only the cursor is overridden: the content still handles its events.
    #[test]
    fn the_content_still_receives_events() {
        let root: TestElement<'_> = interaction_override(button("press me").on_press(Message))
            .interaction(mouse::Interaction::Crosshair)
            .into();
        let mut ui = simulator(root);

        let _ = ui.click("press me").expect("on screen");

        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Message]);
    }

    /// Without a `diff` of its own, the default one would clear the child's
    /// state on every rebuild — and the next `tree.children[0]` would panic.
    #[test]
    fn a_diff_keeps_the_content_state() {
        let element: TestElement<'_> = interaction_override(text_input("type here", "")).into();
        let mut tree = Tree::new(&element);
        let before = tree.children[0].tag;

        let next: TestElement<'_> = interaction_override(text_input("type here", "typed")).into();
        tree.diff(&next);

        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].tag, before);
        assert_ne!(before, tree::Tag::stateless());
    }

    #[test]
    fn the_override_holds_over_the_content() {
        let widget: InteractionOverride<'_, Message> =
            InteractionOverride::new(text("content")).interaction(mouse::Interaction::Crosshair);
        let bounds = Rectangle::new(Point::new(10.0, 10.0), Size::new(80.0, 20.0));

        assert_eq!(
            widget.interaction_at(bounds, over(bounds, 5.0, 5.0)),
            mouse::Interaction::Crosshair
        );
    }

    /// A parent takes the strongest interaction among its children, so the
    /// override must not reach past the content's own bounds.
    #[test]
    fn the_override_stops_at_the_content_bounds() {
        let widget: InteractionOverride<'_, Message> =
            InteractionOverride::new(text("content")).interaction(mouse::Interaction::Crosshair);
        let bounds = Rectangle::new(Point::new(10.0, 10.0), Size::new(80.0, 20.0));

        assert_eq!(
            widget.interaction_at(bounds, over(bounds, 200.0, 5.0)),
            mouse::Interaction::None
        );
        assert_eq!(
            widget.interaction_at(bounds, mouse::Cursor::Unavailable),
            mouse::Interaction::None
        );
    }

    /// The wrapper returns the content's own layout node, so every method
    /// hands that node straight back rather than descending into it.
    #[test]
    fn drawing_reaches_the_content() {
        let root: TestElement<'_> = interaction_override(text("content")).into();
        let mut ui = simulator(root);

        let _ = ui.snapshot(&iced::Theme::Light).expect("drawn");
    }

    #[test]
    fn the_default_override_is_none() {
        let widget: InteractionOverride<'_, Message> = InteractionOverride::new(text("content"));
        let bounds = Rectangle::new(Point::new(0.0, 0.0), Size::new(80.0, 20.0));

        assert_eq!(
            widget.interaction_at(bounds, over(bounds, 5.0, 5.0)),
            mouse::Interaction::None
        );
    }
}
