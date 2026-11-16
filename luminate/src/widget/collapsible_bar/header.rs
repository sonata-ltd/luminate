//! A collapsible header with an external edge-directed shadow.

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, svg};
use iced::border::Radius;
use iced::{Color, Event, Length, Padding, Pixels, Rectangle, Shadow, Size, Vector};
use iced_animate::{AnimLength, Motion};

use crate::descriptor::{Axis, ShadowSide};

use super::core::{Core, Paint};

/// An external shadow whose offset is directed by a [`Header`]'s axis and
/// [`ShadowSide`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeShadow {
    /// Colour of the shadow.
    pub color: Color,
    /// Distance from the header along the selected side.
    pub offset: f32,
    /// Radius of the soft edge.
    pub blur_radius: f32,
}

impl EdgeShadow {
    /// Creates an edge-directed external shadow.
    #[must_use]
    pub const fn new(color: Color, offset: f32, blur_radius: f32) -> Self {
        Self {
            color,
            offset,
            blur_radius,
        }
    }

    fn oriented(self, axis: Axis, side: ShadowSide) -> Shadow {
        let distance = self.offset.abs();
        let sign = match side {
            ShadowSide::Leading => -1.0,
            ShadowSide::Trailing => 1.0,
        };
        let offset = match axis {
            Axis::Horizontal => Vector::new(0.0, sign * distance),
            Axis::Vertical => Vector::new(sign * distance, 0.0),
        };

        Shadow {
            color: self.color,
            offset,
            blur_radius: self.blur_radius,
        }
    }
}

/// Paint used by a [`Header`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Fill behind the whole header.
    pub background: Color,
    /// Painted over the toggle while the pointer is over it.
    pub hover_overlay: Color,
    /// Colour of the toggle chevron.
    pub icon: Color,
    /// External shadow. `None` draws no shadow.
    pub shadow: Option<EdgeShadow>,
}

catalog!(|theme| {
    let palette = theme.extended_palette();

    Style {
        background: palette.background.base.color,
        hover_overlay: palette.background.weak.color,
        icon: palette.background.base.text,
        shadow: Some(EdgeShadow::new(
            Color {
                a: 0.05,
                ..Color::BLACK
            },
            2.0,
            20.0,
        )),
    }
});

/// A header whose collapse axis animates between its full and collapsed
/// size.
///
/// A horizontal header is a row whose height collapses; a vertical one is a
/// column whose width collapses. Unlike a [`Sidebar`](super::sidebar::Sidebar),
/// its edge shadow is a real external [`Shadow`]. The shadow points toward
/// [`shadow_side`](Self::shadow_side): top/bottom for a horizontal header and
/// left/right for a vertical one.
///
/// # Example
///
/// ```
/// use iced_luminate::descriptor::{Axis, ShadowSide};
/// use iced_luminate::iced::widget::text;
/// use iced_luminate::iced::{Element, Theme};
/// use iced_luminate::widget::header::header;
///
/// let bar: Element<'_, (), Theme, iced_luminate::Renderer> =
///     header([text("Project").into(), text("Account").into()])
///         .axis(Axis::Horizontal)
///         .shadow_side(ShadowSide::Trailing)
///         .into();
/// ```
pub struct Header<'a, Message, Theme = iced::Theme, Renderer = crate::Renderer>
where
    Theme: Catalog,
{
    core: Core<'a, Message, Theme, Renderer>,
    shadow_side: ShadowSide,
    class: Theme::Class<'a>,
}

impl<Message, Theme, Renderer> std::fmt::Debug for Header<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Header")
            .field("core", &self.core)
            .field("shadow_side", &self.shadow_side)
            .finish_non_exhaustive()
    }
}

impl<'a, Message, Theme, Renderer> Header<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    /// An empty horizontal header with neutral metrics (collapsed 50,
    /// toggle header 44, icon 20, padding 10, spacing 5).
    #[must_use]
    pub fn new() -> Self {
        Self::with_children(Vec::new())
    }

    /// A horizontal header holding `children`, with neutral metrics.
    #[must_use]
    pub fn with_children(
        children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
    ) -> Self {
        Self {
            core: Core::with_children(children, Axis::Horizontal),
            shadow_side: ShadowSide::default(),
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

    /// Whether the header is collapsed; it animates toward this value.
    #[must_use]
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.core.collapsed = collapsed;
        self
    }

    /// Shows the built-in collapse toggle in a leading row (or column).
    #[must_use]
    pub fn show_toggle(mut self, show: bool) -> Self {
        self.core.show_toggle = show;
        self
    }

    /// Layout axis (default [`Axis::Horizontal`]).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.core.axis = axis;
        self
    }

    /// Edge toward which the shadow falls.
    ///
    /// [`ShadowSide::Leading`] means top for a horizontal header and left
    /// for a vertical one; [`ShadowSide::Trailing`] means bottom and right.
    #[must_use]
    pub fn shadow_side(mut self, side: ShadowSide) -> Self {
        self.shadow_side = side;
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

    /// Extent of the toggle header along the flex main axis (default 44).
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

    /// Rounds the background and the shadow that follows it.
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

impl<Message, Theme, Renderer> Default for Header<'_, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Header<'_, Message, Theme, Renderer>
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
        let shadow = colors.shadow.map_or_else(Shadow::default, |shadow| {
            shadow.oriented(self.core.axis, self.shadow_side)
        });

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
                outer_shadow: shadow,
                inner_edge_shadow: None,
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

impl<'a, Message, Theme, Renderer> From<Header<'a, Message, Theme, Renderer>>
    for iced::Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: Catalog + 'a,
    Renderer: renderer::Renderer + svg::Renderer + 'a,
{
    fn from(header: Header<'a, Message, Theme, Renderer>) -> Self {
        Self::new(header)
    }
}

/// A [`Header`] holding `children`.
#[must_use]
pub fn header<'a, Message, Theme, Renderer>(
    children: impl IntoIterator<Item = iced::Element<'a, Message, Theme, Renderer>>,
) -> Header<'a, Message, Theme, Renderer>
where
    Theme: Catalog,
    Renderer: renderer::Renderer,
{
    Header::with_children(children)
}

#[cfg(test)]
mod tests {
    use iced::Size;
    use iced::advanced::widget::Tree;
    use iced::advanced::{Widget, layout, renderer::Headless};
    use iced::widget::text;

    use super::*;

    fn headless() -> crate::Renderer {
        iced_test::futures::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("tiny_skia needs no GPU")
    }

    #[test]
    fn the_shadow_offset_follows_axis_and_side() {
        let shadow = EdgeShadow::new(Color::BLACK, 2.0, 20.0);

        assert_eq!(
            shadow
                .oriented(Axis::Horizontal, ShadowSide::Leading)
                .offset,
            Vector::new(0.0, -2.0)
        );
        assert_eq!(
            shadow
                .oriented(Axis::Horizontal, ShadowSide::Trailing)
                .offset,
            Vector::new(0.0, 2.0)
        );
        assert_eq!(
            shadow.oriented(Axis::Vertical, ShadowSide::Leading).offset,
            Vector::new(-2.0, 0.0)
        );
        assert_eq!(
            shadow.oriented(Axis::Vertical, ShadowSide::Trailing).offset,
            Vector::new(2.0, 0.0)
        );
    }

    #[test]
    fn a_header_is_horizontal_by_default() {
        let renderer = headless();
        let mut header: Header<'_, (), iced::Theme, crate::Renderer> =
            header([text("first").into(), text("second").into()]);
        let mut tree = Tree::new(&header as &dyn Widget<_, _, _>);
        let node = header.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(500.0, 400.0)),
        );

        let children = node.children();
        assert!(
            children[1].bounds().x > children[0].bounds().x,
            "children form a row: {children:?}",
        );
    }

    #[test]
    fn the_iced_theme_catalog_has_the_css_like_shadow() {
        let style = <iced::Theme as Catalog>::style(
            &iced::Theme::Light,
            &<iced::Theme as Catalog>::default(),
        );
        let shadow = style.shadow.expect("the default header casts a shadow");

        assert_eq!(shadow.offset, 2.0);
        assert_eq!(shadow.blur_radius, 20.0);
        assert!((shadow.color.a - 0.05).abs() < f32::EPSILON);
    }
}
