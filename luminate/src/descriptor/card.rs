//! The card descriptor: a titled header, the content and a controls row.

use std::fmt;

use iced::{Length, Pixels};
use iced_texture_cache::TextureCache;

use crate::Element;

/// A card with a header, the content and optional controls below it.
pub struct Card<'a, Message> {
    /// Header title.
    pub title: &'a str,
    /// The body of the card; `None` shows
    /// `no_content_screen`.
    pub content: Option<Element<'a, Message>>,
    /// Element shown below the content.
    pub controls: Option<Element<'a, Message>>,
    /// Height; `None` uses shrink setting.
    pub height: Option<Length>,
    /// Caps the card's height; the content shrinks to leave the header and
    /// the controls fully visible.
    pub max_height: Option<Pixels>,
    /// Width; `None` uses the theme's card width.
    pub width: Option<Length>,
    /// Caches the header's rasterization when set (store the handle in
    /// application state so it survives rebuilds).
    pub header_cache: Option<TextureCache>,
    /// Shown in place of the content while there is none (default
    /// nothing).
    pub no_content_screen: Option<Element<'a, Message>>,
    /// Disables the halo shadow around the card.
    pub disable_background_shadow: bool,
    /// Disables card decorations.
    pub disable_decorations: bool,
    /// Clips the content.
    pub clip: bool,
}

impl<'a, Message> Card<'a, Message> {
    /// A card titled `title`, with no pages yet.
    #[must_use]
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            content: None,
            controls: None,
            height: None,
            max_height: None,
            width: None,
            header_cache: None,
            no_content_screen: None,
            disable_background_shadow: false,
            disable_decorations: false,
            clip: false,
        }
    }

    /// The body of the card. For pages that slide, pass a
    /// [`Luminate::pager`](crate::Luminate::pager).
    #[must_use]
    pub fn content(mut self, content: impl Into<Element<'a, Message>>) -> Self {
        self.content = Some(content.into());
        self
    }

    /// Element shown below the content.
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

    /// Sets custom `no content` screen.
    #[must_use]
    pub fn no_content_screen(mut self, screen: impl Into<Element<'a, Message>>) -> Self {
        self.no_content_screen = Some(screen.into());
        self
    }

    /// Disables the halo shadow around the card.
    #[must_use]
    pub fn disable_background_shadow(mut self, disabled: bool) -> Self {
        self.disable_background_shadow = disabled;
        self
    }

    /// Disables default decorations on the card.
    #[must_use]
    pub fn disable_decorations(mut self, disabled: bool) -> Self {
        self.disable_decorations = disabled;
        self
    }

    /// Clips the content outside of the card.
    #[must_use]
    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }
}

impl<Message> fmt::Debug for Card<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Card")
            .field("title", &self.title)
            .field("content", &self.content.is_some())
            .field("controls", &self.controls.is_some())
            .field("max_height", &self.max_height)
            .field("width", &self.width)
            .field("header_cache", &self.header_cache.is_some())
            .field("no_content_screen", &self.no_content_screen.is_some())
            .field("disable_background_shadow", &self.disable_background_shadow)
            .field("disable_decorations", &self.disable_decorations)
            .field("clip", &self.clip)
            .finish()
    }
}
