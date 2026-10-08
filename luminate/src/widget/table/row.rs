use std::rc::Rc;

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::border::Radius;
use iced::{Alignment, Background, Border, Color, Event, Length, Rectangle, Size, Vector, touch};

use super::{Catalog, Meta, Rows, Shared, Status};
use crate::widget::pick_indicator;
use crate::{Element, Renderer, Theme};

pub(super) struct RowWidget<'a, Message> {
    index: usize,
    cells: Vec<Element<'a, Message>>,
    meta: Meta<Message>,
    shared: Rc<Shared<'a>>,
}

#[derive(Debug)]
struct State {
    hovered: bool,
    pressed: bool,
    indicator: pick_indicator::State,
}

impl<'a, Message> RowWidget<'a, Message> {
    pub(super) fn new(index: usize, rows: &Rows<'a, Message>, shared: Rc<Shared<'a>>) -> Self {
        Self {
            index,
            cells: (rows.cells)(index),
            meta: (rows.meta)(index),
            shared,
        }
    }

    fn first_cell(&self) -> usize {
        usize::from(self.shared.indicator.is_some())
    }

    fn status(&self, state: &State, cursor: mouse::Cursor, bounds: Rectangle) -> Status {
        let enabled = !self.meta.disabled;

        Status {
            hovered: enabled && cursor.is_over(bounds),
            pressed: enabled && state.pressed,
            pick: self.meta.pick,
            disabled: self.meta.disabled,
            odd: self.shared.striped && self.index % 2 == 1,
            active: self.shared.focused.get()
                && self.shared.keyboard.get()
                && self.shared.active.get() == Some(self.index),
        }
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for RowWidget<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            hovered: false,
            pressed: false,
            indicator: pick_indicator::State::new(self.meta.pick, self.shared.motion.as_ref()),
        })
    }

    fn children(&self) -> Vec<Tree> {
        self.cells
            .iter()
            .map(|cell| Tree::new(cell.as_widget()))
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(&self.cells);
        tree.state
            .downcast_mut::<State>()
            .indicator
            .retarget(self.meta.pick, self.shared.motion.as_ref());
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
        let spans = self.shared.spans.borrow();
        let first = self.first_cell();

        let nodes = self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .zip(&self.shared.aligns)
            .enumerate()
            .map(|(column, ((cell, child), (align_x, align_y)))| {
                let span = spans.get(first + column).copied().unwrap_or_default();
                let space = Size::new(span.width, size.height);

                let limits = layout::Limits::new(Size::ZERO, space);
                let mut node = cell.as_widget_mut().layout(child, renderer, &limits);

                node.move_to_mut((span.x, 0.0));
                node.align_mut(Alignment::from(*align_x), Alignment::from(*align_y), space);
                node
            })
            .collect();

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
        for ((cell, child), layout) in self
            .cells
            .iter_mut()
            .zip(&mut tree.children)
            .zip(layout.children())
        {
            cell.as_widget_mut().update(
                child, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }

        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<State>();

        let hovered = cursor.is_over(bounds);
        if hovered != state.hovered {
            state.hovered = hovered;
            shell.request_redraw();
        }

        if shell.is_event_captured() {
            return;
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. })
                if hovered && self.meta.on_press.is_some() =>
            {
                state.pressed = true;
                self.shared.pressed.set(Some(self.index));
                shell.capture_event();
                shell.request_redraw();
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. })
                if state.pressed =>
            {
                state.pressed = false;
                shell.request_redraw();

                if hovered && let Some(message) = &self.meta.on_press {
                    shell.publish(message.clone());
                    shell.capture_event();
                }
            }
            _ => {}
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
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();
        let status = self.status(state, cursor, bounds);
        let paint = theme.style(&self.shared.class, status);

        if let Some(background) = paint.row_background {
            let mut radius = Radius::from(paint.cell_radius);

            if self.index + 1 == self.shared.count {
                // The last row sits in the table's rounded bottom corners.
                let table = theme
                    .style(&self.shared.class, Status::default())
                    .border
                    .radius;
                radius.bottom_left = table.bottom_left;
                radius.bottom_right = table.bottom_right;
            }

            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    snap: true,
                    border: Border {
                        radius,
                        ..Default::default()
                    },
                    ..renderer::Quad::default()
                },
                background,
            );
        }

        if let Some(metrics) = self.shared.indicator
            && let Some(slot) = self.shared.spans.borrow().first()
        {
            let slot = Rectangle {
                x: bounds.x + slot.x,
                width: slot.width,
                ..bounds
            };
            pick_indicator::draw(renderer, slot, &state.indicator, metrics, paint.indicator);
        }

        for ((cell, child), layout) in self.cells.iter().zip(&tree.children).zip(layout.children())
        {
            cell.as_widget()
                .draw(child, renderer, theme, style, layout, cursor, viewport);
        }

        if status.disabled
            && let Some(veil) = paint.row_veil
            && let Some(clip) = bounds.intersection(viewport)
        {
            // A layer's clip replaces the parent's instead of narrowing it, so
            // intersect with the viewport or the veil spills over the header.
            renderer.with_layer(clip, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        snap: true,
                        ..renderer::Quad::default()
                    },
                    veil,
                );
            });
        }

        if status.active && paint.focus_ring.width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: paint.focus_ring,
                    snap: true,
                    ..renderer::Quad::default()
                },
                Background::Color(Color::TRANSPARENT),
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
        let cells = self
            .cells
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((cell, child), layout)| {
                cell.as_widget()
                    .mouse_interaction(child, layout, cursor, viewport, renderer)
            })
            .max()
            .unwrap_or_default();

        if cells == mouse::Interaction::None
            && self.meta.on_press.is_some()
            && cursor.is_over(layout.bounds())
        {
            mouse::Interaction::Pointer
        } else {
            cells
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds());
        operation.traverse(&mut |operation| {
            for ((cell, child), layout) in self
                .cells
                .iter_mut()
                .zip(&mut tree.children)
                .zip(layout.children())
            {
                cell.as_widget_mut()
                    .operate(child, layout, renderer, operation);
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
        overlay::from_children(
            &mut self.cells,
            tree,
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message: Clone + 'a> From<RowWidget<'a, Message>> for Element<'a, Message> {
    fn from(row: RowWidget<'a, Message>) -> Self {
        Element::new(row)
    }
}
