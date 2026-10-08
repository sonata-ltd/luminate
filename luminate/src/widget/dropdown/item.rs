use std::rc::Rc;

use iced::advanced::svg::{Renderer as _, Svg};
use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Alignment, Event, Length, Rectangle, Size, Vector, touch};

use super::{Catalog, Shared, Status};
use crate::{Element, Renderer, Theme};

pub(super) struct Item<'a, Message> {
    position: usize,
    content: Element<'a, Message>,
    selected: bool,
    shared: Rc<Shared<'a>>,
}

impl<'a, Message> Item<'a, Message> {
    pub(super) fn new(
        position: usize,
        content: Element<'a, Message>,
        selected: bool,
        shared: Rc<Shared<'a>>,
    ) -> Self {
        Self {
            position,
            content,
            selected,
            shared,
        }
    }

    fn check_space(&self) -> f32 {
        self.shared.check_size + self.shared.item_padding.right
    }
}

impl<Message> Widget<Message, Theme, Renderer> for Item<'_, Message> {
    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.content.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fill, Length::Fill)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let size = limits.max();
        let padding = self.shared.item_padding;
        let space = Size::new(
            (size.width - padding.left - self.check_space()).max(0.0),
            (size.height - padding.top - padding.bottom).max(0.0),
        );

        let mut content = self.content.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, space),
        );
        content.move_to_mut((padding.left, padding.top));
        content.align_mut(Alignment::Start, Alignment::Center, space);

        layout::Node::with_children(size, vec![content])
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
        let content = layout.children().next().expect("the content of an item");
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        if shell.is_event_captured() || !cursor.is_over(layout.bounds()) {
            return;
        }

        match event {
            Event::Mouse(mouse::Event::CursorMoved { .. })
                if self.shared.hovered.get() != Some(self.position) =>
            {
                self.shared.hovered.set(Some(self.position));
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                self.shared.chosen.set(Some(self.position));
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let paint = theme.style(&self.shared.class, Status::Opened);
        let hovered = self.shared.hovered.get() == Some(self.position);

        let background = if self.selected {
            Some(paint.item_selected)
        } else if hovered {
            Some(paint.item_hovered)
        } else {
            None
        };

        if let Some(background) = background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::border::rounded(paint.item_radius),
                    snap: true,
                    ..renderer::Quad::default()
                },
                background,
            );
        }

        let text_style = renderer::Style {
            text_color: if self.selected {
                paint.item_selected_text
            } else {
                paint.item_text
            },
        };

        if let Some(content) = layout.children().next() {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                &text_style,
                content,
                cursor,
                viewport,
            );
        }

        if self.selected {
            let size = self.shared.check_size;
            let check = Rectangle {
                x: bounds.x + bounds.width - self.shared.item_padding.right - size,
                y: bounds.center_y() - size / 2.0,
                width: size,
                height: size,
            };
            renderer.draw_svg(
                Svg::new(self.shared.check.clone()).color(paint.check),
                check,
                bounds,
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
        let content =
            layout
                .children()
                .next()
                .map_or_else(mouse::Interaction::default, |content| {
                    self.content.as_widget().mouse_interaction(
                        &tree.children[0],
                        content,
                        cursor,
                        viewport,
                        renderer,
                    )
                });

        if content == mouse::Interaction::None && cursor.is_over(layout.bounds()) {
            mouse::Interaction::Pointer
        } else {
            content
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        if let Some(content) = layout.children().next() {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                content,
                renderer,
                operation,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let content = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            content,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: 'a> From<Item<'a, Message>> for Element<'a, Message> {
    fn from(item: Item<'a, Message>) -> Self {
        Element::new(item)
    }
}
