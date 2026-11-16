//! The collapse-toggle chevrons.

use std::sync::LazyLock;

use iced::advanced::svg;

use crate::descriptor::Axis;

static LEFT: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(include_bytes!("assets/chevron-left.svg")));
static RIGHT: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(include_bytes!("assets/chevron-right.svg")));
static UP: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(include_bytes!("assets/chevron-up.svg")));
static DOWN: LazyLock<svg::Handle> =
    LazyLock::new(|| svg::Handle::from_memory(include_bytes!("assets/chevron-down.svg")));

/// The toggle chevron: it points the way the bar edge will move when
/// pressed (a column collapses its width, a row its height).
pub(super) fn chevron(axis: Axis, collapsed: bool) -> svg::Handle {
    match (axis, collapsed) {
        (Axis::Vertical, false) => LEFT.clone(),
        (Axis::Vertical, true) => RIGHT.clone(),
        (Axis::Horizontal, false) => UP.clone(),
        (Axis::Horizontal, true) => DOWN.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_chevron_points_the_way_the_sidebar_will_move() {
        assert_ne!(
            chevron(Axis::Vertical, false),
            chevron(Axis::Vertical, true)
        );
        assert_ne!(
            chevron(Axis::Horizontal, false),
            chevron(Axis::Horizontal, true)
        );
        assert_ne!(
            chevron(Axis::Vertical, false),
            chevron(Axis::Horizontal, false)
        );
    }
}
