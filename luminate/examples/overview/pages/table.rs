use iced::font::Weight;
use iced::widget::{Space, column, container};
use iced::{Length, Padding};
use iced_luminate::descriptor::Button;
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::theme::typography::{TextSize, TextStyle, styled_text};
use iced_luminate::widget::table::column as table_column;
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::Hero;
use crate::iso::scenes_flat;

#[derive(Debug, Clone)]
pub(crate) enum Message {
    ActionPressed,
    Selected(u32),
}

pub(crate) struct TablePage {
    luminate: Luminate,
    users: Vec<User>,
    selected: Option<u32>,
}

#[derive(Debug, Clone)]
struct User {
    id: u32,
    name: String,
    email: String,
    active: bool,
}

impl Page for TablePage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        let users = (1..=10_000)
            .map(|id| User {
                id,
                name: format!("User {id}"),
                email: format!("user{id}@example.com"),
                active: id % 3 != 0,
            })
            .collect();

        Self {
            luminate: luminate.clone(),
            users,
            selected: None,
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::ActionPressed => eprintln!("table: action pressed"),
            Message::Selected(id) => self.selected = Some(id),
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        column![
            Hero::new(
                luminate,
                &scenes_flat::BUTTONS,
                "Table",
                "A virtualized selectable table",
                Space::new(),
            )
            .showcase(luminate.button(Button::new("Action").on_press(Message::ActionPressed)))
            .view(),
            container(example(luminate, &self.users, self.selected))
                .padding(Padding::from(luminate.theme().spacing.xl))
        ]
        .into()
    }
}

fn example<'a>(
    luminate: &Luminate,
    users: &'a [User],
    selected: Option<u32>,
) -> Element<'a, Message> {
    let header_text = |content: &'static str| -> Element<'static, Message> {
        styled_text(content, TextStyle::text(TextSize::Xs, Weight::Semibold)).into()
    };

    let row_text = |content: String| -> Element<'static, Message> {
        styled_text(content, TextStyle::text(TextSize::Sm, Weight::Normal)).into()
    };

    luminate
        .table(
            [
                table_column(header_text("ID"), move |user: &User| {
                    row_text(user.id.to_string())
                })
                .width(70),
                table_column(header_text("Name"), move |user: &User| {
                    row_text(user.name.clone())
                })
                .width(180),
                table_column(header_text("Email"), move |user: &User| {
                    row_text(user.email.clone())
                }),
                table_column(header_text("Status"), move |user: &User| {
                    row_text(if user.active {
                        "Active".to_string()
                    } else {
                        "Disabled".to_string()
                    })
                })
                .width(Length::Fill),
            ],
            users,
        )
        .on_select(|user| Message::Selected(user.id))
        .selected(move |user| Some(user.id) == selected)
        .disabled(|user| !user.active)
        .row_key(|user| user.id)
        .striped(true)
        .width(Length::Fill)
        .height(400)
        .into()
}
