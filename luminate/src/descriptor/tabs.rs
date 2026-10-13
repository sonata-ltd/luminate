//! The tab-bar descriptor.

use std::fmt;

use iced::Length;

/// A segmented tab bar with an animated selection backing.
///
/// The application owns the selection: pass the current index on every
/// rebuild and update it when [`on_select`](Self::on_select) fires; the
/// backing glides to whatever it is handed. The kit draws the selected tab's
/// label in the active look and the others in the inactive one, so a tab
/// carries no selection flag of its own.
pub struct Tabs<'a, Message> {
    /// The tabs, in order.
    pub tabs: Vec<TabItem<'a>>,
    /// Index of the selected tab (default 0; clamped to the last tab).
    pub active: usize,
    /// Width of the bar (default `Fill`).
    pub width: Length,
    /// Called with the index of the pressed tab.
    pub on_select: Option<Box<dyn Fn(usize) -> Message + 'a>>,
}

/// One tab of a [`Tabs`] bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabItem<'a> {
    /// The label shown.
    pub label: &'a str,
}

impl<'a> TabItem<'a> {
    /// A tab labelled `label`.
    #[must_use]
    pub const fn new(label: &'a str) -> Self {
        Self { label }
    }
}

impl<'a> From<&'a str> for TabItem<'a> {
    fn from(label: &'a str) -> Self {
        Self::new(label)
    }
}

impl<'a, Message> Tabs<'a, Message> {
    /// A bar of `tabs` with the first one selected.
    #[must_use]
    pub fn new(tabs: impl IntoIterator<Item = impl Into<TabItem<'a>>>) -> Self {
        Self {
            tabs: tabs.into_iter().map(Into::into).collect(),
            active: 0,
            width: Length::Fill,
            on_select: None,
        }
    }

    /// Appends a tab.
    #[must_use]
    pub fn push(mut self, tab: impl Into<TabItem<'a>>) -> Self {
        self.tabs.push(tab.into());
        self
    }

    /// The selected tab; the selection backing animates to it.
    #[must_use]
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    /// Sets the width of the bar.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Message published with the index of the pressed tab.
    #[must_use]
    pub fn on_select(mut self, on_select: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }
}

impl<Message> fmt::Debug for Tabs<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tabs")
            .field("tabs", &self.tabs.len())
            .field("active", &self.active)
            .field("width", &self.width)
            .field("selects", &self.on_select.is_some())
            .finish()
    }
}
