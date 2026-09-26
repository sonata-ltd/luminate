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
mod fit;
mod length;
mod morph;
mod motion;
mod values;

#[cfg(feature = "svg-path")]
mod parse;

pub use data::{PathBuilder, PathData, PathError};
pub use fit::{Fit, Placement};
pub use morph::Morph;
pub use motion::MotionPath;
pub use values::{DrawRange, Pose};

#[allow(unused_imports)] // not yet used outside `path`
pub(crate) use cubic::Cubic;
#[allow(unused_imports)] // used by the path widget (feature `geometry`)
pub(crate) use data::Subpath;
pub(crate) use length::ArcLength;
