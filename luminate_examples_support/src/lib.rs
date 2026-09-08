//! Scaffolding shared by the workspace examples.
//!
//! Used as a dev-dependency by `tiers` (`iced_animate`), `compositor` and
//! `pager` (`iced_texture_cache`) and `overview` (`iced_luminate`). Everything
//! here is generic over the example's `Message`, `Theme` and `Renderer`; the
//! demo cells live in [`demo`], the three compositor-tier ones behind the
//! `texture` feature.
//!
//! The crate enables no backend, executor or platform feature itself, so it
//! builds only as part of an example or `--workspace` build (see
//! `Cargo.toml`).

pub mod bench;
pub mod demo;

mod autoplay;
mod rebuilds;

use iced::advanced::text::Renderer as TextRenderer;
use iced::widget::{Space, button, column, container, row, text};
use iced::{Alignment, Color, Element, Font, Length, Padding};
use iced_animate::widget::{shape, sized};
use iced_animate::{Anim, MotionKey, key};

pub use autoplay::{Autoplay, ENV as AUTOPLAY_ENV};
pub use rebuilds::RebuildCounter;

/// The resting blue.
pub const IDLE: Color = Color::from_rgb(0.29, 0.42, 0.82);
/// The active orange.
pub const ACTIVE: Color = Color::from_rgb(0.95, 0.49, 0.20);
/// Titles.
pub const INK: Color = Color::from_rgb(0.13, 0.15, 0.20);
/// Code snippets and secondary copy.
pub const MUTED: Color = Color::from_rgb(0.42, 0.46, 0.55);
/// The stage background.
pub const STAGE: Color = Color::from_rgb(0.96, 0.97, 0.99);
/// The stage border.
pub const STAGE_BORDER: Color = Color::from_rgb(0.87, 0.89, 0.93);

/// `Radius::new` is not `const`, and the demo poses are constants.
#[must_use]
pub const fn uniform_radius(value: f32) -> iced::border::Radius {
    iced::border::Radius {
        top_left: value,
        top_right: value,
        bottom_right: value,
        bottom_left: value,
    }
}

/// Side of the square every demo animates, in logical pixels.
pub const BOX: f32 = 44.0;

/// Height of every stage, so no cell resizes when its content moves.
pub const STAGE_HEIGHT: f32 = 104.0;

/// The square the compositor demos move around.
#[must_use]
pub fn square<'a, Message, Theme, Renderer>(color: Color) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    shape()
        .width(BOX)
        .height(BOX)
        .fill(color)
        .radius(uniform_radius(8.0))
        .into()
}

/// A dot pushed sideways by an animated left margin.
///
/// Uses the layout tier deliberately: it is the shortest way to turn a scalar
/// into visible horizontal motion, and it makes two curves comparable frame by
/// frame.
#[must_use]
pub fn marker<'a, Message, Theme, Renderer>(
    offset: Anim<f32>,
    color: Color,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: iced::advanced::Renderer + 'a,
{
    row![
        sized(Space::new().height(1)).width(offset),
        shape()
            .width(14)
            .height(14)
            .fill(color)
            .radius(uniform_radius(7.0)),
    ]
    .align_y(Alignment::Center)
    .into()
}

/// A compact button for the enter/exit lane.
#[must_use]
pub fn small_button<'a, Message, Theme, Renderer>(
    label: &'a str,
    on_press: Message,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: text::Catalog + button::Catalog + 'a,
    Renderer: TextRenderer + 'a,
{
    small_button_maybe(label, Some(on_press))
}

/// [`small_button`], disabled when `on_press` is `None`.
#[must_use]
pub fn small_button_maybe<'a, Message, Theme, Renderer>(
    label: &'a str,
    on_press: Option<Message>,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: text::Catalog + button::Catalog + 'a,
    Renderer: TextRenderer + 'a,
{
    button(text(label).size(11))
        .padding(Padding::from([3.0, 8.0]))
        .on_press_maybe(on_press)
        .into()
}

/// Where the pieces of a [`cell`] go; the examples differ slightly.
#[derive(Debug, Clone, Copy)]
pub struct CellStyle {
    /// Width of the stage container.
    pub stage_width: Length,
    /// Puts the code snippet above the stage instead of below it.
    pub code_first: bool,
    /// Font size of the code snippet.
    pub code_size: f32,
    /// Whether the cell fills its grid column.
    pub fill: bool,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            stage_width: Length::Fill,
            code_first: false,
            code_size: 10.0,
            fill: true,
        }
    }
}

/// Title, live demo, and the code that drives it.
///
/// The snippet is a literal, not extracted from the source: the two sit next
/// to each other in [`demo`] so they are edited together.
#[must_use]
pub fn cell<'a, Message, Theme, Renderer>(
    title: &'a str,
    code: &'a str,
    demo: Element<'a, Message, Theme, Renderer>,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let title = text(title).size(14).color(INK);
    let code = text(code)
        .size(style.code_size)
        .font(Font::MONOSPACE)
        .color(MUTED);
    let stage = container(demo)
        .width(style.stage_width)
        .height(Length::Fixed(STAGE_HEIGHT))
        .padding(12)
        .align_y(Alignment::Center)
        .style(|_| container::Style {
            background: Some(iced::Background::Color(STAGE)),
            border: iced::Border {
                color: STAGE_BORDER,
                width: 1.0,
                radius: 10.0.into(),
            },
            ..Default::default()
        });

    let cell = if style.code_first {
        column![title, code, stage]
    } else {
        column![title, stage, code]
    }
    .spacing(8);

    let cell = if style.fill {
        cell.width(Length::Fill)
    } else {
        cell
    };

    cell.into()
}

/// One chip in the enter/exit lane.
#[derive(Debug, Clone, Copy)]
pub struct Chip {
    /// Stable identity, minted by the page that owns the lane.
    pub id: u64,
    /// Set when the user removes it. It keeps being drawn until its exit
    /// animation reports [`Presence::Gone`](iced_animate::Presence::Gone).
    pub leaving: bool,
}

impl Chip {
    /// A chip that is present and staying.
    #[must_use]
    pub fn new(id: u64) -> Self {
        Self { id, leaving: false }
    }

    /// The chip's animation identity.
    ///
    /// Derived from the id rather than from a position, which is the whole
    /// reason animation state lives outside the widget tree: tree state is
    /// addressed positionally and would follow the wrong chip the moment one
    /// in the middle leaves.
    #[must_use]
    pub fn key(&self) -> MotionKey {
        key!(self.id)
    }
}
