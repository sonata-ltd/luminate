#![doc = include_str!("../README.md")]

pub mod curves;
pub mod widget;

#[doc(hidden)]
pub mod testing;

mod decay;
mod engine;
mod host;
mod key;
mod length;
mod set;
mod shape;
mod sized;
mod spring;
mod track;
mod value;

pub use decay::Decay;
pub use engine::{Motion, Presence, TickStatus};
pub use key::MotionKey;
pub use length::AnimLength;
pub use set::MotionSet;
pub use spring::{Spring, SpringParams};
pub use track::{Curve, CurveKind, Easing, Tier};
pub use value::{Anim, Animatable, MAX_COMPONENTS};

/// Support items the `key!` and `motion_set!` macros expand to.
///
/// Not part of the public API: anything here may change without notice.
#[doc(hidden)]
pub mod __private {
    pub use crate::key::site_hash;
}
