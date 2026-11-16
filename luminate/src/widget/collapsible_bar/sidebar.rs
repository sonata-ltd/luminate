//! A collapsible sidebar with an inner edge shadow.

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, svg};
use iced::border::Radius;
use iced::{Color, Event, Length, Padding, Pixels, Rectangle, Shadow, Size, Vector};
use iced_animate::{AnimLength, Motion};

use crate::descriptor::Axis;

use super::core::{Core, Paint};

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
    core: Core<'a, Message, Theme, Renderer>,
    class: Theme::Class<'a>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Sidebar<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sidebar")
            .field("core", &self.core)
            .finish_non_exhaustive()
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
            core: Core::with_children(children, Axis::Vertical),
            class: Theme::default(),
        }
    }

    /// Appends a child (void-sized elements are dropped).
    #[must_use]
    pub fn push(mut self, child: impl Into<iced::Element<'a, Message, Theme, Renderer>>) -> Self {
        self.core = self.core.push(child);
        self
    }

    /// Appends every child.
    #[must_use]
    pub fn extend(
        mut self,
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        self.core = self.core.extend(children);
        self
    }

    /// Sets the width, which may be an animated value.
    #[must_use]
    pub fn width(mut self, width: impl Into<AnimLength>) -> Self {
        self.core.width = width.into();
        self
    }

    /// Sets the height, which may be an animated value.
    #[must_use]
    pub fn height(mut self, height: impl Into<AnimLength>) -> Self {
        self.core.height = height.into();
        self
    }

    /// Binds the collapse animation to `motion`.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.core.motion = Some(motion);
        self
    }

    /// Whether the sidebar is collapsed; it animates toward this value.
    #[must_use]
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.core.collapsed = collapsed;
        self
    }

    /// Shows the built-in collapse toggle in a header row (or column).
    #[must_use]
    pub fn show_toggle(mut self, show: bool) -> Self {
        self.core.show_toggle = show;
        self
    }

    /// Layout axis (default [`Axis::Vertical`]).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.core.axis = axis;
        self
    }

    /// Publishes the requested `collapsed` value when the toggle is pressed.
    #[must_use]
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> Message + 'a) -> Self {
        self.core.on_toggle = Some(Box::new(on_toggle));
        self
    }

    /// Extent along the collapse axis when collapsed (default 50).
    #[must_use]
    pub fn collapsed_size(mut self, size: impl Into<Pixels>) -> Self {
        self.core.collapsed_size = size.into().0;
        self
    }

    /// Extent of the toggle header along the flex main axis: the height of
    /// the header row of a column, the width of the header column of a row
    /// (default 44).
    #[must_use]
    pub fn header_size(mut self, size: impl Into<Pixels>) -> Self {
        self.core.header_size = size.into().0;
        self
    }

    /// Side length of the toggle chevron (default 20).
    #[must_use]
    pub fn icon_size(mut self, size: impl Into<Pixels>) -> Self {
        self.core.icon_size = size.into().0;
        self
    }

    /// Padding around the children (default 10 on every side).
    #[must_use]
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.core.padding = padding.into();
        self
    }

    /// Gap between children (default 5).
    #[must_use]
    pub fn spacing(mut self, spacing: impl Into<Pixels>) -> Self {
        self.core.spacing = spacing.into().0;
        self
    }

    /// Rounds the corners of the background, each by its own radius.
    ///
    /// Only the fill is rounded, nothing is clipped: children stay clear of
    /// the corners as long as the padding is at least `0.3 × radius`.
    #[must_use]
    pub fn border_radius(mut self, radius: impl Into<Radius>) -> Self {
        self.core.border_radius = radius.into();
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
        self.core.size()
    }

    fn tag(&self) -> tree::Tag {
        Core::<Message, Theme, Renderer>::tag()
    }

    fn state(&self) -> tree::State {
        self.core.state()
    }

    fn children(&self) -> Vec<Tree> {
        self.core.trees()
    }

    fn diff(&self, tree: &mut Tree) {
        self.core.diff(tree);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        self.core.operate(tree, layout, renderer, operation);
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
        self.core.update(
            tree, event, layout, cursor, renderer, clipboard, shell, viewport,
        );
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.core.layout(tree, renderer, limits)
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
        let colors = theme.style(&self.class);
        self.core.draw(
            tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
            Paint {
                background: colors.background,
                hover_overlay: colors.hover_overlay,
                icon: colors.icon,
                outer_shadow: Shadow::default(),
                inner_edge_shadow: colors.edge_shadow,
            },
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
        self.core
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.core
            .overlay(tree, layout, renderer, viewport, translation)
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
    use iced::{Event, Point, Size, touch};
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

        assert_eq!(moved(&mut ui), RedrawRequest::NextFrame);
        assert_eq!(moved(&mut ui), RedrawRequest::Wait);
    }
}
