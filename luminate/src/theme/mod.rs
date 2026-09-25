//! Luminate design tokens and the theme built from them.
//!
//! [`Theme`] is a plain value with public fields: every colour, metric and
//! type style the kit draws with ([`ButtonTheme`], [`InputTheme`], …,
//! derived from a [`Palette`](palette::Palette) and a
//! [`TypographyTheme`](typography::TypographyTheme)). It is also a
//! first-class iced theme: it implements `iced::theme::Base` and a `Catalog`
//! for every widget the kit uses, so hand it to
//! `iced::application(..).theme(..)` and every element the kit builds, and
//! every iced widget inside a Luminate [`Element`](crate::Element), draws
//! with the tokens. [`Theme::LIGHT`] and [`Theme::DARK`] are the two shipped
//! looks; build another with struct update syntax or from a palette with
//! [`Theme::light`] / [`Theme::dark`].
//!
//! ```
//! use iced_luminate::iced::theme::Base;
//! use iced_luminate::theme::Theme;
//! use iced_luminate::theme::palette::Palette;
//!
//! let dark = Theme::DARK;
//! assert_eq!(dark.base().background_color, dark.background);
//!
//! let brand = Theme::light(Palette {
//!     accent: iced_luminate::iced::Color::from_rgb8(200, 0, 120),
//!     ..Palette::LIGHT
//! });
//! assert_eq!(brand.button.primary.active.background, brand.palette.accent);
//! ```
//!
//! The class enums ([`ButtonClass`], [`ContainerClass`], …) name the looks
//! iced widgets can pick with `.class(..)`; each also accepts a style
//! closure through `.style(..)`.
//!
//! Supported widgets, everything that can appear inside a Luminate
//! [`Element`](crate::Element): iced's `button`, `text_input`, `container`,
//! `text`, `svg`, `scrollable`, `rule`, `checkbox`, `toggler`, `slider`
//! (and `vertical_slider`), `radio`, `pick_list`, `overlay::menu`,
//! `combo_box`, `progress_bar` and `text_editor`, plus the kit's own
//! [`MultiBorder`](crate::widget::multi_border::MultiBorder),
//! [`Sidebar`](crate::widget::sidebar::Sidebar) and
//! [`ErrorBubble`](crate::widget::error_bubble::ErrorBubble). Widgets without a
//! `Catalog` here (`pane_grid`, `qr_code`, …) need `iced::Theme`.

pub mod palette;
pub mod typography;

mod catalog;
pub(crate) mod metrics;
mod tokens;

pub use catalog::{
    ButtonClass, CheckboxClass, ContainerClass, InputClass, MenuClass, PickListClass,
    ProgressBarClass, RadioClass, RuleClass, ScrollableClass, SliderClass, SvgClass, TextClass,
    TextEditorClass, TogglerClass,
};
pub use tokens::{
    ButtonPadding, ButtonStatusColors, ButtonTheme, ButtonVariant, CardTheme, ErrorBubbleTheme,
    FadingScrollableTheme, InputTheme, ScrollableTheme, SidebarTheme, Theme,
};
