//! A [`Length`] whose fixed value can be animated.

use iced_core::Length;

use crate::{Anim, Tier};

/// A [`Length`] whose [`Fixed`] amount may come from an animated track.
///
/// The other variants are relative to the parent, so there is nothing in them
/// to animate, only [`Fixed`] carries a number of its own.
///
/// Like [`Anim`], this must be resolved inside a widget's `layout`, never
/// while building the view. That is the whole reason [`Sized`] exists: the
/// stock iced widgets take a plain `Length` when they are constructed, so a
/// value handed to one of them is frozen until the next rebuild.
///
/// [`Fixed`]: Self::Fixed
/// [`Sized`]: crate::widget::Sized
#[derive(Debug, Clone, Default)]
pub enum AnimLength {
    /// Fill all remaining space.
    Fill,
    /// Fill a portion of the remaining space, relative to siblings.
    FillPortion(u16),
    /// Take as little space as possible.
    #[default]
    Shrink,
    /// A fixed amount, possibly animated.
    Fixed(Anim<f32>),
}

impl AnimLength {
    /// Resolves to a concrete [`Length`] for this frame.
    ///
    /// Call from `layout`.
    #[must_use]
    pub fn resolve(&self) -> Length {
        match self {
            AnimLength::Fill => Length::Fill,
            AnimLength::FillPortion(portion) => Length::FillPortion(*portion),
            AnimLength::Shrink => Length::Shrink,
            // A bouncy spring may overshoot below zero; a length cannot.
            AnimLength::Fixed(value) => Length::Fixed(value.get().max(0.0)),
        }
    }

    /// Returns `true` if this length is currently in motion.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        match self {
            AnimLength::Fixed(value) => value.is_animating(),
            _ => false,
        }
    }

    /// Returns `true` if this length is bound to a track, whether or not that
    /// track is moving right now.
    ///
    /// The difference from [`is_animating`](Self::is_animating) matters to
    /// anything that must not change *how* it draws when the motion starts or
    /// stops: a settled track is still going to move again.
    #[must_use]
    pub fn is_live(&self) -> bool {
        match self {
            AnimLength::Fixed(value) => value.is_live(),
            _ => false,
        }
    }

    /// The value to advertise from [`Widget::size_hint`].
    ///
    /// A live track is reported as [`Length::Shrink`] rather than its current
    /// number, because containers *delete* children they consider empty:
    ///
    /// ```text
    /// // iced_widget::Column::push
    /// if !child_size.is_void() {
    ///     self.children.push(child);
    /// }
    /// ```
    ///
    /// and [`Size::is_void`] is true for exactly `Fixed(0.0)` (`iced_widget`
    /// 0.14; if a later iced stops deleting void children this hint can
    /// return the real value). A height
    /// animating up from zero is `Fixed(0.0)` at the instant the view is
    /// built, so the widget would be dropped from its parent, and since the
    /// whole point of the engine is that animation needs no rebuild, nothing
    /// would ever bring it back. The row would silently stay empty until some
    /// unrelated interaction rebuilt the view.
    ///
    /// The real value is still applied in `layout`, so a genuinely zero-height
    /// box still measures as zero. Only the hint is widened.
    ///
    /// [`Widget::size_hint`]: iced_core::Widget::size_hint
    /// [`Size::is_void`]: iced_core::Size::is_void
    #[must_use]
    pub fn size_hint(&self) -> Length {
        match self {
            AnimLength::Fixed(value) if value.is_live() => Length::Shrink,
            other => other.resolve(),
        }
    }

    /// Grows this length so it also encloses `child`.
    ///
    /// Mirrors [`Length::enclose`], with one exception: a live length is
    /// returned unchanged. Resolving it here would freeze the animation into
    /// a constant, and `Length::enclose` never changes a `Fixed` anyway.
    #[must_use]
    pub fn enclose(self, child: Length) -> Self {
        match &self {
            AnimLength::Fixed(value) if value.is_live() => self,
            _ => AnimLength::from(self.resolve().enclose(child)),
        }
    }

    /// Marks the underlying track as read during `layout`.
    ///
    /// A moving length changes the layout, so the host has to relayout and not
    /// merely repaint. Called by the widgets that consume an [`AnimLength`].
    pub fn mark_layout_tier(&self) {
        if let AnimLength::Fixed(value) = self {
            value.mark_tier(Tier::Layout);
        }
    }
}

impl From<Length> for AnimLength {
    fn from(length: Length) -> Self {
        match length {
            Length::Fill => AnimLength::Fill,
            Length::FillPortion(portion) => AnimLength::FillPortion(portion),
            Length::Shrink => AnimLength::Shrink,
            Length::Fixed(value) => AnimLength::Fixed(Anim::constant(value)),
        }
    }
}

impl From<f32> for AnimLength {
    fn from(value: f32) -> Self {
        AnimLength::Fixed(Anim::constant(value))
    }
}

impl From<u32> for AnimLength {
    fn from(value: u32) -> Self {
        AnimLength::Fixed(Anim::constant(value as f32))
    }
}

impl From<iced_core::Pixels> for AnimLength {
    fn from(value: iced_core::Pixels) -> Self {
        AnimLength::Fixed(Anim::constant(value.0))
    }
}

impl From<Anim<f32>> for AnimLength {
    fn from(value: Anim<f32>) -> Self {
        AnimLength::Fixed(value)
    }
}

impl From<&Anim<f32>> for AnimLength {
    fn from(value: &Anim<f32>) -> Self {
        AnimLength::Fixed(value.clone())
    }
}

impl From<Anim<iced_core::Pixels>> for AnimLength {
    fn from(value: Anim<iced_core::Pixels>) -> Self {
        AnimLength::Fixed(value.retype(|pixels| pixels.0))
    }
}

#[cfg(test)]
mod tests {
    use iced_core::{Length, Pixels};

    use crate::testing::FrameClock;
    use crate::{Anim, AnimLength, Motion, curves::QUICK, key};
    use std::time::Duration;

    use crate::{Curve, SpringParams};

    #[test]
    fn integer_and_pixel_lengths_mirror_iced() {
        assert_eq!(AnimLength::from(12_u32).resolve(), Length::Fixed(12.0));
        assert_eq!(AnimLength::from(Pixels(7.5)).resolve(), Length::Fixed(7.5));
        assert_eq!(AnimLength::from(Length::Fill).resolve(), Length::Fill);
        assert_eq!(AnimLength::from(3.0_f32).resolve(), Length::Fixed(3.0));
    }

    #[test]
    fn live_handles_convert_by_value_and_by_reference() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let _ = m.to(key, QUICK, 0.0_f32);
        let side = m.to(key, QUICK, 40.0_f32);

        let width = AnimLength::from(&side);
        let height = AnimLength::from(side);
        assert!(width.is_animating() && height.is_animating());

        let _ = clock.run_until_settled();
        assert_eq!(width.resolve(), Length::Fixed(40.0));
        assert_eq!(height.resolve(), Length::Fixed(40.0));
    }

    #[test]
    fn a_pixels_handle_becomes_a_length() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let _ = m.to(key, QUICK, Pixels(0.0));
        let px = m.to(key, QUICK, Pixels(24.0));

        let length = AnimLength::from(px);
        assert!(length.is_animating());
        let _ = clock.run_until_settled();
        assert_eq!(length.resolve(), Length::Fixed(24.0));

        let constant: Anim<Pixels> = Pixels(5.0).into();
        assert_eq!(AnimLength::from(constant).resolve(), Length::Fixed(5.0));
    }

    /// A fast spring for tests; not the shipped `curves::SMOOTH`.
    const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

    #[test]
    fn an_animated_zero_length_is_never_advertised_as_void() {
        let m = Motion::new();
        let height: AnimLength = m.to(key!(), FAST, 0.0_f32).into();

        assert_eq!(
            height.resolve(),
            Length::Fixed(0.0),
            "layout must still measure a zero-height box as zero"
        );

        // `Column::push` drops any child whose hint is void, and nothing rebuilds
        // the view to bring it back, so a value animating up from zero would
        // vanish from its parent permanently.
        assert!(
            !iced::Size::new(Length::Fill, height.size_hint()).is_void(),
            "an animated zero must not read as an empty child"
        );
    }

    #[test]
    fn enclose_leaves_a_live_length_alone_and_widens_a_constant() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 10.0_f32);
        let live = AnimLength::from(m.to(key, FAST, 20.0_f32));
        assert!(live.is_animating());
        assert!(live.enclose(Length::Fill).is_animating(), "still live");
        assert_eq!(
            AnimLength::Shrink.enclose(Length::Fill).resolve(),
            Length::Fill
        );
    }

    #[test]
    fn lengths_never_resolve_negative() {
        assert_eq!(AnimLength::from(-5.0).resolve(), Length::Fixed(0.0));
    }
}
