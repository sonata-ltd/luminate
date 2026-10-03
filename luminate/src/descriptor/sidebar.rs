//! The sidebar descriptor and the [`Axis`] shared with the widget layer.

use std::fmt;

use iced::border::Radius;
use iced_animate::AnimLength;

use crate::Element;

/// Which way a sidebar lays its children out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Axis {
    /// Children in a column; collapsing animates the width.
    #[default]
    Vertical,
    /// Children in a row; collapsing animates the height.
    Horizontal,
}

/// A collapsible sidebar.
///
/// The application owns `collapsed`: pass the current value on every rebuild
/// and flip it when [`on_toggle`](Self::on_toggle) fires; the sidebar
/// animates toward whatever it is handed.
pub struct Sidebar<'a, Message> {
    /// The items, in order.
    pub children: Vec<Element<'a, Message>>,
    /// Width, possibly animated (default `Shrink`).
    pub width: AnimLength,
    /// Height, possibly animated (default `Shrink`).
    pub height: AnimLength,
    /// Layout axis (default `Vertical`).
    pub axis: Axis,
    /// Whether the sidebar is collapsed.
    pub collapsed: bool,
    /// Shows the built-in collapse toggle in a header row (or column).
    pub show_toggle: bool,
    /// Called with the requested `collapsed` value when the toggle is
    /// pressed.
    pub on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
    /// Corners of the background, each its own radius (default square).
    pub border_radius: Radius,
}

impl<'a, Message> Sidebar<'a, Message> {
    /// A sidebar holding `children`.
    #[must_use]
    pub fn new(children: impl IntoIterator<Item = impl Into<Element<'a, Message>>>) -> Self {
        Self {
            children: children.into_iter().map(Into::into).collect(),
            width: AnimLength::Shrink,
            height: AnimLength::Shrink,
            axis: Axis::default(),
            collapsed: false,
            show_toggle: false,
            on_toggle: None,
            border_radius: Radius::default(),
        }
    }

    /// Appends a child.
    #[must_use]
    pub fn push(mut self, child: impl Into<Element<'a, Message>>) -> Self {
        self.children.push(child.into());
        self
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

    /// Layout axis of the children (default `Vertical`).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Whether the sidebar is collapsed; it animates toward this value.
    #[must_use]
    pub fn collapsed(mut self, collapsed: bool) -> Self {
        self.collapsed = collapsed;
        self
    }

    /// Shows the built-in collapse toggle.
    #[must_use]
    pub fn show_toggle(mut self, show: bool) -> Self {
        self.show_toggle = show;
        self
    }

    /// Message published with the requested `collapsed` value when the
    /// built-in toggle is pressed.
    #[must_use]
    pub fn on_toggle(mut self, on_toggle: impl Fn(bool) -> Message + 'a) -> Self {
        self.on_toggle = Some(Box::new(on_toggle));
        self
    }

    /// Rounds the corners of the background, each by its own radius:
    /// `12.0` for all four, `border::bottom_left(25.0)` for one, or
    /// `Radius::default().top_left(4.0).bottom_left(25.0)` for any mix.
    #[must_use]
    pub fn border_radius(mut self, radius: impl Into<Radius>) -> Self {
        self.border_radius = radius.into();
        self
    }
}

impl<Message> fmt::Debug for Sidebar<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sidebar")
            .field("children", &self.children.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("axis", &self.axis)
            .field("collapsed", &self.collapsed)
            .field("show_toggle", &self.show_toggle)
            .field("toggles", &self.on_toggle.is_some())
            .field("border_radius", &self.border_radius)
            .finish()
    }
}
