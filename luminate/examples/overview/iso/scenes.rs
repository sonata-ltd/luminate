//! ARCHIVE: the extruded version of the icon set, kept for reference.
//!
//! This file is deliberately NOT declared as a module in `iso/mod.rs`, so it
//! is never compiled. It targets the earlier model, where an element carried
//! a `height` and a `shadow` and `Shape` had a `Plate` variant, and it will
//! not build against the current one without being ported.
//!
//! The page icons, one `Scene` each.
//!
//! Scene units are arbitrary: [`Scene`] is rescaled to its canvas, so what
//! matters is the proportion between elements, not their absolute size.
//!
//! Two rules keep the set coherent. Prisms must not pass through one another,
//! because the draw order sorts whole elements. And an icon leads with at
//! most one [`Tone::Accent`] element, so the eye lands in the same place on
//! every page.

use super::{DEPTH, Element3, Scene, Shape, Tone, Vec3};

/// Two stacked pills, the near one accent.
pub(crate) const BUTTONS: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Pill,
            Vec3::new(-4.0, -4.0, DEPTH + 2.0),
            (34.0, 12.0),
            DEPTH,
            Tone::Muted,
        ),
        Element3::new(
            Shape::Pill,
            Vec3::new(4.0, 4.0, 0.0),
            (34.0, 12.0),
            DEPTH,
            Tone::Accent,
        ),
    ],
};

/// A field with a caret floating above it.
pub(crate) const INPUTS: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (36.0, 16.0),
            DEPTH,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 1.5 },
            Vec3::new(-11.0, 0.0, DEPTH + 3.0),
            (4.0, 13.0),
            7.0,
            Tone::Accent,
        ),
    ],
};

/// A slab and its after-image, lifted straight off it.
pub(crate) const SNAPSHOT: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (30.0, 30.0),
            DEPTH,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 4.0 },
            Vec3::new(0.0, 0.0, DEPTH + 8.0),
            (30.0, 30.0),
            DEPTH,
            Tone::Accent,
        )
        .ghost(0.55),
    ],
};

/// Three plates fanned out along the view direction.
pub(crate) const CARD: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Plate { radius: 4.0 },
            Vec3::new(-6.0, -6.0, 10.0),
            (28.0, 28.0),
            0.0,
            Tone::Accent,
        ),
        Element3::new(
            Shape::Plate { radius: 4.0 },
            Vec3::new(0.0, 0.0, 5.0),
            (28.0, 28.0),
            0.0,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Plate { radius: 4.0 },
            Vec3::new(6.0, 6.0, 0.0),
            (28.0, 28.0),
            0.0,
            Tone::Muted,
        ),
    ],
};

/// One slab and the trail it left, rising along an arc.
pub(crate) const MOTION: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(-14.0, 0.0, 0.0),
            (14.0, 14.0),
            DEPTH,
            Tone::Neutral,
        )
        .ghost(0.35),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(-4.0, 0.0, 3.0),
            (14.0, 14.0),
            DEPTH,
            Tone::Neutral,
        )
        .ghost(0.55),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(6.0, 0.0, 6.0),
            (14.0, 14.0),
            DEPTH,
            Tone::Neutral,
        )
        .ghost(0.75),
        Element3::new(
            Shape::Slab { radius: 3.0 },
            Vec3::new(16.0, 0.0, 9.0),
            (14.0, 14.0),
            DEPTH,
            Tone::Accent,
        ),
    ],
};

/// Three bars of one length and rising heights.
pub(crate) const WEIGHT: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, -12.0, 0.0),
            (26.0, 6.0),
            4.0,
            Tone::Muted,
        ),
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (26.0, 6.0),
            9.0,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, 12.0, 0.0),
            (26.0, 6.0),
            15.0,
            Tone::Accent,
        ),
    ],
};

/// A grid of small slabs at mixed heights, one of them accent.
pub(crate) const SHOWCASE: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(-12.0, -12.0, 0.0),
            (9.0, 9.0),
            5.0,
            Tone::Muted,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(-12.0, 0.0, 0.0),
            (9.0, 9.0),
            9.0,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(-12.0, 12.0, 0.0),
            (9.0, 9.0),
            6.0,
            Tone::Muted,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(0.0, -12.0, 0.0),
            (9.0, 9.0),
            11.0,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(0.0, 0.0, 0.0),
            (9.0, 9.0),
            16.0,
            Tone::Accent,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(0.0, 12.0, 0.0),
            (9.0, 9.0),
            8.0,
            Tone::Muted,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(12.0, -12.0, 0.0),
            (9.0, 9.0),
            6.0,
            Tone::Muted,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(12.0, 0.0, 0.0),
            (9.0, 9.0),
            10.0,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Slab { radius: 1.5 },
            Vec3::new(12.0, 12.0, 0.0),
            (9.0, 9.0),
            5.0,
            Tone::Muted,
        ),
    ],
};

/// A narrow tall panel beside a wide low one.
pub(crate) const NESTED_SIDEBAR: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Slab { radius: 2.0 },
            Vec3::new(0.0, -12.0, 0.0),
            (30.0, 10.0),
            14.0,
            Tone::Accent,
        ),
        Element3::new(
            Shape::Slab { radius: 2.0 },
            Vec3::new(0.0, 6.0, 0.0),
            (30.0, 24.0),
            6.0,
            Tone::Neutral,
        ),
    ],
};

/// Three bars of falling length: a paragraph seen from above.
pub(crate) const TYPOGRAPHY: Scene = Scene {
    elements: &[
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, -10.0, 0.0),
            (32.0, 5.0),
            5.0,
            Tone::Accent,
        ),
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, 0.0, 0.0),
            (24.0, 5.0),
            5.0,
            Tone::Neutral,
        ),
        Element3::new(
            Shape::Bar { radius: 2.0 },
            Vec3::new(0.0, 10.0, 0.0),
            (16.0, 5.0),
            5.0,
            Tone::Muted,
        ),
    ],
};

/// Every scene, for the tests that hold the set to one standard.
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
