//! The button descriptor.

use std::fmt;

use iced::Length;
use iced::advanced::svg;
use iced::advanced::widget;
use iced::widget::text::LineHeight;

/// Visual weight of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonHierarchy {
    /// Accent-filled default.
    #[default]
    Primary,
    /// Neutral fill.
    Secondary,
    /// Text only, no fill.
    Tertiary,
    /// Red fill for dangerous actions.
    Destructive,
}

/// Size preset of a button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    /// Compact preset; the default.
    #[default]
    Small,
    /// Middle preset.
    Medium,
    /// Largest preset.
    Large,
}

/// What a button shows.
#[derive(Debug, Clone)]
pub enum ButtonContent<'a> {
    /// A text label.
    Text(&'a str),
    /// An icon only.
    Icon(svg::Handle),
    /// An icon followed by a text label.
    Combined {
        /// The icon, drawn before the text.
        icon: svg::Handle,
        /// The label.
        text: &'a str,
    },
}

/// A button. `on_press: None` is a disabled button.
#[derive(Clone)]
pub struct Button<'a, Message> {
    /// Label, icon, or both.
    pub content: ButtonContent<'a>,
    /// Visual weight (default `Primary`).
    pub hierarchy: ButtonHierarchy,
    /// Size preset (default `Small`).
    pub size: ButtonSize,
    /// Width (default `Shrink`).
    pub width: Length,
    /// Height (default `Shrink`).
    pub height: Length,
    /// Overrides the label's line height.
    pub line_height: Option<LineHeight>,
    /// Widget id, for operations such as focusing.
    pub id: Option<widget::Id>,
    /// Message published on press; `None` disables the button.
    pub on_press: Option<Message>,
    /// Interaction cursor override
    pub force_default_interaction: bool,
    /// Disable ring
    pub flat: bool,
}

impl<'a, Message> Button<'a, Message> {
    /// A text button with default hierarchy and size.
    #[must_use]
    pub fn new(label: &'a str) -> Self {
        Self::with_content(ButtonContent::Text(label))
    }

    /// An icon-only button with default hierarchy and size.
    #[must_use]
    pub fn with_icon(icon: impl Into<svg::Handle>) -> Self {
        Self::with_content(ButtonContent::Icon(icon.into()))
    }

    fn with_content(content: ButtonContent<'a>) -> Self {
        Self {
            content,
            hierarchy: ButtonHierarchy::default(),
            size: ButtonSize::default(),
            width: Length::Shrink,
            height: Length::Shrink,
            line_height: None,
            id: None,
            on_press: None,
            force_default_interaction: false,
            flat: false,
        }
    }

    /// Sets the text; combined with the icon if one is set.
    #[must_use]
    pub fn label(mut self, text: &'a str) -> Self {
        self.content = match self.content {
            ButtonContent::Icon(icon) | ButtonContent::Combined { icon, .. } => {
                ButtonContent::Combined { icon, text }
            }
            ButtonContent::Text(_) => ButtonContent::Text(text),
        };
        self
    }

    /// Sets the icon; combined with the text if one is set.
    #[must_use]
    pub fn icon(mut self, icon: impl Into<svg::Handle>) -> Self {
        let icon = icon.into();
        self.content = match self.content {
            ButtonContent::Text(text) | ButtonContent::Combined { text, .. } => {
                ButtonContent::Combined { icon, text }
            }
            ButtonContent::Icon(_) => ButtonContent::Icon(icon),
        };
        self
    }

    /// Visual weight; `Primary` is the accent-filled default.
    #[must_use]
    pub fn hierarchy(mut self, hierarchy: ButtonHierarchy) -> Self {
        self.hierarchy = hierarchy;
        self
    }

    /// Size preset (default `Small`).
    #[must_use]
    pub fn size(mut self, size: ButtonSize) -> Self {
        self.size = size;
        self
    }

    /// Sets the width (default `Shrink`).
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the height (default `Shrink`).
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Overrides the label's line height.
    #[must_use]
    pub fn line_height(mut self, line_height: impl Into<LineHeight>) -> Self {
        self.line_height = Some(line_height.into());
        self
    }

    /// Sets the widget id.
    #[must_use]
    pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Enables the button and sets the message it publishes.
    #[must_use]
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }

    /// Enables the button if `message` is `Some`, disables it otherwise.
    #[must_use]
    pub fn on_press_maybe(mut self, message: Option<Message>) -> Self {
        self.on_press = message;
        self
    }

    /// Force disable pointer cursor
    #[must_use]
    pub fn force_default_interaction(mut self) -> Self {
        self.force_default_interaction = true;
        self
    }

    /// Disable ring and interaction
    #[must_use]
    pub fn flat(mut self) -> Self {
        self.flat = true;
        self
    }
}

impl<Message> fmt::Debug for Button<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Button")
            .field("content", &self.content)
            .field("hierarchy", &self.hierarchy)
            .field("size", &self.size)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("line_height", &self.line_height)
            .field("id", &self.id)
            .field("enabled", &self.on_press.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon() -> svg::Handle {
        svg::Handle::from_memory(b"<svg xmlns='http://www.w3.org/2000/svg'/>".as_slice())
    }

    #[test]
    fn label_and_icon_combine_in_either_order() {
        let a = Button::<()>::new("Go").icon(icon());
        let b = Button::<()>::with_icon(icon()).label("Go");
        assert!(matches!(
            a.content,
            ButtonContent::Combined { text: "Go", .. }
        ));
        assert!(matches!(
            b.content,
            ButtonContent::Combined { text: "Go", .. }
        ));
    }

    #[test]
    fn setting_a_label_twice_replaces_it() {
        let b = Button::<()>::new("One").label("Two");
        assert!(matches!(b.content, ButtonContent::Text("Two")));
    }

    #[test]
    fn debug_needs_no_debug_message() {
        struct Opaque;
        let text = format!("{:?}", Button::new("x").on_press(Opaque));
        assert!(text.contains("enabled: true"), "{text}");
    }

    #[test]
    fn on_press_maybe_disables_with_none() {
        let b = Button::new("x").on_press(1).on_press_maybe(None);
        assert!(b.on_press.is_none());
    }
}
