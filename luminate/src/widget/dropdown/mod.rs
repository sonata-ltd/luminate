//! A dropdown with search and a virtualized menu.

mod item;
mod menu;
mod search;
mod style;
mod widget;

use std::borrow::Borrow;
use std::cell::Cell;
use std::fmt::{self, Display};
use std::rc::Rc;
use std::sync::LazyLock;

use iced::advanced::svg;
use iced::advanced::widget::Id;
use iced::widget::text::LineHeight;
use iced::widget::{text, text_input};
use iced::{Background, Border, Color, Font, Length, Padding, Pixels};
use iced_animate::Motion;

use crate::theme::InputClass;
use crate::{Element, Theme};

use item::Item;
pub use style::{Catalog, Status, Style, StyleFn};

/// Whether the dropdown is open or focused. Drives the focus ring of a wrapping `multi_border`.
#[must_use]
pub fn is_engaged(tree: &iced::advanced::widget::Tree) -> bool {
    widget::is_engaged(tree)
}

static CHEVRON: LazyLock<svg::Handle> = LazyLock::new(|| {
    svg::Handle::from_memory(
        br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg>"#
            .as_slice(),
    )
});

static CHECK: LazyLock<svg::Handle> = LazyLock::new(|| {
    svg::Handle::from_memory(
        br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>"#
            .as_slice(),
    )
});

/// A dropdown with an optional search field and a virtualized menu.
pub struct Dropdown<'a, T, Message> {
    options: &'a [T],
    selected: Option<usize>,
    on_select: Option<Box<dyn Fn(T) -> Message + 'a>>,
    view: Option<Box<dyn Fn(&'a T) -> Element<'a, Message> + 'a>>,
    search_key: Option<Box<dyn Fn(&'a T) -> String + 'a>>,
    query: &'a str,
    on_search: Option<Box<dyn Fn(String) -> Message + 'a>>,
    filter: bool,
    placeholder: &'a str,
    empty: &'a str,
    width: Length,
    height: f32,
    padding: Padding,
    handle_size: f32,
    menu_height: f32,
    menu_padding: f32,
    item_height: f32,
    item_padding: Padding,
    text_size: Option<Pixels>,
    line_height: LineHeight,
    font: Option<Font>,
    on_open: Option<Message>,
    on_close: Option<Message>,
    motion: Option<Motion>,
    id: Option<Id>,
    class: <Theme as Catalog>::Class<'a>,
    pub(crate) label: Option<&'a str>,
    pub(crate) hint: Option<&'a str>,
    pub(crate) error: Option<&'a str>,
    pub(crate) action: Option<Element<'a, Message>>,
}

/// A dropdown over `options` with `selected` chosen, publishing `on_select(option)` on a pick.
pub fn dropdown<'a, T, V, Message>(
    options: &'a [T],
    selected: Option<V>,
    on_select: impl Fn(T) -> Message + 'a,
) -> Dropdown<'a, T, Message>
where
    T: PartialEq + 'a,
    V: Borrow<T>,
{
    let selected = selected.and_then(|selected| {
        options
            .iter()
            .position(|option| option == selected.borrow())
    });

    Dropdown {
        options,
        selected,
        on_select: Some(Box::new(on_select)),
        view: None,
        search_key: None,
        query: "",
        on_search: None,
        filter: true,
        placeholder: "",
        empty: "Nothing found",
        width: Length::Fill,
        height: 34.0,
        padding: Padding::from([5.0, 8.0]),
        handle_size: 20.0,
        menu_height: 200.0,
        menu_padding: 6.0,
        item_height: 34.0,
        item_padding: Padding::from([7.0, 8.0]),
        text_size: None,
        line_height: LineHeight::default(),
        font: None,
        on_open: None,
        on_close: None,
        motion: None,
        id: None,
        class: <Theme as Catalog>::default(),
        label: None,
        hint: None,
        error: None,
        action: None,
    }
}

impl<'a, T, Message> Dropdown<'a, T, Message> {
    /// Text shown while nothing is selected.
    #[must_use]
    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// Like the `on_select` of [`dropdown`], if `Some`; `None` disables the dropdown.
    #[must_use]
    pub fn on_select_maybe<F>(mut self, on_select: Option<F>) -> Self
    where
        F: Fn(T) -> Message + 'a,
    {
        self.on_select = on_select.map(|f| Box::new(f) as Box<dyn Fn(T) -> Message + 'a>);
        self
    }

    /// Builds the content of a menu item. Defaults to the option's `Display`.
    #[must_use]
    pub fn item(mut self, view: impl Fn(&'a T) -> Element<'a, Message> + 'a) -> Self {
        self.view = Some(Box::new(view));
        self
    }

    /// Enables search. The application owns the `query` and receives edits through `on_search`.
    #[must_use]
    pub fn search(mut self, query: &'a str, on_search: impl Fn(String) -> Message + 'a) -> Self {
        self.query = query;
        self.on_search = Some(Box::new(on_search));
        self
    }

    /// Text matched against the query for an option. Defaults to its `Display`.
    #[must_use]
    pub fn search_key(mut self, key: impl Fn(&'a T) -> String + 'a) -> Self {
        self.search_key = Some(Box::new(key));
        self
    }

    /// Whether the widget filters options by the query. Disable when the application filters.
    #[must_use]
    pub fn filter(mut self, filter: bool) -> Self {
        self.filter = filter;
        self
    }

    /// Text shown when the search matches nothing.
    #[must_use]
    pub fn empty(mut self, text: &'a str) -> Self {
        self.empty = text;
        self
    }

    /// Width of the field.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Height of the field.
    #[must_use]
    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    /// Padding of the field.
    #[must_use]
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Largest height of the menu.
    #[must_use]
    pub fn menu_height(mut self, height: f32) -> Self {
        self.menu_height = height;
        self
    }

    /// Padding around the items inside the menu.
    #[must_use]
    pub fn menu_padding(mut self, padding: f32) -> Self {
        self.menu_padding = padding;
        self
    }

    /// Padding inside an item.
    #[must_use]
    pub fn item_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.item_padding = padding.into();
        self
    }

    /// Size of the chevron.
    #[must_use]
    pub fn handle_size(mut self, size: f32) -> Self {
        self.handle_size = size;
        self
    }

    pub(crate) fn is_enabled(&self) -> bool {
        self.on_select.is_some()
    }

    /// Height of an item. The same for all items, which virtualization needs.
    #[must_use]
    pub fn item_height(mut self, height: f32) -> Self {
        self.item_height = height;
        self
    }

    /// Text size of the field and the items.
    #[must_use]
    pub fn text_size(mut self, size: impl Into<Pixels>) -> Self {
        self.text_size = Some(size.into());
        self
    }

    /// Line height of the field and the items.
    #[must_use]
    pub fn text_line_height(mut self, line_height: impl Into<LineHeight>) -> Self {
        self.line_height = line_height.into();
        self
    }

    /// Font of the field and the items.
    #[must_use]
    pub fn font(mut self, font: impl Into<Font>) -> Self {
        self.font = Some(font.into());
        self
    }

    /// Message published when the menu opens.
    #[must_use]
    pub fn on_open(mut self, message: Message) -> Self {
        self.on_open = Some(message);
        self
    }

    /// Message published when the menu closes.
    #[must_use]
    pub fn on_close(mut self, message: Message) -> Self {
        self.on_close = Some(message);
        self
    }

    /// Animation engine for the chevron.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// Id of the dropdown, for focus operations.
    #[must_use]
    pub fn id(mut self, id: impl Into<Id>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Style function in place of the theme's style.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self {
        self.class = Box::new(style);
        self
    }

    /// Theme class.
    #[must_use]
    pub fn class(mut self, class: impl Into<<Theme as Catalog>::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    /// Caption shown above the field by `Luminate::dropdown`.
    #[must_use]
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    /// Hint shown below the field by `Luminate::dropdown`.
    #[must_use]
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// Error shown by `Luminate::dropdown`; `None` clears it.
    #[must_use]
    pub fn error(mut self, message: Option<&'a str>) -> Self {
        self.error = message;
        self
    }

    /// Element placed to the right of the field by `Luminate::dropdown`.
    #[must_use]
    pub fn action(mut self, action: impl Into<Element<'a, Message>>) -> Self {
        self.action = Some(action.into());
        self
    }
}

struct Options<'a, Message> {
    count: usize,
    view: Box<dyn Fn(usize) -> Element<'a, Message> + 'a>,
    key: Box<dyn Fn(usize) -> String + 'a>,
    select: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    selected: Option<usize>,
}

struct Shared<'a> {
    hovered: Cell<Option<usize>>,
    chosen: Cell<Option<usize>>,
    class: <Theme as Catalog>::Class<'a>,
    item_padding: Padding,
    check: svg::Handle,
    check_size: f32,
    chevron: svg::Handle,
}

#[derive(Clone, Copy)]
struct TextSettings {
    size: Option<Pixels>,
    line_height: LineHeight,
    font: Option<Font>,
}

impl TextSettings {
    fn text<'a>(self, label: String) -> iced::widget::Text<'a, Theme, crate::Renderer> {
        let mut label = text(label).line_height(self.line_height);
        if let Some(size) = self.size {
            label = label.size(size);
        }
        if let Some(font) = self.font {
            label = label.font(font);
        }
        label
    }
}

impl<'a, Message: 'a> Options<'a, Message> {
    fn new<T: Clone + Display + 'a>(
        options: &'a [T],
        selected: Option<usize>,
        on_select: Option<Box<dyn Fn(T) -> Message + 'a>>,
        view: Option<Box<dyn Fn(&'a T) -> Element<'a, Message> + 'a>>,
        search_key: Option<Box<dyn Fn(&'a T) -> String + 'a>>,
        settings: TextSettings,
    ) -> Self {
        Self {
            count: options.len(),
            view: Box::new(move |i| match &view {
                Some(view) => view(&options[i]),
                None => settings.text(options[i].to_string()).into(),
            }),
            key: Box::new(move |i| match &search_key {
                Some(key) => key(&options[i]),
                None => options[i].to_string(),
            }),
            select: on_select.map(|on_select| {
                Box::new(move |i: usize| on_select(options[i].clone()))
                    as Box<dyn Fn(usize) -> Message + 'a>
            }),
            selected,
        }
    }
}

fn search_input<'a, Message: Clone + 'a>(
    hint: &str,
    query: &'a str,
    on_search: Rc<dyn Fn(String) -> Message + 'a>,
    settings: TextSettings,
    id: Option<Id>,
) -> Element<'a, Message> {
    let mut input = text_input(hint, query)
        .on_input(move |query| on_search(query))
        .padding(0)
        .line_height(settings.line_height)
        .width(Length::Fill)
        .class(InputClass::Custom(Box::new(|theme: &Theme, _status| {
            let tokens = theme.input;
            text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                icon: tokens.text,
                placeholder: tokens.placeholder,
                value: tokens.text,
                selection: tokens.ring,
            }
        })));

    if let Some(size) = settings.size {
        input = input.size(size);
    }
    if let Some(font) = settings.font {
        input = input.font(font);
    }
    if let Some(id) = id {
        input = input.id(id);
    }

    input.into()
}

impl<'a, T, Message> From<Dropdown<'a, T, Message>> for Element<'a, Message>
where
    T: Clone + Display + 'a,
    Message: Clone + 'a,
{
    fn from(dropdown: Dropdown<'a, T, Message>) -> Self {
        let Dropdown {
            options,
            selected,
            on_select,
            view,
            search_key,
            query,
            on_search,
            filter,
            placeholder,
            empty,
            width,
            height,
            padding,
            handle_size,
            menu_height,
            menu_padding,
            item_height,
            item_padding,
            text_size,
            line_height,
            font,
            on_open,
            on_close,
            motion,
            id,
            class,
            ..
        } = dropdown;

        let settings = TextSettings {
            size: text_size,
            line_height,
            font,
        };

        let selected_label = selected.map(|i| options[i].to_string());
        let on_search: Option<Rc<dyn Fn(String) -> Message + 'a>> = on_search.map(Rc::from);

        let field = match &on_search {
            Some(on_search) => search_input(
                selected_label.as_deref().unwrap_or(placeholder),
                query,
                Rc::clone(on_search),
                settings,
                id.clone(),
            ),
            None => settings
                .text(selected_label.unwrap_or_else(|| placeholder.to_owned()))
                .into(),
        };

        Element::new(widget::DropdownWidget {
            options: Rc::new(Options::new(
                options, selected, on_select, view, search_key, settings,
            )),
            field,
            searchable: on_search.is_some(),
            filter,
            query,
            on_search,
            on_open,
            on_close,
            width,
            height,
            padding,
            handle_size,
            menu_height,
            menu_padding,
            item_height,
            empty,
            motion,
            id,
            shared: Rc::new(Shared {
                hovered: Cell::new(None),
                chosen: Cell::new(None),
                class,
                item_padding,
                check: CHECK.clone(),
                check_size: 20.0,
                chevron: CHEVRON.clone(),
            }),
            menu: None,
            indices: None,
        })
    }
}

impl<T, Message> fmt::Debug for Dropdown<'_, T, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Dropdown")
            .field("options", &self.options.len())
            .field("selected", &self.selected)
            .field("searchable", &self.on_search.is_some())
            .field("width", &self.width)
            .finish_non_exhaustive()
    }
}
