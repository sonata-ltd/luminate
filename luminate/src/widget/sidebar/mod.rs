//! A collapsible sidebar that animates between its full and collapsed size.

use iced::advanced::renderer::Quad;
use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, svg};
use iced::border::Radius;
use iced::gradient::Linear;
use iced::{
    Color, Degrees, Event, Gradient, Length, Padding, Pixels, Rectangle, Size, Vector, touch,
};
use iced_animate::curves::STRUCTURAL;
use iced_animate::{Anim, AnimLength, Motion, MotionKey, Tier};

use crate::descriptor::Axis;

mod compute_layout;
mod icons;

/// Colours a [`Sidebar`] is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Fill behind the whole sidebar.
    pub background: Color,
    /// Painted over the toggle while the pointer is over it.
    pub hover_overlay: Color,
    /// Colour of the toggle chevron.
    pub icon: Color,
    /// Colour of the thin band along the inner edge (the right edge of a
    /// column, the bottom edge of a row). `None` draws no band.
    pub edge_shadow: Option<Color>,
}

catalog!(|theme| {
    let palette = theme.extended_palette();

    Style {
        background: palette.background.weak.color,
        hover_overlay: palette.background.strong.color,
        icon: palette.background.weak.text,
        edge_shadow: Some(Color {
            a: 0.05,
            ..palette.background.base.text
        }),
    }
});

/// Default extent along the collapse axis when collapsed.
const DEFAULT_COLLAPSED_SIZE: f32 = 50.0;
/// Default extent of the toggle header along the flex main axis.
const DEFAULT_HEADER_SIZE: f32 = 44.0;
/// Default side length of the toggle chevron.
const DEFAULT_ICON_SIZE: f32 = 20.0;
/// Default padding around the children.
const DEFAULT_PADDING: Padding = Padding::new(10.0);
/// Default gap between children.
const DEFAULT_SPACING: f32 = 5.0;
/// Width of the band drawn along the inner edge for `Style::edge_shadow`.
const EDGE_SHADOW_WIDTH: f32 = 6.0;

/// A container whose collapse axis animates between its full size and a
/// collapsed size.
///
/// A vertical sidebar is a column whose width collapses; a horizontal one
/// is a row whose height collapses. The application owns `collapsed`: pass
/// the current value on every rebuild and react to
/// [`on_toggle`](Self::on_toggle) by flipping it. The widget animates toward
/// whatever it is handed when a [`motion`](Self::motion) is set, and jumps
/// otherwise.
///
/// The default width and height are `Shrink`, enclosing the children: a
/// `Fill` child makes the sidebar fill its parent along that axis unless
/// [`width`](Self::width) / [`height`](Self::height) say otherwise.
///
/// # Example
///
/// ```
/// use iced_luminate::descriptor::Axis;
/// use iced_luminate::iced::widget::text;
/// use iced_luminate::iced::{Element, Theme};
/// use iced_luminate::animate::Motion;
/// use iced_luminate::widget::sidebar::sidebar;
///
/// #[derive(Clone)]
/// enum Message {
///     Toggle(bool),
/// }
///
/// let motion = Motion::new();
/// let collapsed = false; // application state
/// let bar: Element<'_, Message, Theme, iced_luminate::Renderer> =
///     sidebar([text("Home").into(), text("Settings").into()])
///         .motion(motion.clone())
///         .axis(Axis::Vertical)
///         .collapsed(collapsed)
///         .show_toggle(true)
///         .collapsed_size(48.0)
///         .on_toggle(Message::Toggle)
///         .into();
/// ```
pub struct Sidebar<'a, Message, Theme = iced::Theme, Renderer = crate::Renderer>
where
    Theme: Catalog,
{
    children: Vec<iced::Element<'a, Message, Theme, Renderer>>,
    width: AnimLength,
    height: AnimLength,
    collapsed_size: f32,
    header_size: f32,
    icon_size: f32,
    padding: Padding,
    spacing: f32,
    collapsed: bool,
    show_toggle: bool,
    axis: Axis,
    on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
    /// Engine driving the collapse. Without one the sidebar collapses in a
    /// single frame.
    motion: Option<Motion>,
    /// Corners of the background, each its own radius. Square by default.
    border_radius: Radius,
    class: Theme::Class<'a>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Sidebar<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sidebar")
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
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct State {
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
    fn progress(&self) -> f32 {
        self.collapse.get().clamp(0.0, 1.0)
    }
}

impl<'a, Message, Theme, Renderer> Sidebar<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    /// An empty sidebar with neutral metrics (collapsed 50, header 44,
    /// icon 20, padding 10, spacing 5).
    #[must_use]
    pub fn new() -> Self {
        Self::with_children(Vec::new())
    }

    /// A sidebar holding `children`, with neutral metrics.
    #[must_use]
    pub fn with_children(
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
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
            axis: Axis::Vertical,
            on_toggle: None,
            motion: None,
            border_radius: Radius::default(),
            class: Theme::default(),
        }
        .extend(children)
    }

    /// Appends a child (void-sized elements are dropped).
    #[must_use]
    pub fn push(mut self, child: impl Into<iced::Element<'a, Message, Theme, Renderer>>) -> Self {
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
    #[must_use]
    pub fn extend(
        self,
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        children.into_iter().fold(self, Self::push)
    }

    /// Sets the width, which may be an animated value.
    #[must_use]
    pub fn width(mut self, width: impl Into<AnimLength>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height, which may be an animated value.
    #[must_use]
    pub fn height(mut self, height: impl Into<AnimLength>) -> Self {
        self.height = height.into();
        self
    }

    /// Binds the collapse animation to `motion`.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// Whether the sidebar is collapsed; it animates toward this value.
    #[must_use]
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Shows the built-in collapse toggle in a header row (or column).
    #[must_use]
    pub fn show_toggle(mut self, show: bool) -> Self {
        self.show_toggle = show;
        self
    }

    /// Layout axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Publishes the requested `collapsed` value when the toggle is pressed.
    #[must_use]
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> Message + 'a) -> Self {
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }

    /// Extent along the collapse axis when collapsed (default 50).
    #[must_use]
    pub fn collapsed_size(mut self, size: impl Into<Pixels>) -> Self {
        self.collapsed_size = size.into().0;
        self
    }

    /// Extent of the toggle header along the flex main axis: the height of
    /// the header row of a column, the width of the header column of a row
    /// (default 44).
    #[must_use]
    pub fn header_size(mut self, size: impl Into<Pixels>) -> Self {
        self.header_size = size.into().0;
        self
    }

    /// Side length of the toggle chevron (default 20).
    #[must_use]
    pub fn icon_size(mut self, size: impl Into<Pixels>) -> Self {
        self.icon_size = size.into().0;
        self
    }

    /// Padding around the children (default 10 on every side).
    #[must_use]
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Gap between children (default 5).
    #[must_use]
    pub fn spacing(mut self, spacing: impl Into<Pixels>) -> Self {
        self.spacing = spacing.into().0;
        self
    }

    /// Rounds the corners of the background, each by its own radius:
    /// `12.0` for all four, `border::bottom_left(25.0)` for one, or
    /// `Radius::default().top_left(4.0).bottom_left(25.0)` for any mix.
    ///
    /// Only the fill is rounded, nothing is clipped: children stay clear of
    /// the corners as long as the padding is at least `0.3 × radius`.
    #[must_use]
    pub fn border_radius(mut self, radius: impl Into<Radius>) -> Self {
        self.border_radius = radius.into();
        self
    }

    /// Sets the style with a closure.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self
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

    /// Header extent along the flex main axis (0 when the toggle is hidden).
    fn header_extent(&self) -> f32 {
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

    /// The corners of the edge-shadow band. The band runs along the inner
    /// edge, so it takes that edge's corners: otherwise it would square them
    /// off again.
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

impl<Message, Theme, Renderer> Default for Sidebar<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Sidebar<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer + svg::Renderer,
{
    fn size(&self) -> Size<Length> {
        // A live length is advertised as `Shrink`, never `Fixed(0.0)`: the
        // void hint would make a parent drop the widget. See
        // `AnimLength::size_hint`.
        Size::new(self.width.size_hint(), self.height.size_hint())
    }

    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(self.motion.as_ref(), self.collapsed))
    }

    fn children(&self) -> Vec<Tree> {
        self.children.iter().map(Tree::new).collect()
    }

    fn diff(&self, tree: &mut Tree) {
        // Idempotent; touches the track every build so it is never collected.
        tree.state
            .downcast_mut::<State>()
            .retarget(self.motion.as_ref(), self.collapsed);

        tree.diff_children(&self.children);
    }

    fn operate(
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

    fn layout(
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
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };

        let state = tree.state.downcast_ref::<State>();
        let colors = theme.style(&self.class);

        renderer.fill_quad(
            Quad {
                bounds,
                border: iced::Border {
                    radius: self.border_radius,
                    ..Default::default()
                },
                ..Default::default()
            },
            colors.background,
        );

        if let Some(shadow) = colors.edge_shadow {
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
                    colors.hover_overlay,
                );
            }

            renderer.draw_svg(
                svg::Svg {
                    handle: icons::chevron(self.axis, self.collapsed),
                    color: Some(colors.icon),
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
        // size they must be clipped to the sidebar; at rest the parent's
        // viewport is enough and a layer would cost a render pass.
        if state.progress() > 0.0 || state.collapse.is_animating() {
            renderer.with_layer(visible, draw_children);
        } else {
            draw_children(renderer);
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

impl<'a, Message, Theme, Renderer> From<Sidebar<'a, Message, Theme, Renderer>>
    for iced::Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: Catalog + 'a,
    Renderer: renderer::Renderer + svg::Renderer + 'a,
{
    fn from(sidebar: Sidebar<'a, Message, Theme, Renderer>) -> Self {
        Self::new(sidebar)
    }
}

/// A [`Sidebar`] holding `children`.
#[must_use]
pub fn sidebar<'a, Message, Theme, Renderer>(
    children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
) -> Sidebar<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    Sidebar::with_children(children)
}

#[cfg(test)]
mod tests {

    use iced::advanced::widget::Tree;
    use iced::advanced::{layout, renderer::Headless};
    use iced::time::Instant;
    use iced::widget::text;
    use iced::{Point, Size};
    use iced_animate::Motion;
    use iced_test::Simulator;

    use super::*;

    type Bar<'a> = Sidebar<'a, (), iced::Theme, crate::Renderer>;

    fn headless() -> crate::Renderer {
        iced_test::futures::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("tiny_skia needs no GPU")
    }

    fn bar(motion: &Motion, collapsed: bool) -> Bar<'static> {
        sidebar([text("first item").into(), text("second").into()])
            .motion(motion.clone())
            .collapsed(collapsed)
            .collapsed_size(50.0)
    }

    fn limits() -> layout::Limits {
        layout::Limits::new(Size::ZERO, Size::new(500.0, 400.0))
    }

    #[test]
    fn the_iced_theme_catalog_follows_the_palette() {
        let light = <iced::Theme as Catalog>::style(
            &iced::Theme::Light,
            &<iced::Theme as Catalog>::default(),
        );
        let dark = <iced::Theme as Catalog>::style(
            &iced::Theme::Dark,
            &<iced::Theme as Catalog>::default(),
        );
        assert_ne!(light.background, dark.background);
        assert_ne!(light.icon, dark.icon);
        assert!(light.edge_shadow.is_some());
    }

    #[test]
    fn a_changed_collapsed_prop_starts_the_animation() {
        let motion = Motion::new();
        let renderer = headless();
        let mut tree = Tree::new(&bar(&motion, false) as &dyn iced::advanced::Widget<_, _, _>);
        let expanded = bar(&motion, false)
            .layout(&mut tree, &renderer, &limits())
            .size()
            .width;

        let mut collapsed_bar = bar(&motion, true);
        collapsed_bar.diff(&mut tree);
        let start = Instant::now();
        let _ = motion.tick(start);
        let _ = motion.tick(start + std::time::Duration::from_millis(48));
        let mid = collapsed_bar
            .layout(&mut tree, &renderer, &limits())
            .size()
            .width;
        assert!(mid < expanded, "shrinking: {mid} < {expanded}");

        for frame in 4..200 {
            let _ = motion.tick(start + std::time::Duration::from_millis(16 * frame));
        }
        let done = collapsed_bar
            .layout(&mut tree, &renderer, &limits())
            .size()
            .width;
        assert!((done - 50.0).abs() < 0.6, "collapsed: {done}");
    }

    #[test]
    fn a_shrink_sidebar_fits_its_widest_child() {
        let renderer = headless();
        let motion = Motion::new();
        let mut sidebar = bar(&motion, false);
        let mut tree = Tree::new(&sidebar as &dyn iced::advanced::Widget<_, _, _>);
        let width = sidebar.layout(&mut tree, &renderer, &limits()).size().width;
        assert!(width > 60.0, "wider than the collapsed minimum: {width}");
        assert!(width < 500.0, "not filling the parent: {width}");
    }

    #[test]
    fn a_horizontal_toggle_reserves_a_column() {
        let renderer = headless();
        let motion = Motion::new();
        let mut sidebar = bar(&motion, false)
            .axis(Axis::Horizontal)
            .show_toggle(true)
            .header_size(44.0);
        let mut tree = Tree::new(&sidebar as &dyn iced::advanced::Widget<_, _, _>);
        let node = sidebar.layout(&mut tree, &renderer, &limits());
        let first_child_x = node.children()[0].bounds().x;
        assert!(
            first_child_x >= 44.0,
            "children start right of the header column: {first_child_x}"
        );
    }

    #[test]
    fn padding_can_differ_per_side() {
        let renderer = headless();
        let motion = Motion::new();
        let mut sidebar = bar(&motion, false).padding(Padding {
            top: 1.0,
            right: 2.0,
            bottom: 3.0,
            left: 40.0,
        });
        let mut tree = Tree::new(&sidebar as &dyn iced::advanced::Widget<_, _, _>);
        let node = sidebar.layout(&mut tree, &renderer, &limits());
        let first = node.children()[0].bounds();
        assert!((first.x - 40.0).abs() < 0.5, "{first:?}");
        assert!((first.y - 1.0).abs() < 0.5, "{first:?}");
    }

    #[test]
    fn pressing_the_toggle_publishes_the_flipped_value() {
        #[derive(Debug, Clone, PartialEq)]
        struct Toggle(bool);

        let root: iced::Element<'_, Toggle, iced::Theme, crate::Renderer> =
            sidebar([text("item").into()])
                .show_toggle(true)
                .header_size(44.0)
                .icon_size(20.0)
                .collapsed(false)
                .on_toggle(Toggle)
                .into();
        let mut ui = Simulator::with_size(iced::Settings::default(), Size::new(300.0, 200.0), root);

        // The toggle is a 20 px square centred in the 44 px header.
        ui.point_at(Point::new(22.0, 22.0));
        let _ = ui.simulate(iced_test::simulator::click());
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Toggle(true)]);
    }

    #[test]
    fn a_touch_on_the_toggle_counts_as_a_press() {
        #[derive(Debug, Clone, PartialEq)]
        struct Toggle(bool);

        let root: iced::Element<'_, Toggle, iced::Theme, crate::Renderer> =
            sidebar([text("item").into()])
                .show_toggle(true)
                .header_size(44.0)
                .icon_size(20.0)
                .collapsed(true)
                .on_toggle(Toggle)
                .into();
        let mut ui = Simulator::with_size(iced::Settings::default(), Size::new(300.0, 200.0), root);

        let position = Point::new(22.0, 22.0);
        ui.point_at(position);
        let _ = ui.simulate([Event::Touch(touch::Event::FingerPressed {
            id: touch::Finger(0),
            position,
        })]);
        assert_eq!(ui.into_messages().collect::<Vec<_>>(), vec![Toggle(false)]);
    }

    #[test]
    fn entering_the_toggle_requests_one_redraw() {
        use iced::advanced::clipboard;
        use iced::window::RedrawRequest;
        use iced_test::runtime::UserInterface;
        use iced_test::runtime::user_interface::{Cache, State};

        let root: iced::Element<'_, (), iced::Theme, crate::Renderer> =
            sidebar([text("item").into()])
                .show_toggle(true)
                .header_size(44.0)
                .icon_size(20.0)
                .into();
        let mut renderer = headless();
        let mut ui = UserInterface::build(
            root,
            Size::new(300.0, 200.0),
            Cache::default(),
            &mut renderer,
        );
        let mut messages = Vec::new();
        let over = Point::new(22.0, 22.0);
        let mut moved = |ui: &mut UserInterface<'_, (), iced::Theme, crate::Renderer>| {
            let (state, _) = ui.update(
                &[Event::Mouse(mouse::Event::CursorMoved { position: over })],
                mouse::Cursor::Available(over),
                &mut renderer,
                &mut clipboard::Null,
                &mut messages,
            );
            match state {
                State::Updated { redraw_request, .. } => redraw_request,
                State::Outdated => panic!("the interface stays valid"),
            }
        };

        // Entering the toggle changes the highlight: one redraw is requested.
        assert_eq!(moved(&mut ui), RedrawRequest::NextFrame);
        // Staying on it changes nothing: no request, no redraw churn.
        assert_eq!(moved(&mut ui), RedrawRequest::Wait);
    }
}
