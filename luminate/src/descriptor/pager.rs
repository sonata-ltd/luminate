//! The page-stack descriptor.

use std::fmt;

use iced::{Length, Pixels};

use crate::Element;

/// A sliding page stack showing one page at a time.
pub struct Pager<'a, Message> {
    /// The pages, in order.
    pub pages: Vec<Element<'a, Message>>,
    /// Index of the page shown (default 0; clamped to the last page).
    pub current: usize,
    /// Width (default `Fill`; `Shrink` means the widest visible page).
    pub width: Length,
    /// Caps the height; overflowing pages are clipped.
    pub max_height: Option<Pixels>,
    /// Keeps the height of the page a slide arrived at (default `false`).
    pub lock_height: bool,
}

impl<'a, Message> Pager<'a, Message> {
    /// A stack of `pages` showing the first one.
    #[must_use]
    pub fn new(pages: impl IntoIterator<Item = impl Into<Element<'a, Message>>>) -> Self {
        Self {
            pages: pages.into_iter().map(Into::into).collect(),
            current: 0,
            width: Length::Fill,
            max_height: None,
            lock_height: false,
        }
    }

    /// The page to show.
    #[must_use]
    pub fn current(mut self, current: usize) -> Self {
        self.current = current;
        self
    }

    /// Sets the width.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Caps the height.
    #[must_use]
    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = Some(max_height.into());
        self
    }

    /// Holds the height after a slide: it interpolates to the next page's
    /// height, then stays there even if the page's own height changes later.
    /// Measured again on the next switch or when the width changes.
    #[must_use]
    pub fn lock_height(mut self, lock: bool) -> Self {
        self.lock_height = lock;
        self
    }
}

impl<Message> fmt::Debug for Pager<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pager")
            .field("pages", &self.pages.len())
            .field("current", &self.current)
            .field("width", &self.width)
            .field("max_height", &self.max_height)
            .field("lock_height", &self.lock_height)
            .finish()
    }
}
