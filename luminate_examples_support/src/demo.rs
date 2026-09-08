//! The twelve one-idea demo cells shared by `tiers` (`iced_animate`),
//! `compositor` (`iced_texture_cache`) and `overview` (`iced_luminate`).
//!
//! Every cell takes the engine, the current pose and a [`CellStyle`] and
//! returns an element. The nine engine cells are generic over the message,
//! theme and renderer; the three compositor cells (behind the `texture`
//! feature) are fixed to `iced_texture_cache::Renderer`, the only renderer
//! that records textures.
//!
//! Each cell's caption is a literal copy of the code below it. They sit next
//! to each other on purpose: edit both.

use std::time::Duration;

use iced::advanced::text::Renderer as TextRenderer;
use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Color, Element, Font, Padding};
use iced_animate::curves::{BOUNCY, QUICK, SMOOTH};
use iced_animate::widget::{shape, sized};
use iced_animate::{Curve, Easing, Motion, Presence, key, motion_set};

use crate::{
    ACTIVE, BOX, CellStyle, Chip, IDLE, MUTED, cell, marker, small_button, small_button_maybe,
    uniform_radius,
};

// Tier 1: paint
//
// `shape()` resolves its colours, radius and border inside `draw`. A stock
// iced widget cannot: its style closure runs while the view is being built,
// so a colour written there is a snapshot. A moving paint value asks the host
// for another frame without triggering layout.

/// Animated fill colour.
#[must_use]
pub fn fill<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let color = m.to(key!(), SMOOTH, if on { ACTIVE } else { IDLE });

    cell(
        "Fill",
        "Animated primitive figure",
        shape()
            .width(BOX)
            .height(BOX)
            .fill(color)
            .radius(uniform_radius(8.0))
            .into(),
        style,
    )
}

/// Animated corner radius.
#[must_use]
pub fn radius<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let corner = m.to(
        key!(),
        SMOOTH,
        uniform_radius(if on { BOX / 2.0 } else { 6.0 }),
    );

    cell(
        "Radius",
        "Animated primitive figure",
        shape()
            .width(BOX)
            .height(BOX)
            .fill(IDLE)
            .radius(corner)
            .into(),
        style,
    )
}

/// Two tracks under one key, told apart by a discriminator.
#[must_use]
pub fn border<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    // Separate tracks because they want separate ranges, one key because they
    // belong to the same idea.
    let width = m.to(key!("width"), SMOOTH, if on { 6.0_f32 } else { 1.0 });
    let color = m.to(key!("color"), SMOOTH, if on { ACTIVE } else { MUTED });

    cell(
        "Border",
        "Animated primitive figure",
        shape()
            .width(BOX)
            .height(BOX)
            .fill(Color::TRANSPARENT)
            .radius(uniform_radius(8.0))
            .border_width(width)
            .border_color(color)
            .into(),
        style,
    )
}

// Tier 2: layout
//
// `sized()` resolves width, height and padding inside `layout`, so the child
// is measured again every frame. This costs more, but it also moves nearby
// elements.

/// Animated size; the dot beside the square is pushed along.
#[must_use]
pub fn size<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let side = m.to(key!(), SMOOTH, if on { 76.0_f32 } else { BOX });

    cell(
        "Size",
        "Animated primitive figure with layout",
        row![
            sized(shape().fill(IDLE).radius(uniform_radius(8.0)))
                .width(side.clone())
                .height(side),
            shape()
                .width(8)
                .height(8)
                .fill(MUTED)
                .radius(uniform_radius(4.0)),
        ]
        .spacing(8)
        .align_y(Alignment::Center)
        .into(),
        style,
    )
}

/// Animated padding around a fixed inner box.
#[must_use]
pub fn padding<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let pad = m.to(key!(), SMOOTH, Padding::from(if on { 18.0 } else { 4.0 }));

    cell(
        "Padding",
        "Animated primitive figure with layout",
        container(
            sized(
                shape()
                    .width(32)
                    .height(32)
                    .fill(IDLE)
                    .radius(uniform_radius(8.0)),
            )
            .padding(pad),
        )
        .style(|_| container::Style {
            background: Some(iced::Background::Color(Color::from_rgb(0.88, 0.90, 0.94))),
            border: iced::Border {
                radius: 10.0.into(),
                ..Default::default()
            },
            ..Default::default()
        })
        .into(),
        style,
    )
}

// ===================================================== Composing the pieces

motion_set! {
    /// Everything that changes when the badge lights up.
    ///
    /// A set lets the view name a state instead of listing each interpolation.
    struct Badge -> BadgeAnim {
        side: f32,
        fill: Color,
        radius: iced::border::Radius,
    }
}

const BADGE_IDLE: Badge = Badge {
    side: BOX,
    fill: IDLE,
    radius: uniform_radius(6.0),
};

const BADGE_ACTIVE: Badge = Badge {
    side: 72.0,
    fill: ACTIVE,
    radius: uniform_radius(36.0),
};

/// Three properties, two tiers, one `to_set` call.
#[must_use]
pub fn property_set<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    // Each field gets its own track under a key derived from this one, so
    // they stay independent values while sharing an identity and a curve.
    let s = m.to_set(key!(), SMOOTH, if on { BADGE_ACTIVE } else { BADGE_IDLE });

    cell(
        "Multiple set",
        "Three properties animation",
        sized(shape().fill(s.fill).radius(s.radius))
            .width(s.side.clone())
            .height(s.side)
            .into(),
        style,
    )
}

/// Same target, same frame, different physics.
#[must_use]
pub fn spring_vs_ease<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let target = if on { 64.0_f32 } else { 0.0 };

    // A spring keeps its velocity when retargeted. An ease restarts instead.
    let sprung = m.to(key!("spring"), QUICK, target);
    let eased = m.to(
        key!("ease"),
        Curve::ease(Easing::EaseInOut, Duration::from_millis(320)),
        target,
    );

    cell(
        "Comparison",
        "Spring vs Ease",
        column![marker(sprung, IDLE), marker(eased, MUTED)]
            .spacing(10)
            .into(),
        style,
    )
}

/// Three lanes off one call site, staggered with `delayed`.
#[must_use]
pub fn staggered<'a, Message, Theme, Renderer>(
    m: &Motion,
    on: bool,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    let target = if on { 64.0_f32 } else { 0.0 };

    // `delayed` holds the pose for the delay, then runs. The discriminator is
    // what tells the rows apart.
    let lane = |i: u64| {
        let curve = SMOOTH.delayed(Duration::from_millis(90 * i));
        m.to(key!(i), curve, target)
    };

    cell(
        "Delay / Stagger",
        "Three lanes off one call site",
        column![
            marker(lane(0), IDLE),
            marker(lane(1), IDLE),
            marker(lane(2), IDLE)
        ]
        .spacing(8)
        .into(),
        style,
    )
}

/// Chips that grow in with `enter` and shrink out with `retire`.
///
/// `on_remove` receives the id of the last chip that is still present.
#[must_use]
pub fn entering_and_leaving<'a, Message, Theme, Renderer>(
    m: &Motion,
    chips: &'a [Chip],
    on_add: Message,
    on_remove: impl FnOnce(u64) -> Message,
    style: CellStyle,
) -> Element<'a, Message, Theme, Renderer>
where
    Message: Clone + 'a,
    Theme: text::Catalog + container::Catalog + button::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
    Renderer: TextRenderer<Font = Font> + 'a,
{
    // `enter` replays only the first time it sees a key, so the page never has
    // to track which chips are new. `retire` is its mirror: it marks the key as
    // leaving, and `presence` says when it is finally safe to stop drawing.
    let mut lane = row![].spacing(6).align_y(Alignment::Center);

    for chip in chips {
        let side = if chip.leaving {
            let side = m.retire(chip.key(), QUICK, 0.0_f32);

            if m.presence(chip.key()) == Presence::Gone {
                continue;
            }

            side
        } else {
            m.enter(chip.key(), BOUNCY, 0.0_f32, 20.0_f32)
        };

        lane = lane.push(
            sized(shape().fill(IDLE).radius(uniform_radius(4.0)))
                .width(side.clone())
                .height(side),
        );
    }

    let last = chips.iter().rev().find(|c| !c.leaving).map(|c| c.id);

    cell(
        "Enter / Exit",
        "Chips that grow and shrink out",
        column![
            lane,
            row![
                small_button("add", on_add),
                small_button_maybe("remove", last.map(on_remove)),
            ]
            .spacing(6),
        ]
        .spacing(10)
        .into(),
        style,
    )
}

// Tier 0: compositor
//
// `Cached` records its content into a texture once and then composites that
// texture every frame under a transform. Offset, scale and opacity are
// consumed by the compositor itself, so a frame of this animation costs one
// textured quad. It does not run layout, record again, or redraw the content.
//
// The flip side: the widget keeps its original layout box. Its neighbours do
// not move, and the image can leave the box. Wrap content that is *static
// while it is transformed*; content that relayouts every frame is re-recorded
// every frame and gains nothing.

#[cfg(feature = "texture")]
use crate::square;
#[cfg(feature = "texture")]
use iced::widget::Space;
#[cfg(feature = "texture")]
use iced::{Length, Vector};
#[cfg(feature = "texture")]
use iced_animate::curves::FADE;
#[cfg(feature = "texture")]
use iced_texture_cache::{TextureCache, cached};

/// Animated translation of a cached texture.
#[cfg(feature = "texture")]
#[must_use]
pub fn translate<'a, Message, Theme>(
    m: &Motion,
    on: bool,
    cache: &TextureCache,
    style: CellStyle,
) -> Element<'a, Message, Theme, iced_texture_cache::Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
{
    let offset = m.to(
        key!(),
        SMOOTH,
        if on {
            Vector::new(64.0, 0.0)
        } else {
            Vector::ZERO
        },
    );

    cell(
        "Translate",
        "Animated texture layer",
        row![
            cached(cache.clone(), square(IDLE)).translate(offset),
            Space::new().width(Length::Fill),
        ]
        .into(),
        style,
    )
}

/// Animated scale about the texture's own centre.
#[cfg(feature = "texture")]
#[must_use]
pub fn scale<'a, Message, Theme>(
    m: &Motion,
    on: bool,
    cache: &TextureCache,
    style: CellStyle,
) -> Element<'a, Message, Theme, iced_texture_cache::Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
{
    // A spring that overshoots slightly: the square arrives a touch too big
    // and settles back. Physical, not a keyframe.
    let factor = m.to(key!(), BOUNCY, if on { 1.6_f32 } else { 1.0 });

    cell(
        "Scale",
        "Animated texture layer",
        cached(cache.clone(), square(IDLE))
            .scale(factor)
            // Recording at 2x keeps the enlarged texture from going soft.
            .supersample(2.0)
            .pixel_snap(iced_texture_cache::PixelSnap::Never)
            .into(),
        style,
    )
}

/// Animated opacity of the whole subtree as one image.
#[cfg(feature = "texture")]
#[must_use]
pub fn opacity<'a, Message, Theme>(
    m: &Motion,
    on: bool,
    cache: &TextureCache,
    style: CellStyle,
) -> Element<'a, Message, Theme, iced_texture_cache::Renderer>
where
    Message: 'a,
    Theme: text::Catalog + container::Catalog + 'a,
    <Theme as text::Catalog>::Class<'a>: From<text::StyleFn<'a, Theme>>,
    <Theme as container::Catalog>::Class<'a>: From<container::StyleFn<'a, Theme>>,
{
    // Opacity has no useful momentum, so use a predictable duration curve.
    let alpha = m.to(key!(), FADE, if on { 0.15_f32 } else { 1.0 });

    cell(
        "Opacity",
        "Animated texture layer",
        cached(cache.clone(), square(IDLE)).opacity(alpha).into(),
        style,
    )
}
