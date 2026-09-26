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

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use iced::time::Instant;
use iced::widget::scrollable;
use iced_luminate::animate::path::{DrawRange, Fit, MotionPath, PathData};
use iced_luminate::animate::widget::path;
use iced_luminate::animate::{
    Curve, Easing, Motion, MotionKey, Repeat, SpringParams, curves::SMOOTH, key,
};
use iced_luminate::iced::widget::{column, stack, text};
use iced_luminate::iced::{Alignment, Length, Point, Rectangle, Size};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::texture::TextureCache;
use iced_luminate::{Element, Luminate, Renderer, Theme};
use luminate_examples_support::{ACTIVE, CellStyle, IDLE, MUTED, RebuildCounter, cell, demo};

use crate::hero::Hero;
use crate::iso::scenes_flat;

fn parsed(d: &str) -> Arc<PathData> {
    Arc::new(PathData::parse(d).expect("a literal path parses"))
}

static CHECK: LazyLock<Arc<PathData>> = LazyLock::new(|| parsed("M4 12 L10 18 L20 6"));
static BURGER: LazyLock<Arc<PathData>> =
    LazyLock::new(|| parsed("M4 6 L20 6 M4 12 L20 12 M4 18 L20 18"));
static CROSS: LazyLock<Arc<PathData>> =
    LazyLock::new(|| parsed("M5 5 L19 19 M12 12 L12 12 M19 5 L5 19"));
/// A ring centred in a 120 × 80 stage, in its pixels.
static ORBIT: LazyLock<MotionPath> =
    LazyLock::new(|| MotionPath::new(parsed("M90 40 A30 30 0 1 1 30 40 A30 30 0 1 1 90 40 Z")));
static ARROW: LazyLock<Arc<PathData>> = LazyLock::new(|| parsed("M-6 -4 L6 0 L-6 4 Z"));

/// One lap in four seconds, for ever.
const LAP: Curve = Curve::ease(Easing::Linear, Duration::from_secs(4)).repeat(Repeat::forever());

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

/// Fourteen one-idea animations.
pub(crate) struct MotionPage {
    luminate: Luminate,
    on: bool,
    last_tick: Instant,
    /// One texture per compositor-tier demo. A `TextureCache` handle *is*
    /// the texture's identity, so it lives in page state, never in `view()`.
    caches: [TextureCache; 3],
    rebuilds: RebuildCounter,
    /// Progress of the arrow's lap around [`ORBIT`], started once in `new`.
    orbit: MotionKey,
}

impl Page for MotionPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        let orbit = key!();
        // Started once: `play` in `view()` would restart the lap on every rebuild.
        let _ = luminate.motion().play(orbit, LAP, 0.0_f32, 1.0);

        Self {
            luminate: luminate.clone(),
            on: false,
            last_tick: Instant::now(),
            caches: std::array::from_fn(|_| TextureCache::new()),
            rebuilds: RebuildCounter::new(),
            orbit,
        }
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
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
            drawn(m, on),
            morphed(on),
            orbiting(m, self.orbit),
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

fn drawn<'a>(m: &Motion, on: bool) -> Element<'a, Message> {
    let range = m.to(
        key!(),
        SMOOTH,
        if on {
            DrawRange::FULL
        } else {
            DrawRange::EMPTY
        },
    );

    cell(
        "Draw",
        "path(&CHECK).stroke(ACTIVE, 2.5)\n    \
         .draw(m.to(key!(), SMOOTH, if on { FULL } else { EMPTY }))",
        path(&*CHECK)
            .width(48)
            .height(48)
            .view_box(Rectangle::new(Point::ORIGIN, Size::new(24.0, 24.0)))
            .stroke(ACTIVE, 2.5)
            .draw(range)
            .into(),
        STYLE,
    )
}

fn morphed<'a>(on: bool) -> Element<'a, Message> {
    cell(
        "Morph",
        "path(if on { &CROSS } else { &BURGER }).stroke(ACTIVE, 2.5)\n    \
         .morph(SpringParams::default()).carry_velocity(true)",
        path(if on { &*CROSS } else { &*BURGER })
            .width(48)
            .height(48)
            .view_box(Rectangle::new(Point::ORIGIN, Size::new(24.0, 24.0)))
            .stroke(ACTIVE, 2.5)
            .morph(SpringParams::default())
            .carry_velocity(true)
            .into(),
        STYLE,
    )
}

fn orbiting<'a>(m: &Motion, lap: MotionKey) -> Element<'a, Message> {
    let lap = m.get::<f32>(lap).unwrap_or_else(|| 0.0.into());
    let stage = Rectangle::new(Point::ORIGIN, Size::new(120.0, 80.0));

    cell(
        "Motion path",
        "path(&ARROW).fill(ACTIVE)\n    \
         .pose(lap.map(|u| ORBIT.pose_at(u)))  // lap repeats forever",
        stack![
            path(ORBIT.path())
                .width(120)
                .height(80)
                .view_box(stage)
                .fit(Fit::None)
                .stroke(IDLE, 1.5),
            path(&*ARROW)
                .width(120)
                .height(80)
                .fill(ACTIVE)
                .pose(lap.map(|u| ORBIT.pose_at(u))),
        ]
        .into(),
        STYLE,
    )
}
