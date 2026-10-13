//! What to build, as plain data.
//!
//! A descriptor is a struct with public fields and builder methods; it says
//! *what* a control is (its label, its size, the message it publishes) and
//! nothing about how it looks. [`Luminate`](crate::Luminate) turns each one into
//! an [`Element`](crate::Element) drawn with the [`Theme`](crate::Theme)
//! the application runs with.
//!
//! ```
//! use iced_luminate::descriptor::{Button, ButtonHierarchy, ButtonSize};
//!
//! #[derive(Debug, Clone)]
//! enum Message {
//!     Save,
//! }
//!
//! let save = Button::new("Save")
//!     .hierarchy(ButtonHierarchy::Primary)
//!     .size(ButtonSize::Medium)
//!     .on_press(Message::Save);
//! assert!(save.on_press.is_some());
//! ```

mod button;
mod card;
mod input;
mod pager;
mod sidebar;
mod tabs;

pub use button::{Button, ButtonContent, ButtonHierarchy, ButtonSize};
pub use card::Card;
pub use input::Input;
pub use pager::Pager;
pub use sidebar::{Axis, Sidebar};
pub use tabs::{TabItem, Tabs};
