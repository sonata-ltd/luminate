//! The widgets that read animated values where they are used.
//!
//! A stock iced widget takes its `Length`, `Padding` and colours when it is
//! constructed, while the view is being built. A value read there is a
//! snapshot and holds still until the next rebuild. The three widgets here
//! store [`Anim`] handles instead and resolve them inside their own `layout`
//! and `draw`, on the frame being painted:
//!
//! | Widget | Resolves in | Animates |
//! |---|---|---|
//! | [`Shape`] | `draw` | fill, border, corner radius (and its own size) |
//! | [`Sized`] | `layout` | width, height, padding and collapse of any child |
//! | [`Host`] | `update` | nothing, it advances the clock for everything below it |
//! | `PathShape` | `draw` | stroke, fill, drawn range, morph, pose of a vector path (feature `geometry`) |
//!
//! Each has a free-function constructor in iced's style: [`shape()`],
//! [`sized()`], [`host()`].
//!
//! [`Anim`]: crate::Anim

pub use crate::host::{Host, host};
#[cfg(feature = "geometry")]
pub use crate::path_render::MeshSupport;
#[cfg(feature = "geometry")]
pub use crate::path_widget::{FillRule, LineCap, LineJoin, PathShape, PathSource, path};
pub use crate::shape::{PixelSnap, Shape, shape};
pub use crate::sized::{Sized, sized};
