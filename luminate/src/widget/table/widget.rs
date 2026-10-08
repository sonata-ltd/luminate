use std::rc::Rc;

use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::border::Radius;
use iced::keyboard::{self, key};
use iced::{Alignment, Background, Color, Event, Length, Padding, Rectangle, Size, Vector, touch};

use super::layout::spans;
use super::{Catalog, RowRequest, Rows, Shared, Status};
use crate::widget::virtual_list::scroll_into_view;
use crate::{Element, Renderer, Theme};

pub(super) struct Parts<'a, Message> {
    pub(super) headers: Vec<Element<'a, Message>>,
    pub(super) widths: Vec<Length>,
    pub(super) body: Element<'a, Message>,
    pub(super) data: Rc<Rows<'a, Message>>,
    pub(super) shared: Rc<Shared<'a>>,
    pub(super) width: Length,
    pub(super) height: Length,
    pub(super) row_height: f32,
    pub(super) header_height: f32,
    pub(super) cell_padding_x: f32,
    pub(super) inset: Padding,
    pub(super) scrolls: bool,
    pub(super) id: Option<Id>,
}

pub(super) struct TableWidget<'a, Message> {
    parts: Parts<'a, Message>,
}

#[derive(Debug, Default)]
struct State {
    focus: Focus,
    active: Option<usize>,
    keyboard: bool,
    request: RowRequest,
}

#[derive(Debug, Default)]
struct Focus {
    focused: bool,
}

impl State {
    /// A press takes the focus into the table or out of it, and hands the
    /// cursor over to the mouse, which hides the keyboard ring.
    fn focus_on_press<Message>(
        &mut self,
        event: &Event,
        inside: bool,
        shell: &mut Shell<'_, Message>,
    ) {
        if matches!(
            event,
            Event::Mouse(mouse::Event::ButtonPressed(_))
                | Event::Touch(touch::Event::FingerPressed { .. })
        ) {
            self.keyboard = false;

            if inside != self.focus.focused {
                self.focus.focused = inside;
                shell.request_redraw();
            }
        }
    }
}

impl Focusable for Focus {
    fn is_focused(&self) -> bool {
        self.focused
    }

    fn focus(&mut self) {
        self.focused = true;
    }

    fn unfocus(&mut self) {
        self.focused = false;
    }
}

impl<'a, Message> TableWidget<'a, Message> {
    pub(super) fn new(parts: Parts<'a, Message>) -> Self {
        Self { parts }
    }

    fn is_enabled(&self, row: usize) -> bool {
        !(self.parts.data.disabled)(row)
    }

    fn enabled_from(&self, from: usize) -> Option<usize> {
        (from..self.parts.data.count).find(|&row| self.is_enabled(row))
    }

    fn enabled_until(&self, from: usize) -> Option<usize> {
        let last = self.parts.data.count.checked_sub(1)?;
        (0..=from.min(last)).rev().find(|&row| self.is_enabled(row))
    }

    fn navigate(&self, active: Option<usize>, named: key::Named, page: usize) -> Option<usize> {
        let last = self.parts.data.count.checked_sub(1)?;
        let active = active.map(|row| row.min(last));

        match (named, active) {
            (key::Named::ArrowDown, None) | (key::Named::Home, _) => self.enabled_from(0),
            (key::Named::ArrowUp, None) | (key::Named::End, _) => self.enabled_until(last),
            (key::Named::ArrowDown, Some(row)) => self.enabled_from(row + 1).or(Some(row)),
            (key::Named::ArrowUp, Some(row)) => row
                .checked_sub(1)
                .and_then(|above| self.enabled_until(above))
                .or(Some(row)),
            (key::Named::PageDown, row) => {
                let target = row.map_or(0, |row| (row + page).min(last));
                self.enabled_from(target)
                    .or_else(|| self.enabled_until(target))
            }
            (key::Named::PageUp, row) => {
                let target = row.map_or(0, |row| row.saturating_sub(page));
                self.enabled_until(target)
                    .or_else(|| self.enabled_from(target))
            }
            _ => None,
        }
    }

    fn ensure_visible(
        &mut self,
        body: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        row: usize,
    ) {
        if !self.parts.scrolls {
            return;
        }

        let top = row as f32 * self.parts.row_height + self.parts.inset.top;
        scroll_into_view(
            &mut self.parts.body,
            body,
            layout,
            renderer,
            top,
            top + self.parts.row_height,
        );
    }
}

impl<Message: Clone> Widget<Message, Theme, Renderer> for TableWidget<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn children(&self) -> Vec<Tree> {
        self.parts
            .headers
            .iter()
            .map(|header| Tree::new(header.as_widget()))
            .chain(std::iter::once(Tree::new(self.parts.body.as_widget())))
            .collect()
    }

    fn diff(&self, tree: &mut Tree) {
        let children: Vec<&dyn Widget<Message, Theme, Renderer>> = self
            .parts
            .headers
            .iter()
            .map(Element::as_widget)
            .chain(std::iter::once(self.parts.body.as_widget()))
            .collect();

        tree.diff_children(&children);
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.parts.width, self.parts.height)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let Parts {
            headers,
            widths,
            body,
            data,
            shared,
            width,
            height,
            row_height,
            header_height,
            cell_padding_x,
            inset,
            scrolls,
            ..
        } = &mut self.parts;

        let limits = limits.width(*width).height(*height);
        let (header_trees, body_tree) = tree.children.split_at_mut(headers.len());

        let unbounded = layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, *header_height));
        let measured: Vec<f32> = headers
            .iter_mut()
            .zip(header_trees.iter_mut())
            .zip(widths.iter())
            .map(|((header, child), width)| match width {
                Length::Shrink => {
                    header
                        .as_widget_mut()
                        .layout(child, renderer, &unbounded)
                        .size()
                        .width
                }
                _ => 0.0,
            })
            .collect();

        let indicator = shared.indicator.map(|metrics| Length::Fixed(metrics.size));
        let all_widths: Vec<Length> = indicator
            .into_iter()
            .chain(widths.iter().copied())
            .collect();
        let all_measured: Vec<f32> = indicator.map(|_| 0.0).into_iter().chain(measured).collect();

        let (columns, content_width) = spans(
            limits.max().width,
            &all_widths,
            &all_measured,
            *cell_padding_x,
            *inset,
        );
        let table_width = limits
            .resolve(*width, Length::Shrink, Size::new(content_width, 0.0))
            .width;

        let first = usize::from(shared.indicator.is_some());
        let mut nodes: Vec<layout::Node> = headers
            .iter_mut()
            .zip(header_trees.iter_mut())
            .zip(&shared.aligns)
            .enumerate()
            .map(|(column, ((header, child), (align_x, _)))| {
                let span = columns[first + column];
                let space = Size::new(span.width, *header_height);
                let mut node = header.as_widget_mut().layout(
                    child,
                    renderer,
                    &layout::Limits::new(Size::ZERO, space),
                );
                node.move_to_mut((span.x + inset.left, 0.0));
                node.align_mut(Alignment::from(*align_x), Alignment::Center, space);
                node
            })
            .collect();

        *shared.spans.borrow_mut() = columns;

        let body_max_height = if *scrolls {
            let intrinsic = Size::new(
                table_width,
                *header_height + data.count as f32 * *row_height + inset.top + inset.bottom,
            );
            (limits.resolve(*width, *height, intrinsic).height - *header_height).max(0.0)
        } else {
            f32::INFINITY
        };

        let body_node = body
            .as_widget_mut()
            .layout(
                &mut body_tree[0],
                renderer,
                &layout::Limits::new(Size::ZERO, Size::new(table_width, body_max_height)),
            )
            .move_to((0.0, *header_height));

        let size = limits.resolve(
            *width,
            *height,
            Size::new(table_width, *header_height + body_node.size().height),
        );

        nodes.push(body_node);
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
        let header_count = self.parts.headers.len();
        let Tree {
            state, children, ..
        } = tree;
        let (header_trees, body_tree) = children.split_at_mut(header_count);
        let body_layout = layout
            .children()
            .nth(header_count)
            .expect("the body of the table");

        for ((header, child), layout) in self
            .parts
            .headers
            .iter_mut()
            .zip(header_trees.iter_mut())
            .zip(layout.children())
        {
            header.as_widget_mut().update(
                child, event, layout, cursor, renderer, clipboard, shell, viewport,
            );
        }

        self.parts.body.as_widget_mut().update(
            &mut body_tree[0],
            event,
            body_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let state = state.downcast_mut::<State>();
        let mut moved = None;

        state.focus_on_press(event, cursor.is_over(layout.bounds()), shell);

        if let Some(row) = self.parts.shared.pressed.take() {
            state.active = Some(row);
        }

        if let Some(row) = state.request.0.take()
            && let Some(last) = self.parts.data.count.checked_sub(1)
        {
            let row = row.min(last);
            state.active = Some(row);
            moved = Some(row);

            state.keyboard = true;

            shell.request_redraw();
        }

        if state.focus.focused
            && !shell.is_event_captured()
            && let Event::Keyboard(keyboard::Event::KeyPressed {
                key: keyboard::Key::Named(named),
                ..
            }) = event
        {
            let visible = if self.parts.scrolls {
                body_layout.bounds().height
            } else {
                viewport.height
            };
            let page = ((visible / self.parts.row_height).floor() as usize).max(1);

            match named {
                key::Named::Enter | key::Named::Space => {
                    if let Some(row) = state.active
                        && row < self.parts.data.count
                        && let Some(message) = (self.parts.data.meta)(row).on_press
                    {
                        shell.publish(message);
                    }
                    state.keyboard = true;
                    shell.capture_event();
                }
                key::Named::Escape => {
                    state.focus.focused = false;
                    shell.capture_event();
                    shell.request_redraw();
                }
                named => {
                    if let Some(row) = self.navigate(state.active, *named, page) {
                        state.active = Some(row);
                        moved = Some(row);

                        state.keyboard = true;

                        shell.capture_event();
                        shell.request_redraw();
                    }
                }
            }
        }

        if let Some(row) = moved {
            self.ensure_visible(&mut body_tree[0], body_layout, renderer, row);
        }

        self.parts.shared.keyboard.set(state.keyboard);
        self.parts.shared.active.set(state.active);
        self.parts.shared.focused.set(state.focus.focused);
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
        let paint = theme.style(&self.parts.shared.class, Status::default());
        let header_count = self.parts.headers.len();

        let state = tree.state.downcast_ref::<State>();

        self.parts.shared.keyboard.set(state.keyboard);
        self.parts.shared.active.set(state.active);
        self.parts.shared.focused.set(state.focus.focused);

        if let Some(background) = paint.background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: iced::Border {
                        radius: paint.border.radius,
                        ..iced::Border::default()
                    },
                    snap: true,
                    ..renderer::Quad::default()
                },
                background,
            );
        }

        let header = Rectangle {
            height: self.parts.header_height,
            ..bounds
        };

        if let Some(background) = paint.header_background {
            renderer.fill_quad(
                renderer::Quad {
                    bounds: header,
                    border: iced::Border {
                        radius: Radius {
                            bottom_left: 0.0,
                            bottom_right: 0.0,
                            ..paint.border.radius
                        },
                        ..iced::Border::default()
                    },
                    snap: true,
                    ..renderer::Quad::default()
                },
                background,
            );
        }

        let header_style = renderer::Style {
            text_color: paint.header_text,
        };

        for ((header, child), layout) in self
            .parts
            .headers
            .iter()
            .zip(&tree.children)
            .zip(layout.children())
        {
            header.as_widget().draw(
                child,
                renderer,
                theme,
                &header_style,
                layout,
                cursor,
                viewport,
            );
        }

        renderer.fill_quad(
            renderer::Quad {
                bounds: Rectangle {
                    y: header.y + header.height - 1.0,
                    height: 1.0,
                    ..header
                },
                snap: true,
                ..renderer::Quad::default()
            },
            Background::Color(paint.divider),
        );

        if let Some(body_layout) = layout.children().nth(header_count) {
            self.parts.body.as_widget().draw(
                &tree.children[header_count],
                renderer,
                theme,
                style,
                body_layout,
                cursor,
                viewport,
            );
        }

        if paint.border.width > 0.0 {
            renderer.fill_quad(
                renderer::Quad {
                    bounds,
                    border: paint.border,
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
        self.parts
            .headers
            .iter()
            .chain(std::iter::once(&self.parts.body))
            .zip(&tree.children)
            .zip(layout.children())
            .map(|((child, state), layout)| {
                child
                    .as_widget()
                    .mouse_interaction(state, layout, cursor, viewport, renderer)
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
        let Tree {
            state, children, ..
        } = tree;
        let state = state.downcast_mut::<State>();
        let bounds = layout.bounds();

        operation.focusable(self.parts.id.as_ref(), bounds, &mut state.focus);
        operation.custom(self.parts.id.as_ref(), bounds, &mut state.request);

        operation.traverse(&mut |operation| {
            for ((child, state), layout) in self
                .parts
                .headers
                .iter_mut()
                .chain(std::iter::once(&mut self.parts.body))
                .zip(children.iter_mut())
                .zip(layout.children())
            {
                child
                    .as_widget_mut()
                    .operate(state, layout, renderer, operation);
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
        let children: Vec<_> = self
            .parts
            .headers
            .iter_mut()
            .chain(std::iter::once(&mut self.parts.body))
            .zip(tree.children.iter_mut())
            .zip(layout.children())
            .filter_map(|((child, state), layout)| {
                child
                    .as_widget_mut()
                    .overlay(state, layout, renderer, viewport, translation)
            })
            .collect();

        (!children.is_empty()).then(|| overlay::Group::with_children(children).overlay())
    }
}
