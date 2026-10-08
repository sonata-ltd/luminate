use iced::{Background, Border, Color, Shadow};

/// State of the field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Waiting.
    Active,
    /// The cursor is over the field.
    Hovered,
    /// The menu is open.
    Opened,
    /// The dropdown is disabled.
    Disabled,
}

/// Colors of the dropdown: field, menu and items.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Field background.
    pub background: Background,
    /// Field border.
    pub border: Border,
    /// Color of the selected value.
    pub text: Color,
    /// Color of the placeholder.
    pub placeholder: Color,
    /// Color of the chevron.
    pub handle: Color,
    /// Menu background.
    pub menu_background: Background,
    /// Menu border.
    pub menu_border: Border,
    /// Menu shadow.
    pub menu_shadow: Shadow,
    /// Background of the highlighted item.
    pub item_hovered: Background,
    /// Background of the selected item.
    pub item_selected: Background,
    /// Corner radius of an item background.
    pub item_radius: f32,
    /// Text color of an item.
    pub item_text: Color,
    /// Text color of the selected item.
    pub item_selected_text: Color,
    /// Color of the check mark.
    pub check: Color,
}

catalog!(status: Status, |theme, status| {
    let palette = theme.extended_palette();

    Style {
        background: palette.background.base.color.into(),
        border: Border {
            color: if status == Status::Opened {
                palette.primary.base.color
            } else {
                palette.background.strong.color
            },
            width: 1.0,
            radius: 6.0.into(),
        },
        text: palette.background.base.text,
        placeholder: palette.background.strong.color,
        handle: palette.background.base.text,
        menu_background: palette.background.base.color.into(),
        menu_border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 6.0.into(),
        },
        menu_shadow: Shadow::default(),
        item_hovered: palette.background.weak.color.into(),
        item_selected: palette.primary.weak.color.into(),
        item_radius: 4.0,
        item_text: palette.background.base.text,
        item_selected_text: palette.primary.weak.text,
        check: palette.primary.base.color,
    }
});
