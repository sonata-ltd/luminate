//! [`Luminate::tabs`].

use iced::widget::{container, stack};
use iced::{Alignment, Color, Length, Padding};

use crate::descriptor::Tabs;
use crate::luminate::Luminate;
use crate::theme::typography::styled_text;
use crate::widget::tabs::Tabs as Widget;
use crate::{Element, Theme};

impl Luminate {
    /// Builds a tab bar on the kit's engine: the selection backing and the
    /// separators animate, and every metric and colour comes from the tabs
    /// tokens. The selected tab's label is drawn in the active look, the
    /// others in the inactive one.
    #[must_use]
    pub fn tabs<'a, M: Clone + 'a>(&self, descriptor: Tabs<'a, M>) -> Element<'a, M> {
        let Tabs {
            tabs,
            active,
            width,
            on_select,
        } = descriptor;

        let tokens = self.theme.tabs;
        let active = active.min(tabs.len().saturating_sub(1));

        let items = tabs
            .into_iter()
            .enumerate()
            .map(|(index, tab)| self.tab_label(tab.label, index == active));

        let mut bar = Widget::<M, Theme>::with_children(items)
            .active(active)
            .width(width)
            .padding(tokens.padding)
            .spacing(tokens.spacing)
            .radius(tokens.radius)
            .backing_radius(tokens.backing_radius)
            .motion(self.motion.clone());

        if let Some(on_select) = on_select {
            bar = bar.on_select(on_select);
        }

        bar.into()
    }

    /// A centred label in the active or inactive look. A transparent copy
    /// in the active look sits under it, so the tab keeps the width of its
    /// heavier state and the bar does not shift when the selection moves.
    fn tab_label<'a, M: 'a>(&self, label: &'a str, selected: bool) -> Element<'a, M> {
        let tokens = self.theme.tabs;

        let (style, color) = if selected {
            (tokens.selected_label_style, tokens.selected_label_color)
        } else {
            (tokens.label_style, tokens.label_color)
        };

        let text = |style, color| {
            styled_text(label, style)
                .color(color)
                .width(Length::Fill)
                .align_x(Alignment::Center)
        };

        container(stack![
            text(tokens.selected_label_style, Color::TRANSPARENT),
            text(style, color),
        ])
        .padding(Padding::from(tokens.item_padding))
        .width(Length::Fill)
        .into()
    }
}
