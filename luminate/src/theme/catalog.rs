//! [`Theme`] as an iced theme.
//!
//! [`Base`] makes it usable as the application theme; the `Catalog` impls
//! let every widget the kit uses (`button`, `text_input`, `container`,
//! `text`, `svg`, `scrollable`, `rule`, and the three kit widgets) and every
//! other stock widget an application may put inside a Luminate element
//! (`checkbox`, `toggler`, `slider`, `radio`, `pick_list`, `overlay::menu`,
//! `progress_bar`, `text_editor`, `combo_box`) style itself from the tokens. Each iced widget gets a class enum
//! ([`ButtonClass`], [`InputClass`], …) naming the looks Luminate draws, plus
//! a `Custom` variant so `.style(|theme, status| …)` closures keep working.
//! The kit widgets keep a boxed style function as their class, as phase 7's
//! `catalog!` macro expects.

use std::fmt;

use iced::border::Radius;
use iced::overlay::menu;
use iced::theme::{self, Base, Mode};
use iced::widget::{
    button, checkbox, combo_box, container, pick_list, progress_bar, radio, rule, scrollable,
    slider, svg, text, text_editor, text_input, toggler,
};
use iced::{Background, Border, Color, Shadow, Vector, border};

use crate::descriptor::ButtonHierarchy;
use crate::theme::Theme;
use crate::theme::palette::mix;
use crate::widget::{error_bubble, multi_border, sidebar};

// --- Base ---------------------------------------------------------------

impl Base for Theme {
    /// [`Theme::DARK`] for [`Mode::Dark`], [`Theme::LIGHT`] otherwise.
    fn default(preference: Mode) -> Self {
        match preference {
            Mode::Dark => Self::DARK,
            Mode::None | Mode::Light => Self::LIGHT,
        }
    }

    fn mode(&self) -> Mode {
        self.mode
    }

    fn base(&self) -> theme::Style {
        theme::Style {
            background_color: self.background,
            text_color: self.palette.text_primary,
        }
    }

    /// iced's six-colour palette, for the runtime's own overlays. Luminate
    /// has no success/warning colours: `success` is the accent and
    /// `warning` is `red.s400`.
    fn palette(&self) -> Option<theme::Palette> {
        Some(theme::Palette {
            background: self.background,
            text: self.palette.text_primary,
            primary: self.palette.accent,
            success: self.palette.accent,
            warning: self.palette.red.s400,
            danger: self.palette.red.s500,
        })
    }

    fn name(&self) -> &str {
        self.name
    }
}

// --- button -------------------------------------------------------------

/// How a `button` is styled by [`Theme`].
pub enum ButtonClass<'a> {
    /// One of the four Luminate hierarchies.
    Hierarchy(ButtonHierarchy),
    /// A style function.
    Custom(button::StyleFn<'a, Theme>),
}

impl<'a> From<button::StyleFn<'a, Theme>> for ButtonClass<'a> {
    fn from(f: button::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for ButtonClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hierarchy(h) => f.debug_tuple("Hierarchy").field(h).finish(),
            Self::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

fn button_style(
    theme: &Theme,
    hierarchy: ButtonHierarchy,
    status: button::Status,
) -> button::Style {
    let tokens = theme.button;
    let variant = tokens.variant(hierarchy);

    let colors = match status {
        button::Status::Active => variant.active,
        button::Status::Hovered => variant.hover,
        button::Status::Pressed => variant.pressed,
        button::Status::Disabled => variant.disabled,
    };

    let shadow = match (variant.shadow, status) {
        (
            Some(shadow),
            button::Status::Active | button::Status::Hovered | button::Status::Pressed,
        ) => shadow,
        _ => Shadow::default(),
    };

    button::Style {
        background: Some(Background::Color(colors.background)),
        text_color: colors.text,
        border: border::rounded(tokens.radius),
        shadow,
        ..button::Style::default()
    }
}

impl button::Catalog for Theme {
    type Class<'a> = ButtonClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        ButtonClass::Hierarchy(ButtonHierarchy::Primary)
    }

    fn style(&self, class: &Self::Class<'_>, status: button::Status) -> button::Style {
        match class {
            ButtonClass::Hierarchy(hierarchy) => button_style(self, *hierarchy, status),
            ButtonClass::Custom(f) => f(self, status),
        }
    }
}

// --- text_input ---------------------------------------------------------

/// How a `text_input` is styled by [`Theme`].
///
/// A hovered field deliberately looks like an active one: the input tokens
/// carry no hover colour, and the focus ring is the only state cue.
pub enum InputClass<'a> {
    /// The normal look.
    Normal,
    /// The error look: red border, red focus ring.
    Error,
    /// A style function.
    Custom(text_input::StyleFn<'a, Theme>),
}

impl<'a> From<text_input::StyleFn<'a, Theme>> for InputClass<'a> {
    fn from(f: text_input::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for InputClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Normal => "Normal",
            Self::Error => "Error",
            Self::Custom(_) => "Custom(..)",
        })
    }
}

fn input_style(theme: &Theme, is_error: bool, status: text_input::Status) -> text_input::Style {
    let tokens = theme.input;
    let is_disabled = matches!(status, text_input::Status::Disabled);

    // `Disabled` is matched first: a field nobody can type into states that
    // it is inert, not that its contents are wrong, so it keeps the plain
    // border even while its value is in error.
    let (border_color, border_width) = match (status, is_error) {
        (text_input::Status::Disabled, _) => (tokens.border, 1.0),
        (text_input::Status::Focused { .. }, false) => (Color::TRANSPARENT, 0.0),
        (_, true) => (tokens.border_error, 1.0),
        (_, false) => (tokens.border, 1.0),
    };

    text_input::Style {
        background: Background::Color(if is_disabled {
            tokens.background_disabled
        } else {
            tokens.background
        }),
        border: Border {
            color: border_color,
            width: border_width,
            radius: tokens.radius.into(),
        },
        icon: tokens.text,
        placeholder: tokens.placeholder,
        value: if is_disabled {
            theme.palette.text_disabled
        } else {
            tokens.text
        },
        selection: tokens.ring,
    }
}

impl text_input::Catalog for Theme {
    type Class<'a> = InputClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        InputClass::Normal
    }

    fn style(&self, class: &Self::Class<'_>, status: text_input::Status) -> text_input::Style {
        match class {
            InputClass::Normal => input_style(self, false, status),
            InputClass::Error => input_style(self, true, status),
            InputClass::Custom(f) => f(self, status),
        }
    }
}

// --- container ----------------------------------------------------------

/// How a `container` is styled by [`Theme`].
pub enum ContainerClass<'a> {
    /// No fill, no border (the default).
    Transparent,
    /// The application surface: [`Theme::background`] with primary text.
    Surface,
    /// A card: the card fill, its corner radius and its tight outline
    /// shadow.
    Card,
    /// A card header: the card fill, the card radius on the top corners
    /// only, and the header shadow.
    CardHeader,
    /// The wide halo shadow beneath a card (no fill).
    CardHalo,
    /// A style function.
    Custom(container::StyleFn<'a, Theme>),
}

impl<'a> From<container::StyleFn<'a, Theme>> for ContainerClass<'a> {
    fn from(f: container::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for ContainerClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Transparent => "Transparent",
            Self::Surface => "Surface",
            Self::Card => "Card",
            Self::CardHeader => "CardHeader",
            Self::CardHalo => "CardHalo",
            Self::Custom(_) => "Custom(..)",
        })
    }
}

impl container::Catalog for Theme {
    type Class<'a> = ContainerClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        ContainerClass::Transparent
    }

    fn style(&self, class: &Self::Class<'_>) -> container::Style {
        let card = self.card;
        let text = Some(self.palette.text_primary);

        match class {
            ContainerClass::Transparent => container::Style::default(),
            ContainerClass::Surface => container::Style {
                text_color: text,
                background: Some(Background::Color(self.background)),
                ..container::Style::default()
            },
            ContainerClass::Card => container::Style {
                text_color: text,
                background: Some(Background::Color(card.background)),
                border: border::rounded(card.radius),
                shadow: card.card_shadow,
                ..container::Style::default()
            },
            ContainerClass::CardHeader => container::Style {
                text_color: text,
                background: Some(Background::Color(card.background)),
                border: Border {
                    radius: Radius {
                        top_left: card.radius,
                        top_right: card.radius,
                        bottom_right: 0.0,
                        bottom_left: 0.0,
                    },
                    ..Border::default()
                },
                shadow: card.header_shadow,
                ..container::Style::default()
            },
            ContainerClass::CardHalo => container::Style {
                border: border::rounded(card.radius),
                shadow: card.halo_shadow,
                ..container::Style::default()
            },
            ContainerClass::Custom(f) => f(self),
        }
    }
}

// --- text ---------------------------------------------------------------

/// How a `text` is styled by [`Theme`].
pub enum TextClass<'a> {
    /// Inherits the surrounding colour (the default).
    Inherit,
    /// The caption above a Luminate input.
    Label,
    /// The hint below a Luminate input.
    Hint,
    /// The hint below a Luminate input in the error state.
    HintError,
    /// A style function.
    Custom(text::StyleFn<'a, Theme>),
}

impl<'a> From<text::StyleFn<'a, Theme>> for TextClass<'a> {
    fn from(f: text::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for TextClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Inherit => "Inherit",
            Self::Label => "Label",
            Self::Hint => "Hint",
            Self::HintError => "HintError",
            Self::Custom(_) => "Custom(..)",
        })
    }
}

impl text::Catalog for Theme {
    type Class<'a> = TextClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        TextClass::Inherit
    }

    fn style(&self, class: &Self::Class<'_>) -> text::Style {
        let color = match class {
            TextClass::Inherit => None,
            TextClass::Label => Some(self.input.label_text),
            TextClass::Hint => Some(self.input.hint_text),
            TextClass::HintError => Some(self.input.hint_text_error),
            TextClass::Custom(f) => return f(self),
        };

        text::Style { color }
    }
}

// --- svg ----------------------------------------------------------------

/// How an `svg` is styled by [`Theme`].
pub enum SvgClass<'a> {
    /// The image's own colours (the default).
    Original,
    /// Every shape painted in one colour.
    Tint(Color),
    /// The icon of a Luminate button: painted in the label colour of the
    /// hierarchy, resolved per status like the label itself (a hovered icon
    /// takes the hover text colour, a disabled one the disabled text colour).
    ButtonIcon {
        /// The button's hierarchy.
        hierarchy: ButtonHierarchy,
        /// Whether the button is disabled.
        disabled: bool,
    },
    /// A style function.
    Custom(svg::StyleFn<'a, Theme>),
}

impl<'a> From<svg::StyleFn<'a, Theme>> for SvgClass<'a> {
    fn from(f: svg::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for SvgClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Original => f.write_str("Original"),
            Self::Tint(color) => f.debug_tuple("Tint").field(color).finish(),
            Self::ButtonIcon {
                hierarchy,
                disabled,
            } => f
                .debug_struct("ButtonIcon")
                .field("hierarchy", hierarchy)
                .field("disabled", disabled)
                .finish(),
            Self::Custom(_) => f.write_str("Custom(..)"),
        }
    }
}

impl svg::Catalog for Theme {
    type Class<'a> = SvgClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        SvgClass::Original
    }

    fn style(&self, class: &Self::Class<'_>, status: svg::Status) -> svg::Style {
        match class {
            SvgClass::Original => svg::Style::default(),
            SvgClass::Tint(color) => svg::Style {
                color: Some(*color),
            },
            SvgClass::ButtonIcon {
                hierarchy,
                disabled,
            } => {
                let variant = self.button.variant(*hierarchy);
                let colors = match (disabled, status) {
                    (true, _) => variant.disabled,
                    (false, svg::Status::Idle) => variant.active,
                    (false, svg::Status::Hovered) => variant.hover,
                };
                svg::Style {
                    color: Some(colors.text),
                }
            }
            SvgClass::Custom(f) => f(self, status),
        }
    }
}

// --- scrollable ---------------------------------------------------------

/// How a `scrollable` is styled by [`Theme`].
pub enum ScrollableClass<'a> {
    /// The Luminate bars (the default).
    Standard,
    /// A style function.
    Custom(scrollable::StyleFn<'a, Theme>),
}

impl<'a> From<scrollable::StyleFn<'a, Theme>> for ScrollableClass<'a> {
    fn from(f: scrollable::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for ScrollableClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Standard => "Standard",
            Self::Custom(_) => "Custom(..)",
        })
    }
}

fn scrollable_style(theme: &Theme, status: scrollable::Status) -> scrollable::Style {
    let tokens = theme.scrollable;

    let rail = |scroller: Color| scrollable::Rail {
        background: Some(Background::Color(tokens.rail)),
        border: border::rounded(tokens.radius),
        scroller: scrollable::Scroller {
            background: Background::Color(scroller),
            border: border::rounded(tokens.radius),
        },
    };

    let auto_scroll = scrollable::AutoScroll {
        background: Background::Color(theme.background.scale_alpha(0.9)),
        border: border::rounded(u32::MAX)
            .width(1)
            .color(theme.palette.text_primary.scale_alpha(0.8)),
        shadow: Shadow {
            color: Color::BLACK.scale_alpha(0.7),
            offset: Vector::ZERO,
            blur_radius: 2.0,
        },
        icon: theme.palette.text_primary.scale_alpha(0.8),
    };

    let (vertical, horizontal) = match status {
        scrollable::Status::Active { .. } => (tokens.scroller, tokens.scroller),
        scrollable::Status::Hovered {
            is_horizontal_scrollbar_hovered,
            is_vertical_scrollbar_hovered,
            ..
        } => (
            if is_vertical_scrollbar_hovered {
                tokens.scroller_hover
            } else {
                tokens.scroller
            },
            if is_horizontal_scrollbar_hovered {
                tokens.scroller_hover
            } else {
                tokens.scroller
            },
        ),
        scrollable::Status::Dragged {
            is_horizontal_scrollbar_dragged,
            is_vertical_scrollbar_dragged,
            ..
        } => (
            if is_vertical_scrollbar_dragged {
                tokens.scroller_dragged
            } else {
                tokens.scroller
            },
            if is_horizontal_scrollbar_dragged {
                tokens.scroller_dragged
            } else {
                tokens.scroller
            },
        ),
    };

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail(vertical),
        horizontal_rail: rail(horizontal),
        gap: None,
        auto_scroll,
    }
}

impl scrollable::Catalog for Theme {
    type Class<'a> = ScrollableClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        ScrollableClass::Standard
    }

    fn style(&self, class: &Self::Class<'_>, status: scrollable::Status) -> scrollable::Style {
        match class {
            ScrollableClass::Standard => scrollable_style(self, status),
            ScrollableClass::Custom(f) => f(self, status),
        }
    }
}

// --- rule ---------------------------------------------------------------

/// How a `rule` is styled by [`Theme`].
pub enum RuleClass<'a> {
    /// A full-length line in [`Theme::divider`] (the default).
    Standard,
    /// A style function.
    Custom(rule::StyleFn<'a, Theme>),
}

impl<'a> From<rule::StyleFn<'a, Theme>> for RuleClass<'a> {
    fn from(f: rule::StyleFn<'a, Theme>) -> Self {
        Self::Custom(f)
    }
}

impl fmt::Debug for RuleClass<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Standard => "Standard",
            Self::Custom(_) => "Custom(..)",
        })
    }
}

impl rule::Catalog for Theme {
    type Class<'a> = RuleClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        RuleClass::Standard
    }

    fn style(&self, class: &Self::Class<'_>) -> rule::Style {
        match class {
            RuleClass::Standard => rule::Style {
                color: self.divider,
                radius: 0.0.into(),
                fill_mode: rule::FillMode::Full,
                snap: true,
            },
            RuleClass::Custom(f) => f(self),
        }
    }
}

// --- stock widgets ------------------------------------------------------
//
// Widgets the kit does not build itself but an application may put inside a
// Luminate element. Each has a `Default` look derived from the tokens and a
// `Custom` style function.

macro_rules! stock_class {
    ($(#[$meta:meta])* $name:ident, $widget:ident) => {
        $(#[$meta])*
        pub enum $name<'a> {
            /// The Luminate look (the default).
            Default,
            /// A style function.
            Custom($widget::StyleFn<'a, Theme>),
        }

        impl<'a> From<$widget::StyleFn<'a, Theme>> for $name<'a> {
            fn from(f: $widget::StyleFn<'a, Theme>) -> Self {
                Self::Custom(f)
            }
        }

        impl fmt::Debug for $name<'_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(match self {
                    Self::Default => "Default",
                    Self::Custom(_) => "Custom(..)",
                })
            }
        }
    };
}

stock_class!(
    /// How a `checkbox` is styled by [`Theme`].
    CheckboxClass,
    checkbox
);
stock_class!(
    /// How a `toggler` is styled by [`Theme`].
    TogglerClass,
    toggler
);
stock_class!(
    /// How a `slider` (and `vertical_slider`) is styled by [`Theme`].
    SliderClass,
    slider
);
stock_class!(
    /// How a `radio` is styled by [`Theme`].
    RadioClass,
    radio
);
stock_class!(
    /// How a `pick_list` is styled by [`Theme`].
    PickListClass,
    pick_list
);
stock_class!(
    /// How an `overlay::menu` (the drop-down of a `pick_list` or
    /// `combo_box`) is styled by [`Theme`].
    MenuClass,
    menu
);
stock_class!(
    /// How a `progress_bar` is styled by [`Theme`].
    ProgressBarClass,
    progress_bar
);
stock_class!(
    /// How a `text_editor` is styled by [`Theme`].
    TextEditorClass,
    text_editor
);

/// A field's hovered fill: the field colour nudged toward the text colour.
fn hovered(theme: &Theme, background: Color) -> Color {
    mix(background, theme.palette.text_primary, 0.06)
}

fn checkbox_style(theme: &Theme, status: checkbox::Status) -> checkbox::Style {
    let input = theme.input;
    let primary = theme.button.primary;

    let (background, icon_color, border_color) = match status {
        checkbox::Status::Active { is_checked: true } => (
            primary.active.background,
            primary.active.text,
            primary.active.background,
        ),
        checkbox::Status::Hovered { is_checked: true } => (
            primary.hover.background,
            primary.hover.text,
            primary.hover.background,
        ),
        checkbox::Status::Active { is_checked: false } => {
            (input.background, input.background, input.border)
        }
        checkbox::Status::Hovered { is_checked: false } => (
            hovered(theme, input.background),
            input.background,
            theme.palette.accent,
        ),
        checkbox::Status::Disabled { is_checked } => (
            input.background_disabled,
            if is_checked {
                theme.palette.text_disabled
            } else {
                input.background_disabled
            },
            input.border,
        ),
    };

    checkbox::Style {
        background: Background::Color(background),
        icon_color,
        border: Border {
            color: border_color,
            width: 1.0,
            radius: 4.0.into(),
        },
        text_color: None,
    }
}

impl checkbox::Catalog for Theme {
    type Class<'a> = CheckboxClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        CheckboxClass::Default
    }

    fn style(&self, class: &Self::Class<'_>, status: checkbox::Status) -> checkbox::Style {
        match class {
            CheckboxClass::Default => checkbox_style(self, status),
            CheckboxClass::Custom(f) => f(self, status),
        }
    }
}

fn toggler_style(theme: &Theme, status: toggler::Status) -> toggler::Style {
    let input = theme.input;
    let primary = theme.button.primary;

    let (background, foreground) = match status {
        toggler::Status::Active { is_toggled: true } => {
            (primary.active.background, primary.active.text)
        }
        toggler::Status::Hovered { is_toggled: true } => {
            (primary.hover.background, primary.hover.text)
        }
        toggler::Status::Active { is_toggled: false } => (input.border, theme.palette.white),
        toggler::Status::Hovered { is_toggled: false } => {
            (hovered(theme, input.border), theme.palette.white)
        }
        toggler::Status::Disabled { is_toggled } => (
            if is_toggled {
                primary.disabled.background
            } else {
                input.background_disabled
            },
            theme.palette.text_disabled,
        ),
    };

    toggler::Style {
        background: Background::Color(background),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground: Background::Color(foreground),
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: None,
        border_radius: None,
        padding_ratio: 0.1,
    }
}

impl toggler::Catalog for Theme {
    type Class<'a> = TogglerClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        TogglerClass::Default
    }

    fn style(&self, class: &Self::Class<'_>, status: toggler::Status) -> toggler::Style {
        match class {
            TogglerClass::Default => toggler_style(self, status),
            TogglerClass::Custom(f) => f(self, status),
        }
    }
}

fn slider_style(theme: &Theme, status: slider::Status) -> slider::Style {
    let accent = theme.palette.accent;
    let primary = theme.button.primary;

    let (handle_background, border_width) = match status {
        slider::Status::Active => (theme.palette.white, 1.0),
        slider::Status::Hovered => (theme.palette.white, 2.0),
        slider::Status::Dragged => (primary.pressed.background, 2.0),
    };

    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Background::Color(accent),
                Background::Color(theme.input.border),
            ),
            width: 4.0,
            border: border::rounded(2.0),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle { radius: 8.0 },
            background: Background::Color(handle_background),
            border_width,
            border_color: accent,
        },
    }
}

impl slider::Catalog for Theme {
    type Class<'a> = SliderClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        SliderClass::Default
    }

    fn style(&self, class: &Self::Class<'_>, status: slider::Status) -> slider::Style {
        match class {
            SliderClass::Default => slider_style(self, status),
            SliderClass::Custom(f) => f(self, status),
        }
    }
}

fn radio_style(theme: &Theme, status: radio::Status) -> radio::Style {
    let input = theme.input;
    let accent = theme.palette.accent;

    let primary = theme.button.primary;

    // A selected radio is filled like a checked checkbox (accent fill, white
    // dot), which stays legible on dark fields where an accent dot would not.
    let (background, dot_color, border_color) = match status {
        radio::Status::Active { is_selected: true } => (
            primary.active.background,
            primary.active.text,
            primary.active.background,
        ),
        radio::Status::Hovered { is_selected: true } => (
            primary.hover.background,
            primary.hover.text,
            primary.hover.background,
        ),
        radio::Status::Active { is_selected: false } => {
            (input.background, input.background, input.border)
        }
        radio::Status::Hovered { is_selected: false } => {
            (hovered(theme, input.background), input.background, accent)
        }
    };

    radio::Style {
        background: Background::Color(background),
        dot_color,
        border_width: 1.0,
        border_color,
        text_color: None,
    }
}

impl radio::Catalog for Theme {
    type Class<'a> = RadioClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        RadioClass::Default
    }

    fn style(&self, class: &Self::Class<'_>, status: radio::Status) -> radio::Style {
        match class {
            RadioClass::Default => radio_style(self, status),
            RadioClass::Custom(f) => f(self, status),
        }
    }
}

fn pick_list_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let input = theme.input;
    let accent = theme.palette.accent;

    let (background, border_color, handle_color) = match status {
        pick_list::Status::Active => (input.background, input.border, input.text),
        pick_list::Status::Hovered => (hovered(theme, input.background), accent, accent),
        pick_list::Status::Opened { .. } => (input.background, accent, accent),
    };

    pick_list::Style {
        text_color: input.text,
        placeholder_color: input.placeholder,
        handle_color,
        background: Background::Color(background),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: input.radius.into(),
        },
    }
}

impl pick_list::Catalog for Theme {
    type Class<'a> = PickListClass<'a>;

    fn default<'a>() -> <Self as pick_list::Catalog>::Class<'a> {
        PickListClass::Default
    }

    fn style(
        &self,
        class: &<Self as pick_list::Catalog>::Class<'_>,
        status: pick_list::Status,
    ) -> pick_list::Style {
        match class {
            PickListClass::Default => pick_list_style(self, status),
            PickListClass::Custom(f) => f(self, status),
        }
    }
}

fn menu_style(theme: &Theme) -> menu::Style {
    let card = theme.card;
    let primary = theme.button.primary;

    menu::Style {
        background: Background::Color(card.background),
        border: Border {
            color: theme.divider,
            width: 1.0,
            radius: theme.input.radius.into(),
        },
        text_color: theme.palette.text_primary,
        selected_text_color: primary.active.text,
        selected_background: Background::Color(primary.active.background),
        shadow: card.card_shadow,
    }
}

impl menu::Catalog for Theme {
    type Class<'a> = MenuClass<'a>;

    fn default<'a>() -> <Self as menu::Catalog>::Class<'a> {
        MenuClass::Default
    }

    fn style(&self, class: &<Self as menu::Catalog>::Class<'_>) -> menu::Style {
        match class {
            MenuClass::Default => menu_style(self),
            MenuClass::Custom(f) => f(self),
        }
    }
}

impl combo_box::Catalog for Theme {}

impl progress_bar::Catalog for Theme {
    type Class<'a> = ProgressBarClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        ProgressBarClass::Default
    }

    fn style(&self, class: &Self::Class<'_>) -> progress_bar::Style {
        match class {
            ProgressBarClass::Default => progress_bar::Style {
                background: Background::Color(self.scrollable.rail),
                bar: Background::Color(self.palette.accent),
                border: border::rounded(self.scrollable.radius),
            },
            ProgressBarClass::Custom(f) => f(self),
        }
    }
}

/// The `text_input` look for a `text_editor`, which has no focus ring of its
/// own: the accent border marks focus instead.
fn text_editor_style(theme: &Theme, status: text_editor::Status) -> text_editor::Style {
    let input = theme.input;
    let is_disabled = matches!(status, text_editor::Status::Disabled);

    let border_color = match status {
        text_editor::Status::Focused { .. } => theme.palette.accent,
        _ => input.border,
    };

    text_editor::Style {
        background: Background::Color(if is_disabled {
            input.background_disabled
        } else {
            input.background
        }),
        border: Border {
            color: border_color,
            width: 1.0,
            radius: input.radius.into(),
        },
        placeholder: input.placeholder,
        value: if is_disabled {
            theme.palette.text_disabled
        } else {
            input.text
        },
        selection: input.ring,
    }
}

impl text_editor::Catalog for Theme {
    type Class<'a> = TextEditorClass<'a>;

    fn default<'a>() -> Self::Class<'a> {
        TextEditorClass::Default
    }

    fn style(&self, class: &Self::Class<'_>, status: text_editor::Status) -> text_editor::Style {
        match class {
            TextEditorClass::Default => text_editor_style(self, status),
            TextEditorClass::Custom(f) => f(self, status),
        }
    }
}

// --- kit widgets --------------------------------------------------------

impl sidebar::Catalog for Theme {
    type Class<'a> = sidebar::StyleFn<'a, Self>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme| {
            let tokens = theme.sidebar;

            sidebar::Style {
                background: tokens.background,
                hover_overlay: tokens.hover_overlay,
                icon: tokens.icon,
                edge_shadow: tokens.edge_shadow,
            }
        })
    }

    fn style(&self, class: &Self::Class<'_>) -> sidebar::Style {
        class(self)
    }
}

impl multi_border::Catalog for Theme {
    type Class<'a> = multi_border::StyleFn<'a, Self>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(|_, _| multi_border::Style::default())
    }

    fn style(&self, class: &Self::Class<'_>, status: multi_border::Status) -> multi_border::Style {
        class(self, status)
    }
}

impl error_bubble::Catalog for Theme {
    type Class<'a> = error_bubble::StyleFn<'a, Self>;

    fn default<'a>() -> Self::Class<'a> {
        Box::new(|theme| {
            let tokens = theme.error_bubble;

            error_bubble::Style {
                background: tokens.background,
                text: tokens.text,
                radius: tokens.radius,
            }
        })
    }

    fn style(&self, class: &Self::Class<'_>) -> error_bubble::Style {
        class(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_picks_the_look_by_mode_and_reports_it() {
        assert_eq!(<Theme as Base>::default(Mode::Dark), Theme::DARK);
        assert_eq!(<Theme as Base>::default(Mode::Light), Theme::LIGHT);
        assert_eq!(<Theme as Base>::default(Mode::None), Theme::LIGHT);
        assert_eq!(Theme::DARK.mode(), Mode::Dark);
        assert_eq!(Theme::LIGHT.name(), "Luminate Light");

        let base = Theme::DARK.base();
        assert_eq!(base.background_color, Theme::DARK.background);
        assert_eq!(base.text_color, Theme::DARK.palette.text_primary);
        assert_eq!(
            Base::palette(&Theme::LIGHT).map(|p| p.primary),
            Some(Theme::LIGHT.palette.accent)
        );
    }

    #[test]
    fn button_classes_read_the_tokens() {
        let theme = Theme::LIGHT;
        let primary = button::Catalog::style(
            &theme,
            &ButtonClass::Hierarchy(ButtonHierarchy::Primary),
            button::Status::Active,
        );
        assert_eq!(
            primary.background,
            Some(Background::Color(theme.palette.accent))
        );
        assert_eq!(primary.text_color, theme.button.primary.active.text);

        let pressed = button::Catalog::style(
            &theme,
            &ButtonClass::Hierarchy(ButtonHierarchy::Destructive),
            button::Status::Pressed,
        );
        assert_eq!(
            pressed.background,
            Some(Background::Color(
                theme.button.destructive.pressed.background
            ))
        );

        let disabled = button::Catalog::style(
            &theme,
            &<Theme as button::Catalog>::default(),
            button::Status::Disabled,
        );
        assert_eq!(disabled.shadow, Shadow::default(), "no glow while disabled");

        let custom: ButtonClass<'_> = (Box::new(|_: &Theme, _| button::Style {
            text_color: Color::WHITE,
            ..button::Style::default()
        }) as button::StyleFn<'_, Theme>)
            .into();
        assert_eq!(
            button::Catalog::style(&theme, &custom, button::Status::Active).text_color,
            Color::WHITE
        );
    }

    #[test]
    fn input_classes_read_the_tokens() {
        let theme = Theme::LIGHT;
        let normal =
            text_input::Catalog::style(&theme, &InputClass::Normal, text_input::Status::Active);
        assert_eq!(normal.border.color, theme.input.border);
        let error =
            text_input::Catalog::style(&theme, &InputClass::Error, text_input::Status::Active);
        assert_eq!(error.border.color, theme.input.border_error);
        let focused = text_input::Catalog::style(
            &theme,
            &InputClass::Normal,
            text_input::Status::Focused { is_hovered: false },
        );
        assert_eq!(focused.border.width, 0.0, "the ring replaces the border");
        let disabled =
            text_input::Catalog::style(&theme, &InputClass::Normal, text_input::Status::Disabled);
        assert_eq!(disabled.value, theme.palette.text_disabled);
        assert_eq!(
            disabled.background,
            Background::Color(theme.input.background_disabled)
        );
    }

    #[test]
    fn container_and_text_classes_read_the_tokens() {
        let theme = Theme::DARK;
        let header = container::Catalog::style(&theme, &ContainerClass::CardHeader);
        assert_eq!(header.border.radius.top_left, theme.card.radius);
        assert_eq!(header.border.radius.bottom_left, 0.0);
        assert_eq!(
            header.background,
            Some(Background::Color(theme.card.background))
        );
        let card = container::Catalog::style(&theme, &ContainerClass::Card);
        assert_eq!(card.shadow, theme.card.card_shadow);
        assert_eq!(
            container::Catalog::style(&theme, &ContainerClass::Transparent),
            container::Style::default()
        );

        assert_eq!(
            text::Catalog::style(&theme, &TextClass::Inherit).color,
            None
        );
        assert_eq!(
            text::Catalog::style(&theme, &TextClass::HintError).color,
            Some(theme.input.hint_text_error)
        );
        assert_eq!(
            rule::Catalog::style(&theme, &RuleClass::Standard).color,
            theme.divider
        );
    }

    #[test]
    fn button_icons_follow_the_label_colour() {
        let theme = Theme::LIGHT;
        let class = |disabled| SvgClass::ButtonIcon {
            hierarchy: ButtonHierarchy::Secondary,
            disabled,
        };
        let style = |class: &SvgClass<'_>, status| svg::Catalog::style(&theme, class, status);
        assert_eq!(
            style(&class(false), svg::Status::Idle).color,
            Some(theme.button.secondary.active.text)
        );
        assert_eq!(
            style(&class(false), svg::Status::Hovered).color,
            Some(theme.button.secondary.hover.text)
        );
        assert_eq!(
            style(&class(true), svg::Status::Hovered).color,
            Some(theme.button.secondary.disabled.text)
        );
        assert_eq!(
            style(&SvgClass::Tint(Color::WHITE), svg::Status::Idle).color,
            Some(Color::WHITE)
        );
    }

    #[test]
    fn stock_widget_catalogs_are_legible_in_both_looks() {
        use crate::theme::palette::wcag::contrast;

        for theme in [Theme::LIGHT, Theme::DARK] {
            let name = theme.name;
            let fill = |b: Background| match b {
                Background::Color(c) => c,
                Background::Gradient(_) => unreachable!("tokens are flat colours"),
            };

            let checked = checkbox::Catalog::style(
                &theme,
                &<Theme as checkbox::Catalog>::default(),
                checkbox::Status::Active { is_checked: true },
            );
            assert!(
                contrast(checked.icon_color, fill(checked.background)) >= 3.0,
                "{name} checkbox"
            );

            let on = toggler::Catalog::style(
                &theme,
                &<Theme as toggler::Catalog>::default(),
                toggler::Status::Active { is_toggled: true },
            );
            assert!(
                contrast(fill(on.foreground), fill(on.background)) >= 3.0,
                "{name} toggler"
            );
            let off = toggler::Catalog::style(
                &theme,
                &<Theme as toggler::Catalog>::default(),
                toggler::Status::Active { is_toggled: false },
            );
            assert!(
                contrast(fill(off.background), theme.background) >= 1.5,
                "{name} toggler off"
            );

            let list = pick_list::Catalog::style(
                &theme,
                &<Theme as pick_list::Catalog>::default(),
                pick_list::Status::Active,
            );
            assert!(
                contrast(list.text_color, fill(list.background)) >= 4.5,
                "{name} pick_list"
            );

            let menu = menu::Catalog::style(&theme, &<Theme as menu::Catalog>::default());
            assert!(
                contrast(menu.text_color, fill(menu.background)) >= 4.5,
                "{name} menu"
            );
            assert!(
                contrast(menu.selected_text_color, fill(menu.selected_background)) >= 4.5,
                "{name} menu selection"
            );

            let editor = text_editor::Catalog::style(
                &theme,
                &<Theme as text_editor::Catalog>::default(),
                text_editor::Status::Active,
            );
            assert!(
                contrast(editor.value, fill(editor.background)) >= 4.5,
                "{name} text_editor"
            );

            let bar =
                progress_bar::Catalog::style(&theme, &<Theme as progress_bar::Catalog>::default());
            // The bar reads against the page; the track only has to show.
            assert!(
                contrast(fill(bar.bar), theme.background) >= 3.0,
                "{name} progress bar"
            );
            assert!(
                contrast(fill(bar.background), theme.background) >= 1.2,
                "{name} progress track"
            );

            let radio = radio::Catalog::style(
                &theme,
                &<Theme as radio::Catalog>::default(),
                radio::Status::Active { is_selected: true },
            );
            assert!(
                contrast(radio.dot_color, fill(radio.background)) >= 3.0,
                "{name} radio"
            );
        }
    }

    #[test]
    fn kit_widget_catalogs_read_the_tokens() {
        let theme = Theme::DARK;
        let bar = sidebar::Catalog::style(&theme, &<Theme as sidebar::Catalog>::default());
        assert_eq!(bar.background, theme.sidebar.background);
        let bubble =
            error_bubble::Catalog::style(&theme, &<Theme as error_bubble::Catalog>::default());
        assert_eq!(bubble.background, theme.error_bubble.background);
        assert_eq!(bubble.radius, theme.error_bubble.radius);
        let ring = multi_border::Catalog::style(
            &theme,
            &<Theme as multi_border::Catalog>::default(),
            multi_border::Status::default(),
        );
        assert!(ring.rings.is_empty());
    }
}
