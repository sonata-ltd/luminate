use iced::advanced::widget::{Operation, Tree};
use iced::advanced::{Clipboard, Layout, Shell, layout, mouse, overlay, renderer};
use iced::keyboard::{self, key};
use iced::{Event, Point, Size, touch};

use super::{Options, Shared};
use crate::widget::virtual_list::scroll_into_view;
use crate::{Element, Renderer, Theme};

const GAP: f32 = 4.0;

pub(super) struct Menu<'a, 'b, Message> {
    pub(super) position: Point,
    pub(super) field: Size,
    pub(super) max_height: f32,
    pub(super) item_height: f32,
    pub(super) count: usize,
    pub(super) indices: Option<&'b [usize]>,
    pub(super) element: &'b mut Element<'a, Message>,
    pub(super) tree: &'b mut Tree,
    pub(super) open: &'b mut bool,
    pub(super) hovered: &'b mut Option<usize>,
    pub(super) reveal: &'b mut bool,
    pub(super) options: &'b Options<'a, Message>,
    pub(super) shared: &'b Shared<'a>,
    pub(super) on_close: Option<&'b Message>,
    pub(super) on_search: Option<&'b (dyn Fn(String) -> Message + 'a)>,
}

impl<Message: Clone> Menu<'_, '_, Message> {
    fn close(&mut self, shell: &mut Shell<'_, Message>) {
        *self.open = false;
        if let Some(on_close) = self.on_close {
            shell.publish(on_close.clone());
        }
        shell.request_redraw();
    }

    fn choose(&mut self, position: usize, shell: &mut Shell<'_, Message>) {
        let option = self.indices.map_or(position, |indices| indices[position]);

        if let Some(select) = &self.options.select {
            shell.publish(select(option));
        }

        if let Some(on_search) = self.on_search {
            shell.publish(on_search(String::new()));
        }

        self.close(shell);
    }

    fn hover(&mut self, position: usize, layout: Layout<'_>, renderer: &Renderer) {
        *self.hovered = Some(position);
        self.shared.hovered.set(Some(position));

        let top = position as f32 * self.item_height;
        scroll_into_view(
            self.element,
            self.tree,
            layout,
            renderer,
            top,
            top + self.item_height,
        );
    }
}

impl<Message: Clone> overlay::Overlay<Message, Theme, Renderer> for Menu<'_, '_, Message> {
    fn layout(&mut self, renderer: &Renderer, bounds: Size) -> layout::Node {
        let below = bounds.height - (self.position.y + self.field.height) - GAP;
        let above = self.position.y - GAP;
        let downward = below >= self.max_height || below >= above;
        let room = if downward { below } else { above };

        let limits = layout::Limits::new(
            Size::new(self.field.width, 0.0),
            Size::new(self.field.width, room.min(self.max_height).max(0.0)),
        );

        let node = self
            .element
            .as_widget_mut()
            .layout(self.tree, renderer, &limits);
        let height = node.size().height;

        let x = self
            .position
            .x
            .min(bounds.width - self.field.width)
            .max(0.0);
        let y = if downward {
            self.position.y + self.field.height + GAP
        } else {
            self.position.y - GAP - height
        };

        node.move_to(Point::new(x, y))
    }

    fn draw(
        &self,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
    ) {
        let bounds = layout.bounds();
        self.element
            .as_widget()
            .draw(self.tree, renderer, theme, style, layout, cursor, &bounds);
    }

    fn update(
        &mut self,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
    ) {
        let bounds = layout.bounds();

        if std::mem::take(self.reveal)
            && let Some(position) = *self.hovered
        {
            self.hover(position, layout, renderer);
        }

        self.element.as_widget_mut().update(
            self.tree, event, layout, cursor, renderer, clipboard, shell, &bounds,
        );

        if let Some(position) = self.shared.hovered.get() {
            *self.hovered = Some(position);
        }

        if let Some(position) = self.shared.chosen.take() {
            self.choose(position, shell);
            return;
        }

        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        ) && cursor.is_over(bounds)
        {
            shell.capture_event();
        }

        let Event::Keyboard(keyboard::Event::KeyPressed {
            key: keyboard::Key::Named(named),
            modifiers,
            ..
        }) = event
        else {
            return;
        };

        let Some(last) = self.count.checked_sub(1) else {
            if *named == key::Named::Escape {
                self.close(shell);
                shell.capture_event();
            }
            return;
        };

        let page = ((self.max_height / self.item_height).floor() as usize).max(1);
        let current = *self.hovered;

        let next = match (named, modifiers.shift()) {
            (key::Named::ArrowDown, _) | (key::Named::Tab, false) => {
                Some(current.map_or(0, |i| if i >= last { 0 } else { i + 1 }))
            }
            (key::Named::ArrowUp, _) | (key::Named::Tab, true) => {
                Some(current.map_or(last, |i| if i == 0 { last } else { i - 1 }))
            }
            (key::Named::Home, _) => Some(0),
            (key::Named::End, _) => Some(last),
            (key::Named::PageDown, _) => Some(current.map_or(0, |i| (i + page).min(last))),
            (key::Named::PageUp, _) => Some(current.map_or(0, |i| i.saturating_sub(page))),
            (key::Named::Enter | key::Named::Space, _)
                if *named == key::Named::Enter || self.on_search.is_none() =>
            {
                if let Some(position) = current {
                    self.choose(position, shell);
                }
                shell.capture_event();
                return;
            }
            (key::Named::Escape, _) => {
                self.close(shell);
                shell.capture_event();
                return;
            }
            _ => None,
        };

        if let Some(position) = next {
            self.hover(position, layout, renderer);
            shell.capture_event();
            shell.request_redraw();
        }
    }

    fn mouse_interaction(
        &self,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();
        let inner = self
            .element
            .as_widget()
            .mouse_interaction(self.tree, layout, cursor, &bounds, renderer);

        if inner == mouse::Interaction::None && cursor.is_over(bounds) {
            mouse::Interaction::Idle
        } else {
            inner
        }
    }

    fn operate(&mut self, layout: Layout<'_>, renderer: &Renderer, operation: &mut dyn Operation) {
        self.element
            .as_widget_mut()
            .operate(self.tree, layout, renderer, operation);
    }
}
