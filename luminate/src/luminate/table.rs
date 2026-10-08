//! [`Luminate::table`].

use iced::Color;
use iced::font::Weight;
use iced::widget::{self, text};

use crate::luminate::Luminate;
use crate::theme::typography::{TextSize, TextStyle, styled_text};
use crate::widget::table::{self, Column, Table};
use crate::{Renderer, Theme};

impl Luminate {
    /// A table of `rows` with the given `columns`, on the kit's engine and with
    /// the row height, header height, cell padding and row inset from the
    /// table tokens; the colours come from the theme's `table::Catalog`.
    ///
    /// The tokens are only the defaults: set a metric on the returned table
    /// to override it.
    #[must_use]
    pub fn table<'a, T, M>(
        &self,
        columns: impl IntoIterator<Item = Column<'a, T, M>>,
        rows: &'a [T],
    ) -> Table<'a, T, M> {
        let tokens = self.theme.table;

        table::table(columns, rows)
            .row_height(tokens.row_height)
            .header_height(tokens.header_height)
            .cell_padding_x(tokens.padding_x)
            .inset(tokens.inset)
            .motion(self.motion.clone())
    }

    /// Styled header text that can be used in the tables.
    #[must_use]
    pub fn table_header_text<'a>(
        &self,
        content: impl text::IntoFragment<'a>,
    ) -> widget::Text<'a, Theme, Renderer> {
        styled_text(content, TextStyle::text(TextSize::Xs, Weight::Semibold))
            .color(self.theme().palette.text_secondary)
    }

    /// Styled row text that can be used in the tables.
    #[must_use]
    pub fn table_row_text<'a>(
        &self,
        content: impl text::IntoFragment<'a>,
    ) -> widget::Text<'a, Theme, Renderer> {
        self.table_row_text_style().apply(content)
    }

    /// The style of [`table_row_text`](Self::table_row_text) detached from the
    /// kit. It is `Copy`, so a cell closure can own it instead of cloning the
    /// kit or borrowing it, which would tie the table to the kit's lifetime.
    #[must_use]
    pub fn table_row_text_style(&self) -> TableRowText {
        TableRowText {
            color: self.theme().palette.text_primary,
        }
    }
}

/// The text style of a table row, see [`Luminate::table_row_text_style`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableRowText {
    color: Color,
}

impl TableRowText {
    /// `content` in the table row style.
    #[must_use]
    pub fn apply<'a>(
        self,
        content: impl text::IntoFragment<'a>,
    ) -> widget::Text<'a, Theme, Renderer> {
        styled_text(content, TextStyle::text(TextSize::Sm, Weight::Normal)).color(self.color)
    }
}
