use iced::{Background, Border, Color};

use crate::widget::pick_indicator::{self, Pick};

/// What a row is like while it is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Status {
    /// The cursor is over the row.
    pub hovered: bool,
    /// The mouse button is down on the row.
    pub pressed: bool,
    /// The pick state of the row.
    pub pick: Pick,
    /// The row cannot be selected.
    pub disabled: bool,
    /// An odd row of a striped table.
    pub odd: bool,
    /// The keyboard cursor is on the row and the table is focused.
    pub active: bool,
}

/// Colors of the table.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Background of the table.
    pub background: Option<Background>,
    /// Border of the table.
    pub border: Border,
    /// Background of the header.
    pub header_background: Option<Background>,
    /// Default text color of the headers.
    pub header_text: Color,
    /// Line under the header.
    pub divider: Color,
    /// Background of a row in its state.
    pub row_background: Option<Background>,
    /// Veil over the content of a disabled row.
    pub row_veil: Option<Background>,
    /// Radius of the cell.
    pub cell_radius: f32,
    /// Border of the keyboard cursor.
    pub focus_ring: Border,
    /// Colors of the pick indicator.
    pub indicator: pick_indicator::Style,
}

catalog!(status: Status, |theme, status| {
    let palette = theme.extended_palette();
    let accent = palette.primary.base.color;

    let row_background = match status {
        Status { pick: Pick::On, .. } => Some(palette.primary.weak.color.into()),
        Status { pressed: true, .. } => Some(palette.background.strong.color.into()),
        Status { hovered: true, .. } => Some(palette.background.weak.color.into()),
        Status { odd: true, .. } => Some(palette.background.weakest.color.into()),
        _ => None,
    };

    Style {
        background: Some(palette.background.base.color.into()),
        border: Border {
            color: palette.background.strong.color,
            width: 1.0,
            radius: 8.0.into(),
        },
        header_background: Some(palette.background.weak.color.into()),
        header_text: palette.background.weak.text,
        divider: palette.background.strong.color,
        row_background,
        row_veil: Some(palette.background.base.color.scale_alpha(0.5).into()),
        cell_radius: 12.0,
        focus_ring: Border {
            color: accent,
            width: 1.0,
            radius: 0.0.into(),
        },
        indicator: pick_indicator::Style {
            ring: if status.pick == Pick::Off {
                palette.background.strong.color
            } else {
                accent
            },
            dot: Color::WHITE,
            fill: accent
        },
    }
});
