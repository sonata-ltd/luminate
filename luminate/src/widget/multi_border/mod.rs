//! Multiple border rings around any element, with hover/press/focus state.

use std::slice::from_ref;

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer};
use iced::{Event, Length, Rectangle, Size, Vector, touch, window};

mod draw;
mod types;

pub use types::{Ring, Side, Status, Style};

catalog!(status: Status, |_theme, _status| Style::default());

/// How the widget decides it is focused.
///
/// Neither variant observes keyboard focus traversal or window focus: with
/// [`Click`](Self::Click) a `Tab` press or a window `Unfocused` event
/// changes nothing; only a press inside or outside the content does.
/// [`MultiBorder::focused`] overrides both.
#[derive(Default)]
pub enum Focus<'a> {
    /// A press on the content focuses, a press elsewhere blurs. The press
    /// counts even when the content captured it, that is exactly what a
    /// click into a text input does.
    #[default]
    Click,
    /// Asks the content's widget tree, e.g. whether a text input inside is
    /// focused.
    ///
    /// Evaluated after the content has handled a mouse, touch or keyboard
    /// event or a `RedrawRequested` window event: the events that can move
    /// focus, plus the redraw that follows a focus operation (`focus_next`,
    /// …). Other window events and input-method events skip it, so the tree
    /// walk never runs for them.
    Custom(Box<dyn Fn(&Tree) -> bool + 'a>),
}

impl std::fmt::Debug for Focus<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Focus::Click => "Focus::Click",
            Focus::Custom(_) => "Focus::Custom(..)",
        })
    }
}

#[derive(Debug, Default)]
struct State {
    is_pressed: bool,
    is_focused: bool,
    is_hovered: bool,
}

/// Draws border rings (and an optional background) around `content` and
/// tracks its hover, press and focus state for styling.
///
/// The style comes from the theme's [`Catalog`] or a
/// [`style`](Self::style) closure and may change with the [`Status`].
///
/// An outer ring either takes space around the content or is drawn past
/// the layout box. Taking space is the default, and because the layout
/// cannot ask the style how much (it depends on the status),
/// [`outer_thickness`](Self::outer_thickness) reserves it up front. A ring
/// marked [`overflowing`](Ring::overflowing) needs no reservation: the
/// widget's box stays the size of its content and the ring is painted over
/// whatever sits beside it — right for an outline that appears only while
/// pressed or focused, where reserving room would space every widget apart
/// even at rest. Hover and press are tracked over the content rectangle,
/// not the reserved gutter.
///
/// # Example
///
/// ```
/// use iced_luminate::iced::widget::text;
/// use iced_luminate::iced::{Color, Element, Theme};
/// use iced_luminate::widget::multi_border::{Ring, Style, multi_border};
///
/// let ring_width = 2.0;
/// let bordered: Element<'_, (), Theme, iced_luminate::Renderer> =
///     multi_border(text("hello"))
///         .outer_thickness(ring_width)
///         .style(move |_theme, status| {
///             if status.is_hovered {
///                 Style::new().ring(Ring::outer(ring_width, Color::BLACK).radius(6.0))
///             } else {
///                 Style::new()
///             }
///         })
///         .into();
/// ```
pub struct MultiBorder<'a, Message, Theme = iced::Theme, Renderer = crate::Renderer>
where
    Theme: Catalog,
{
    content: iced::Element<'a, Message, Theme, Renderer>,
    class: Theme::Class<'a>,
    disabled: bool,
    /// Overrides the tracked focus when set.
    focused: Option<bool>,
    width: Option<Length>,
    height: Option<Length>,
    outer_thickness: f32,
    focus: Focus<'a>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for MultiBorder<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiBorder")
            .field("disabled", &self.disabled)
            .field("focused", &self.focused)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("outer_thickness", &self.outer_thickness)
            .field("focus", &self.focus)
            .finish_non_exhaustive()
    }
}

impl<'a, Message, Theme, Renderer> MultiBorder<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    /// Borders around `content`, styled by the catalog's default class.
    #[must_use]
    pub fn new(content: impl Into<iced::Element<'a, Message, Theme, Renderer>>) -> Self {
        Self {
            content: content.into(),
            class: Theme::default(),
            disabled: false,
            focused: None,
            width: None,
            height: None,
            outer_thickness: 0.0,
            focus: Focus::Click,
        }
    }

    /// Styles by status with a closure.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    /// Uses a class from the theme's catalog.
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    /// Reports the widget as disabled: no hover, press or focus.
    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Overrides the tracked focus state.
    #[must_use]
    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = Some(focused);
        self
    }

    /// How focus is detected (default: [`Focus::Click`]).
    #[must_use]
    pub fn focus(mut self, focus: Focus<'a>) -> Self {
        self.focus = focus;
        self
    }

    /// Overrides the width (default: the content's).
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = Some(width.into());
        self
    }

    /// Overrides the height (default: the content's).
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = Some(height.into());
        self
    }

    /// Reserves `thickness` around the content for outer rings.
    ///
    /// The layout box grows by `thickness` on every side and the content is
    /// inset by it. Every [`Style`] the class can produce must satisfy
    /// `style.reserved_thickness() <= thickness`: a debug build asserts it
    /// when drawing; a release build paints the excess outside the layout
    /// box, unclipped.
    ///
    /// Only rings that take room count. A ring marked
    /// [`overflowing`](Ring::overflowing) is drawn past the layout box by
    /// design and needs no reservation, so a widget whose every outer ring
    /// overflows never calls this at all — the default of `0.0` is right,
    /// and its box stays the size of its content.
    ///
    /// Layout runs before the style is resolved (it has neither the theme
    /// nor the [`Status`]), so the widget cannot ask the style how much to
    /// reserve: the total is declared here, up front, for the thickest
    /// style the class can produce.
    ///
    /// # Panics
    /// In debug builds, when `thickness` is negative or not finite, and at
    /// draw time when the resolved style's
    /// [`reserved_thickness`](Style::reserved_thickness) exceeds the
    /// reserved `thickness`.
    #[must_use]
    pub fn outer_thickness(mut self, thickness: f32) -> Self {
        debug_assert!(
            thickness.is_finite() && thickness >= 0.0,
            "outer thickness must be finite and non-negative, got {thickness}"
        );
        self.outer_thickness = thickness;
        self
    }

    fn status(&self, state: &State) -> Status {
        if self.disabled {
            return Status {
                is_disabled: true,
                ..Status::default()
            };
        }

        Status {
            is_hovered: state.is_hovered,
            is_pressed: state.is_pressed,
            is_focused: self.focused.unwrap_or(state.is_focused),
            is_disabled: false,
        }
    }

    fn resolve(&self, theme: &Theme, status: Status) -> Style {
        let style = theme.style(&self.class, status);
        debug_assert!(
            style.reserved_thickness() <= self.outer_thickness + 0.01,
            "the style's reserved outer rings ({} px) exceed the reserved outer thickness \
             ({} px); call `outer_thickness` with the thickest style the class produces, \
             or mark the ring `overflowing` to draw it past the layout box",
            style.reserved_thickness(),
            self.outer_thickness
        );
        style
    }
}

/// The single child of a [`MultiBorder`] layout.
fn content_layout(layout: Layout<'_>) -> Layout<'_> {
    layout
        .children()
        .next()
        .expect("MultiBorder layout has exactly one child")
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for MultiBorder<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        let content = self.content.as_widget().size();

        Size::new(
            self.width.unwrap_or(content.width),
            self.height.unwrap_or(content.height),
        )
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
        let hint = self.content.as_widget().size();
        let width = self.width.unwrap_or(hint.width);
        let height = self.height.unwrap_or(hint.height);

        // Outer rings take space; inner rings are drawn over the content.
        layout::padded(limits, width, height, self.outer_thickness, |limits| {
            self.content
                .as_widget_mut()
                .layout(&mut tree.children[0], renderer, limits)
        })
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
        if !layout.bounds().intersects(viewport) {
            return;
        }

        let content_layout = content_layout(layout);
        let status = self.status(tree.state.downcast_ref::<State>());
        let resolved = self.resolve(theme, status);
        let content_bounds = content_layout.bounds();

        draw::background_and_outer(renderer, &resolved, content_bounds);

        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            content_layout,
            cursor,
            viewport,
        );

        draw::inner_rings(renderer, &resolved, content_bounds);
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
        let content_layout = content_layout(layout);

        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            content_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );

        // The status is constant while disabled: nothing to track or redraw.
        if self.disabled {
            return;
        }

        let is_custom = matches!(self.focus, Focus::Custom(_));
        let custom_focus = match (&self.focus, event) {
            (
                Focus::Custom(detect),
                Event::Mouse(_)
                | Event::Touch(_)
                | Event::Keyboard(_)
                | Event::Window(window::Event::RedrawRequested(_)),
            ) => Some(detect(&tree.children[0])),
            _ => None,
        };

        let bounds = content_layout.bounds();
        let state = tree.state.downcast_mut::<State>();
        let before = (state.is_pressed, state.is_focused, state.is_hovered);

        state.is_hovered = cursor.is_over(bounds);

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerPressed { .. }) => {
                state.is_pressed = state.is_hovered;
                if !is_custom {
                    state.is_focused = state.is_hovered;
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
            | Event::Touch(touch::Event::FingerLifted { .. } | touch::Event::FingerLost { .. }) => {
                state.is_pressed = false;
            }
            Event::Mouse(mouse::Event::CursorLeft) => {
                state.is_hovered = false;
                state.is_pressed = false;
            }
            _ => {}
        }

        if let Some(focused) = custom_focus {
            state.is_focused = focused;
        }

        if before != (state.is_pressed, state.is_focused, state.is_hovered) {
            shell.request_redraw();
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.content.as_widget_mut().operate(
            &mut tree.children[0],
            content_layout(layout),
            renderer,
            operation,
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            content_layout(layout),
            cursor,
            viewport,
            renderer,
        )
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
            content_layout(layout),
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<MultiBorder<'a, Message, Theme, Renderer>>
    for iced::Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: Catalog + 'a,
    Renderer: renderer::Renderer + 'a,
{
    fn from(value: MultiBorder<'a, Message, Theme, Renderer>) -> Self {
        iced::Element::new(value)
    }
}

/// Wraps `content` in a [`MultiBorder`].
#[must_use]
pub fn multi_border<'a, Message, Theme, Renderer>(
    content: impl Into<iced::Element<'a, Message, Theme, Renderer>>,
) -> MultiBorder<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer + 'a,
{
    MultiBorder::new(content)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use iced::advanced::widget::tree;
    use iced::widget::{button, text, text_input};
    use iced::{Point, Size};
    use iced_test::Simulator;

    use super::*;

    type Element<'a> = iced::Element<'a, (), iced::Theme, crate::Renderer>;

    fn recording(
        status: Rc<Cell<Status>>,
        content: impl Into<Element<'static>>,
    ) -> MultiBorder<'static, (), iced::Theme, crate::Renderer> {
        multi_border(content).style(move |_, s| {
            status.set(s);
            Style::default()
        })
    }

    fn simulator(root: Element<'_>) -> Simulator<'_, (), iced::Theme, crate::Renderer> {
        Simulator::with_size(iced::Settings::default(), Size::new(200.0, 100.0), root)
    }

    /// Whether any `text_input` in `tree` is focused (the kit's `Focus::Custom`).
    fn text_input_focused(tree: &Tree) -> bool {
        type Paragraph = <crate::Renderer as iced::advanced::text::Renderer>::Paragraph;

        if tree.tag == tree::Tag::of::<text_input::State<Paragraph>>() {
            return tree
                .state
                .downcast_ref::<text_input::State<Paragraph>>()
                .is_focused();
        }

        tree.children.iter().any(text_input_focused)
    }

    #[test]
    fn hovering_sets_and_leaves_clear_the_status() {
        let status = Rc::new(Cell::new(Status::default()));
        let mut ui = simulator(recording(status.clone(), text("content")).into());

        ui.point_at(Point::new(10.0, 10.0));
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(status.get().is_hovered, "{:?}", status.get());

        ui.point_at(Point::new(500.0, 500.0));
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(!status.get().is_hovered, "{:?}", status.get());
    }

    #[test]
    fn the_reserved_gutter_is_not_hoverable() {
        let status = Rc::new(Cell::new(Status::default()));
        let root: Element<'_> = recording(status.clone(), text("content"))
            .outer_thickness(10.0)
            .into();
        let mut ui = simulator(root);

        // Inside the node, but in the 10 px ring gutter around the content.
        ui.point_at(Point::new(3.0, 3.0));
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(!status.get().is_hovered, "{:?}", status.get());

        ui.point_at(Point::new(15.0, 15.0));
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(status.get().is_hovered, "{:?}", status.get());
    }

    #[test]
    fn clicking_inside_focuses_and_outside_blurs() {
        let status = Rc::new(Cell::new(Status::default()));
        let mut ui = simulator(recording(status.clone(), button(text("content"))).into());

        let _ = ui.click("content").expect("on screen");
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(status.get().is_focused, "{:?}", status.get());
        assert!(
            !status.get().is_pressed,
            "released again: {:?}",
            status.get()
        );

        ui.point_at(Point::new(190.0, 95.0));
        let _ = ui.simulate(iced_test::simulator::click());
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(!status.get().is_focused, "{:?}", status.get());
    }

    #[test]
    fn a_touch_presses_and_lifting_releases() {
        let status = Rc::new(Cell::new(Status::default()));
        let mut ui = simulator(recording(status.clone(), text("content")).into());
        let finger = touch::Finger(0);
        let position = Point::new(10.0, 10.0);

        ui.point_at(position);
        let _ = ui.simulate([Event::Touch(touch::Event::FingerPressed {
            id: finger,
            position,
        })]);
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(status.get().is_pressed, "{:?}", status.get());

        let _ = ui.simulate([Event::Touch(touch::Event::FingerLifted {
            id: finger,
            position,
        })]);
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(!status.get().is_pressed, "{:?}", status.get());
        assert!(
            status.get().is_focused,
            "a touch focuses: {:?}",
            status.get()
        );
    }

    #[test]
    fn custom_focus_follows_the_text_input_inside() {
        let status = Rc::new(Cell::new(Status::default()));
        let root: Element<'_> = recording(status.clone(), text_input("type here", ""))
            .focus(Focus::Custom(Box::new(text_input_focused)))
            .into();
        let mut ui = simulator(root);

        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(!status.get().is_focused, "{:?}", status.get());

        let _ = ui.click("type here").expect("on screen");
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(status.get().is_focused, "{:?}", status.get());

        ui.point_at(Point::new(190.0, 95.0));
        let _ = ui.simulate(iced_test::simulator::click());
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(!status.get().is_focused, "{:?}", status.get());
    }

    #[test]
    fn a_focused_override_and_disabled_win_over_tracked_state() {
        let status = Rc::new(Cell::new(Status::default()));
        let root: Element<'_> = recording(status.clone(), text("content"))
            .focused(true)
            .into();
        let mut ui = simulator(root);
        let _ = ui.snapshot(&iced::Theme::Light);
        assert!(status.get().is_focused);

        let root: Element<'_> = recording(status.clone(), text("content"))
            .focused(true)
            .disabled(true)
            .into();
        let mut ui = simulator(root);
        let _ = ui.snapshot(&iced::Theme::Light);
        assert_eq!(
            status.get(),
            Status {
                is_disabled: true,
                ..Status::default()
            }
        );
    }

    #[test]
    fn a_fixed_width_reserves_the_outer_thickness_of_a_closure_style() {
        let root: Element<'_> = multi_border(text("content"))
            .width(Length::Fixed(80.0))
            .outer_thickness(3.0)
            .style(|_, _| Style::new().ring(Ring::outer(2.0, iced::Color::BLACK).offset(1.0)))
            .into();
        let mut ui = simulator(root);
        let bounds = ui.find("content").expect("on screen").bounds();
        // The widget is 80 wide; 3 are reserved for the ring on each side.
        assert!((bounds.width - 74.0).abs() < 0.5, "{bounds:?}");
        assert!((bounds.x - 3.0).abs() < 0.5, "{bounds:?}");
    }

    #[test]
    fn an_overflowing_ring_takes_no_room_from_the_content() {
        let ring = |overflowing: bool| {
            let ring = Ring::outer(2.0, iced::Color::BLACK).offset(1.0);
            if overflowing {
                ring.overflowing()
            } else {
                ring
            }
        };
        let content = |overflowing: bool| {
            let root: Element<'_> = multi_border(text("content"))
                .width(Length::Fixed(80.0))
                .outer_thickness(if overflowing { 0.0 } else { 3.0 })
                .style(move |_, _| Style::new().ring(ring(overflowing)))
                .into();
            let mut ui = simulator(root);
            ui.find("content").expect("on screen").bounds()
        };

        // Reserved, the ring insets the content by its 3 px on each side;
        // overflowing, the content keeps the full width and the ring is
        // painted past the layout box.
        let reserved = content(false);
        let overflowing = content(true);
        assert!((reserved.width - 74.0).abs() < 0.5, "{reserved:?}");
        assert!((overflowing.width - 80.0).abs() < 0.5, "{overflowing:?}");
        assert!((overflowing.x - 0.0).abs() < 0.5, "{overflowing:?}");
    }

    /// A ring wider than the reservation is a bug worth catching — but only
    /// when it asked for room in the first place.
    #[test]
    #[should_panic(expected = "exceed the reserved outer thickness")]
    fn a_reserved_ring_over_its_reservation_is_a_programming_error() {
        let root: Element<'_> = multi_border(text("content"))
            .outer_thickness(1.0)
            .style(|_, _| Style::new().ring(Ring::outer(5.0, iced::Color::BLACK)))
            .into();
        let mut ui = simulator(root);
        let _ = ui.snapshot(&iced::Theme::Light);
    }

    #[test]
    fn an_overflowing_ring_never_trips_the_reservation_assert() {
        let root: Element<'_> = multi_border(text("content"))
            .style(|_, _| {
                Style::new().ring(
                    Ring::outer(5.0, iced::Color::BLACK)
                        .offset(2.0)
                        .overflowing(),
                )
            })
            .into();
        let mut ui = simulator(root);
        let _ = ui.snapshot(&iced::Theme::Light);
    }

    #[test]
    fn focus_defaults_to_click() {
        assert!(matches!(Focus::default(), Focus::Click));
    }
}
