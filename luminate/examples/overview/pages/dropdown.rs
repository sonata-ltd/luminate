//! Dropdown: label and hint, search, an error and a 10 000-option virtualized menu.

use iced_luminate::iced::widget::column;
use iced_luminate::router::{Action, Page, Registry};
use iced_luminate::widget::dropdown::dropdown;
use iced_luminate::{Element, Luminate, Renderer, Theme};

use crate::hero::{Hero, category};
use crate::iso::scenes_flat;

/// Messages of the dropdown page.
#[derive(Debug, Clone)]
pub(crate) enum Message {
    /// A language was picked.
    Language(String),
    /// A city was picked.
    City(String),
    /// The city search query changed.
    CityQuery(String),
    /// An item of the long list was picked.
    Item(String),
    /// The item search query changed.
    ItemQuery(String),
}

/// Dropdowns bound to their own selection and search state.
pub(crate) struct DropdownPage {
    luminate: Luminate,
    languages: Vec<String>,
    cities: Vec<String>,
    items: Vec<String>,
    language: Option<String>,
    city: Option<String>,
    city_query: String,
    item: Option<String>,
    item_query: String,
}

impl Page for DropdownPage {
    type Message = Message;
    type NavigationOptions = ();
    type Context = Luminate;
    type Theme = Theme;
    type Renderer = Renderer;

    fn new(luminate: &Luminate, _: &Registry) -> Self {
        let strings = |values: &[&str]| values.iter().map(ToString::to_string).collect();

        Self {
            luminate: luminate.clone(),
            languages: strings(&["Rust", "Zig", "Go", "Kotlin", "Swift"]),
            cities: strings(&["Berlin", "Tokyo", "Moscow", "Washington", "Amsterdam"]),
            items: (1..=10_000).map(|id| format!("Item {id}")).collect(),
            language: None,
            city: None,
            city_query: String::new(),
            item: Some(String::from("Item 5000")),
            item_query: String::new(),
        }
    }

    fn update(&mut self, message: Message) -> Action<Message> {
        match message {
            Message::Language(value) => self.language = Some(value),
            Message::City(value) => self.city = Some(value),
            Message::CityQuery(value) => self.city_query = value,
            Message::Item(value) => self.item = Some(value),
            Message::ItemQuery(value) => self.item_query = value,
        }

        Action::none()
    }

    fn view(&self) -> Element<'_, Message> {
        let luminate = &self.luminate;

        Hero::new(
            luminate,
            &scenes_flat::INPUTS,
            "Dropdown",
            "A select with search and a virtualized menu",
            example(self),
        )
        .showcase(
            luminate.dropdown(
                dropdown(&self.languages, self.language.as_ref(), Message::Language)
                    .placeholder("Pick a language")
                    .label("Language")
                    .hint("Click to open the menu"),
            ),
        )
        .view()
    }
}

fn example(data: &DropdownPage) -> Element<'_, Message> {
    let luminate = &data.luminate;

    let error = data.city.is_none().then_some("Pick a city");

    category(
        luminate,
        "Variants",
        column![
            luminate.dropdown(
                dropdown(&data.cities, data.city.as_ref(), Message::City)
                    .placeholder("Search a city")
                    .label("City")
                    .hint("Type to filter, any script works")
                    .error(error)
                    .search(&data.city_query, Message::CityQuery)
            ),
            luminate.dropdown(
                dropdown(&data.items, data.item.as_ref(), Message::Item)
                    .placeholder("Pick an item")
                    .label("10 000 options")
                    .hint("Only the visible items are built")
                    .search(&data.item_query, Message::ItemQuery)
            ),
            luminate.dropdown(
                dropdown(&data.languages, None::<&String>, Message::Language)
                    .placeholder("Disabled")
                    .label("No handler")
                    .on_select_maybe(None::<fn(String) -> Message>)
            ),
        ]
        .spacing(luminate.theme().spacing.xl),
    )
}
