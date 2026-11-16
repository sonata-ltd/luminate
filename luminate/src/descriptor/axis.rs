//! Orientation and edge types shared by collapsible bars.

/// Which way a collapsible bar lays its children out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Axis {
    /// Children in a column; collapsing animates the width.
    #[default]
    Vertical,
    /// Children in a row; collapsing animates the height.
    Horizontal,
}

/// Which axis-relative edge an external shadow falls toward.
///
/// For a horizontal bar, [`Leading`](Self::Leading) is the top edge and
/// [`Trailing`](Self::Trailing) is the bottom edge. For a vertical bar they
/// are the left and right edges respectively.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ShadowSide {
    /// Top for a horizontal bar, left for a vertical one.
    Leading,
    /// Bottom for a horizontal bar, right for a vertical one.
    #[default]
    Trailing,
}
