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

use iced::widget::container;
use iced_animate::key;
use iced_luminate::animate::curves::QUICK;
use iced_luminate::descriptor::{Button, ButtonHierarchy};
use iced_luminate::iced::font::Weight;
use iced_luminate::iced::widget::{column, row, slider};
use iced_luminate::iced::{Alignment, Color, Padding};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::theme::typography::{DisplaySize, TextSize, TextStyle, styled_text};
use iced_luminate::widget::weighted_text::{WeightLayout, weighted_text};
use iced_luminate::{Element, Luminate, Renderer, Theme};

/// The weight a resting label sits at.
const REST: f32 = 200.0;

/// The weight a hovered or selected label moves to.
///
/// A hundred units: enough to read as a change of state, little enough not to
/// read as a different type style. Below fifty the text only looks sharper.
const EMPHASIS: f32 = 700.0;

/// Messages of the weight page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// The axis slider moved.
    Axis(f32),
    /// The pointer entered or left a label in one of the two rows.
    /// A navigation item was clicked.
    ToggleBounds,
    ToggleWeightSnapping,
    TogglePlayAnim,
}

/// Inter's `wght` axis, by hand and animated.
pub(crate) struct WeightPage {
    luminate: Luminate,
    axis: f32,
    show_borders: bool,
    snap_weight: bool,
    play_anim: bool,
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
            show_borders: false,
            snap_weight: false,
            play_anim: false,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Axis(weight) => self.axis = weight,
            Message::ToggleBounds => self.show_borders = !self.show_borders,
            Message::ToggleWeightSnapping => self.snap_weight = !self.snap_weight,
            Message::TogglePlayAnim => self.play_anim = !self.play_anim,
        }
        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        column![Self::welcome(), self.axis(), self.weighted_text_snapping()]
            .spacing(50)
            .into()
    }
}

impl WeightPage {
    fn welcome<'a>() -> Element<'a, Message> {
        column![
            styled_text(
                "Weight",
                TextStyle::display(DisplaySize::Sm, Weight::Semibold)
            ),
            styled_text(
                "An interactive demo of Inter’s animated wght axis, showing how font weight changes smoothly with a slider and how two layout strategies, Live and Snapped, affect text width, hover states, and selected navigation items.",
                TextStyle::text(TextSize::Md, Weight::Normal)
            ),
        ].into()
    }

    /// The whole axis under a slider: no animation, no rounding, every weight
    /// the face carries.
    fn axis(&self) -> Element<'_, Message> {
        column![
            column![
                weighted_text(
                    "Freedom begins within.",
                    TextStyle::display(DisplaySize::Xs, Weight::Normal),
                )
                .weight(self.axis),
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

    /// One row of labels that thicken on hover, under one layout policy.
    fn weighted_text_snapping(&self) -> Element<'_, Message> {
        let l = &self.luminate;
        let motion = self.luminate.motion();

        let weight = motion.to(key!(), QUICK, if self.play_anim { EMPHASIS } else { REST });

        let text: Element<'_, Message> = weighted_text(
            "Weighted text",
            TextStyle::display(DisplaySize::Sm, Weight::Normal),
        )
        .weight(weight)
        .weight_layout(if self.snap_weight {
            WeightLayout::Snapped
        } else {
            WeightLayout::Live
        })
        .into();

        column![
            column![
                if self.show_borders {
                    text.explain(Color::from_rgb8(255, 12, 12))
                } else {
                    text
                },
                container(styled_text(
                    if self.snap_weight { "Snapped" } else { "Live" },
                    TextStyle::text(TextSize::Sm, Weight::Normal)
                ))
                .padding(Padding {
                    top: -5.0,
                    ..Default::default()
                }),
            ]
            .spacing(5),
            row![
                l.button(
                    Button::new(if self.play_anim { "Reset" } else { "Play" })
                        .on_press(Message::TogglePlayAnim)
                ),
                l.button(
                    Button::new("Toggle weight snapping")
                        .on_press(Message::ToggleWeightSnapping)
                        .hierarchy(ButtonHierarchy::Secondary)
                ),
                l.button(
                    Button::new("Toggle bounds")
                        .on_press(Message::ToggleBounds)
                        .hierarchy(ButtonHierarchy::Secondary)
                ),
            ]
            .spacing(10),
        ]
        .spacing(15)
        .into()
    }
}
