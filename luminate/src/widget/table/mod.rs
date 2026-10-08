//! A virtualized table with optional row selection.

mod layout;
mod row;
mod style;
mod widget;

use std::cell::{Cell, RefCell};
use std::fmt;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::rc::Rc;

use iced::advanced::widget::Id;
use iced::widget::Space;
use iced::{Length, Task};
use iced::{Padding, alignment};
use iced_animate::Motion;

use crate::widget::fading_scrollable::fading_scrollable;
use crate::widget::pick_indicator::{self, Pick};
use crate::widget::virtual_list::VirtualList;
use crate::{Element, Theme};

pub use style::{Catalog, Status, Style, StyleFn};

/// A table column: a header and a function that builds the cell of a row.
pub struct Column<'a, T, Message> {
    header: Element<'a, Message>,
    view: Box<dyn Fn(&'a T) -> Element<'a, Message> + 'a>,
    width: Length,
    align_x: alignment::Horizontal,
    align_y: alignment::Vertical,
}

/// A column with `header`; `view` builds the cell of a row.
pub fn column<'a, T, E, Message>(
    header: impl Into<Element<'a, Message>>,
    view: impl Fn(&'a T) -> E + 'a,
) -> Column<'a, T, Message>
where
    T: 'a,
    E: Into<Element<'a, Message>>,
{
    Column {
        header: header.into(),
        view: Box::new(move |row| view(row).into()),
        width: Length::Fill,
        align_x: alignment::Horizontal::Left,
        align_y: alignment::Vertical::Center,
    }
}

impl<T, Message> Column<'_, T, Message> {
    /// Width of the column: `Fixed` as is, `Shrink` to the header, `Fill` and `FillPortion` share the rest.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Horizontal alignment of the cells.
    #[must_use]
    pub fn align_x(mut self, alignment: impl Into<alignment::Horizontal>) -> Self {
        self.align_x = alignment.into();
        self
    }

    /// Vertical alignment of the cells within a row.
    #[must_use]
    pub fn align_y(mut self, alignment: impl Into<alignment::Vertical>) -> Self {
        self.align_y = alignment.into();
        self
    }
}

/// A table builder. It stores what the cells are built from, not the cells.
pub struct Table<'a, T, Message> {
    columns: Vec<Column<'a, T, Message>>,
    rows: &'a [T],
    on_select: Option<Box<dyn Fn(&'a T) -> Message + 'a>>,
    selected: Option<Box<dyn Fn(&'a T) -> bool + 'a>>,
    pending: Option<Box<dyn Fn(&'a T) -> bool + 'a>>,
    disabled: Option<Box<dyn Fn(&'a T) -> bool + 'a>>,
    row_key: Option<Box<dyn Fn(&'a T) -> u64 + 'a>>,
    placeholder: Option<Element<'a, Message>>,
    width: Length,
    height: Length,
    row_height: f32,
    header_height: f32,
    cell_padding_x: f32,
    inset: Padding,
    indicator: Option<pick_indicator::Metrics>,
    striped: bool,
    motion: Option<Motion>,
    id: Option<Id>,
    class: <Theme as Catalog>::Class<'a>,
}

/// A table of `rows` with the given `columns`.
pub fn table<'a, T, Message>(
    columns: impl IntoIterator<Item = Column<'a, T, Message>>,
    rows: &'a [T],
) -> Table<'a, T, Message> {
    Table {
        columns: columns.into_iter().collect(),
        rows,
        on_select: None,
        selected: None,
        pending: None,
        disabled: None,
        row_key: None,
        placeholder: None,
        width: Length::Fill,
        height: Length::Shrink,
        row_height: 32.0,
        header_height: 28.0,
        cell_padding_x: 8.0,
        inset: Padding::default(),
        indicator: Some(pick_indicator::Metrics::default()),
        striped: false,
        motion: None,
        id: None,
        class: <Theme as Catalog>::default(),
    }
}

impl<'a, T, Message> Table<'a, T, Message> {
    /// Enables selection: a click or Enter on a row publishes `on_select(row)`.
    #[must_use]
    pub fn on_select(mut self, on_select: impl Fn(&'a T) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }

    /// Like `on_select`, if `Some`; `None` disables selection.
    #[must_use]
    pub fn on_select_maybe<F>(mut self, on_select: Option<F>) -> Self
    where
        F: Fn(&'a T) -> Message + 'a,
    {
        self.on_select = on_select.map(|f| Box::new(f) as Box<dyn Fn(&'a T) -> Message + 'a>);
        self
    }

    /// Which rows are selected. The application owns the selection.
    #[must_use]
    pub fn selected(mut self, selected: impl Fn(&'a T) -> bool + 'a) -> Self {
        self.selected = Some(Box::new(selected));
        self
    }

    /// Which rows are being applied: picked, not confirmed yet.
    #[must_use]
    pub fn pending(mut self, pending: impl Fn(&'a T) -> bool + 'a) -> Self {
        self.pending = Some(Box::new(pending));
        self
    }

    /// Which rows cannot be selected.
    #[must_use]
    pub fn disabled(mut self, disabled: impl Fn(&'a T) -> bool + 'a) -> Self {
        self.disabled = Some(Box::new(disabled));
        self
    }

    /// A key for a row, so that its state follows the data when rows are reordered. Keys must be unique.
    #[must_use]
    pub fn row_key<K: Hash>(mut self, key: impl Fn(&'a T) -> K + 'a) -> Self {
        self.row_key = Some(Box::new(move |row| {
            let mut hasher = DefaultHasher::new();
            key(row).hash(&mut hasher);
            hasher.finish()
        }));
        self
    }

    /// What to show when there are no rows.
    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<Element<'a, Message>>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Width of the table.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Height of the table. `Shrink` scrolls with the page; `Fixed` and `Fill` scroll on their own under a pinned header.
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Height of a row. The same for all rows, which virtualization needs.
    #[must_use]
    pub fn row_height(mut self, height: f32) -> Self {
        self.row_height = height;
        self
    }

    /// Height of the header.
    #[must_use]
    pub fn header_height(mut self, height: f32) -> Self {
        self.header_height = height;
        self
    }

    /// Horizontal padding of a cell.
    #[must_use]
    pub fn cell_padding_x(mut self, padding: f32) -> Self {
        self.cell_padding_x = padding;
        self
    }

    /// Padding between the table edge and its rows.
    #[must_use]
    pub fn inset(mut self, inset: impl Into<Padding>) -> Self {
        self.inset = inset.into();
        self
    }

    /// Dimensions of the pick indicator; `None` hides it. It shows only with `on_select`.
    #[must_use]
    pub fn indicator(mut self, metrics: Option<pick_indicator::Metrics>) -> Self {
        self.indicator = metrics;
        self
    }

    /// Gives odd rows their own background.
    #[must_use]
    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    /// Animation engine for the indicator and smooth scrolling.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// Id of the table, for focus operations and `scroll_to_row`.
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
}

struct Meta<Message> {
    pick: Pick,
    disabled: bool,
    on_press: Option<Message>,
}

struct Rows<'a, Message> {
    count: usize,
    cells: Box<dyn Fn(usize) -> Vec<Element<'a, Message>> + 'a>,
    meta: Box<dyn Fn(usize) -> Meta<Message> + 'a>,
    disabled: Box<dyn Fn(usize) -> bool + 'a>,
}

struct Logic<'a, T, Message> {
    on_select: Option<Box<dyn Fn(&'a T) -> Message + 'a>>,
    selected: Option<Box<dyn Fn(&'a T) -> bool + 'a>>,
    pending: Option<Box<dyn Fn(&'a T) -> bool + 'a>>,
    disabled: Option<Box<dyn Fn(&'a T) -> bool + 'a>>,
    any_pending: bool,
}

impl<'a, Message: 'a> Rows<'a, Message> {
    fn new<T: 'a>(
        rows: &'a [T],
        views: Vec<Box<dyn Fn(&'a T) -> Element<'a, Message> + 'a>>,
        logic: Logic<'a, T, Message>,
    ) -> Self {
        let Logic {
            on_select,
            selected,
            pending,
            disabled,
            any_pending,
        } = logic;

        let disabled: Rc<dyn Fn(&'a T) -> bool + 'a> = match disabled {
            Some(disabled) => Rc::from(disabled),
            None => Rc::new(|_| false),
        };

        Self {
            count: rows.len(),
            cells: Box::new(move |i| views.iter().map(|view| view(&rows[i])).collect()),
            meta: {
                let disabled = Rc::clone(&disabled);
                Box::new(move |i| {
                    let row = &rows[i];
                    let is_disabled = disabled(row);

                    Meta {
                        pick: Pick::resolve(
                            selected.as_ref().is_some_and(|selected| selected(row)),
                            pending.as_ref().is_some_and(|pending| pending(row)),
                            any_pending,
                        ),
                        disabled: is_disabled,
                        on_press: if is_disabled {
                            None
                        } else {
                            on_select.as_ref().map(|on_select| on_select(row))
                        },
                    }
                })
            },
            disabled: Box::new(move |i| disabled(&rows[i])),
        }
    }
}

/// What the rows read while they draw. Created anew on every `view` and filled
/// by the table before it lays out the body.
struct Shared<'a> {
    spans: RefCell<Vec<layout::Span>>,
    aligns: Vec<(alignment::Horizontal, alignment::Vertical)>,
    active: Cell<Option<usize>>,
    keyboard: Cell<bool>,
    focused: Cell<bool>,
    pressed: Cell<Option<usize>>,
    indicator: Option<pick_indicator::Metrics>,
    striped: bool,
    count: usize,
    motion: Option<Motion>,
    class: <Theme as Catalog>::Class<'a>,
}

impl<'a, T, Message> From<Table<'a, T, Message>> for Element<'a, Message>
where
    T: 'a,
    Message: Clone + 'a,
{
    fn from(table: Table<'a, T, Message>) -> Self {
        let Table {
            columns,
            rows,
            on_select,
            selected,
            pending,
            disabled,
            row_key,
            placeholder,
            width,
            height,
            row_height,
            header_height,
            cell_padding_x,
            inset,
            indicator,
            striped,
            motion,
            id,
            class,
        } = table;

        let indicator = indicator.filter(|_| on_select.is_some());

        let any_pending = pending
            .as_ref()
            .is_some_and(|pending| rows.iter().any(pending));

        let mut headers = Vec::with_capacity(columns.len());
        let mut widths = Vec::with_capacity(columns.len());
        let mut aligns = Vec::with_capacity(columns.len());
        let mut views = Vec::with_capacity(columns.len());

        for column in columns {
            headers.push(column.header);
            widths.push(column.width);
            aligns.push((column.align_x, column.align_y));
            views.push(column.view);
        }

        let data = Rc::new(Rows::new(
            rows,
            views,
            Logic {
                on_select,
                selected,
                pending,
                disabled,
                any_pending,
            },
        ));

        let shared = Rc::new(Shared {
            spans: RefCell::new(Vec::new()),
            aligns,
            active: Cell::new(None),
            keyboard: Cell::new(false),
            focused: Cell::new(false),
            pressed: Cell::new(None),
            indicator,
            striped,
            count: rows.len(),
            motion: motion.clone(),
            class,
        });

        let scrolls = !matches!(height, Length::Shrink);

        let body: Element<'a, Message> = if rows.is_empty() {
            placeholder.unwrap_or_else(|| Space::new().into())
        } else {
            let list = VirtualList::new(rows.len(), row_height, {
                let data = Rc::clone(&data);
                let shared = Rc::clone(&shared);
                move |i| row::RowWidget::new(i, &data, Rc::clone(&shared)).into()
            })
            .padding(inset);

            let list = match row_key {
                Some(key) => list.key(move |i| key(&rows[i])),
                None => list,
            };

            if scrolls {
                let scroll = fading_scrollable(list).height(Length::Fill);
                match motion {
                    Some(motion) => scroll.motion(motion).into(),
                    None => scroll.into(),
                }
            } else {
                list.into()
            }
        };

        Element::new(widget::TableWidget::new(widget::Parts {
            headers,
            widths,
            body,
            data,
            shared,
            width,
            height,
            row_height,
            header_height,
            cell_padding_x,
            inset,
            scrolls,
            id,
        }))
    }
}

impl<T, Message> fmt::Debug for Table<'_, T, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Table")
            .field("columns", &self.columns.len())
            .field("rows", &self.rows.len())
            .field("selectable", &self.on_select.is_some())
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

impl<T, Message> fmt::Debug for Column<'_, T, Message> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Column")
            .field("width", &self.width)
            .field("align_x", &self.align_x)
            .field("align_y", &self.align_y)
            .finish_non_exhaustive()
    }
}

/// A request to scroll to a row, delivered to a table by `operation::scroll_to_row`.
#[derive(Debug, Default)]
pub struct RowRequest(Option<usize>);

/// Operations on tables.
pub mod operation {
    use iced::Rectangle;
    use iced::advanced::widget::{Id, Operation};

    use super::RowRequest;

    /// Scrolls the table with `id` to `row`, even if the row is not built.
    #[must_use]
    pub fn scroll_to_row<T>(id: Id, row: usize) -> impl Operation<T> {
        struct ScrollToRow {
            target: Id,
            row: usize,
        }

        impl<T> Operation<T> for ScrollToRow {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
                operate(self);
            }

            fn custom(
                &mut self,
                id: Option<&Id>,
                _bounds: Rectangle,
                state: &mut dyn std::any::Any,
            ) {
                if id == Some(&self.target)
                    && let Some(request) = state.downcast_mut::<RowRequest>()
                {
                    request.0 = Some(self.row);
                }
            }
        }

        ScrollToRow { target: id, row }
    }
}

/// Scrolls the table with `id` to `row`, even if the row is not built.
pub fn scroll_to_row<T>(id: impl Into<Id>, row: usize) -> Task<T>
where
    T: Send + 'static,
{
    iced::advanced::widget::operate(operation::scroll_to_row(id.into(), row))
}
