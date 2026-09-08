//! Reusable widgets the Luminate kit is built from.
//!
//! Each widget is styled through its own `Catalog`, implemented for
//! `iced::Theme` here (neutral colours derived from the theme's extended
//! palette) and for [`theme::Theme`](crate::theme::Theme) in `theme/` (from
//! the Luminate tokens). A `Style` holds paint only; sizes reach a widget
//! through its builder methods.
//!
//! Only the widget types and their helper functions are re-exported here.
//! Per-widget `Style`, `Status`, `Catalog` and `StyleFn` live in their
//! modules (`widget::multi_border::Style`, …), as in `iced::widget`.

/// Generates the `StyleFn` alias, the `Catalog` trait and its `iced::Theme`
/// implementation for the widget module it is invoked in.
///
/// The module must define `Style` (and `Status` for the first form). The
/// closure is the `iced::Theme` default class. The implementation for
/// `crate::theme::Theme` lives in `theme/`, next to the tokens it reads.
macro_rules! catalog {
    // A style that depends on an interaction status.
    (status: $status:ty, |$theme:ident, $st:ident| $default:expr) => {
        /// Computes the [`Style`] for a [`Status`].
        pub type StyleFn<'a, Theme> = Box<dyn Fn(&Theme, $status) -> Style + 'a>;

        /// A theme that can style this widget.
        pub trait Catalog {
            /// The item class of the [`Catalog`].
            type Class<'a>;

            /// The default class produced by the [`Catalog`].
            fn default<'a>() -> Self::Class<'a>;

            /// The [`Style`] of a class with the given status.
            fn style(&self, class: &Self::Class<'_>, status: $status) -> Style;
        }

        impl Catalog for iced::Theme {
            type Class<'a> = StyleFn<'a, Self>;

            fn default<'a>() -> Self::Class<'a> {
                Box::new(|$theme: &iced::Theme, $st: $status| $default)
            }

            fn style(&self, class: &Self::Class<'_>, status: $status) -> Style {
                class(self, status)
            }
        }
    };
    // A style that depends on the theme alone.
    (|$theme:ident| $default:expr) => {
        /// Computes the [`Style`].
        pub type StyleFn<'a, Theme> = Box<dyn Fn(&Theme) -> Style + 'a>;

        /// A theme that can style this widget.
        pub trait Catalog {
            /// The item class of the [`Catalog`].
            type Class<'a>;

            /// The default class produced by the [`Catalog`].
            fn default<'a>() -> Self::Class<'a>;

            /// The [`Style`] of a class.
            fn style(&self, class: &Self::Class<'_>) -> Style;
        }

        impl Catalog for iced::Theme {
            type Class<'a> = StyleFn<'a, Self>;

            fn default<'a>() -> Self::Class<'a> {
                Box::new(|$theme: &iced::Theme| $default)
            }

            fn style(&self, class: &Self::Class<'_>) -> Style {
                class(self)
            }
        }
    };
}

pub mod error_bubble;
pub mod multi_border;
pub mod sidebar;
pub mod weighted_text;
