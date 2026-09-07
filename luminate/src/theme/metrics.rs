//! Metric helpers the tokens need as `const` (iced's own constructors are
//! not). Crate-private on purpose: a theme author writes the resulting
//! values into the token structs directly.

use iced::Padding;

/// Symmetric vertical/horizontal padding.
pub(crate) const fn padding_vh(vertical: f32, horizontal: f32) -> Padding {
    Padding {
        top: vertical,
        right: horizontal,
        bottom: vertical,
        left: horizontal,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_vh_is_symmetric() {
        let p = padding_vh(7.0, 15.0);
        assert_eq!((p.top, p.bottom, p.left, p.right), (7.0, 7.0, 15.0, 15.0));
    }
}
