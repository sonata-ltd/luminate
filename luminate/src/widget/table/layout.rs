use iced::{Length, Padding};

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Span {
    pub(super) x: f32,
    pub(super) width: f32,
}

/// Divides `available` between the columns.
///
/// Fixed and `Shrink` columns (measured by the header) go first, the rest is
/// shared by `Fill` and `FillPortion`. Column edges are rounded to whole pixels,
/// so neighboring backgrounds leave no gaps and the widths sum exactly.
pub(super) fn spans(
    available: f32,
    widths: &[Length],
    measured: &[f32],
    cell_padding_x: f32,
    inset: Padding,
) -> (Vec<Span>, f32) {
    let padding = 2.0 * cell_padding_x;
    let available = available - inset.left - inset.right;

    let rigid = |i: usize, width: Length| -> Option<f32> {
        match width {
            Length::Fixed(px) => Some(px),
            Length::Shrink => Some(
                measured
                    .get(i)
                    .copied()
                    .filter(|w| w.is_finite())
                    .unwrap_or(0.0),
            ),
            Length::Fill | Length::FillPortion(_) => None,
        }
    };

    let rigid_total: f32 = widths
        .iter()
        .enumerate()
        .filter_map(|(i, width)| rigid(i, *width))
        .sum::<f32>()
        + padding * widths.len() as f32;

    let portions: f32 = widths
        .iter()
        .map(|width| f32::from(width.fill_factor()))
        .sum();

    let free = if available.is_finite() {
        (available - rigid_total).max(0.0)
    } else {
        0.0
    };

    let mut spans = Vec::with_capacity(widths.len());
    let mut cursor = 0.0_f32;

    for (i, width) in widths.iter().enumerate() {
        let content = rigid(i, *width)
            .unwrap_or_else(|| free * f32::from(width.fill_factor()) / portions.max(1.0));

        let start = cursor.round();
        cursor += content + padding;
        let end = cursor.round();

        spans.push(Span {
            x: inset.left + start + cell_padding_x,
            width: (end - start - padding).max(0.0),
        });
    }

    (spans, cursor.round() + inset.left + inset.right)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_fill_and_portions_fill_the_width_exactly() {
        let widths = [
            Length::Fixed(100.0),
            Length::Fill,
            Length::FillPortion(2),
            Length::Shrink,
        ];
        let (spans, total) = spans(
            600.0,
            &widths,
            &[0.0, 0.0, 0.0, 40.0],
            8.0,
            Padding::default(),
        );

        assert_eq!(total, 600.0);
        assert_eq!(
            spans[0],
            Span {
                x: 8.0,
                width: 100.0
            }
        );
        assert_eq!(spans[3].width, 40.0);
        assert!((spans[2].width - 2.0 * spans[1].width).abs() <= 1.0);
        for pair in spans.windows(2) {
            assert_eq!(pair[0].x + pair[0].width + 16.0, pair[1].x);
        }
    }

    #[test]
    fn edges_land_on_whole_pixels() {
        let widths = [Length::Fill, Length::Fill, Length::Fill];
        let (spans, total) = spans(100.0, &widths, &[0.0; 3], 0.0, Padding::default());

        assert_eq!(total, 100.0);
        for span in spans {
            assert_eq!(span.x.fract(), 0.0);
            assert_eq!(span.width.fract(), 0.0);
        }
    }

    #[test]
    fn the_inset_shifts_the_columns_and_is_kept_in_the_total() {
        let widths = [Length::Fixed(100.0), Length::Fill];
        let cell_padding_x = 8.0;
        let (spans, total) = spans(
            600.0,
            &widths,
            &[0.0; 2],
            cell_padding_x,
            Padding::default(),
        );

        assert_eq!(total, 600.0);
        assert_eq!(
            spans[0],
            Span {
                x: cell_padding_x,
                width: 100.0
            }
        );

        let last = spans[1];
        assert_eq!(last.x + last.width + cell_padding_x, 600.0);
    }
}
