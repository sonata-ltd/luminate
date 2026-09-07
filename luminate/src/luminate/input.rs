//! [`Luminate::input`].

use iced::Pixels;
use iced::advanced::widget::tree::{self, Tag};
use iced::widget::{Column, text_input};

use crate::descriptor::Input;
use crate::luminate::Luminate;
use crate::theme::typography::styled_text;
use crate::theme::{InputClass, TextClass, Theme};
use crate::widget::error_bubble::ErrorBubble;
use crate::widget::multi_border::{Focus, Ring, Style, multi_border};
use crate::{Element, Renderer};

/// Whether any `text_input` in `tree` is focused.
fn text_input_focused(tree: &tree::Tree) -> bool {
    type Paragraph = <Renderer as iced::advanced::text::Renderer>::Paragraph;

    if tree.tag == Tag::of::<text_input::State<Paragraph>>() {
        return tree
            .state
            .downcast_ref::<text_input::State<Paragraph>>()
            .is_focused();
    }

    tree.children.iter().any(text_input_focused)
}

impl Luminate {
    /// Builds a text input with its optional label, hint and error bubble.
    /// Colours come from the theme's [`InputClass`] and [`TextClass`]es;
    /// the focus ring is a [`MultiBorder`](crate::widget::multi_border::MultiBorder) outer
    /// ring sized from the input tokens.
    pub fn input<'a, M: Clone + 'a>(&self, descriptor: Input<'a, M>) -> Element<'a, M> {
        let Input {
            value,
            placeholder,
            label,
            hint,
            error,
            height,
            width,
            id,
            secure,
            size,
            on_input,
            on_submit,
        } = descriptor;

        let tokens = self.theme.input;
        let is_error = error.is_some();
        let is_enabled = on_input.is_some();

        let mut input: text_input::TextInput<'a, M, Theme, Renderer> =
            text_input(placeholder, value)
                .font(tokens.text_style.font())
                .size(size.unwrap_or(Pixels(tokens.text_style.size)))
                .line_height(tokens.text_style.line_height())
                .padding(tokens.padding)
                .width(width)
                .secure(secure)
                .on_input_maybe(on_input)
                .on_submit_maybe(on_submit)
                .class(if is_error {
                    InputClass::Error
                } else {
                    InputClass::Normal
                });

        if let Some(id) = id {
            input = input.id(id);
        }

        // Like the button's pressed ring, the focus ring is drawn outside
        // the layout box: a field keeps the same footprint focused or not,
        // so focusing one never nudges the form around it.
        let bordered = multi_border(input)
            .disabled(!is_enabled)
            .focus(Focus::Custom(Box::new(text_input_focused)))
            .style(move |theme: &Theme, status| {
                if status.is_disabled || !status.is_focused {
                    return Style::new();
                }

                let t = theme.input;
                let color = if is_error { t.ring_error } else { t.ring };

                Style::new().ring(
                    Ring::outer(t.ring_width, color)
                        .offset(t.ring_offset)
                        .radius(t.ring_radius)
                        .overflowing(),
                )
            });

        let bordered = bordered.height(height.unwrap_or(tokens.height));

        // Colours come from the theme's `error_bubble::Catalog` default
        // class; the metrics are forwarded from the tokens.
        let bubble = self.theme.error_bubble;
        let with_bubble: Element<'a, M> = ErrorBubble::new(bordered, error)
            .padding(bubble.padding)
            .gap(bubble.gap)
            .arrow(bubble.arrow_width, bubble.arrow_height)
            .arrow_offset(bubble.arrow_right_offset)
            .right_offset(bubble.right_offset)
            .text_style(
                bubble.text_style.font(),
                bubble.text_style.size,
                bubble.text_style.line_height(),
            )
            .into();

        if label.is_none() && hint.is_none() {
            return with_bubble;
        }

        let mut column = Column::new().width(width);

        if let Some(label) = label {
            column = column.push(styled_text(label, tokens.label_style).class(TextClass::Label));
        }

        column = column.push(with_bubble);

        if let Some(hint) = hint {
            let class = if is_error {
                TextClass::HintError
            } else {
                TextClass::Hint
            };
            column = column.push(styled_text(hint, tokens.hint_style).class(class));
        }

        column.spacing(tokens.spacing).into()
    }
}
