//! The card descriptor: a titled header, a sliding page stack and a controls
//! row.

use std::fmt;

use iced::{Length, Pixels};
use iced_texture_cache::TextureCache;

use crate::Element;

/// A card with a header, a sliding page stack and optional controls below
/// the pages.
pub struct Card<'a, Message> {
    /// Header title.
    pub title: &'a str,
    /// The pages of the stack.
    pub pages: Vec<Element<'a, Message>>,
    /// Index of the page shown (clamped to the last page).
    pub current: usize,
    /// Element shown below the page stack.
    pub controls: Option<Element<'a, Message>>,
    /// Height; `None` uses shrink setting.
    pub height: Option<Length>,
    /// Caps the card's height; the page stack shrinks to leave the header
    /// and the controls fully visible.
    pub max_height: Option<Pixels>,
    /// Width; `None` uses the theme's card width.
    pub width: Option<Length>,
    /// Caches the header's rasterization when set (store the handle in
    /// application state so it survives rebuilds).
    pub header_cache: Option<TextureCache>,
    /// Disables the halo shadow around the card.
    pub disable_background_shadow: bool,
}

impl<'a, Message> Card<'a, Message> {
    /// A card titled `title`, with no pages yet.
    #[must_use]
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            pages: Vec::new(),
            current: 0,
            controls: None,
            height: None,
            max_height: None,
            width: None,
            header_cache: None,
            disable_background_shadow: false,
        }
    }

    /// The pages of the stack and which one is shown.
    #[must_use]
    pub fn pages(
        mut self,
        pages: impl IntoIterator<Item = impl Into<Element<'a, Message>>>,
        current: usize,
    ) -> Self {
        self.pages = pages.into_iter().map(Into::into).collect();
        self.current = current;
        self
    }

    /// Element shown below the page stack.
    #[must_use]
    pub fn controls(mut self, controls: impl Into<Element<'a, Message>>) -> Self {
        self.controls = Some(controls.into());
        self
    }

    /// Sets the card's height.
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = Some(height.into());
        self
    }

    /// Caps the card's height.
    #[must_use]
    pub fn max_height(mut self, max_height: impl Into<Pixels>) -> Self {
        self.max_height = Some(max_height.into());
        self
    }

    /// Sets the card's width.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = Some(width.into());
        self
    }

    /// Caches the header's rasterization under `cache`.
    #[must_use]
    pub fn header_cache(mut self, cache: TextureCache) -> Self {
        self.header_cache = Some(cache);
        self
    }

    /// Disables the halo shadow around the card.
    #[must_use]
    pub fn disable_background_shadow(mut self, disabled: bool) -> Self {
        self.disable_background_shadow = disabled;
        self
    }
}

impl<Message> fmt::Debug for Card<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Card")
            .field("title", &self.title)
            .field("pages", &self.pages.len())
            .field("current", &self.current)
            .field("controls", &self.controls.is_some())
            .field("max_height", &self.max_height)
            .field("width", &self.width)
            .field("header_cache", &self.header_cache.is_some())
            .finish()
    }
}
