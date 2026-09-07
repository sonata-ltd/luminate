//! The token structs and the two shipped looks.
//!
//! [`Theme`] is a plain `Copy` value with public fields: every colour,
//! metric and type style the kit draws with. Customise one with struct
//! update syntax:
//!
//! ```
//! use iced_luminate::theme::{CardTheme, Theme};
//!
//! let wide = Theme {
//!     name: "Luminate Light, wide cards",
//!     card: CardTheme {
//!         width: 520.0,
//!         ..Theme::LIGHT.card
//!     },
//!     ..Theme::LIGHT
//! };
//! assert_eq!(wide.card.width, 520.0);
//! ```
//!
//!, or derive a whole look from a palette with [`Theme::light`] /
//! [`Theme::dark`].

use iced::theme::Mode;
use iced::{Color, Padding, Shadow, Vector};

use crate::descriptor::{ButtonContent, ButtonHierarchy, ButtonSize};
use crate::theme::metrics::padding_vh;
use crate::theme::palette::{Palette, mix, with_alpha};
use crate::theme::typography::{TextStyle, TypographyTheme};

/// Colours of a button in one status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonStatusColors {
    /// Fill of the button.
    pub background: Color,
    /// Colour of the label (and icon).
    pub text: Color,
}

/// One button hierarchy (primary, secondary, …).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonVariant {
    /// Colours at rest.
    pub active: ButtonStatusColors,
    /// Colours while hovered.
    pub hover: ButtonStatusColors,
    /// Colours while pressed (darker than hover in the shipped looks).
    pub pressed: ButtonStatusColors,
    /// Colours while disabled.
    pub disabled: ButtonStatusColors,
    /// Ring drawn around the button while pressed.
    pub ring: Color,
    /// Drop shadow while enabled, if any.
    pub shadow: Option<Shadow>,
}

/// Paddings of one button size, by content.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonPadding {
    /// Padding around a text-only button.
    pub text: Padding,
    /// Padding around an icon-only button.
    pub icon: Padding,
    /// Padding around an icon-and-text button.
    pub icon_and_text: Padding,
}

impl ButtonPadding {
    /// The padding for `content`.
    #[must_use]
    pub const fn for_content(&self, content: &ButtonContent<'_>) -> Padding {
        match content {
            ButtonContent::Text(_) => self.text,
            ButtonContent::Icon(_) => self.icon,
            ButtonContent::Combined { .. } => self.icon_and_text,
        }
    }
}

/// Everything a button is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ButtonTheme {
    /// Colours of the primary hierarchy.
    pub primary: ButtonVariant,
    /// Colours of the secondary hierarchy.
    pub secondary: ButtonVariant,
    /// Colours of the tertiary hierarchy.
    pub tertiary: ButtonVariant,
    /// Colours of the destructive hierarchy.
    pub destructive: ButtonVariant,
    /// Paddings of the small size.
    pub small: ButtonPadding,
    /// Paddings of the medium size.
    pub medium: ButtonPadding,
    /// Paddings of the large size.
    pub large: ButtonPadding,
    /// Corner radius of the button.
    pub radius: f32,
    /// Thickness of the pressed ring.
    pub ring_width: f32,
    /// Gap between the button and the ring.
    pub ring_offset: f32,
    /// Corner radius of the pressed ring.
    ///
    /// Calibrated by eye, not derived. A ring concentric with a 10 px
    /// corner, 2 px out and 2 px thick, would be 14.0; the shipped looks
    /// draw it at 13.0, a pixel tighter, as the original kit did. Written
    /// down rather than computed so that reinstating the formula cannot
    /// silently round it back up.
    pub ring_radius: f32,
    /// Type style of the label.
    pub label: TextStyle,
    /// Icon side length in logical pixels.
    pub icon_size: f32,
    /// Gap between icon and label.
    pub icon_spacing: f32,
}

impl ButtonTheme {
    /// The variant for a hierarchy.
    #[must_use]
    pub const fn variant(&self, hierarchy: ButtonHierarchy) -> &ButtonVariant {
        match hierarchy {
            ButtonHierarchy::Primary => &self.primary,
            ButtonHierarchy::Secondary => &self.secondary,
            ButtonHierarchy::Tertiary => &self.tertiary,
            ButtonHierarchy::Destructive => &self.destructive,
        }
    }

    /// The paddings for a size.
    #[must_use]
    pub const fn padding(&self, size: ButtonSize) -> &ButtonPadding {
        match size {
            ButtonSize::Small => &self.small,
            ButtonSize::Medium => &self.medium,
            ButtonSize::Large => &self.large,
        }
    }
}

/// Everything a text input is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputTheme {
    /// Fill of the input field.
    pub background: Color,
    /// Fill while disabled.
    pub background_disabled: Color,
    /// Typed text colour (disabled inputs use the palette's
    /// `text_disabled`).
    pub text: Color,
    /// Placeholder text colour.
    pub placeholder: Color,
    /// Border colour.
    pub border: Color,
    /// Border colour in the error state.
    pub border_error: Color,
    /// Ring drawn around a focused input; also the selection colour.
    pub ring: Color,
    /// Ring drawn around a focused input in the error state.
    pub ring_error: Color,
    /// Colour of the label above the input.
    pub label_text: Color,
    /// Colour of the hint below the input.
    pub hint_text: Color,
    /// Hint colour in the error state.
    pub hint_text_error: Color,
    /// Corner radius of the field.
    pub radius: f32,
    /// Padding inside the field.
    pub padding: Padding,
    /// Thickness of the focus ring.
    pub ring_width: f32,
    /// Gap between the field and the focus ring.
    pub ring_offset: f32,
    /// Corner radius of the focus ring. Concentric with the field's corner
    /// (`radius + ring_offset + ring_width`), written down for the same
    /// reason the button's is.
    pub ring_radius: f32,
    /// Type style of the typed text.
    pub text_style: TextStyle,
    /// Type style of the label.
    pub label_style: TextStyle,
    /// Type style of the hint.
    pub hint_style: TextStyle,
    /// Gap between label, input and hint.
    pub spacing: f32,
}

/// Everything a sidebar is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SidebarTheme {
    /// Fill behind the whole sidebar.
    pub background: Color,
    /// Painted over the collapse toggle while hovered.
    pub hover_overlay: Color,
    /// Colour of the toggle chevron (read by the sidebar's `Style` from
    /// phase 7 on).
    pub icon: Color,
    /// Colour of the band along the inner edge, if any (read by the
    /// sidebar's `Style` from phase 7 on).
    pub edge_shadow: Option<Color>,
    /// Extent of the header row (or column) along the collapse axis.
    pub header_size: f32,
    /// Size along the collapse axis when collapsed.
    pub collapsed_size: f32,
    /// Side length of the collapse-toggle icon.
    pub icon_size: f32,
    /// Padding around the children.
    pub padding: f32,
    /// Gap between children.
    pub spacing: f32,
}

/// Everything a card is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CardTheme {
    /// Fill of the card.
    pub background: Color,
    /// Corner radius of the card; the header's top corners use it too.
    pub radius: f32,
    /// Width when the descriptor sets none.
    pub width: f32,
    /// Padding around the header title.
    pub header_padding: Padding,
    /// Shadow under the header row.
    pub header_shadow: Shadow,
    /// Type style of the header title.
    pub header_style: TextStyle,
    /// Tight shadow that outlines the card.
    pub card_shadow: Shadow,
    /// Wide, soft shadow beneath the card.
    pub halo_shadow: Shadow,
}

impl CardTheme {
    /// Height of the header row: padding plus one line of the header style.
    #[must_use]
    pub const fn header_height(&self) -> f32 {
        self.header_padding.top + self.header_padding.bottom + self.header_style.line_height
    }
}

/// Everything an error bubble is drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ErrorBubbleTheme {
    /// Fill of the bubble and arrow.
    pub background: Color,
    /// Message text colour.
    pub text: Color,
    /// Type style of the message.
    pub text_style: TextStyle,
    /// Padding around the message.
    pub padding: Padding,
    /// Distance from the bubble's right edge to the child's right edge.
    pub right_offset: f32,
    /// Width of the arrow.
    pub arrow_width: f32,
    /// Height of the arrow.
    pub arrow_height: f32,
    /// Distance from the bubble's right edge to the arrow.
    pub arrow_right_offset: f32,
    /// Space between the arrow tip and the child.
    pub gap: f32,
    /// Corner radius of the bubble.
    pub radius: f32,
}

/// Everything a scrollable's bars are drawn with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollableTheme {
    /// Fill of the rail behind the scroller.
    pub rail: Color,
    /// Fill of the scroller at rest.
    pub scroller: Color,
    /// Fill of the scroller while its bar is hovered.
    pub scroller_hover: Color,
    /// Fill of the scroller while dragged.
    pub scroller_dragged: Color,
    /// Corner radius of rail and scroller.
    pub radius: f32,
}

/// The values Luminate draws with. This is also an iced theme: it implements
/// `iced::theme::Base` and a `Catalog` for every widget the kit uses. See
/// [`theme`](crate::theme).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Theme {
    /// Reported by `Base::name`. iced compares names to detect a theme
    /// change between frames, so a custom look must set a `name` distinct
    /// from every other theme the application can return (in particular
    /// from `"Luminate Light"` / `"Luminate Dark"`), or the runtime keeps
    /// drawing the previous one.
    pub name: &'static str,
    /// Reported by `Base::mode`.
    pub mode: Mode,
    /// The application surface (`Base::base().background_color`).
    pub background: Color,
    /// Colour of `Rule`s.
    pub divider: Color,
    /// The colours everything below is derived from.
    pub palette: Palette,
    /// The type roles everything below is derived from.
    pub typography: TypographyTheme,
    /// Button tokens.
    pub button: ButtonTheme,
    /// Text-input tokens.
    pub input: InputTheme,
    /// Sidebar tokens.
    pub sidebar: SidebarTheme,
    /// Card tokens.
    pub card: CardTheme,
    /// Error-bubble tokens.
    pub error_bubble: ErrorBubbleTheme,
    /// Scrollable tokens.
    pub scrollable: ScrollableTheme,
}

/// The light look's `button` tokens; see [`Theme::light`].
const fn light_button(p: &Palette, typography: &TypographyTheme) -> ButtonTheme {
    ButtonTheme {
        primary: variant(
            p.accent,
            mix(p.accent, p.black, 0.1),
            mix(p.accent, p.black, 0.2),
            mix(p.accent, p.white, 0.5),
            p.white,
            p.white,
            p.focus,
            Some(Shadow {
                color: with_alpha(p.accent, 0.5),
                offset: Vector::ZERO,
                blur_radius: 10.0,
            }),
        ),
        secondary: variant(
            p.gray.s50,
            mix(p.gray.s50, p.black, 0.1),
            mix(p.gray.s50, p.black, 0.2),
            mix(p.gray.s50, p.white, 0.5),
            p.text_primary,
            p.text_disabled,
            p.focus,
            None,
        ),
        tertiary: variant(
            Color::TRANSPARENT,
            with_alpha(p.black, 0.06),
            with_alpha(p.black, 0.12),
            Color::TRANSPARENT,
            p.text_secondary,
            p.text_disabled,
            p.focus,
            None,
        ),
        destructive: variant(
            p.red.s500,
            p.red.s600,
            p.red.s700,
            mix(p.red.s500, p.white, 0.5),
            p.white,
            p.white,
            p.red.s100,
            None,
        ),
        small: ButtonPadding {
            text: padding_vh(7.0, 15.0),
            icon: padding_vh(5.0, 5.0),
            icon_and_text: padding_vh(7.0, 10.0),
        },
        medium: ButtonPadding {
            text: padding_vh(9.0, 17.0),
            icon: padding_vh(7.0, 7.0),
            icon_and_text: padding_vh(9.0, 7.0),
        },
        large: ButtonPadding {
            text: padding_vh(11.0, 19.0),
            icon: padding_vh(9.0, 9.0),
            icon_and_text: padding_vh(11.0, 11.0),
        },
        radius: 10.0,
        ring_width: 2.0,
        ring_offset: 2.0,
        ring_radius: 13.0,
        label: typography.label,
        icon_size: 20.0,
        icon_spacing: 5.0,
    }
}

/// The light look's `input` tokens; see [`Theme::light`].
const fn light_input(p: &Palette, typography: &TypographyTheme) -> InputTheme {
    InputTheme {
        background: p.gray.s25,
        background_disabled: p.gray.s50,
        text: p.text_primary,
        placeholder: p.text_placeholder,
        border: p.gray.s100,
        border_error: p.red.s500,
        ring: p.focus,
        ring_error: p.red.s200,
        label_text: p.text_secondary,
        hint_text: p.text_secondary,
        hint_text_error: p.red.s500,
        radius: 10.0,
        padding: padding_vh(7.0, 8.0),
        ring_width: 3.5,
        ring_offset: 0.0,
        ring_radius: 13.5,
        text_style: typography.body,
        label_style: typography.label,
        hint_style: typography.caption,
        spacing: 6.0,
    }
}

/// The light look's `sidebar` tokens; see [`Theme::light`].
const fn light_sidebar(p: &Palette) -> SidebarTheme {
    SidebarTheme {
        background: p.gray.s30,
        hover_overlay: with_alpha(p.black, 0.06),
        icon: p.text_primary,
        edge_shadow: Some(with_alpha(p.black, 0.03)),
        header_size: 44.0,
        collapsed_size: 50.0,
        icon_size: 20.0,
        padding: 10.0,
        spacing: 5.0,
    }
}

/// The light look's `card` tokens; see [`Theme::light`].
const fn light_card(p: &Palette, typography: &TypographyTheme) -> CardTheme {
    CardTheme {
        background: p.white,
        radius: 25.0,
        width: 400.0,
        header_padding: Padding {
            top: 15.0,
            right: 17.0,
            bottom: 15.0,
            left: 17.0,
        },
        header_shadow: Shadow {
            color: with_alpha(p.black, 0.1),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 0.0,
        },
        header_style: typography.heading,
        card_shadow: Shadow {
            color: with_alpha(p.black, 0.6),
            offset: Vector::ZERO,
            blur_radius: 1.0,
        },
        halo_shadow: Shadow {
            color: with_alpha(p.black, 0.25),
            offset: Vector::new(0.0, 38.0),
            blur_radius: 90.0,
        },
    }
}

/// The light look's `error_bubble` tokens; see [`Theme::light`].
const fn light_error_bubble(p: &Palette, typography: &TypographyTheme) -> ErrorBubbleTheme {
    ErrorBubbleTheme {
        background: p.red.s200,
        text: p.text_primary,
        text_style: typography.emphasis,
        padding: padding_vh(6.0, 10.0),
        right_offset: 7.0,
        arrow_width: 11.0,
        arrow_height: 5.5,
        arrow_right_offset: 11.0,
        gap: 5.5,
        radius: 10.0,
    }
}

/// The light look's `scrollable` tokens; see [`Theme::light`].
const fn light_scrollable(p: &Palette) -> ScrollableTheme {
    ScrollableTheme {
        rail: p.gray.s50,
        scroller: p.gray.s300,
        scroller_hover: p.gray.s400,
        scroller_dragged: p.accent,
        radius: 2.0,
    }
}

/// The dark look's `button` tokens; see [`Theme::dark`].
const fn dark_button(p: &Palette, background: Color, light: &Theme) -> ButtonTheme {
    ButtonTheme {
        primary: variant(
            p.accent,
            mix(p.accent, p.black, 0.1),
            mix(p.accent, p.black, 0.2),
            mix(p.accent, background, 0.5),
            p.white,
            p.white,
            p.focus,
            Some(Shadow {
                color: with_alpha(p.accent, 0.35),
                offset: Vector::ZERO,
                blur_radius: 10.0,
            }),
        ),
        secondary: variant(
            p.gray.s800,
            mix(p.gray.s800, p.white, 0.1),
            mix(p.gray.s800, p.white, 0.2),
            mix(p.gray.s800, background, 0.5),
            p.text_primary,
            p.text_disabled,
            p.focus,
            None,
        ),
        tertiary: variant(
            Color::TRANSPARENT,
            with_alpha(p.white, 0.06),
            with_alpha(p.white, 0.12),
            Color::TRANSPARENT,
            p.text_secondary,
            p.text_disabled,
            p.focus,
            None,
        ),
        // One step darker than the light look so white text keeps
        // AA contrast on every enabled status.
        destructive: variant(
            p.red.s600,
            p.red.s700,
            p.red.s800,
            mix(p.red.s600, background, 0.5),
            p.white,
            p.white,
            p.red.s800,
            None,
        ),
        ..light.button
    }
}

/// The dark look's `input` tokens; see [`Theme::dark`].
const fn dark_input(p: &Palette, light: &Theme) -> InputTheme {
    InputTheme {
        background: p.gray.s800,
        background_disabled: p.gray.s900,
        text: p.text_primary,
        placeholder: p.text_placeholder,
        border: p.gray.s600,
        border_error: p.red.s400,
        ring: p.focus,
        ring_error: with_alpha(p.red.s400, 0.4),
        label_text: p.text_secondary,
        hint_text: p.text_secondary,
        hint_text_error: p.red.s400,
        ..light.input
    }
}

/// The dark look's `card` tokens; see [`Theme::dark`].
const fn dark_card(p: &Palette, light: &Theme) -> CardTheme {
    CardTheme {
        background: p.gray.s800,
        header_shadow: Shadow {
            color: with_alpha(p.black, 0.4),
            offset: Vector::new(0.0, 1.0),
            blur_radius: 0.0,
        },
        card_shadow: Shadow {
            color: with_alpha(p.black, 0.8),
            offset: Vector::ZERO,
            blur_radius: 1.0,
        },
        halo_shadow: Shadow {
            color: with_alpha(p.black, 0.5),
            offset: Vector::new(0.0, 38.0),
            blur_radius: 90.0,
        },
        ..light.card
    }
}

#[allow(clippy::too_many_arguments)]
const fn variant(
    active: Color,
    hover: Color,
    pressed: Color,
    disabled: Color,
    text: Color,
    text_disabled: Color,
    ring: Color,
    shadow: Option<Shadow>,
) -> ButtonVariant {
    ButtonVariant {
        active: ButtonStatusColors {
            background: active,
            text,
        },
        hover: ButtonStatusColors {
            background: hover,
            text,
        },
        pressed: ButtonStatusColors {
            background: pressed,
            text,
        },
        disabled: ButtonStatusColors {
            background: disabled,
            text: text_disabled,
        },
        ring,
        shadow,
    }
}

impl Theme {
    /// The light look: [`Theme::light`] of [`Palette::LIGHT`].
    pub const LIGHT: Self = Self::light(Palette::LIGHT);

    /// The dark look: [`Theme::dark`] of [`Palette::DARK`]. A first pass:
    /// the same metrics as [`Theme::LIGHT`], surfaces from the dark end of
    /// the gray scale, and every text/background pair checked for contrast
    /// in the tests.
    pub const DARK: Self = Self::dark(Palette::DARK);

    /// Derives the light arrangement from `p`: white surfaces, fills from
    /// the light end of the gray scale, the palette's dark text tiers.
    #[must_use]
    pub const fn light(p: Palette) -> Self {
        let typography = TypographyTheme::DEFAULT;

        Self {
            name: "Luminate Light",
            mode: Mode::Light,
            background: p.white,
            divider: p.gray.s100,
            palette: p,
            typography,
            button: light_button(&p, &typography),
            input: light_input(&p, &typography),
            sidebar: light_sidebar(&p),
            card: light_card(&p, &typography),
            error_bubble: light_error_bubble(&p, &typography),
            scrollable: light_scrollable(&p),
        }
    }

    /// Derives the dark arrangement from `p`: the metrics of
    /// [`Theme::light`], surfaces from the dark end of the gray scale, and
    /// the palette's (light) text tiers.
    #[must_use]
    pub const fn dark(p: Palette) -> Self {
        let light = Self::light(p);
        let background = p.gray.s900;

        Self {
            name: "Luminate Dark",
            mode: Mode::Dark,
            background,
            divider: p.gray.s700,
            button: dark_button(&p, background, &light),
            // The field is one step lighter than the page and its border two,
            // so both read against the surface; a disabled field sinks back
            // to the page colour.
            input: dark_input(&p, &light),
            sidebar: SidebarTheme {
                background: p.gray.s800,
                hover_overlay: with_alpha(p.white, 0.08),
                icon: p.text_primary,
                edge_shadow: Some(with_alpha(p.white, 0.05)),
                ..light.sidebar
            },
            card: dark_card(&p, &light),
            error_bubble: ErrorBubbleTheme {
                background: p.red.s800,
                text: p.text_primary,
                ..light.error_bubble
            },
            scrollable: ScrollableTheme {
                rail: p.gray.s800,
                scroller: p.gray.s600,
                scroller_hover: p.gray.s500,
                scroller_dragged: p.accent,
                radius: 2.0,
            },
            ..light
        }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::LIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::palette::wcag::{contrast, over};

    fn finite(c: Color) -> bool {
        [c.r, c.g, c.b, c.a]
            .iter()
            .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
    }

    fn well_formed(t: &Theme) {
        for v in [
            &t.button.primary,
            &t.button.secondary,
            &t.button.tertiary,
            &t.button.destructive,
        ] {
            for c in [v.active, v.hover, v.pressed, v.disabled] {
                assert!(finite(c.background) && finite(c.text), "{}", t.name);
            }
            assert!(finite(v.ring));
            assert_ne!(
                v.pressed, v.hover,
                "{}: pressed must differ from hover",
                t.name
            );
        }
        assert!(t.sidebar.collapsed_size > 0.0 && t.sidebar.header_size > 0.0);
        assert!(t.card.width > 0.0);
        assert!(
            (t.card.header_height() - 58.0).abs() < 1e-6,
            "{}",
            t.card.header_height()
        );
    }

    /// Thresholds from decision D11.
    fn readable(t: &Theme) {
        let bg = t.background;
        let b = t.button;

        for (name, v) in [
            ("primary", b.primary),
            ("secondary", b.secondary),
            ("tertiary", b.tertiary),
            ("destructive", b.destructive),
        ] {
            // The accent-filled primary keeps AA (4.5) on every enabled
            // status; the others meet the UI-component minimum (3.0).
            let enabled_min = if name == "primary" { 4.5 } else { 3.0 };
            for (status, colors, min) in [
                ("active", v.active, enabled_min),
                ("hover", v.hover, enabled_min),
                ("pressed", v.pressed, enabled_min),
                ("disabled", v.disabled, 1.8),
            ] {
                let fill = over(colors.background, bg);
                let ratio = contrast(colors.text, fill);
                assert!(
                    ratio >= min,
                    "{} {name} {status}: {ratio:.2} < {min}",
                    t.name
                );
            }
        }

        let i = t.input;
        let ratio = |fg, bg| contrast(fg, bg);
        assert_ne!(i.background, bg, "{}: field reads against the page", t.name);
        assert!(ratio(i.border, bg) >= 1.5, "{} input border", t.name);
        assert!(ratio(i.text, i.background) >= 4.5, "{} input value", t.name);
        assert!(
            ratio(i.placeholder, i.background) >= 3.0,
            "{} placeholder",
            t.name
        );
        assert!(
            ratio(t.palette.text_disabled, i.background_disabled) >= 1.8,
            "{} disabled input",
            t.name
        );
        for surface in [bg, t.card.background] {
            assert!(ratio(i.label_text, surface) >= 4.5, "{} label", t.name);
            assert!(ratio(i.hint_text, surface) >= 4.5, "{} hint", t.name);
            assert!(
                ratio(i.hint_text_error, surface) >= 3.0,
                "{} error hint",
                t.name
            );
            assert!(
                ratio(t.palette.text_primary, surface) >= 4.5,
                "{} text",
                t.name
            );
        }
        assert!(
            ratio(t.error_bubble.text, t.error_bubble.background) >= 4.5,
            "{} bubble",
            t.name
        );
        assert!(
            ratio(t.palette.text_primary, t.sidebar.background) >= 4.5,
            "{} sidebar",
            t.name
        );
    }

    #[test]
    fn the_light_theme_is_well_formed_and_readable() {
        well_formed(&Theme::LIGHT);
        readable(&Theme::LIGHT);
        assert_eq!(Theme::default(), Theme::LIGHT);
        assert_eq!(Theme::LIGHT.mode, Mode::Light);
    }

    #[test]
    fn the_dark_theme_is_well_formed_and_readable() {
        well_formed(&Theme::DARK);
        readable(&Theme::DARK);
        assert_eq!(Theme::DARK.mode, Mode::Dark);
        assert_ne!(Theme::DARK.background, Theme::LIGHT.background);
        // White on the dark destructive fills keeps AA too.
        let d = Theme::DARK.button.destructive;
        for colors in [d.active, d.hover, d.pressed] {
            let ratio = contrast(colors.text, over(colors.background, Theme::DARK.background));
            assert!(ratio >= 4.5, "dark destructive: {ratio:.2}");
        }
        assert_ne!(Theme::DARK.name, Theme::LIGHT.name);
    }

    #[test]
    fn a_custom_palette_changes_the_derived_tokens() {
        let palette = Palette {
            accent: Color::from_rgb8(200, 0, 120),
            ..Palette::LIGHT
        };
        let theme = Theme::light(palette);
        assert_eq!(theme.button.primary.active.background, palette.accent);
        assert_eq!(theme.scrollable.scroller_dragged, palette.accent);
        assert_eq!(theme.button.small, Theme::LIGHT.button.small);
    }

    /// A ring concentric with the control's corner would be
    /// `radius + ring_offset + ring_width`. The input's is exactly that;
    /// the button's is a pixel tighter, calibrated by eye. Both are written
    /// into the tokens rather than computed, so this test is what keeps the
    /// intent — and the deviation — from being refactored away.
    #[test]
    fn the_ring_radii_are_authored_not_derived() {
        let concentric = |radius: f32, offset: f32, width: f32| radius + offset + width;

        for theme in [Theme::LIGHT, Theme::DARK] {
            let i = theme.input;
            assert_eq!(
                i.ring_radius,
                concentric(i.radius, i.ring_offset, i.ring_width),
                "{}: the input's focus ring is concentric",
                theme.name
            );

            let b = theme.button;
            assert_eq!(b.ring_radius, 13.0, "{}", theme.name);
            assert_eq!(
                concentric(b.radius, b.ring_offset, b.ring_width) - b.ring_radius,
                1.0,
                "{}: the button's ring is deliberately a pixel tighter",
                theme.name
            );
        }
    }

    #[test]
    fn disabled_labels_use_the_disabled_text_tier() {
        let t = Theme::LIGHT;
        assert_eq!(t.button.secondary.disabled.text, t.palette.text_disabled);
        assert_eq!(t.button.tertiary.disabled.text, t.palette.text_disabled);
    }

    #[test]
    fn padding_follows_the_content() {
        let p = Theme::LIGHT.button.small;
        assert_eq!(p.for_content(&ButtonContent::Text("x")), p.text);
        assert_eq!(
            p.for_content(&ButtonContent::Icon(
                iced::advanced::svg::Handle::from_memory(b"<svg/>".as_slice())
            )),
            p.icon
        );
    }
}
