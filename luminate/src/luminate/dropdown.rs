use std::fmt::Display;

use iced::widget::{Column, Row};
use iced::{Alignment, Pixels};

use crate::Element;
use crate::luminate::Luminate;
use crate::theme::typography::styled_text;
use crate::theme::{TextClass, Theme};
use crate::widget::dropdown::{self, Dropdown};
use crate::widget::error_bubble::ErrorBubble;
use crate::widget::multi_border::{Focus, Ring, Style, multi_border};

impl Luminate {
    /// A dropdown styled as a Luminate input: input metrics and font, menu tokens, label, hint, error bubble and focus ring.
    pub fn dropdown<'a, T, M>(&self, dropdown: Dropdown<'a, T, M>) -> Element<'a, M>
    where
        T: Clone + Display + 'a,
        M: Clone + 'a,
    {
        let input = self.theme.input;
        let tokens = self.theme.dropdown;

        let mut dropdown = dropdown;
        let label = dropdown.label.take();
        let hint = dropdown.hint.take();
        let error = dropdown.error.take();
        let action = dropdown.action.take();
        let is_error = error.is_some();
        let is_enabled = dropdown.is_enabled();

        let base = <Theme as dropdown::Catalog>::default();
        let dropdown = dropdown
            .height(input.height)
            .padding(input.padding)
            .text_size(Pixels(input.text_style.size))
            .text_line_height(input.text_style.line_height())
            .font(input.text_style.font())
            .menu_height(tokens.menu_height)
            .menu_padding(tokens.menu_padding)
            .item_height(tokens.item_height)
            .item_padding(tokens.item_padding)
            .handle_size(tokens.handle_size)
            .motion(self.motion.clone())
            .style(move |theme: &Theme, status| {
                let mut style = base(theme, status);
                if is_error {
                    style.border.color = theme.input.border_error;
                }
                style
            });

        let bordered = multi_border(Element::from(dropdown))
            .disabled(!is_enabled)
            .focus(Focus::Custom(Box::new(dropdown::is_engaged)))
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
            })
            .height(input.height);

        let bubble = self.theme.error_bubble;
        let field: Element<'a, M> = ErrorBubble::new(bordered, error)
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

        let field: Element<'a, M> = match action {
            Some(action) => Row::new()
                .push(field)
                .push(action)
                .spacing(input.spacing)
                .align_y(Alignment::Center)
                .into(),
            None => field,
        };

        if label.is_none() && hint.is_none() {
            return field;
        }

        let mut column = Column::new();

        if let Some(label) = label {
            column = column.push(styled_text(label, input.label_style).class(TextClass::Label));
        }

        column = column.push(field);

        if let Some(hint) = hint {
            let class = if is_error {
                TextClass::HintError
            } else {
                TextClass::Hint
            };
            column = column.push(styled_text(hint, input.hint_style).class(class));
        }

        column.spacing(input.spacing).into()
    }
}
