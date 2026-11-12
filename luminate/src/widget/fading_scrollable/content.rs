//! Fractional correction inside Iced's scrollable; its viewport stays fixed.
use iced::Element;
use iced::advanced::{
    Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer,
    widget::{Operation, Tree},
};
use iced::{Event, Length, Rectangle, Size, Vector};
use std::{cell::Cell, rc::Rc};

pub(super) fn view<'a, M: 'a, Theme: 'a, Renderer: iced::advanced::Renderer + 'a>(
    content: Element<'a, M, Theme, Renderer>,
    offset: Rc<Cell<Vector>>,
) -> Element<'a, M, Theme, Renderer> {
    Element::new(Shift { content, offset })
}
struct Shift<'a, M, Theme, Renderer> {
    content: Element<'a, M, Theme, Renderer>,
    offset: Rc<Cell<Vector>>,
}
impl<M, Theme, Renderer> Shift<'_, M, Theme, Renderer> {
    fn correction(&self) -> Vector {
        self.offset.get()
    }
}
impl<M, Theme, Renderer: iced::advanced::Renderer> Widget<M, Theme, Renderer>
    for Shift<'_, M, Theme, Renderer>
{
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }
    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn size_hint(&self) -> Size<Length> {
        self.content.as_widget().size_hint()
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
    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, M>,
        viewport: &Rectangle,
    ) {
        let correction = self.correction();
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor + correction,
            renderer,
            clipboard,
            shell,
            &(*viewport + correction),
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
        let correction = self.correction();
        renderer.with_translation(-correction, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor + correction,
                &(*viewport + correction),
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor + self.correction(),
            &(*viewport + self.correction()),
            renderer,
        )
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
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, M, Theme, Renderer>> {
        let correction = self.correction();
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation - correction,
        )
    }
}
