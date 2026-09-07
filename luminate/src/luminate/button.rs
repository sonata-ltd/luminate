//! [`Luminate::button`].

use iced::Alignment::Center;
use iced::Length::Fill;
use iced::widget::{Svg, button, container, row};

use crate::descriptor::{Button, ButtonContent};
use crate::luminate::Luminate;
use crate::theme::typography::styled_text;
use crate::theme::{ButtonClass, SvgClass, Theme};
use crate::widget::multi_border::{Ring, Style, multi_border};
use crate::{Element, Renderer};

impl Luminate {
    /// Builds a button. Colours come from the theme's
    /// [`ButtonClass`] for the descriptor's hierarchy (the icon's from the
    /// matching [`SvgClass::ButtonIcon`]); the pressed ring is a
    /// [`MultiBorder`](crate::widget::multi_border::MultiBorder) outer ring sized from the
    /// button tokens.
    pub fn button<'a, M: Clone + 'a>(&self, descriptor: Button<'a, M>) -> Element<'a, M> {
        let Button {
            content,
            hierarchy,
            size,
            width,
            height,
            line_height,
            id,
            on_press,
        } = descriptor;

        let tokens = self.theme.button;
        let is_disabled = on_press.is_none();
        let padding = tokens.padding(size).for_content(&content);

        let label = move |text: &'a str| {
            let mut label = styled_text::<Theme, Renderer>(text, tokens.label)
                .align_x(Center)
                .width(Fill);
            if let Some(line_height) = line_height {
                label = label.line_height(line_height);
            }
            label
        };
        // The icon follows the label colour per status through the theme's
        // `svg::Catalog`, like the label does through `button::Catalog`.
        let icon = move |handle| {
            Svg::<Theme>::new(handle)
                .width(tokens.icon_size)
                .height(tokens.icon_size)
                .class(SvgClass::ButtonIcon {
                    hierarchy,
                    disabled: is_disabled,
                })
        };

        let content: Element<'a, M> = match content {
            ButtonContent::Text(text) => label(text).into(),
            ButtonContent::Icon(handle) => icon(handle).into(),
            ButtonContent::Combined { icon: handle, text } => row![icon(handle), label(text)]
                .spacing(tokens.icon_spacing)
                .align_y(Center)
                .into(),
        };

        let button = button(content)
            .padding(padding)
            .width(width)
            .height(height)
            .on_press_maybe(on_press)
            .class(ButtonClass::Hierarchy(hierarchy));

        // The pressed ring is drawn outside the layout box, so a row of
        // buttons sits as tightly at rest as it would with no ring at all;
        // an ancestor that clips will cut it. The closure reads the tokens
        // from the theme it is handed, capturing only the hierarchy.
        let ringed =
            multi_border(button)
                .disabled(is_disabled)
                .style(move |theme: &Theme, status| {
                    if !status.is_pressed || status.is_disabled {
                        return Style::new();
                    }

                    let t = theme.button;
                    let variant = t.variant(hierarchy);

                    Style::new().ring(
                        Ring::outer(t.ring_width, variant.ring)
                            .offset(t.ring_offset)
                            .radius(t.ring_radius)
                            .overflowing(),
                    )
                });

        match id {
            Some(id) => container(ringed).id(id).into(),
            None => ringed.into(),
        }
    }
}
