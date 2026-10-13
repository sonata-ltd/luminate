use iced::{Padding, widget::container};
use iced_luminate::{Element, Luminate, Renderer, Theme, descriptor::Tabs as TabBar};
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
            self.luminate.tabs(
                TabBar::new(["Section 1", "Section 2", "Section 3"])
                    .active(self.active_tab)
                    .on_select(Message::SelectTab),
            ),
        )
        .padding(Padding::from(5))
        .into()
    }
}
