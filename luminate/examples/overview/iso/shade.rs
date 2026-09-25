//! Colour. One tone per shape, taken from the theme so the icon set follows
//! light and dark.
//!
//! There is no shading: the icons are flat shapes seen through an isometric
//! camera, and depth is carried by position and overlap rather than by light.

use iced_luminate::Theme;
use iced_luminate::iced::Color;
use iced_luminate::iced::theme::Mode;
use iced_luminate::theme::palette::with_alpha;

/// The colour role of a shape.
///
/// The colour behind a role depends on the theme's mode, because both
/// palettes share one gray scale: a mid-gray that reads on a white page
/// disappears on a dark one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Tone {
    /// The body of an icon.
    Neutral,
    /// Supporting shapes, a step back from [`Tone::Neutral`].
    Muted,
    /// The one shape an icon leads with.
    Accent,
    /// Custom defined color
    Custom(Color),
}

impl Tone {
    /// The colour of this role under `theme`, at full opacity.
    pub(crate) fn base(self, theme: &Theme) -> Color {
        let palette = &theme.palette;

        match self {
            Self::Accent => palette.accent,
            Self::Neutral => match theme.mode {
                Mode::Dark => palette.gray.s300,
                Mode::Light | Mode::None => palette.gray.s500,
            },
            Self::Muted => match theme.mode {
                Mode::Dark => palette.gray.s600,
                Mode::Light | Mode::None => palette.gray.s300,
            },
            Self::Custom(color) => color,
        }
    }

    /// The colour of this role, its alpha scaled by `alpha`.
    pub(crate) fn color(self, theme: &Theme, alpha: f32) -> Color {
        let base = self.base(theme);

        with_alpha(base, base.a * alpha)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_neutral_base_follows_the_mode() {
        assert_ne!(
            Tone::Neutral.base(&Theme::LIGHT),
            Tone::Neutral.base(&Theme::DARK)
        );
    }

    #[test]
    fn the_muted_base_follows_the_mode() {
        assert_ne!(
            Tone::Muted.base(&Theme::LIGHT),
            Tone::Muted.base(&Theme::DARK)
        );
    }

    #[test]
    fn the_accent_is_the_palette_accent_in_both_modes() {
        assert_eq!(
            Tone::Accent.base(&Theme::LIGHT),
            Theme::LIGHT.palette.accent
        );
        assert_eq!(Tone::Accent.base(&Theme::DARK), Theme::DARK.palette.accent);
    }

    #[test]
    fn muted_and_neutral_stay_apart_in_both_modes() {
        for theme in [&Theme::LIGHT, &Theme::DARK] {
            assert_ne!(Tone::Neutral.base(theme), Tone::Muted.base(theme));
        }
    }

    #[test]
    fn alpha_scales_the_colour() {
        assert_eq!(Tone::Neutral.color(&Theme::DARK, 0.5).a, 0.5);
        assert_eq!(Tone::Neutral.color(&Theme::DARK, 1.0).a, 1.0);
    }
}
