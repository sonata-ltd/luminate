use iced::{Alignment, Color, Length, Padding, font::Weight, widget::container};
use iced_luminate::{
    Element, Luminate, Renderer, Theme,
    theme::typography::{TextSize, TextStyle, styled_text},
    widget::tabs::tabs,
};
use iced_page_router::{Action, Page};

pub(crate) struct Tabs {
    luminate: Luminate,
    active_tab: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    SelectTab(usize),
}

impl Page for Tabs {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(context: &Self::Context, _: &iced_page_router::Registry) -> Self {
        Self {
            luminate: context.clone(),
            active_tab: 0,
        }
    }

    fn update(&mut self, message: Self::Message) -> Action<Self::Message> {
        match message {
            Message::SelectTab(i) => self.active_tab = i,
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        container(
            tabs([
                tab_item("Section 1"),
                tab_item("Section 2"),
                tab_item("Section 3"),
            ])
            .active(self.active_tab)
            .on_select(Message::SelectTab)
            .motion(self.luminate.motion().clone()),
        )
        .padding(Padding::from(5))
        .into()
    }
}

fn tab_item(label: &str) -> Element<'_, Message> {
    container(
        styled_text(label, TextStyle::text(TextSize::Sm, Weight::Normal))
            .color(Color::from_rgb8(25, 31, 38))
            .width(Length::Fill)
            .align_x(Alignment::Center),
    )
    .padding(Padding::from(3))
    .width(Length::Fill)
    .into()
}
