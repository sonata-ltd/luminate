//! Colours: the two tint scales and the [`Palette`] a theme is derived from.

use iced::Color;

/// A thirteen-step tint scale from the lightest tint (`s25`) to the darkest
/// shade (`s900`). Relative luminance strictly decreases along the scale
/// (tested).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ColorScale {
    /// Step 25, the lightest tint.
    pub s25: Color,
    /// Step 30.
    pub s30: Color,
    /// Step 40.
    pub s40: Color,
    /// Step 50.
    pub s50: Color,
    /// Step 100.
    pub s100: Color,
    /// Step 200.
    pub s200: Color,
    /// Step 300.
    pub s300: Color,
    /// Step 400.
    pub s400: Color,
    /// Step 500.
    pub s500: Color,
    /// Step 600.
    pub s600: Color,
    /// Step 700.
    pub s700: Color,
    /// Step 800.
    pub s800: Color,
    /// Step 900, the darkest shade.
    pub s900: Color,
}

impl ColorScale {
    /// The neutral scale.
    pub const GRAY: Self = Self {
        s25: Color::from_rgb8(253, 253, 253),
        s30: Color::from_rgb8(250, 250, 250),
        s40: Color::from_rgb8(240, 240, 240),
        s50: Color::from_rgb8(230, 231, 232),
        s100: Color::from_rgb8(207, 209, 210),
        s200: Color::from_rgb8(185, 186, 189),
        s300: Color::from_rgb8(162, 164, 167),
        s400: Color::from_rgb8(139, 142, 146),
        s500: Color::from_rgb8(116, 120, 124),
        s600: Color::from_rgb8(93, 98, 103),
        s700: Color::from_rgb8(71, 75, 81),
        s800: Color::from_rgb8(48, 53, 60),
        s900: Color::from_rgb8(25, 31, 38),
    };

    /// The error scale. `s30`/`s40` are interpolated between `s25` and
    /// `s50`.
    pub const RED: Self = Self {
        s25: Color::from_rgb8(255, 251, 250),
        s30: Color::from_rgb8(255, 249, 248),
        s40: Color::from_rgb8(254, 246, 245),
        s50: Color::from_rgb8(254, 243, 242),
        s100: Color::from_rgb8(254, 228, 226),
        s200: Color::from_rgb8(253, 195, 190),
        s300: Color::from_rgb8(253, 162, 155),
        s400: Color::from_rgb8(246, 115, 105),
        s500: Color::from_rgb8(240, 68, 56),
        s600: Color::from_rgb8(210, 60, 48),
        s700: Color::from_rgb8(181, 53, 41),
        s800: Color::from_rgb8(151, 46, 33),
        s900: Color::from_rgb8(122, 39, 26),
    };

    /// The steps, lightest first.
    #[must_use]
    pub const fn steps(&self) -> [Color; 13] {
        [
            self.s25, self.s30, self.s40, self.s50, self.s100, self.s200, self.s300, self.s400,
            self.s500, self.s600, self.s700, self.s800, self.s900,
        ]
    }
}

/// The colours a [`Theme`](crate::theme::Theme) is built from.
///
/// Every field is read by at least one token: pass a custom palette to
/// [`Theme::light`](crate::theme::Theme::light) or
/// [`Theme::dark`](crate::theme::Theme::dark) to re-derive them all.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    /// Brand accent; fills primary buttons and the dragged scroller.
    pub accent: Color,
    /// Translucent accent used for focus rings and text selection.
    pub focus: Color,
    /// Pure white.
    pub white: Color,
    /// Pure black.
    pub black: Color,
    /// Strongest text colour: primary content, headers, typed input.
    pub text_primary: Color,
    /// Secondary text: secondary buttons, input labels and hints.
    pub text_secondary: Color,
    /// Tertiary text: tertiary buttons, input labels and hints.
    pub text_tertiary: Color,
    /// Quaternary text: quaternary buttons, input labels and hints.
    pub text_quaternary: Color,
    /// Text of disabled controls.
    pub text_disabled: Color,
    /// Input placeholder text.
    pub text_placeholder: Color,
    /// The neutral scale.
    pub gray: ColorScale,
    /// The error scale.
    pub red: ColorScale,
}

impl Palette {
    /// The light palette: dark text on light surfaces.
    pub const LIGHT: Self = Self {
        accent: Color::from_rgb8(0, 108, 255),
        focus: Color::from_rgba8(0, 108, 255, 0.25),
        white: Color::WHITE,
        black: Color::BLACK,
        text_primary: ColorScale::GRAY.s900,
        text_secondary: ColorScale::GRAY.s700,
        text_tertiary: ColorScale::GRAY.s600,
        text_quaternary: ColorScale::GRAY.s500,
        text_disabled: ColorScale::GRAY.s300,
        text_placeholder: ColorScale::GRAY.s400,
        gray: ColorScale::GRAY,
        red: ColorScale::RED,
    };

    /// The dark palette: light text on dark surfaces. A first pass, the
    /// accent is the light one (white on it keeps a 4.5:1 contrast) and the
    /// text tiers come from the light end of the gray scale.
    pub const DARK: Self = Self {
        accent: Color::from_rgb8(0, 108, 255),
        focus: Color::from_rgba8(0, 108, 255, 0.35),
        white: Color::WHITE,
        black: Color::BLACK,
        text_primary: ColorScale::GRAY.s25,
        text_secondary: ColorScale::GRAY.s200,
        text_tertiary: ColorScale::GRAY.s600,
        text_quaternary: ColorScale::GRAY.s500,
        text_disabled: ColorScale::GRAY.s600,
        text_placeholder: ColorScale::GRAY.s400,
        gray: ColorScale::GRAY,
        red: ColorScale::RED,
    };
}

/// Linear interpolation between two colours in gamma-encoded sRGB (what
/// `iced::Color` holds), the same mix CSS `color-mix(in srgb)` does, which
/// is right for design tokens but is not linear-light blending. `factor` is
/// clamped to `0.0..=1.0`.
#[must_use]
pub const fn mix(base: Color, overlay: Color, factor: f32) -> Color {
    let factor = if factor.is_nan() {
        0.0
    } else {
        factor.clamp(0.0, 1.0)
    };

    Color {
        r: base.r + (overlay.r - base.r) * factor,
        g: base.g + (overlay.g - base.g) * factor,
        b: base.b + (overlay.b - base.b) * factor,
        a: base.a + (overlay.a - base.a) * factor,
    }
}

/// `color` with its alpha replaced.
#[must_use]
pub const fn with_alpha(color: Color, alpha: f32) -> Color {
    Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: alpha,
    }
}

/// WCAG helpers shared by the palette and token tests.
#[cfg(test)]
pub(crate) mod wcag {
    use iced::Color;

    /// WCAG 2 relative luminance of an opaque colour.
    pub(crate) fn luminance(color: Color) -> f32 {
        let [r, g, b, _] = color.into_linear();
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// `fg` composited over the opaque `bg`.
    pub(crate) fn over(fg: Color, bg: Color) -> Color {
        Color {
            r: bg.r + (fg.r - bg.r) * fg.a,
            g: bg.g + (fg.g - bg.g) * fg.a,
            b: bg.b + (fg.b - bg.b) * fg.a,
            a: 1.0,
        }
    }

    /// WCAG 2 contrast ratio of `fg` (possibly translucent) over `bg`.
    pub(crate) fn contrast(fg: Color, bg: Color) -> f32 {
        let (l1, l2) = (luminance(over(fg, bg)), luminance(bg));
        let (hi, lo) = if l1 > l2 { (l1, l2) } else { (l2, l1) };
        (hi + 0.05) / (lo + 0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::wcag::{contrast, luminance};
    use super::*;

    #[test]
    fn mix_interpolates_and_clamps() {
        let half = mix(Color::BLACK, Color::WHITE, 0.5);
        assert!((half.r - 0.5).abs() < 1e-6 && (half.g - 0.5).abs() < 1e-6);
        assert_eq!(mix(Color::BLACK, Color::WHITE, 2.0), Color::WHITE);
        assert_eq!(mix(Color::BLACK, Color::WHITE, -1.0), Color::BLACK);
        assert_eq!(mix(Color::BLACK, Color::WHITE, f32::NAN), Color::BLACK);
    }

    #[test]
    fn with_alpha_keeps_the_channels() {
        let c = with_alpha(Color::from_rgb8(10, 20, 30), 0.5);
        assert_eq!((c.r, c.g, c.b), (10.0 / 255.0, 20.0 / 255.0, 30.0 / 255.0));
        assert_eq!(c.a, 0.5);
    }

    #[test]
    fn both_scales_get_strictly_darker() {
        for (name, scale) in [("gray", ColorScale::GRAY), ("red", ColorScale::RED)] {
            let steps = scale.steps();
            for (i, pair) in steps.windows(2).enumerate() {
                assert!(
                    luminance(pair[0]) > luminance(pair[1]),
                    "{name} step {i} -> {}: {:?} is not darker than {:?}",
                    i + 1,
                    pair[1],
                    pair[0]
                );
            }
        }
    }

    #[test]
    fn contrast_is_the_wcag_ratio() {
        assert!((contrast(Color::BLACK, Color::WHITE) - 21.0).abs() < 1e-3);
        assert!((contrast(Color::WHITE, Color::WHITE) - 1.0).abs() < 1e-6);
        // Translucent black over white: composited first, so less than 21.
        let half = contrast(with_alpha(Color::BLACK, 0.5), Color::WHITE);
        assert!(half > 3.0 && half < 21.0, "{half}");
    }

    #[test]
    fn palette_text_tiers_read_on_their_surface() {
        for (palette, surface) in [
            (Palette::LIGHT, Color::WHITE),
            (Palette::DARK, ColorScale::GRAY.s900),
        ] {
            assert!(contrast(palette.text_primary, surface) >= 4.5);
            assert!(contrast(palette.text_secondary, surface) >= 4.5);
            assert!(contrast(palette.text_placeholder, surface) >= 3.0);
            assert!(contrast(palette.text_disabled, surface) >= 1.8);
            assert!(contrast(palette.white, palette.accent) >= 4.5);
        }
    }
}
