//! Analytic radial fills: direct, recorded, and layered over interactive content.

use crate::{hero::Hero, iso::scenes_flat};
use iced_luminate::descriptor::Button;
use iced_luminate::iced::widget::{column, container, row, slider, stack, text};
use iced_luminate::iced::{Color, Length, Point, Vector};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::texture::{Radial, TextureCache, cached, radial_gradient};
use iced_luminate::{Element, Luminate, Renderer, Theme};

#[derive(Debug, Clone)]
pub(crate) enum Message {
    X(f32),
    Y(f32),
    Horizontal(f32),
    Vertical(f32),
    Opacity(f32),
    Corners(f32),
    Play,
}
pub(crate) struct RadialPage {
    luminate: Luminate,
    center: Point,
    radii: Vector,
    opacity: f32,
    corners: f32,
    clicks: u32,
    cache: TextureCache,
}
impl Page for RadialPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;
    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            center: Point::new(0.5, 0.85),
            radii: Vector::new(0.65, 0.85),
            opacity: 0.8,
            corners: 20.0,
            clicks: 0,
            cache: TextureCache::new(),
        }
    }
    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::X(x) => self.center.x = x,
            Message::Y(y) => self.center.y = y,
            Message::Horizontal(r) => self.radii.x = r,
            Message::Vertical(r) => self.radii.y = r,
            Message::Opacity(opacity) => self.opacity = opacity,
            Message::Corners(corners) => self.corners = corners,
            Message::Play => self.clicks += 1,
        }
        Action::none()
    }
    fn view(&self) -> Element<'_, Message> {
        // Explicit colours are the paint sample, not application theme tokens.
        let fill = Radial::new()
            .center(self.center)
            .radii(self.radii)
            .add_stop(0.0, Color::from_rgb8(7, 255, 7))
            .add_stop(0.45, Color::from_rgba8(7, 255, 102, 0.6))
            .add_stop(1.0, Color::TRANSPARENT);
        let layer = || {
            radial_gradient(fill)
                .opacity(self.opacity)
                .border_radius(self.corners)
                .width(Length::Fill)
                .height(160)
        };
        let direct = self.panel(layer().into());
        let recorded = self.panel(cached(self.cache.clone(), layer()).into());
        let card: Element<'_, Message> = stack![
            container(
                column![
                    text("Minecraft"),
                    text("Ready to launch").size(14),
                    self.luminate
                        .button(Button::new("Play").on_press(Message::Play)),
                    text(format!("Clicks: {}", self.clicks)).size(14),
                ]
                .spacing(8)
            )
            .padding(16)
            .width(Length::Fill)
            .height(160),
            layer(),
        ]
        .into();
        let sample = |label: &'static str, content| {
            column![text(label).size(14), content]
                .spacing(8)
                .width(Length::Fill)
        };
        let control =
            |label, value: f32, range, on_change: fn(f32) -> Message| -> Element<'_, Message> {
                row![
                    text(format!("{label}: {value:.2}")).width(170),
                    slider(range, value, on_change).step(0.001_f32)
                ]
                .spacing(16)
                .into()
            };
        Hero::new(&self.luminate, &scenes_flat::RADIAL, "Radial gradient",
            "A paint layer: no SVG and no texture recording on the GPU",
            column![
                row![sample("Direct", direct), sample("Inside cached", recorded)].spacing(16),
                sample("Over a button", self.panel(card)),
                text("The overlay leaves the Play button interactive. Both previews use the same parameters.").size(14),
                control("Centre X",self.center.x,-0.5..=1.5,Message::X),
                control("Centre Y",self.center.y,-0.5..=1.5,Message::Y),
                control("Horizontal radius",self.radii.x,0.05..=1.5,Message::Horizontal),
                control("Vertical radius",self.radii.y,0.05..=1.5,Message::Vertical),
                control("Opacity",self.opacity,0.0..=1.0,Message::Opacity),
                control("Corner radius",self.corners,0.0..=80.0,Message::Corners),
            ].spacing(16).width(Length::Fill),
        ).view()
    }
}

impl RadialPage {
    fn panel<'a>(&self, content: Element<'a, Message>) -> Element<'a, Message> {
        let background = self.luminate.theme().palette.gray.s50;
        let corners = self.corners;
        container(content)
            .style(move |_| container::Style {
                background: Some(background.into()),
                border: iced::Border {
                    radius: corners.into(),
                    ..Default::default()
                },
                ..Default::default()
            })
            .into()
    }
}
