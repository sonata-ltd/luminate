//! Collapsible-bar layout: a flex body (plus an optional toggle header) whose collapse
//! axis interpolates between the full and the collapsed size.
//!
//! The collapse axis is the flex *cross* axis: a vertical bar is a
//! column whose width collapses; a horizontal one is a row whose height
//! collapses. The header takes room along the flex main axis (above the
//! column, left of the row). Everything that needs no renderer lives in
//! [`body_limits`] and [`collapse`] so it can be unit-tested.

use iced::advanced::layout::{Limits, Node, flex};
use iced::advanced::widget::Tree;
use iced::{Alignment, Length, Size, Vector};

use super::core::{Core, State};
use crate::descriptor::Axis;

/// Linear interpolation from `full` (progress 0) to `collapsed` (progress 1).
pub(super) fn interpolate(full: f32, collapsed: f32, progress: f32) -> f32 {
    full + (collapsed - full) * progress
}

fn flex_axis(axis: Axis) -> flex::Axis {
    match axis {
        Axis::Vertical => flex::Axis::Vertical,
        Axis::Horizontal => flex::Axis::Horizontal,
    }
}

/// Extent along the collapse axis: the width of a column, the height of a
/// row.
fn collapse_of(axis: Axis, size: Size) -> f32 {
    match axis {
        Axis::Vertical => size.width,
        Axis::Horizontal => size.height,
    }
}

/// Limits for the flex body: `header` subtracted along the flex main axis
/// (never below zero).
pub(super) fn body_limits(axis: Axis, limits: &Limits, header: f32) -> Limits {
    let shrink = |size: Size| match axis {
        Axis::Vertical => Size::new(size.width, (size.height - header).max(0.0)),
        Axis::Horizontal => Size::new((size.width - header).max(0.0), size.height),
    };

    Limits::new(shrink(limits.min()), shrink(limits.max()))
}

/// The body's limits when measuring its natural size: unbounded and
/// unforced along the collapse axis.
fn natural_limits(axis: Axis, body: &Limits) -> Limits {
    let (min, max) = (body.min(), body.max());

    match axis {
        Axis::Vertical => Limits::new(
            Size::new(0.0, min.height),
            Size::new(f32::INFINITY, max.height),
        ),
        Axis::Horizontal => Limits::new(
            Size::new(min.width, 0.0),
            Size::new(max.width, f32::INFINITY),
        ),
    }
}

/// Extents along the collapse axis, computed without a renderer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Collapse {
    /// Extent when expanded: the resolved length, at least the collapsed
    /// size, within the limits.
    pub full: f32,
    /// Extent now: `full` interpolated toward the collapsed size by the
    /// progress, clamped to the limits.
    pub current: f32,
}

/// Resolves the collapse-axis extents. `natural` is the body's measured
/// extent along that axis; it only matters when the length there is
/// `Shrink` (pass `0.0` otherwise).
pub(super) fn collapse(
    axis: Axis,
    limits: &Limits,
    width: Length,
    height: Length,
    natural: f32,
    collapsed_size: f32,
    progress: f32,
) -> Collapse {
    let intrinsic = match axis {
        Axis::Vertical => Size::new(natural.max(collapsed_size), 0.0),
        Axis::Horizontal => Size::new(0.0, natural.max(collapsed_size)),
    };
    let full = collapse_of(axis, limits.resolve(width, height, intrinsic));

    let min = collapse_of(axis, limits.min());
    let max = collapse_of(axis, limits.max());
    let current = interpolate(full, collapsed_size, progress)
        .max(min)
        .min(max);

    Collapse { full, current }
}

/// Lays the bar out: header offset, flex body, collapse-axis extent.
///
/// Children sit flush with the leading edge of a column (`Alignment::Start`,
/// a navigation list) and centred across a row (`Alignment::Center`, a
/// toolbar). When the collapse-axis length is `Shrink` the body is measured
/// once at its natural size; that pass is reused as the final layout while
/// the bar is fully expanded and the limits did not clamp it, so
/// children are laid out once per frame at rest.
pub(super) fn resolve<Message, Theme, Renderer>(
    bar: &mut Core<'_, Message, Theme, Renderer>,
    tree: &mut Tree,
    renderer: &Renderer,
    limits: &Limits,
) -> Node
where
    Renderer: iced::advanced::Renderer,
{
    let axis = bar.axis;
    let progress = tree.state.downcast_ref::<State>().progress();
    let header = bar.header_extent();
    let collapsed_size = bar.collapsed_size;
    let width = bar.width.resolve();
    let height = bar.height.resolve();
    let body_limits = body_limits(axis, limits, header);
    let align = match axis {
        Axis::Vertical => Alignment::Start,
        Axis::Horizontal => Alignment::Center,
    };

    let mut flex = |limits: &Limits, width: Length, height: Length| {
        flex::resolve(
            flex_axis(axis),
            renderer,
            limits,
            width,
            height,
            bar.padding,
            bar.spacing,
            align,
            &mut bar.children,
            &mut tree.children,
        )
    };

    let shrinks = match axis {
        Axis::Vertical => matches!(width, Length::Shrink),
        Axis::Horizontal => matches!(height, Length::Shrink),
    };
    let natural = shrinks.then(|| flex(&natural_limits(axis, &body_limits), width, height));
    let natural_size = natural
        .as_ref()
        .map_or(0.0, |node| collapse_of(axis, node.size()));

    let Collapse { full, current } = collapse(
        axis,
        limits,
        width,
        height,
        natural_size,
        collapsed_size,
        progress,
    );

    let body = match natural {
        Some(node) if progress == 0.0 && (full - natural_size).abs() < f32::EPSILON => node,
        _ => match axis {
            Axis::Vertical => flex(&body_limits, Length::Fixed(current), height),
            Axis::Horizontal => flex(&body_limits, width, Length::Fixed(current)),
        },
    };

    let offset = match axis {
        Axis::Vertical => Vector::new(0.0, header),
        Axis::Horizontal => Vector::new(header, 0.0),
    };
    let children = body
        .children()
        .iter()
        .cloned()
        .map(|node| node.translate(offset))
        .collect();
    let size = match axis {
        Axis::Vertical => Size::new(current, header + body.size().height),
        Axis::Horizontal => Size::new(header + body.size().width, current),
    };

    Node::with_children(size, children)
}

#[cfg(test)]
mod tests {
    use iced::Length;

    use super::*;

    fn limits(min: (f32, f32), max: (f32, f32)) -> Limits {
        Limits::new(Size::new(min.0, min.1), Size::new(max.0, max.1))
    }

    #[test]
    fn interpolation_runs_from_full_to_collapsed() {
        assert_eq!(interpolate(200.0, 50.0, 0.0), 200.0);
        assert_eq!(interpolate(200.0, 50.0, 0.5), 125.0);
        assert_eq!(interpolate(200.0, 50.0, 1.0), 50.0);
    }

    #[test]
    fn the_header_is_subtracted_along_the_flex_main_axis() {
        let outer = limits((100.0, 30.0), (500.0, 400.0));

        let column = body_limits(Axis::Vertical, &outer, 44.0);
        assert_eq!(column.min(), Size::new(100.0, 0.0));
        assert_eq!(column.max(), Size::new(500.0, 356.0));

        let row = body_limits(Axis::Horizontal, &outer, 44.0);
        assert_eq!(row.min(), Size::new(56.0, 30.0));
        assert_eq!(row.max(), Size::new(456.0, 400.0));
    }

    #[test]
    fn a_shrink_column_collapses_from_its_natural_width() {
        let outer = limits((0.0, 0.0), (500.0, 400.0));
        let at = |progress| {
            collapse(
                Axis::Vertical,
                &outer,
                Length::Shrink,
                Length::Shrink,
                200.0,
                50.0,
                progress,
            )
        };
        assert_eq!(
            at(0.0),
            Collapse {
                full: 200.0,
                current: 200.0
            }
        );
        assert_eq!(
            at(0.5),
            Collapse {
                full: 200.0,
                current: 125.0
            }
        );
        assert_eq!(
            at(1.0),
            Collapse {
                full: 200.0,
                current: 50.0
            }
        );
    }

    #[test]
    fn a_shrink_row_collapses_its_height() {
        let outer = limits((0.0, 0.0), (500.0, 400.0));
        let c = collapse(
            Axis::Horizontal,
            &outer,
            Length::Shrink,
            Length::Shrink,
            120.0,
            50.0,
            1.0,
        );
        assert_eq!(
            c,
            Collapse {
                full: 120.0,
                current: 50.0
            }
        );
    }

    #[test]
    fn fixed_and_fill_ignore_the_natural_size() {
        let outer = limits((0.0, 0.0), (500.0, 400.0));
        let fixed = collapse(
            Axis::Vertical,
            &outer,
            Length::Fixed(300.0),
            Length::Shrink,
            200.0,
            50.0,
            0.0,
        );
        assert_eq!(fixed.full, 300.0);

        let fill = collapse(
            Axis::Vertical,
            &outer,
            Length::Fill,
            Length::Shrink,
            200.0,
            50.0,
            0.0,
        );
        assert_eq!(fill.full, 500.0);
    }

    #[test]
    fn the_full_size_is_never_below_the_collapsed_size() {
        let outer = limits((0.0, 0.0), (500.0, 400.0));
        let c = collapse(
            Axis::Vertical,
            &outer,
            Length::Shrink,
            Length::Shrink,
            20.0,
            50.0,
            0.0,
        );
        assert_eq!(c.full, 50.0);
    }

    #[test]
    fn the_current_size_honours_the_parent_minimum_and_maximum() {
        let outer = limits((80.0, 0.0), (150.0, 400.0));
        let collapsed = collapse(
            Axis::Vertical,
            &outer,
            Length::Shrink,
            Length::Shrink,
            200.0,
            50.0,
            1.0,
        );
        assert_eq!(collapsed.current, 80.0, "clamped up to the minimum");

        let expanded = collapse(
            Axis::Vertical,
            &outer,
            Length::Shrink,
            Length::Shrink,
            200.0,
            50.0,
            0.0,
        );
        assert_eq!(expanded.full, 150.0, "clamped down to the maximum");
        assert_eq!(expanded.current, 150.0);
    }
}
