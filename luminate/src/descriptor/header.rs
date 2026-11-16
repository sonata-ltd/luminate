//! The header descriptor.

use std::fmt;

use iced::border::Radius;
use iced_animate::AnimLength;

use crate::Element;
use crate::descriptor::{Axis, ShadowSide};

/// A collapsible header.
///
/// A header lays its children out horizontally by default and casts an
/// external shadow toward its trailing edge. The application owns
/// `collapsed`: pass the current value on every rebuild and flip it when
/// [`on_toggle`](Self::on_toggle) fires.
pub struct Header<'a, Message> {
    /// The items, in order.
    pub children: Vec<Element<'a, Message>>,
    /// Width, possibly animated (default `Shrink`).
    pub width: AnimLength,
    /// Height, possibly animated (default `Shrink`).
    pub height: AnimLength,
    /// Layout axis (default `Horizontal`).
    pub axis: Axis,
    /// Edge toward which the external shadow falls.
    pub shadow_side: ShadowSide,
    /// Whether the header is collapsed.
    pub collapsed: bool,
    /// Shows the built-in collapse toggle in a leading column (or row).
    pub show_toggle: bool,
    /// Called with the requested `collapsed` value when the toggle is
    /// pressed.
    pub on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
    /// Corners of the background, each its own radius (default square).
    pub border_radius: Radius,
}

impl<'a, Message> Header<'a, Message> {
    /// A header holding `children`.
    #[must_use]
    pub fn new(children: impl IntoIterator<Item = impl Into<Element<'a, Message>>>) -> Self {
        Self {
            children: children.into_iter().map(Into::into).collect(),
            width: AnimLength::Shrink,
            height: AnimLength::Shrink,
            axis: Axis::Horizontal,
            shadow_side: ShadowSide::default(),
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

    /// Layout axis of the children (default `Horizontal`).
    #[must_use]
    pub fn axis(mut self, axis: Axis) -> Self {
        self.axis = axis;
        self
    }

    /// Edge toward which the external shadow falls.
    #[must_use]
    pub fn shadow_side(mut self, side: ShadowSide) -> Self {
        self.shadow_side = side;
        self
    }

    /// Whether the header is collapsed; it animates toward this value.
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

    /// Rounds the corners of the background, each by its own radius.
    #[must_use]
    pub fn border_radius(mut self, radius: impl Into<Radius>) -> Self {
        self.border_radius = radius.into();
        self
    }
}

impl<Message> fmt::Debug for Header<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Header")
            .field("children", &self.children.len())
            .field("width", &self.width)
            .field("height", &self.height)
            .field("axis", &self.axis)
            .field("shadow_side", &self.shadow_side)
            .field("collapsed", &self.collapsed)
            .field("show_toggle", &self.show_toggle)
            .field("toggles", &self.on_toggle.is_some())
            .field("border_radius", &self.border_radius)
            .finish()
    }
}
