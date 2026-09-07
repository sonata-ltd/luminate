use iced::{Background, Color};

/// Which side of the content a ring sits on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// Outside the content, growing the widget.
    Outer,
    /// Inside the content's edge, drawn on top of it.
    Inner,
}

/// One ring of border.
///
/// Rings on the same side are ordered by distance from the content edge:
/// the first pushed touches the edge, the next sits beyond it, and so on.
/// outward for [`Side::Outer`], inward for [`Side::Inner`].
/// [`offset`](Self::offset) is the gap between a ring and the previous one
/// on its side (or the content edge for the first).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ring {
    /// Whether the ring sits outside or inside the content.
    pub side: Side,
    /// Thickness of the ring.
    pub width: f32,
    /// Colour of the ring.
    pub color: Color,
    /// Corner radius of the ring.
    pub radius: f32,
    /// Gap toward the content edge: to the previous ring on this side, or
    /// to the content for the first ring.
    pub offset: f32,
    /// Whether the ring takes room in the layout box.
    ///
    /// Outer rings do by default, and the widget's
    /// [`outer_thickness`](super::MultiBorder::outer_thickness) must reserve
    /// them. A ring marked [`overflowing`](Self::overflowing) is drawn
    /// outside the layout box instead: it overlaps whatever sits next to the
    /// widget and is cut by any ancestor that clips.
    ///
    /// Meaningless for [`Side::Inner`], which is drawn over the content and
    /// never takes room; inner rings carry `false`.
    pub reserved: bool,
}

impl Ring {
    /// A ring outside the content.
    ///
    /// # Panics
    /// In debug builds, when `width` is negative or not finite.
    #[must_use]
    pub fn outer(width: f32, color: Color) -> Self {
        Self::new(Side::Outer, width, color)
    }

    /// A ring just inside the content's edge.
    ///
    /// # Panics
    /// In debug builds, when `width` is negative or not finite.
    #[must_use]
    pub fn inner(width: f32, color: Color) -> Self {
        Self::new(Side::Inner, width, color)
    }

    fn new(side: Side, width: f32, color: Color) -> Self {
        debug_assert!(
            width.is_finite() && width >= 0.0,
            "ring width must be finite and non-negative, got {width}"
        );

        Self {
            side,
            width,
            color,
            radius: 0.0,
            offset: 0.0,
            reserved: matches!(side, Side::Outer),
        }
    }

    /// Draws this ring outside the layout box instead of inside it.
    ///
    /// The widget's box stays the size of its content, so the ring overlaps
    /// whatever sits beside it and an ancestor that clips cuts it off. Use
    /// it for a ring that appears only in a transient state — a pressed or
    /// focused outline — where reserving room for it would space every
    /// widget apart even at rest.
    ///
    /// No-op for [`Side::Inner`], which never takes room anyway.
    #[must_use]
    pub fn overflowing(mut self) -> Self {
        self.reserved = false;
        self
    }

    /// Sets the corner radius.
    ///
    /// # Panics
    /// In debug builds, when `radius` is negative or not finite.
    #[must_use]
    pub fn radius(mut self, radius: f32) -> Self {
        debug_assert!(
            radius.is_finite() && radius >= 0.0,
            "ring radius must be finite and non-negative, got {radius}"
        );
        self.radius = radius;
        self
    }

    /// Sets the gap toward the content edge. Rings never overlap, so the
    /// gap cannot be negative.
    ///
    /// # Panics
    /// In debug builds, when `offset` is negative or not finite.
    #[must_use]
    pub fn offset(mut self, offset: f32) -> Self {
        debug_assert!(
            offset.is_finite() && offset >= 0.0,
            "ring offset must be finite and non-negative, got {offset}"
        );
        self.offset = offset;
        self
    }
}

/// The interaction state a [`Style`] is computed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct Status {
    /// Pointer is pressed on the content.
    pub is_pressed: bool,
    /// Pointer is over the content.
    pub is_hovered: bool,
    /// The widget is focused.
    pub is_focused: bool,
    /// The widget is disabled.
    pub is_disabled: bool,
}

/// What a [`MultiBorder`](super::MultiBorder) draws.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    /// Rings, each side ordered from the content edge outward (see
    /// [`Ring`]).
    pub rings: Vec<Ring>,
    /// Fill behind the content. It reaches the innermost outer ring and is
    /// rounded to stay concentric with it; without outer rings it is the
    /// content rectangle rounded like the first inner ring.
    pub background: Option<Background>,
}

impl Style {
    /// An empty style: no rings, no background.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a ring.
    #[must_use]
    pub fn ring(mut self, ring: Ring) -> Self {
        self.rings.push(ring);
        self
    }

    /// Sets the background fill.
    #[must_use]
    pub fn background(mut self, background: impl Into<Background>) -> Self {
        self.background = Some(background.into());
        self
    }

    /// Total thickness of the outer rings (widths plus offsets): how far
    /// the rings extend beyond the content, whether or not the layout
    /// reserves room for them.
    #[must_use]
    pub fn outer_thickness(&self) -> f32 {
        self.thickness(|ring| ring.side == Side::Outer)
    }

    /// Total thickness of the outer rings that take room in the layout —
    /// what [`outer_thickness`](super::MultiBorder::outer_thickness) must
    /// reserve. Rings marked [`overflowing`](Ring::overflowing) are
    /// excluded: they are drawn past the layout box on purpose.
    #[must_use]
    pub fn reserved_thickness(&self) -> f32 {
        self.thickness(|ring| ring.side == Side::Outer && ring.reserved)
    }

    fn thickness(&self, keep: impl Fn(&Ring) -> bool) -> f32 {
        self.rings
            .iter()
            .filter(|ring| keep(ring))
            .map(|ring| ring.width + ring.offset)
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_compare_by_value() {
        let a = Style::new().ring(Ring::outer(1.0, Color::BLACK).radius(2.0));
        let b = Style::new().ring(Ring::outer(1.0, Color::BLACK).radius(2.0));
        assert_eq!(a, b);
        assert_ne!(a, Style::new());
    }

    #[test]
    fn outer_rings_are_reserved_unless_marked_overflowing() {
        assert!(Ring::outer(1.0, Color::BLACK).reserved);
        assert!(!Ring::outer(1.0, Color::BLACK).overflowing().reserved);
        // Inner rings never take room, so they are never reserved.
        assert!(!Ring::inner(1.0, Color::BLACK).reserved);
    }

    #[test]
    fn only_reserved_rings_count_toward_the_reserved_thickness() {
        let style = Style::new()
            .ring(Ring::outer(2.0, Color::BLACK).offset(1.0))
            .ring(Ring::outer(4.0, Color::WHITE).offset(3.0).overflowing())
            .ring(Ring::inner(9.0, Color::BLACK));

        // Both outer rings are painted…
        assert_eq!(style.outer_thickness(), 10.0);
        // …but only the first one asks the layout for room.
        assert_eq!(style.reserved_thickness(), 3.0);
    }

    #[test]
    fn a_wholly_overflowing_style_reserves_nothing() {
        let style = Style::new().ring(Ring::outer(2.0, Color::BLACK).offset(2.0).overflowing());

        assert_eq!(style.outer_thickness(), 4.0);
        assert_eq!(style.reserved_thickness(), 0.0);
    }

    #[test]
    #[should_panic(expected = "ring offset must be finite and non-negative")]
    fn a_negative_offset_is_a_programming_error() {
        let _ = Ring::inner(1.0, Color::BLACK).offset(-1.0);
    }

    #[test]
    #[should_panic(expected = "ring width must be finite and non-negative")]
    fn a_negative_width_is_a_programming_error() {
        let _ = Ring::outer(-1.0, Color::BLACK);
    }
}
