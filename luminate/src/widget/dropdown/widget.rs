use std::f32::consts::PI;
use std::rc::Rc;

use iced::advanced::svg::{Renderer as _, Svg};
use iced::advanced::widget::operation::Focusable;
use iced::advanced::widget::{Id, Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::keyboard::{self, key};
use iced::widget::{container, text, text_input};
use iced::{Event, Length, Padding, Point, Radians, Rectangle, Size, Vector, touch};
use iced_animate::{Anim, Motion, MotionKey, curves};

use super::menu::Menu;
use super::search;
use super::{Catalog, Item, Options, Shared, Status};
use crate::widget::fading_scrollable::fading_scrollable;
use crate::widget::virtual_list::VirtualList;
use crate::{Element, Renderer, Theme};

pub(super) struct DropdownWidget<'a, Message> {
    pub(super) options: Rc<Options<'a, Message>>,
    pub(super) field: Element<'a, Message>,
    pub(super) searchable: bool,
    pub(super) filter: bool,
    pub(super) query: &'a str,
    pub(super) on_search: Option<Rc<dyn Fn(String) -> Message + 'a>>,
    pub(super) on_open: Option<Message>,
    pub(super) on_close: Option<Message>,
    pub(super) width: Length,
    pub(super) height: f32,
    pub(super) padding: Padding,
    pub(super) handle_size: f32,
    pub(super) menu_height: f32,
    pub(super) menu_padding: f32,
    pub(super) item_height: f32,
    pub(super) empty: &'a str,
    pub(super) motion: Option<Motion>,
    pub(super) id: Option<Id>,
    pub(super) shared: Rc<Shared<'a>>,
    pub(super) menu: Option<Element<'a, Message>>,
    pub(super) indices: Option<Rc<Vec<usize>>>,
}

pub(super) struct State {
    open: bool,
    focused: bool,
    hovered: Option<usize>,
    reveal: bool,
    over: bool,
    modifiers: keyboard::Modifiers,
    filter: FilterCache,
    menu: Tree,
    chevron_key: MotionKey,
    chevron: Anim<f32>,
    chevron_open: bool,
}

#[derive(Default)]
struct FilterCache {
    query: String,
    matchers: Vec<String>,
    indices: Option<Rc<Vec<usize>>>,
}

impl FilterCache {
    fn clear(&mut self) {
        *self = Self::default();
    }
}

struct Focus<'s>(&'s mut bool);

impl Focusable for Focus<'_> {
    fn is_focused(&self) -> bool {
        *self.0
    }

    fn focus(&mut self) {
        *self.0 = true;
    }

    fn unfocus(&mut self) {
        *self.0 = false;
    }
}

pub(super) fn is_engaged(tree: &Tree) -> bool {
    if tree.tag == tree::Tag::of::<State>() {
        let state = tree.state.downcast_ref::<State>();
        return state.open || state.focused;
    }

    tree.children.iter().any(is_engaged)
}

fn text_input_focused(tree: &Tree) -> bool {
    type Paragraph = <Renderer as iced::advanced::text::Renderer>::Paragraph;

    if tree.tag == tree::Tag::of::<text_input::State<Paragraph>>() {
        return tree
            .state
            .downcast_ref::<text_input::State<Paragraph>>()
            .is_focused();
    }

    tree.children.iter().any(text_input_focused)
}

impl<'a, Message: Clone + 'a> DropdownWidget<'a, Message> {
    fn enabled(&self) -> bool {
        self.options.select.is_some()
    }

    fn filtered(&self, cache: &mut FilterCache) -> Option<Rc<Vec<usize>>> {
        if !self.searchable || !self.filter {
            return None;
        }

        let parts = search::query_parts(self.query);
        if parts.is_empty() {
            return None;
        }

        if cache.matchers.len() != self.options.count {
            cache.matchers = (0..self.options.count)
                .map(|i| search::normalize(&(self.options.key)(i)))
                .collect();
            cache.indices = None;
        }

        if cache.indices.is_none() || cache.query != self.query {
            let indices = cache
                .matchers
                .iter()
                .enumerate()
                .filter(|(_, matcher)| search::matches(matcher, &parts))
                .map(|(i, _)| i)
                .collect();

            cache.indices = Some(Rc::new(indices));
            self.query.clone_into(&mut cache.query);
        }

        cache.indices.clone()
    }

    fn open(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        state.open = true;
        state.reveal = true;

        let indices = self.filtered(&mut state.filter);
        state.hovered = self.options.selected.and_then(|selected| match &indices {
            Some(indices) => indices.iter().position(|&option| option == selected),
            None => Some(selected),
        });
        self.shared.hovered.set(state.hovered);

        if let Some(on_open) = &self.on_open {
            shell.publish(on_open.clone());
        }
        shell.request_redraw();
    }

    fn close(&self, state: &mut State, shell: &mut Shell<'_, Message>) {
        state.open = false;

        if let Some(on_close) = &self.on_close {
            shell.publish(on_close.clone());
        }
        shell.request_redraw();
    }

    fn build_menu(&self, indices: Option<Rc<Vec<usize>>>) -> Element<'a, Message> {
        let count = indices
            .as_ref()
            .map_or(self.options.count, |indices| indices.len());

        let body: Element<'a, Message> = if count == 0 {
            container(text(self.empty))
                .padding(self.shared.item_padding)
                .width(Length::Fill)
                .into()
        } else {
            let list = VirtualList::new(count, self.item_height, {
                let options = Rc::clone(&self.options);
                let shared = Rc::clone(&self.shared);
                move |position| {
                    let option = indices
                        .as_ref()
                        .map_or(position, |indices| indices[position]);

                    Item::new(
                        position,
                        (options.view)(option),
                        options.selected == Some(option),
                        Rc::clone(&shared),
                    )
                    .into()
                }
            });

            let scroll = fading_scrollable(list).height(Length::Shrink);
            match &self.motion {
                Some(motion) => scroll.motion(motion.clone()).into(),
                None => scroll.into(),
            }
        };

        let shared = Rc::clone(&self.shared);
        container(body)
            .padding(self.menu_padding)
            .width(Length::Fill)
            .style(move |theme: &Theme| {
                let paint = theme.style(&shared.class, Status::Opened);

                container::Style {
                    background: Some(paint.menu_background),
                    border: paint.menu_border,
                    shadow: paint.menu_shadow,
                    text_color: Some(paint.item_text),
                    snap: true,
                }
            })
            .into()
    }

    fn handle_bounds(&self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + bounds.width - self.padding.right - self.handle_size,
            y: bounds.center_y() - self.handle_size / 2.0,
            width: self.handle_size,
            height: self.handle_size,
        }
    }

    fn cycle(&self, y: f32, shell: &mut Shell<'_, Message>) {
        let Some(select) = &self.options.select else {
            return;
        };
        let Some(last) = self.options.count.checked_sub(1) else {
            return;
        };

        let next = match (self.options.selected, y < 0.0) {
            (None, true) => Some(0),
            (None, false) => Some(last),
            (Some(i), true) => (i < last).then_some(i + 1),
            (Some(i), false) => i.checked_sub(1),
        };

        if let Some(option) = next {
            shell.publish(select(option));
        }
        shell.capture_event();
    }
}

impl<'a, Message: Clone + 'a> Widget<Message, Theme, Renderer> for DropdownWidget<'a, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let chevron_key = MotionKey::unique();
        let chevron = match &self.motion {
            Some(motion) => motion.to(chevron_key, curves::QUICK, 0.0),
            None => Anim::constant(0.0),
        };

        tree::State::new(State {
            open: false,
            focused: false,
            hovered: None,
            reveal: false,
            over: false,
            modifiers: keyboard::Modifiers::default(),
            filter: FilterCache::default(),
            menu: Tree::empty(),
            chevron_key,
            chevron,
            chevron_open: false,
        })
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.field.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.field));
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, Length::Fixed(self.height))
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let limits = limits.width(self.width).height(self.height);
        let extra = self.padding.left + self.padding.right + self.handle_size + self.padding.right;
        let space = Size::new(
            (limits.max().width - extra).max(0.0),
            (self.height - self.padding.top - self.padding.bottom).max(0.0),
        );

        let mut field = self.field.as_widget_mut().layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, space),
        );
        field.move_to_mut((self.padding.left, self.padding.top));
        field.align_mut(iced::Alignment::Start, iced::Alignment::Center, space);

        let size = limits.resolve(
            self.width,
            self.height,
            Size::new(field.size().width + extra, self.height),
        );

        layout::Node::with_children(size, vec![field])
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
        let Tree {
            state, children, ..
        } = tree;
        let field_layout = layout.children().next().expect("the field of the dropdown");

        self.field.as_widget_mut().update(
            &mut children[0],
            event,
            field_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        let state = state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let over = cursor.is_over(bounds);

        if self.searchable {
            state.focused = text_input_focused(&children[0]);
        }

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                if state.open {
                    let on_handle = cursor.is_over(self.handle_bounds(bounds));
                    if !over || on_handle || !self.searchable {
                        self.close(state, shell);
                        shell.capture_event();
                    }
                } else if over && self.enabled() {
                    self.open(state, shell);
                    shell.capture_event();
                }

                if !self.searchable {
                    state.focused = over;
                }
            }
            Event::Keyboard(keyboard::Event::KeyPressed { key, text, .. })
                if state.focused && !state.open && self.enabled() =>
            {
                let opens = match key {
                    keyboard::Key::Named(key::Named::ArrowDown | key::Named::ArrowUp) => true,
                    keyboard::Key::Named(key::Named::Enter | key::Named::Space) => !self.searchable,
                    _ => self.searchable && text.is_some(),
                };

                if opens {
                    self.open(state, shell);
                    if !self.searchable {
                        shell.capture_event();
                    }
                }
            }
            Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
            }
            Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { y, .. },
            }) if state.modifiers.command() && over && !state.open => {
                self.cycle(*y, shell);
            }
            _ => {}
        }

        if !state.open {
            state.filter.clear();
        }

        if state.chevron_open != state.open {
            state.chevron_open = state.open;
            let angle = if state.open { PI } else { 0.0 };
            state.chevron = match &self.motion {
                Some(motion) => motion.to(state.chevron_key, curves::QUICK, angle),
                None => Anim::constant(angle),
            };
            shell.request_redraw();
        }

        if over != state.over {
            state.over = over;
            shell.request_redraw();
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
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let state = tree.state.downcast_ref::<State>();

        let status = if !self.enabled() {
            Status::Disabled
        } else if state.open {
            Status::Opened
        } else if cursor.is_over(bounds) {
            Status::Hovered
        } else {
            Status::Active
        };
        let paint = theme.style(&self.shared.class, status);

        renderer.fill_quad(
            renderer::Quad {
                bounds,
                border: paint.border,
                snap: true,
                ..renderer::Quad::default()
            },
            paint.background,
        );

        let text_color = if self.options.selected.is_some() || self.searchable {
            paint.text
        } else {
            paint.placeholder
        };

        if let Some(field) = layout.children().next() {
            self.field.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                &renderer::Style { text_color },
                field,
                cursor,
                &bounds,
            );
        }

        renderer.draw_svg(
            Svg::new(self.shared.chevron.clone())
                .color(paint.handle)
                .rotation(Radians(state.chevron.get())),
            self.handle_bounds(bounds),
            bounds,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let bounds = layout.bounds();

        if !cursor.is_over(bounds) || !self.enabled() {
            return mouse::Interaction::default();
        }

        if self.searchable && !cursor.is_over(self.handle_bounds(bounds)) {
            layout
                .children()
                .next()
                .map_or_else(mouse::Interaction::default, |field| {
                    self.field.as_widget().mouse_interaction(
                        &tree.children[0],
                        field,
                        cursor,
                        viewport,
                        renderer,
                    )
                })
        } else {
            mouse::Interaction::Pointer
        }
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

        if !self.searchable {
            operation.focusable(
                self.id.as_ref(),
                layout.bounds(),
                &mut Focus(&mut state.focused),
            );
        }

        operation.traverse(&mut |operation| {
            if let Some(field) = layout.children().next() {
                self.field
                    .as_widget_mut()
                    .operate(&mut children[0], field, renderer, operation);
            }
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        _renderer: &Renderer,
        _viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        let state = tree.state.downcast_mut::<State>();

        if !state.open {
            return None;
        }

        if self.menu.is_none() {
            let indices = self.filtered(&mut state.filter);
            self.menu = Some(self.build_menu(indices.clone()));
            self.indices = indices;
        }

        let element = self.menu.as_mut()?;
        state.menu.diff(element.as_widget());

        let bounds = layout.bounds();
        let count = self
            .indices
            .as_ref()
            .map_or(self.options.count, |indices| indices.len());

        let State {
            open,
            hovered,
            reveal,
            menu,
            ..
        } = state;

        Some(overlay::Element::new(Box::new(Menu {
            position: Point::new(bounds.x, bounds.y) + translation,
            field: bounds.size(),
            max_height: self.menu_height,
            item_height: self.item_height,
            count,
            indices: self.indices.as_deref().map(Vec::as_slice),
            element,
            tree: menu,
            open,
            hovered,
            reveal,
            options: &self.options,
            shared: &self.shared,
            on_close: self.on_close.as_ref(),
            on_search: self.on_search.as_deref(),
        })))
    }
}
