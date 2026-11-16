//! Collapsible bars sharing one animation, layout and event-handling core.
//!
//! [`Sidebar`] is vertical by default and shades its inner trailing edge.
//! [`Header`] is horizontal by default and casts a configurable external
//! shadow.

mod compute_layout;
mod core;
mod icons;

pub mod header;
pub mod sidebar;

pub use header::{EdgeShadow, Header};
pub use sidebar::Sidebar;
