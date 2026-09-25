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

use std::time::Duration;

use iced::time::Instant;
use iced::widget::scrollable;
use iced_luminate::iced::widget::{column, text};
use iced_luminate::iced::{Alignment, Length};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::texture::TextureCache;
use iced_luminate::{Element, Luminate, Renderer, Theme};
use luminate_examples_support::{CellStyle, MUTED, RebuildCounter, demo};

use crate::hero::Hero;
use crate::iso::scenes_flat;

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
    Tick,
}

/// Twelve one-idea animations.
pub(crate) struct MotionPage {
    luminate: Luminate,
    on: bool,
    last_tick: Instant,
    /// One texture per compositor-tier demo. A `TextureCache` handle *is*
    /// the texture's identity, so it lives in page state, never in `view()`.
    caches: [TextureCache; 3],
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
            last_tick: Instant::now(),
            caches: std::array::from_fn(|_| TextureCache::new()),
            rebuilds: RebuildCounter::new(),
        }
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::time::every(Duration::from_millis(1000)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Tick => {
                self.last_tick = Instant::now();
                self.on = !self.on;
            }
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let m = self.luminate.motion();
        let on = self.on;

        self.rebuilds.bump();

        let cells = column![
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
        ]
        .height(Length::Shrink)
        .spacing(16);

        Hero::new(
            &self.luminate,
            &scenes_flat::MOTION,
            "Motion",
            "Springs and eases, resolved per frame",
            column![
                text(
                    "One button flips every demo between two poses. Nothing below \
                     publishes a message per frame. The engine advances the \
                     values and each widget reads them as it lays out or paints."
                )
                .size(13)
                .color(MUTED),
                scrollable(cells).height(Length::Fill),
                text(format!("view rebuilds: {}", self.rebuilds.count()))
                    .size(12)
                    .color(MUTED),
            ]
            .spacing(14)
            .align_x(Alignment::Start),
        )
        .build()
    }
}
