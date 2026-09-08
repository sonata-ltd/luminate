//! All three animation tiers on one page, built from the demo cells in
//! `luminate_examples_support::demo` with `iced_luminate::Theme`.
//!
//! There is one rule behind all of it: **an animated value must be read
//! inside a widget, never while building the view.** `view()` runs on a
//! click, not on a frame; the engine hands out `Anim<T>` handles and the
//! widget resolves them in its own `layout` or `draw`. The rebuild counter at
//! the bottom proves that animating publishes no message per frame.
//!
//! | Tier | Wrapper | Reads it in | Cost per frame |
//! |---|---|---|---|
//! | Composite | `Cached` | the compositor | a composite of an existing texture |
//! | Paint | `shape` | `draw` | a redraw |
//! | Layout | `sized` | `layout` | a relayout, then a redraw |
//!
//! Rotation is not offered: the compositor blits an axis-aligned rectangle,
//! so a rotation has nowhere to live between the widget and the GPU.

use iced::widget::scrollable;
use iced_luminate::animate::Presence;
use iced_luminate::descriptor::Button;
use iced_luminate::iced::widget::{column, grid, text};
use iced_luminate::iced::{Alignment, Length};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::texture::TextureCache;
use iced_luminate::{Element, Luminate, Renderer, Theme};
use luminate_examples_support::{CellStyle, Chip, MUTED, RebuildCounter, demo};

/// Code above the stage on this page.
const STYLE: CellStyle = CellStyle {
    stage_width: Length::Fill,
    code_first: true,
    code_size: 11.0,
    fill: true,
};

/// Messages of the motion page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Flips every demo between its two poses.
    Toggle,
    /// Adds a chip to the enter/exit lane.
    AddChip,
    /// Marks a chip as leaving; it stays until its exit animation is gone.
    RemoveChip(u64),
}

/// Twelve one-idea animations.
pub(crate) struct MotionPage {
    luminate: Luminate,
    on: bool,
    /// One texture per compositor-tier demo. A `TextureCache` handle *is*
    /// the texture's identity, so it lives in page state, never in `view()`.
    caches: [TextureCache; 3],
    chips: Vec<Chip>,
    next_chip: u64,
    rebuilds: RebuildCounter,
}

impl Page for MotionPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            on: false,
            caches: std::array::from_fn(|_| TextureCache::new()),
            chips: (1..=3).map(Chip::new).collect(),
            next_chip: 4,
            rebuilds: RebuildCounter::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        // Chips whose exit has finished are reaped on the next message; nothing
        // publishes a message when an animation ends.
        let motion = self.luminate.motion().clone();
        self.chips
            .retain(|chip| !chip.leaving || motion.presence(chip.key()) != Presence::Gone);

        match message {
            Message::Toggle => self.on = !self.on,
            Message::AddChip => {
                self.chips.push(Chip::new(self.next_chip));
                self.next_chip += 1;
            }
            Message::RemoveChip(id) => {
                if let Some(chip) = self.chips.iter_mut().find(|c| c.id == id) {
                    chip.leaving = true;
                }
            }
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let m = self.luminate.motion();
        let on = self.on;

        self.rebuilds.bump();

        let cells = grid([
            demo::translate(m, on, &self.caches[0], STYLE),
            demo::scale(m, on, &self.caches[1], STYLE),
            demo::opacity(m, on, &self.caches[2], STYLE),
            demo::fill(m, on, STYLE),
            demo::radius(m, on, STYLE),
            demo::border(m, on, STYLE),
            demo::size(m, on, STYLE),
            demo::padding(m, on, STYLE),
            demo::property_set(m, on, STYLE),
            demo::spring_vs_ease(m, on, STYLE),
            demo::staggered(m, on, STYLE),
            demo::entering_and_leaving(
                m,
                &self.chips,
                Message::AddChip,
                Message::RemoveChip,
                STYLE,
            ),
        ])
        .columns(3)
        .height(Length::Shrink)
        .spacing(16);

        column![
            text("Motion").size(28),
            text(
                "One button flips every demo between two poses. Nothing below \
                 publishes a message per frame. The engine advances the \
                 values and each widget reads them as it lays out or paints."
            )
            .size(13)
            .color(MUTED),
            self.luminate
                .button(Button::new(if on { "Reset" } else { "Play" }).on_press(Message::Toggle)),
            scrollable(cells).height(Length::Fill),
            text(format!("view rebuilds: {}", self.rebuilds.count()))
                .size(12)
                .color(MUTED),
        ]
        .spacing(14)
        .align_x(Alignment::Start)
        .into()
    }
}
