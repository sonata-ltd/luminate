//! `widget::weighted_text`: Inter's `wght` axis, animated.
//!
//! `iced::Font` names nine weights and nothing between them. The bundled
//! Inter is a variable face, and everything below iced — the shaper, the
//! glyph cache key, the rasterizer's `wght` variation — is continuous. This
//! page is the three things worth seeing once that gap is bridged.
//!
//! **The axis.** The slider drives a weight nobody can name: drag it and the
//! sample thickens without a single step.
//!
//! **The rule.** Weight never animates alone. The labels below change weight
//! and background together, on one `QUICK` spring, because two curves would
//! land on different frames and the label would visibly lag its own
//! highlight.
//!
//! **The choice.** The same row is drawn under both layout policies. `Live`
//! measures the line at the weight of the moment, so the row breathes and
//! everything after it moves; `Snapped` measures it at the weight the
//! animation is heading for and holds still. Hover across the two rows and
//! watch the right-hand edges.

use iced::Length;
use iced_luminate::iced::Alignment;
use iced_luminate::iced::font::Weight;
use iced_luminate::iced::widget::{column, row, slider};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::theme::typography::{DisplaySize, TextSize, TextStyle, styled_text};
use iced_luminate::widget::weighted_text::weighted_text;
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;
use crate::iso::scenes_flat;

/// Messages of the weight page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// The axis slider moved.
    Axis(f32),
}

/// Inter's `wght` axis, by hand and animated.
pub(crate) struct WeightPage {
    luminate: Luminate,
    axis: f32,
}

impl Page for WeightPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            axis: 400.0,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Axis(weight) => self.axis = weight,
        }
        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        Hero::new(
            &self.luminate,
            &scenes_flat::WEIGHT,
            "Weight",
            "The variable font weight axis",
            self.axis(),
        )
        .build()
    }
}

impl WeightPage {
    /// The whole axis under a slider: no animation, no rounding, every weight
    /// the face carries.
    fn axis(&self) -> Element<'_, Message> {
        column![
            column![
                weighted_text(
                    "Freedom begins within.",
                    TextStyle::display(DisplaySize::Xs, Weight::Normal),
                )
                .weight(self.axis)
                .width(Length::Fill)
                .align_x(Alignment::Center),
                row![
                    slider(100.0..=900.0, self.axis, Message::Axis).step(1.0_f32),
                    styled_text(
                        format!("{:.0}", self.axis),
                        TextStyle::text(TextSize::Sm, Weight::Medium),
                    )
                    .width(40),
                ]
                .spacing(12)
                .align_y(Alignment::Center),
            ]
            .spacing(10)
        ]
        .spacing(15)
        .into()
    }
}
