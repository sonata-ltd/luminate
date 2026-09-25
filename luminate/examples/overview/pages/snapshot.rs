//! `Lifecycle::Drop` with `into_snapshot` / `restore`.
//!
//! This page is **dropped** when the user leaves it (the default lifecycle).
//! Before it goes, `into_snapshot` hands the router a boxed value; the next
//! `new` instance receives it through `restore`. Here the value is the click
//! count, so it survives although the page does not. Compare
//! [`inputs`](crate::pages::inputs), which is suspended instead.

use std::any::Any;

use iced_luminate::descriptor::Button;
use iced_luminate::iced::widget::{column, text};
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;

/// Messages of the snapshot page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// Count one more click.
    Increment,
}

/// A counter that outlives its page instance.
pub(crate) struct SnapshotPage {
    luminate: Luminate,
    count: u32,
}

impl Page for SnapshotPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        Self {
            luminate: luminate.clone(),
            count: 0,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Increment => self.count += 1,
        }
        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let theme = self.luminate.theme();

        Hero::new(
            &self.luminate,
            "Snapshot",
            "Pages captured and restored",
            column![
                text(format!("clicked {} times", self.count)),
                text("Leave and come back: the page is dropped, the count is restored.").size(12),
                self.luminate
                    .button(Button::new("Click").on_press(Message::Increment)),
            ]
            .spacing(theme.spacing.xl),
        )
        .build()
    }

    fn into_snapshot(self) -> Option<Box<dyn Any>> {
        Some(Box::new(self.count))
    }

    fn restore(&mut self, snapshot: Box<dyn Any>) {
        if let Ok(count) = snapshot.downcast::<u32>() {
            self.count = *count;
        }
    }
}
