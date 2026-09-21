//! Collapses twelve panels at once, one per `target_width`, so how much of
//! the rounded corner survives at the narrow end can be judged against the
//! motion it costs.
//!
//! The panel is deliberately the same width and corner radius as a real
//! context menu — 240px and `radius_s`, 8px — because that ratio is the
//! whole question: the narrow end squeezes the corner horizontally by
//! `target_width`, so at 0.12 an 8px radius is drawn as a 1px sliver and
//! reads as a straight edge.
//!
//! The panels are filled, bordered and striped on purpose. A genie is a shape
//! being deformed, so there has to be a shape: bare text on the window's own
//! background shows the collapse but not the neck, which is the whole thing
//! worth looking at. The stripes make the row-by-row squeeze legible, and the
//! border makes the silhouette's edge legible.
//!
//! `cargo run -p iced_texture_cache --example genie`

use iced::widget::{button, column, container, row, text};
use iced::{Background, Border, Color, Element, Length};
use iced_texture_cache::iced_animate::{Motion, curves::QUICK, key};
use iced_texture_cache::{Corner, GenieShape, TextureCache, cached};

fn main() -> iced::Result {
    iced::application(App::new, App::update, App::view).run()
}

/// The target widths on show. `0.12` is the current default.
const TARGETS: [f32; 12] = [
    0.08, 0.12, 0.16, 0.20, 0.25, 0.30, 0.36, 0.42, 0.50, 0.58, 0.68, 0.80,
];

/// Panels per row. Twelve combinations, six across, two rows.
const PER_ROW: usize = 6;

/// A real context menu's width, so the corner is judged at the ratio it
/// will actually be drawn at.
const PANEL_WIDTH: f32 = 240.0;

/// `radius_s` in the application this is for: the radius a real menu uses.
const PANEL_RADIUS: f32 = 8.0;

/// Room for a panel to collapse into, so the travel has somewhere to go.
const PANEL_SLOT: f32 = 300.0;

/// The panel's fill, well clear of the window's own background.
const PANEL: Color = Color::from_rgb(0.11, 0.13, 0.20);
/// Its edge, so the collapsing silhouette has a visible outline.
const EDGE: Color = Color::from_rgb(0.45, 0.55, 0.85);

/// The two stripe fills. Alternating rows are what make the vertical squeeze
/// readable: at rest they are even, and in the neck they crowd together.
const STRIPES: [Color; 2] = [
    Color::from_rgb(0.16, 0.19, 0.28),
    Color::from_rgb(0.22, 0.26, 0.38),
];

struct App {
    motion: Motion,
    /// One per panel: a cache is the identity of its texture.
    caches: Vec<TextureCache>,
    anchor: Corner,
    collapsed: bool,
}

#[derive(Debug, Clone)]
enum Message {
    Toggle,
    Anchor(Corner),
}

/// Every combination on show, in the order they are laid out.
fn combinations() -> impl Iterator<Item = f32> {
    TARGETS.into_iter()
}

impl App {
    fn new() -> Self {
        Self {
            motion: Motion::new(),
            caches: combinations().map(|_| TextureCache::new()).collect(),
            anchor: Corner::TopLeft,
            collapsed: false,
        }
    }

    // iced's update signature takes the message by value; clippy's
    // `needless_pass_by_value` does not know that.
    #[allow(clippy::needless_pass_by_value)]
    fn update(&mut self, message: Message) {
        match message {
            Message::Toggle => self.collapsed = !self.collapsed,
            Message::Anchor(anchor) => self.anchor = anchor,
        }
    }

    fn view(&self) -> Element<'_, Message, iced::Theme, iced_texture_cache::Renderer> {
        // One progress drives all twelve, so they are compared at the same
        // instant rather than at whatever moment each happens to be at.
        let progress = self
            .motion
            .to(key!(), QUICK, if self.collapsed { 0.0 } else { 1.0 });

        let panels: Vec<Element<'_, Message, iced::Theme, iced_texture_cache::Renderer>> =
            combinations()
                .zip(&self.caches)
                .map(|(target_width, cache)| {
                    let shape = GenieShape {
                        anchor: self.anchor,
                        target_width,
                        // The corner the shape keeps while it is squeezed,
                        // matching the panel's own so the two agree at rest.
                        corner_radius: PANEL_RADIUS,
                        ..GenieShape::default()
                    };

                    column![
                        text(format!("target {target_width:.2}")).size(13),
                        // `progress` is cloned rather than copied: `Anim<f32>`
                        // is not `Copy` and this closure is `FnMut`.
                        container(cached(cache.clone(), panel()).genie(progress.clone(), shape))
                            .height(Length::Fixed(PANEL_SLOT)),
                    ]
                    .spacing(6)
                    .into()
                })
                .collect();

        // Split into rows by owning each chunk: an `Element` cannot be
        // cloned back out of a borrowing `chunks` iterator.
        let mut rows = column![].spacing(20);
        let mut remaining = panels;
        while !remaining.is_empty() {
            let rest = remaining.split_off(PER_ROW.min(remaining.len()));
            rows = rows.push(
                remaining
                    .into_iter()
                    .fold(row![].spacing(14), iced::widget::Row::push),
            );
            remaining = rest;
        }

        let anchors = row![
            button("top left").on_press(Message::Anchor(Corner::TopLeft)),
            button("top right").on_press(Message::Anchor(Corner::TopRight)),
            button("bottom left").on_press(Message::Anchor(Corner::BottomLeft)),
            button("bottom right").on_press(Message::Anchor(Corner::BottomRight)),
        ]
        .spacing(8);

        self.motion
            .host(
                column![
                    row![
                        button("collapse / restore").on_press(Message::Toggle),
                        anchors
                    ]
                    .spacing(16),
                    text(format!("anchor: {:?}", self.anchor)).size(14),
                    rows,
                ]
                .spacing(16)
                .padding(20),
            )
            .into()
    }
}

/// One panel's content. Built per call because each cache records its own
/// copy.
fn panel<'a>() -> Element<'a, Message, iced::Theme, iced_texture_cache::Renderer> {
    let stripes = (0..5).fold(column![].spacing(0), |rows, i| {
        let fill = STRIPES[i % 2];
        rows.push(
            container(text(format!("row {i}")).size(12).color(Color::WHITE))
                .padding(6)
                .width(Length::Fill)
                .style(move |_theme| container::Style {
                    background: Some(Background::Color(fill)),
                    ..container::Style::default()
                }),
        )
    });

    container(column![text("Genie").size(18).color(Color::WHITE), stripes,].spacing(8))
        .padding(10)
        .width(Length::Fixed(PANEL_WIDTH))
        .style(|_theme| container::Style {
            background: Some(Background::Color(PANEL)),
            border: Border {
                color: EDGE,
                width: 2.0,
                radius: PANEL_RADIUS.into(),
            },
            ..container::Style::default()
        })
        .into()
}
