//! `iced_animate::path` on one stage: a circuit that morphs into another, a
//! line that draws itself on, and a car that follows the track.
//!
//! The three sections cycle on their own, one every few seconds; the list on
//! the left jumps to a section and the cycle carries on from there. Every
//! layer of the stage is a `path()` widget in the same 400 × 400 coordinate
//! system (`Fit::None`), so the car's pose, taken from the circuit with
//! `MotionPath::pose_at`, lands exactly on the drawn track.
//!
//! The artwork is drawn here from scratch: the two circuits are hand-written
//! cubic paths, the rings are arcs computed below.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use iced_luminate::animate::path::{DrawRange, Fit, MotionPath, PathData, Perspective};
use iced_luminate::animate::widget::path;
use iced_luminate::animate::{Anim, Curve, Easing, Motion, MotionKey, Repeat, SpringParams};
use iced_luminate::descriptor::{Button, ButtonHierarchy};
use iced_luminate::iced::widget::{column, container, stack, text};
use iced_luminate::iced::{Background, Border, Color, Font, Length, Point, Rectangle, Size};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;
use crate::iso::scenes_flat;

/// The stage's side, in logical pixels and in path units alike.
const STAGE: f32 = 400.0;

/// The stage's centre.
const CENTRE: Point = Point::new(STAGE / 2.0, STAGE / 2.0);

/// Seconds a section stays up before the cycle moves on.
const SECTION_SECONDS: u32 = 6;

/// Seconds between the two circuits while morphing.
const MORPH_SECONDS: u32 = 2;

/// The stage's backdrop and the circuit's colours.
const BACKDROP: Color = Color::from_rgb(0.13, 0.14, 0.15);
const TRACK: Color = Color::from_rgb(0.13, 0.84, 0.93);
const TRACK_DIM: Color = Color::from_rgba(0.13, 0.62, 0.70, 0.30);
const GLOW: Color = Color::from_rgba(0.13, 0.84, 0.93, 0.22);
const GRID: Color = Color::from_rgba(0.40, 0.75, 0.80, 0.28);
const TICK: Color = Color::from_rgba(0.20, 0.62, 0.70, 0.55);
const RING: Color = Color::from_rgba(1.0, 1.0, 1.0, 0.06);

/// The outer ring's six arcs, clockwise from the top.
const ARC_COLOURS: [Color; 6] = [
    Color::from_rgb(0.96, 0.33, 0.33),
    Color::from_rgb(0.98, 0.64, 0.22),
    Color::from_rgb(0.20, 0.90, 0.58),
    Color::from_rgb(0.30, 0.56, 0.98),
    Color::from_rgb(0.13, 0.84, 0.93),
    Color::from_rgb(0.55, 0.62, 0.25),
];

/// Seconds the Motion path section stays up: one pass of its timeline.
const FOLLOW_SECONDS: u32 = 12;

/// The Motion path section's clock: `0 → 1` over one pass of the timeline,
/// for ever. Every layer of the section is a function of it (see
/// [`timeline`]), so they stay in step and the loop joins without a seam.
const CLOCK: Curve = Curve::ease(Easing::Linear, Duration::from_secs(FOLLOW_SECONDS as u64))
    .repeat(Repeat::forever());

/// The stage leaning one way and then the other, for ever.
const SWAY: Curve = Curve::ease(Easing::EaseInOut, Duration::from_millis(5200))
    .repeat(Repeat::forever().alternate());

/// The two leans the stage sways between: top away and a little to the
/// left, then top away and a little to the right.
const LEAN_LEFT: Perspective = Perspective::new(0.55, -0.22, 900.0);
const LEAN_RIGHT: Perspective = Perspective::new(0.40, 0.22, 900.0);

/// The circuit drawing itself on and off again, for ever.
const DRAW_LOOP: Curve = Curve::ease(Easing::EaseInOut, Duration::from_millis(1600)).repeat(
    Repeat::forever()
        .alternate()
        .gap(Duration::from_millis(400)),
);

/// One outer arc drawing itself on.
const ARC_IN: Curve = Curve::ease(Easing::EaseOut, Duration::from_millis(700));

fn parsed(d: &str) -> Arc<PathData> {
    Arc::new(PathData::parse(d).expect("a literal path parses"))
}

/// The first circuit: a long loop with a hairpin at the bottom right.
static CIRCUIT_A: LazyLock<Arc<PathData>> = LazyLock::new(|| {
    parsed(
        "M200 108 C252 106 266 148 244 174 C226 196 252 214 278 204 \
         C308 192 322 226 302 252 C282 278 240 262 224 290 \
         C210 316 176 318 162 294 C148 270 172 254 160 234 \
         C148 214 116 222 110 196 C103 168 138 156 150 140 \
         C162 124 170 109 200 108 Z",
    )
});

/// The second circuit: rounder, with its long straight on the left.
static CIRCUIT_B: LazyLock<Arc<PathData>> = LazyLock::new(|| {
    parsed(
        "M158 118 C200 98 252 128 264 160 C274 186 302 180 308 212 \
         C314 246 272 252 262 274 C252 300 212 302 196 284 \
         C182 268 150 292 130 268 C110 244 138 226 126 202 \
         C114 178 120 138 158 118 Z",
    )
});

/// The car: an arrowhead pointing along +x, drawn around its own origin.
static CAR: LazyLock<Arc<PathData>> = LazyLock::new(|| parsed("M-8 -6 L10 0 L-8 6 L-3 0 Z"));

/// The first circuit, measured for following.
static COURSE: LazyLock<MotionPath> = LazyLock::new(|| MotionPath::new(Arc::clone(&CIRCUIT_A)));

/// A dot every 20 px inside the inner ring, each a hair-short line that a
/// round-capped stroke turns into a disc.
///
/// Strokes rather than filled circles on purpose: a stroke is tessellated
/// one subpath at a time, where a fill of a hundred and fifty little circles
/// sweeps them all at once. Measured on wgpu, the grid costs 53 µs to
/// tessellate this way against 435 µs filled.
static DOTS: LazyLock<Arc<PathData>> = LazyLock::new(|| {
    let mut builder = PathData::builder();
    for row in 0..21 {
        for col in 0..21 {
            let at = Point::new(col as f32 * 20.0, row as f32 * 20.0);
            if at.distance(CENTRE) < 130.0 {
                builder = builder.move_to(at).line_to(Point::new(at.x + 0.01, at.y));
            }
        }
    }
    Arc::new(builder.build().expect("the grid has dots"))
});

/// The dots' diameter.
const DOT: f32 = 2.6;

/// Short radial ticks round the stage.
static TICKS: LazyLock<Arc<PathData>> = LazyLock::new(|| {
    let mut builder = PathData::builder();
    for k in 0..120 {
        let angle = k as f32 / 120.0 * TAU;
        let (inner, outer) = if k % 10 == 0 {
            (158.0, 172.0)
        } else {
            (163.0, 172.0)
        };
        builder = builder
            .move_to(polar(inner, angle))
            .line_to(polar(outer, angle));
    }
    Arc::new(builder.build().expect("the ring has ticks"))
});

/// A thin circle inside the ticks.
static INNER_RING: LazyLock<Arc<PathData>> = LazyLock::new(|| arc(148.0, 0.0, TAU));

/// The stage's round backdrop, as a path so it tilts with everything else.
static DISC: LazyLock<Arc<PathData>> = LazyLock::new(|| arc(199.0, 0.0, TAU));

/// The outer ring's six arcs, a small gap between each.
static ARCS: LazyLock<[Arc<PathData>; 6]> = LazyLock::new(|| {
    let sixth = TAU / 6.0;
    let gap = 0.05;
    std::array::from_fn(|i| {
        let start = i as f32 * sixth - FRAC_PI_2 + gap / 2.0;
        arc(186.0, start, start + sixth - gap)
    })
});

/// The point at `radius` from the centre, `angle` clockwise from +x.
fn polar(radius: f32, angle: f32) -> Point {
    Point::new(
        CENTRE.x + radius * angle.cos(),
        CENTRE.y + radius * angle.sin(),
    )
}

/// A circular arc round the centre, as cubics of at most a quarter turn.
fn arc(radius: f32, from: f32, to: f32) -> Arc<PathData> {
    let pieces = ((to - from).abs() / FRAC_PI_2).ceil().max(1.0) as usize;
    let step = (to - from) / pieces as f32;
    // The control distance that makes a cubic hug a circular arc of `step`.
    let k = 4.0 / 3.0 * (step / 4.0).tan() * radius;

    let mut builder = PathData::builder().move_to(polar(radius, from));
    for piece in 0..pieces {
        let a = from + piece as f32 * step;
        let b = a + step;
        let (pa, pb) = (polar(radius, a), polar(radius, b));
        let c1 = Point::new(pa.x - k * a.sin(), pa.y + k * a.cos());
        let c2 = Point::new(pb.x + k * b.sin(), pb.y - k * b.cos());
        builder = builder.cubic_to(c1, c2, pb);
    }
    Arc::new(builder.build().expect("an arc has segments"))
}

/// The Motion path section, second by second, as pure functions of the
/// time `t` into one pass of [`CLOCK`]. Kept free of widgets so the
/// choreography reads (and can be tuned) in one place.
mod timeline {
    use iced_luminate::animate::path::DrawRange;

    /// The background track draws itself on over this long.
    const TRACK_IN: (f32, f32) = (0.0, 1.4);
    /// Each outer arc draws on over this long, the next one this much later.
    const ARC_IN: f32 = 0.6;
    const ARC_STAGGER: f32 = 0.12;
    /// The car sets off here and takes this long to reach cruising speed.
    const LAUNCH: f32 = 1.2;
    const THROTTLE: f32 = 2.0;
    /// Cruising speed, in laps per second.
    const SPEED: f32 = 1.0 / 3.2;
    /// The trail's length at speed, in laps.
    const TRAIL: f32 = 0.18;
    /// Everything winds down over this window, then the stage rests empty
    /// until the pass ends.
    const WIND_DOWN: (f32, f32) = (9.8, 11.0);

    fn ramp(t: f32, (from, to): (f32, f32)) -> f32 {
        ((t - from) / (to - from)).clamp(0.0, 1.0)
    }

    fn ease_out(x: f32) -> f32 {
        1.0 - (1.0 - x).powi(3)
    }

    fn ease_in_out(x: f32) -> f32 {
        x * x * (3.0 - 2.0 * x)
    }

    /// How much of the stage is still up: `1`, falling to `0` across the
    /// wind-down.
    fn presence(t: f32) -> f32 {
        1.0 - ease_in_out(ramp(t, WIND_DOWN))
    }

    /// The background track: drawn on, then wiped off from its start.
    pub(super) fn track(t: f32) -> DrawRange {
        let drawn = ease_out(ramp(t, TRACK_IN));
        DrawRange::new(1.0 - presence(t), drawn)
    }

    /// The `i`-th outer arc, drawn on in turn, wiped off together.
    pub(super) fn arc(i: usize, t: f32) -> DrawRange {
        let from = 0.2 + i as f32 * ARC_STAGGER;
        let drawn = ease_out(ramp(t, (from, from + ARC_IN)));
        DrawRange::new(1.0 - presence(t), drawn)
    }

    /// How far the car has gone, in laps: standing, then a smooth pull
    /// away (constant acceleration) into a steady speed.
    pub(super) fn distance(t: f32) -> f32 {
        let run = (t - LAUNCH).max(0.0);
        if run < THROTTLE {
            SPEED * run * run / (2.0 * THROTTLE)
        } else {
            SPEED * THROTTLE / 2.0 + SPEED * (run - THROTTLE)
        }
    }

    /// Where the car is on the lap, `0..1`.
    pub(super) fn lap(t: f32) -> f32 {
        distance(t).rem_euclid(1.0)
    }

    /// The light trail behind the car. It grows from nothing as the car
    /// pulls away, may reach back across the start of the lap (a range
    /// below zero, which the widget wraps), and shrinks away in the
    /// wind-down.
    pub(super) fn trail(t: f32) -> DrawRange {
        let head = distance(t);
        let tail = (head - TRAIL * presence(t)).max(0.0);
        let lap = head.floor();
        DrawRange::new(tail - lap, head - lap)
    }

    /// The car's opacity: in as it sets off, out in the wind-down.
    pub(super) fn car(t: f32) -> f32 {
        ramp(t, (LAUNCH - 0.2, LAUNCH + 0.3)) * presence(t)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn the_pass_starts_and_ends_empty_so_the_loop_has_no_seam() {
            for t in [0.0, 11.5, 12.0] {
                assert!(track(t).is_empty(), "track at {t}");
                assert!(trail(t).is_empty(), "trail at {t}");
                assert!(car(t) == 0.0, "car at {t}");
            }
        }

        #[test]
        fn the_car_pulls_away_without_a_jolt() {
            let dt = 1.0 / 60.0;
            let speed = |t: f32| (distance(t + dt) - distance(t)) / dt;
            assert!(speed(LAUNCH + dt) < 0.02, "sets off from rest");
            let at_cruise = speed(LAUNCH + THROTTLE + dt);
            assert!(
                (at_cruise - SPEED).abs() < 0.01,
                "reaches cruising speed: {at_cruise}"
            );
            let before = speed(LAUNCH + THROTTLE - dt);
            assert!((at_cruise - before).abs() < 0.01, "no kink at the handover");
        }

        #[test]
        fn the_trail_follows_the_car_across_the_start_of_the_lap() {
            // Find a moment just after the car crosses the start.
            let t = (0..1200)
                .map(|i| LAUNCH + i as f32 / 100.0)
                .find(|&t| distance(t) > 1.0 && distance(t) < 1.05)
                .expect("the car completes a lap");
            let range = trail(t);
            assert!(
                range.start < 0.0 && range.end > 0.0,
                "reaches back: {range:?}"
            );
            assert!(
                (range.end - range.start - TRAIL).abs() < 1e-3,
                "full length"
            );
        }
    }
}

/// What the stage is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Section {
    Morph,
    Draw,
    Follow,
}

impl Section {
    const ALL: [Self; 3] = [Self::Morph, Self::Draw, Self::Follow];

    fn label(self) -> &'static str {
        match self {
            Self::Morph => "→  SHAPE MORPHING",
            Self::Draw => "→  LINE DRAWING",
            Self::Follow => "→  MOTION PATH",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::Morph => Self::Draw,
            Self::Draw => Self::Follow,
            Self::Follow => Self::Morph,
        }
    }

    /// The `iced_animate` code behind the section, shown beside the stage.
    fn code(self) -> &'static str {
        match self {
            Self::Morph => {
                "path(if on_b { &CIRCUIT_B } else { &CIRCUIT_A })\n    \
                 .stroke(TRACK, 3.0)\n    \
                 .morph(SpringParams::new(0.2, ms(700)))\n    \
                 .carry_velocity(true)"
            }
            Self::Draw => {
                "let range = m.play(key, DRAW_LOOP,\n    \
                 DrawRange::EMPTY, DrawRange::FULL);\n\
                 path(&CIRCUIT_A).stroke(TRACK, 3.0).draw(range)\n\n\
                 // DRAW_LOOP = Curve::ease(EaseInOut, ms(1600))\n\
                 //   .repeat(Repeat::forever().alternate())"
            }
            Self::Follow => {
                "let clock = m.play(key, CLOCK, 0.0, 1.0); // 12 s, forever\n\
                 let t = |c: f32| c * 12.0;\n\
                 path(&CIRCUIT).draw(clock.map(move |c| timeline::track(t(c))))\n\
                 path(&CIRCUIT).draw(clock.map(move |c| timeline::trail(t(c))))\n\
                 path(&CAR).pose(clock.map(move |c| COURSE.pose_at(timeline::lap(t(c)))))\n\
                 // every layer: .perspective(tilt.clone())"
            }
        }
    }
}

/// Messages of the SVG toolset page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// One second passed.
    Tick,
    /// A section was picked from the list.
    Select(Section),
}

/// The SVG toolset stage.
pub(crate) struct SvgToolsetPage {
    luminate: Luminate,
    section: Section,
    /// Seconds since the section came up.
    elapsed: u32,
    /// Which circuit the morph is heading for.
    on_b: bool,
    clock: MotionKey,
    tilt: MotionKey,
    draw: MotionKey,
    arcs: [MotionKey; 6],
}

impl Page for SvgToolsetPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        let page = Self {
            luminate: luminate.clone(),
            section: Section::Follow,
            elapsed: 0,
            on_b: false,
            clock: MotionKey::unique(),
            tilt: MotionKey::unique(),
            draw: MotionKey::unique(),
            arcs: std::array::from_fn(|_| MotionKey::unique()),
        };
        // The sway runs under every section, so it starts once, here.
        let _ = luminate
            .motion()
            .play(page.tilt, SWAY, LEAN_LEFT, LEAN_RIGHT);
        page.enter();
        page
    }

    fn subscription(&self) -> iced::Subscription<Self::Message> {
        iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick)
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Tick => {
                self.elapsed += 1;
                let seconds = if self.section == Section::Follow {
                    FOLLOW_SECONDS
                } else {
                    SECTION_SECONDS
                };
                if self.elapsed >= seconds {
                    self.show(self.section.next());
                } else if self.section == Section::Morph
                    && self.elapsed.is_multiple_of(MORPH_SECONDS)
                {
                    self.on_b = !self.on_b;
                }
            }
            Message::Select(section) => self.show(section),
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let m = self.luminate.motion();

        let list = column(Section::ALL.map(|section| {
            self.luminate.button(
                Button::new(section.label())
                    .hierarchy(if section == self.section {
                        ButtonHierarchy::Secondary
                    } else {
                        ButtonHierarchy::Tertiary
                    })
                    .on_press(Message::Select(section)),
            )
        }))
        .spacing(4);

        let code = container(
            text(self.section.code())
                .font(Font::MONOSPACE)
                .size(12)
                .color(Color::from_rgb(0.62, 0.78, 0.92)),
        )
        .padding(14)
        .width(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(BACKDROP)),
            border: Border {
                radius: 10.0.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });

        // The page column is the theme's `sm` width (480 px): too narrow for
        // the 400 px stage beside anything, so everything stacks.
        let body = column![
            text(
                "Morph shapes, follow motion paths and draw lines with \
                 iced_animate's path tools. Pick a section, or let the \
                 stage cycle through them."
            )
            .size(14),
            container(self.stage(m)).center_x(Length::Fill),
            list,
            code,
        ]
        .spacing(20);

        Hero::new(
            &self.luminate,
            &scenes_flat::MOTION,
            "SVG toolset",
            "Morphing, line drawing and motion paths",
            body,
        )
        .build()
    }
}

impl SvgToolsetPage {
    /// Switches to `section` and starts what it animates.
    fn show(&mut self, section: Section) {
        self.section = section;
        self.elapsed = 0;
        self.enter();
    }

    /// Starts the section's one-shot and endless tracks. `play` restarts,
    /// so it is called on entering a section, never from `view()`.
    fn enter(&self) {
        let m = self.luminate.motion();
        match self.section {
            Section::Morph => {}
            Section::Draw => {
                let _ = m.play(self.draw, DRAW_LOOP, DrawRange::EMPTY, DrawRange::FULL);
                for (i, key) in self.arcs.iter().enumerate() {
                    let curve = ARC_IN.delayed(Duration::from_millis(120 * i as u64));
                    let _ = m.play(*key, curve, DrawRange::EMPTY, DrawRange::FULL);
                }
            }
            Section::Follow => {
                let _ = m.play(self.clock, CLOCK, 0.0_f32, 1.0);
            }
        }
    }

    /// Every layer of the stage, back to front, in one 400 × 400 space,
    /// all seen through the same swaying perspective.
    fn stage<'a>(&self, m: &Motion) -> Element<'a, Message> {
        let tilt = m
            .get::<Perspective>(self.tilt)
            .unwrap_or_else(|| LEAN_LEFT.into());
        let layer = |data: &Arc<PathData>| {
            path(data)
                .width(STAGE)
                .height(STAGE)
                .view_box(Rectangle::new(Point::ORIGIN, Size::new(STAGE, STAGE)))
                .fit(Fit::None)
                .perspective(tilt.clone())
        };
        let full = || Anim::constant(DrawRange::FULL);
        let fetch = |key: MotionKey| m.get::<DrawRange>(key).unwrap_or_else(full);
        // The Motion path section's timeline, in seconds into the pass.
        let clock = m.get::<f32>(self.clock).unwrap_or_else(|| 0.0.into());
        let at = |f: fn(f32) -> DrawRange| clock.map(move |c| f(c * FOLLOW_SECONDS as f32));

        let mut layers: Vec<Element<'a, Message>> = vec![
            layer(&DISC).fill(BACKDROP).into(),
            layer(&DOTS).stroke(GRID, DOT).into(),
            layer(&INNER_RING).stroke(RING, 1.0).into(),
            layer(&TICKS).stroke(TICK, 1.5).into(),
        ];

        for (i, arc) in ARCS.iter().enumerate() {
            let range = match self.section {
                Section::Draw => fetch(self.arcs[i]),
                Section::Follow => clock.map(move |c| timeline::arc(i, c * FOLLOW_SECONDS as f32)),
                Section::Morph => full(),
            };
            let colour = ARC_COLOURS[i];
            let halo = Color { a: 0.25, ..colour };
            layers.push(layer(arc).stroke(halo, 9.0).draw(range.clone()).into());
            layers.push(layer(arc).stroke(colour, 4.0).draw(range).into());
        }

        match self.section {
            Section::Morph => {
                let target = if self.on_b { &*CIRCUIT_B } else { &*CIRCUIT_A };
                let spring = SpringParams::new(0.2, Duration::from_millis(700));
                layers.push(
                    layer(target)
                        .stroke(GLOW, 12.0)
                        .morph(spring)
                        .carry_velocity(true)
                        .into(),
                );
                layers.push(
                    layer(target)
                        .stroke(TRACK, 3.0)
                        .morph(spring)
                        .carry_velocity(true)
                        .into(),
                );
            }
            Section::Draw => {
                let range = fetch(self.draw);
                layers.push(layer(&CIRCUIT_A).stroke(TRACK_DIM, 7.0).into());
                layers.push(
                    layer(&CIRCUIT_A)
                        .stroke(GLOW, 12.0)
                        .draw(range.clone())
                        .into(),
                );
                layers.push(layer(&CIRCUIT_A).stroke(TRACK, 3.0).draw(range).into());
            }
            Section::Follow => {
                let trail = at(timeline::trail);
                layers.push(
                    layer(&CIRCUIT_A)
                        .stroke(TRACK_DIM, 7.0)
                        .draw(at(timeline::track))
                        .into(),
                );
                layers.push(
                    layer(&CIRCUIT_A)
                        .stroke(GLOW, 12.0)
                        .draw(trail.clone())
                        .into(),
                );
                layers.push(layer(&CIRCUIT_A).stroke(TRACK, 3.0).draw(trail).into());
                layers.push(
                    layer(&CAR)
                        .fill(clock.map(|c| Color {
                            a: timeline::car(c * FOLLOW_SECONDS as f32),
                            ..TRACK
                        }))
                        .pose(
                            clock.map(|c| COURSE.pose_at(timeline::lap(c * FOLLOW_SECONDS as f32))),
                        )
                        .into(),
                );
            }
        }

        container(stack(layers))
            .width(Length::Fixed(STAGE))
            .height(Length::Fixed(STAGE))
            .into()
    }
}
