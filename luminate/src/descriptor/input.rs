//! The text-input descriptor.

use std::fmt;

use iced::advanced::widget;
use iced::{Length, Pixels};

/// A single-line text input. `on_input: None` is a read-only input.
pub struct Input<'a, Message> {
    /// The current text.
    pub value: &'a str,
    /// Shown while `value` is empty.
    pub placeholder: &'a str,
    /// Caption above the field.
    pub label: Option<&'a str>,
    /// Hint below the field.
    pub hint: Option<&'a str>,
    /// Error message: `Some` switches to the error look (red border, red
    /// hint) and floats the message in a bubble next to the field.
    pub error: Option<&'a str>,
    /// Height of the field.
    pub height: Option<f32>,
    /// Width of the field, label and hint (default `Fill`).
    pub width: Length,
    /// Widget id, for `text_input::focus` and friends.
    pub id: Option<widget::Id>,
    /// Masks the text (passwords).
    pub secure: bool,
    /// Overrides the text size of the theme's input style.
    pub size: Option<Pixels>,
    /// Called with the new text on every edit; `None` makes the input
    /// read-only.
    pub on_input: Option<Box<dyn Fn(String) -> Message + 'a>>,
    /// Published when Enter is pressed.
    pub on_submit: Option<Message>,
}

impl<'a, Message> Input<'a, Message> {
    /// An input showing `value`, with `placeholder` while empty.
    #[must_use]
    pub fn new(placeholder: &'a str, value: &'a str) -> Self {
        Self {
            value,
            placeholder,
            label: None,
            hint: None,
            error: None,
            height: None,
            width: Length::Fill,
            id: None,
            secure: false,
            size: None,
            on_input: None,
            on_submit: None,
        }
    }

    /// Enables editing and sets the message published on every change.
    #[must_use]
    pub fn on_input(mut self, on_input: impl Fn(String) -> Message + 'a) -> Self {
        self.on_input = Some(Box::new(on_input));
        self
    }

    /// Message published when Enter is pressed.
    #[must_use]
    pub fn on_submit(mut self, message: Message) -> Self {
        self.on_submit = Some(message);
        self
    }

    /// Like [`on_submit`](Self::on_submit) when `Some`.
    #[must_use]
    pub fn on_submit_maybe(mut self, message: Option<Message>) -> Self {
        self.on_submit = message;
        self
    }

    /// Caption above the field.
    #[must_use]
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Hint below the field.
    #[must_use]
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// `Some(message)` shows the error look and the message; `None` the
    /// normal look.
    #[must_use]
    pub fn error(mut self, message: Option<&'a str>) -> Self {
        self.error = message;
        self
    }

    /// Set the height of the input
    #[must_use]
    pub fn height(mut self, height: impl Into<f32>) -> Self {
        self.height = Some(height.into());
        self
    }

    /// Sets the width (default `Fill`).
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Sets the widget id.
    #[must_use]
    pub fn id(mut self, id: impl Into<widget::Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Masks the text.
    #[must_use]
    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    /// Overrides the text size.
    #[must_use]
    pub fn size(mut self, size: impl Into<Pixels>) -> Self {
        self.size = Some(size.into());
        self
    }
}

impl<Message> fmt::Debug for Input<'_, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Input")
            .field("value", &self.value)
            .field("placeholder", &self.placeholder)
            .field("label", &self.label)
            .field("hint", &self.hint)
            .field("error", &self.error)
            .field("width", &self.width)
            .field("id", &self.id)
            .field("secure", &self.secure)
            .field("size", &self.size)
            .field("editable", &self.on_input.is_some())
            .field("submits", &self.on_submit.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_is_one_option() {
        let i = Input::<()>::new("p", "v").error(Some("bad"));
        assert_eq!(i.error, Some("bad"));
        let i = i.error(None);
        assert_eq!(i.error, None);
    }

    #[test]
    fn debug_reports_the_closures_as_flags() {
        let i = Input::new("p", "v").on_input(|_| ()).on_submit(());
        let text = format!("{i:?}");
        assert!(
            text.contains("editable: true") && text.contains("submits: true"),
            "{text}"
        );
    }
}
