//! The page icons, one [`Scene`] each.
//!
//! Scene units are arbitrary: a [`Scene`] is rescaled to its canvas, so what
//! matters is the proportion between shapes, not their absolute size.
//!
//! Three rules keep the set coherent. A shape laid on another gets a higher
//! `z`, which is what puts it on top. Depth reads through elevation and
//! overlap, never through light. And an icon leads with at most one opaque
//! [`Tone::Accent`] shape, so the eye lands in the same place on every page.
//!
//! The extruded ancestors of these scenes are kept, uncompiled, in
//! `scenes.rs` next door.

use iced::Color;

use super::{Element3, Scene, Shape, Tone, Vec3};

/// Two pills, one behind the other.
pub(crate) const BUTTONS: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Pill,
            Vec3::new(-4.0, -4.0, 8.0),
            (32.0, 12.0),
            Tone::Muted,
        ),
        Element3::new(
            Shape::Pill,
            Vec3::new(-4.0, -4.0, 0.0),
            (32.0, 12.0),
            Tone::Custom(Color::from_rgb8(230, 230, 230)),
        ),
        Element3::new(
            Shape::Pill,
            Vec3::new(4.0, 4.0, 0.0),
            (32.0, 12.0),
            Tone::Accent,
        ),
    ],
};

/// A field with a caret resting on it.
pub(crate) const INPUTS: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (36.0, 16.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 1.5 },
            Vec3::new(-9.0, 0.0, 2.0),
            (3.0, 10.0),
            Tone::Accent,
        ),
    ],
};

/// A plate and the copy lifted off it.
pub(crate) const SNAPSHOT: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 5.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (30.0, 30.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 5.0 },
            Vec3::new(0.0, 0.0, 12.0),
            (30.0, 30.0),
            Tone::Accent,
        )
        .ghost(0.6),
    ],
};

/// Two stacked cards, the front one carrying a row.
pub(crate) const CARD: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(-8.0, -8.0, 0.0),
            (26.0, 26.0),
            Tone::Muted,
        ),
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (26.0, 26.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, 0.0, 2.0),
            (16.0, 4.0),
            Tone::Accent,
        ),
    ],
};

/// One plate and the trail it left.
pub(crate) const MOTION: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(-15.0, 0.0, 0.0),
            (13.0, 13.0),
            Tone::Neutral,
        )
        .ghost(0.3),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(-5.0, 0.0, 0.0),
            (13.0, 13.0),
            Tone::Neutral,
        )
        .ghost(0.5),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(5.0, 0.0, 0.0),
            (13.0, 13.0),
            Tone::Neutral,
        )
        .ghost(0.7),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(15.0, 0.0, 0.0),
            (13.0, 13.0),
            Tone::Accent,
        ),
    ],
};

/// Three bars of one length and growing thickness. Without extrusion the
/// weight axis has to read in the plane, so it is the bars that thicken.
pub(crate) const WEIGHT: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, -13.0, 0.0),
            (28.0, 4.0),
            Tone::Muted,
        ),
        Element3::new(
            Shape::Bar { radius: 3.5 },
            Vec3::new(0.0, -1.0, 0.0),
            (28.0, 7.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 5.5 },
            Vec3::new(0.0, 13.0, 0.0),
            (28.0, 11.0),
            Tone::Accent,
        ),
    ],
};

/// A panel with three controls laid on it.
pub(crate) const SHOWCASE: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (38.0, 34.0),
            Tone::Muted,
        ),
        Element3::new(
            Shape::Bar { radius: 2.5 },
            Vec3::new(0.0, -10.0, 2.0),
            (26.0, 5.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 2.0 },
            Vec3::new(-8.0, 2.0, 2.0),
            (10.0, 10.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Pill,
            Vec3::new(7.0, 4.0, 2.0),
            (14.0, 6.0),
            Tone::Accent,
        ),
    ],
};

/// A page, a sidebar on it, and a second sidebar inside that one.
pub(crate) const NESTED_SIDEBAR: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (38.0, 30.0),
            Tone::Muted,
        ),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(0.0, -9.0, 2.0),
            (34.0, 10.0),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 2.0 },
            Vec3::new(0.0, -9.0, 4.0),
            (24.0, 4.0),
            Tone::Accent,
        ),
    ],
};

/// A heading over three lines of body text.
pub(crate) const TYPOGRAPHY: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Bar { radius: 3.0 },
            Vec3::new(0.0, -12.0, 0.0),
            (18.0, 6.0),
            Tone::Accent,
        ),
        Element3::new(
            Shape::Bar { radius: 1.75 },
            Vec3::new(0.0, -2.0, 0.0),
            (30.0, 3.5),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 1.75 },
            Vec3::new(0.0, 4.0, 0.0),
            (30.0, 3.5),
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 1.75 },
            Vec3::new(0.0, 10.0, 0.0),
            (22.0, 3.5),
            Tone::Muted,
        ),
    ],
};

/// Every scene, for the tests that hold the set to one standard.
#[cfg(test)]
pub(crate) const ALL: &[&Scene] = &[
    &BUTTONS,
    &INPUTS,
    &SNAPSHOT,
    &CARD,
    &MOTION,
    &WEIGHT,
    &SHOWCASE,
    &NESTED_SIDEBAR,
    &TYPOGRAPHY,
];
