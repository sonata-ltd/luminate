//! What a palette exists to guarantee, checked through the public tokens:
//! scales get darker step by step and every text/background pair the kit
//! paints is legible (WCAG 2.x ratios).

use iced_luminate::iced::Color;
use iced_luminate::iced::theme::Base;
use iced_luminate::theme::palette::ColorScale;
use iced_luminate::theme::{ButtonVariant, Theme};

fn luminance(c: Color) -> f32 {
    let [r, g, b, _] = c.into_linear();
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Composites `fg` over `bg` (straight alpha).
fn over(fg: Color, bg: Color) -> Color {
    let a = fg.a.clamp(0.0, 1.0);
    Color::from_rgb(
        fg.r * a + bg.r * (1.0 - a),
        fg.g * a + bg.g * (1.0 - a),
        fg.b * a + bg.b * (1.0 - a),
    )
}

fn contrast(text: Color, background: Color, page: Color) -> f32 {
    let bg = over(background, page);
    let fg = over(text, bg);
    let (hi, lo) = {
        let (a, b) = (luminance(fg), luminance(bg));
        (a.max(b), a.min(b))
    };
    (hi + 0.05) / (lo + 0.05)
}

#[test]
fn both_scales_get_darker_at_every_step() {
    for (name, scale) in [("gray", &ColorScale::GRAY), ("red", &ColorScale::RED)] {
        let steps = scale.steps();
        assert_eq!(steps[0], scale.s25);
        assert_eq!(steps[12], scale.s900);
        for pair in steps.windows(2) {
            assert!(
                luminance(pair[0]) > luminance(pair[1]),
                "{name}: {:?} is not darker than {:?}",
                pair[1],
                pair[0]
            );
        }
    }
}

/// Enabled statuses meet AA (4.5) on the accent-filled primary and the
/// UI-component minimum (3.0) elsewhere; disabled controls are exempt from
/// WCAG but must still be discernible (1.8, the kit's disabled text tier).
fn check_variant(theme_name: &str, name: &str, variant: &ButtonVariant, page: Color) {
    let enabled = if name == "primary" { 4.5 } else { 3.0 };
    let states = [
        ("active", &variant.active, enabled),
        ("hover", &variant.hover, enabled),
        ("pressed", &variant.pressed, enabled),
        ("disabled", &variant.disabled, 1.8),
    ];
    for (state, colours, minimum) in states {
        let ratio = contrast(colours.text, colours.background, page);
        assert!(
            ratio >= minimum,
            "{theme_name} {name} {state}: contrast {ratio:.2} < {minimum} ({:?} on {:?})",
            colours.text,
            colours.background
        );
    }
}

#[test]
fn every_button_state_is_legible() {
    for (theme_name, theme) in [("LIGHT", Theme::LIGHT), ("DARK", Theme::DARK)] {
        let page = theme.base().background_color;
        check_variant(theme_name, "primary", &theme.button.primary, page);
        check_variant(theme_name, "secondary", &theme.button.secondary, page);
        check_variant(theme_name, "tertiary", &theme.button.tertiary, page);
        check_variant(theme_name, "destructive", &theme.button.destructive, page);
    }
}

#[test]
fn input_text_placeholder_label_and_hint_are_legible() {
    for (theme_name, theme) in [("LIGHT", Theme::LIGHT), ("DARK", Theme::DARK)] {
        let page = theme.base().background_color;
        let input = &theme.input;
        let text = contrast(input.text, input.background, page);
        let placeholder = contrast(input.placeholder, input.background, page);
        let label = contrast(input.label_text, page, page);
        let hint = contrast(input.hint_text, page, page);
        assert!(text >= 4.5, "{theme_name} input text: {text:.2}");
        assert!(
            placeholder >= 3.0,
            "{theme_name} input placeholder: {placeholder:.2}"
        );
        assert!(label >= 4.5, "{theme_name} input label: {label:.2}");
        assert!(hint >= 4.5, "{theme_name} input hint: {hint:.2}");
    }
}

#[test]
fn page_text_reads_on_every_surface() {
    for (theme_name, theme) in [("LIGHT", Theme::LIGHT), ("DARK", Theme::DARK)] {
        let page = theme.base().background_color;
        let text = theme.base().text_color;
        for (surface_name, surface) in [
            ("page", page),
            ("card", theme.card.background),
            ("sidebar", theme.sidebar.background),
            ("header", theme.header.background),
        ] {
            let ratio = contrast(text, surface, page);
            assert!(
                ratio >= 4.5,
                "{theme_name} text on {surface_name}: {ratio:.2}"
            );
        }
        let bubble = contrast(theme.error_bubble.text, theme.error_bubble.background, page);
        assert!(bubble >= 4.5, "{theme_name} error bubble: {bubble:.2}");
    }
}
