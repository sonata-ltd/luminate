//! Vector paths the engine can animate: drawn on, morphed and followed.
//!
//! A path cannot be an [`Animatable`](crate::Animatable): it holds far more
//! than [`MAX_COMPONENTS`](crate::MAX_COMPONENTS) numbers. What moves is a
//! scalar — which part of the stroke is drawn, how far a morph has got,
//! where along the path something is — and the geometry is worked out from
//! it where it is drawn. Everything expensive (normalising into cubics,
//! measuring arc length, matching two shapes) happens once per path, not
//! once per frame.

mod cubic;
mod data;
mod length;

pub use data::{PathBuilder, PathData, PathError};

#[allow(unused_imports)]
pub(crate) use cubic::Cubic;
#[allow(unused_imports)]
pub(crate) use data::Subpath;
#[allow(unused_imports)] // used by tasks 5, 7
pub(crate) use length::ArcLength;
