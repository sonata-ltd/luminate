//! [`Luminate`]: the kit that turns descriptors into elements.

use std::borrow::Cow;
use std::sync::Arc;

use iced_animate::Motion;

use crate::Element;
use crate::theme::Theme;

mod button;
mod card;
mod input;
mod pager;
mod sidebar;

/// Builds Luminate elements from [`descriptor`](crate::descriptor)s.
///
/// A `Luminate` owns the animation engine ([`Motion`]) its widgets animate on
/// and the [`Theme`] its *metrics* come from. Clones share both, so a router
/// can hand every page a clone ([`Router<Context = Luminate>`](crate::Router))
/// and everything animates off one clock. Wrap the application root in
/// [`host`](Self::host). Without it, nothing advances the clock and every
/// animation holds its first pose (a debug build warns the first time).
///
/// *Colours* come from the theme the runtime draws with, through the
/// `Catalog` impls of [`Theme`]; return [`theme`](Self::theme) from the
/// application's `theme` function so both agree (the crate docs show the
/// whole wiring).
///
/// ```
/// use iced_luminate::descriptor::Button;
/// use iced_luminate::{Element, Luminate};
///
/// #[derive(Debug, Clone)]
/// enum Message {
///     Pressed,
/// }
///
/// let luminate = Luminate::new();
/// let view: Element<'_, Message> =
///     luminate.host(luminate.button(Button::new("Press").on_press(Message::Pressed)));
/// # let _ = view;
/// ```
#[derive(Debug, Clone)]
pub struct Luminate {
    motion: Motion,
    theme: Arc<Theme>,
}

impl Luminate {
    /// A kit with a fresh animation engine and [`Theme::LIGHT`].
    #[must_use]
    pub fn new() -> Self {
        Self::with_theme(Theme::LIGHT)
    }

    /// A kit with a fresh animation engine and `theme`.
    #[must_use]
    pub fn with_theme(theme: Theme) -> Self {
        Self {
            motion: Motion::new(),
            theme: Arc::new(theme),
        }
    }

    /// The theme the kit's metrics come from. Return a copy from the
    /// application's `theme` function (`Theme` is `Copy`) so the runtime
    /// draws the same look.
    #[must_use]
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// The engine every widget built here animates on. Animate application
    /// values against it too; there is no need for a second handle.
    #[must_use]
    pub fn motion(&self) -> &Motion {
        &self.motion
    }

    /// Wraps `content` so the engine is ticked once per frame. Use it
    /// exactly once, around the root of the view.
    #[must_use]
    pub fn host<'a, M: 'a>(&self, content: impl Into<Element<'a, M>>) -> Element<'a, M> {
        self.motion.host(content).into()
    }

    /// The font files to load: pass each to `iced::application(..).font(..)`.
    /// Empty without the `bundled-font` feature.
    ///
    /// The two bundled faces come first, upright then italic, followed by one
    /// copy of each per weight in
    #[cfg_attr(
        feature = "bundled-font",
        doc = "[`DECLARED_WEIGHTS`](crate::theme::typography::DECLARED_WEIGHTS) —"
    )]
    #[cfg_attr(not(feature = "bundled-font"), doc = "`DECLARED_WEIGHTS` —")]
    /// eighteen buffers in all. The copies exist so the text stack can select
    /// the kit's family by exact weight; see `DECLARED_WEIGHTS` for why, and
    /// note that each costs about 900 KB of memory — some 14 MB for the set,
    /// and about 9 ms to build and register, measured in a release build.
    /// That is what it costs to own all nine weights of the family rather
    /// than lose one of them to whatever else is installed; an application
    /// pays it once, at startup, and nothing per frame.
    ///
    /// ```
    /// use iced_luminate::Luminate;
    ///
    /// let fonts = Luminate::fonts();
    /// assert_eq!(fonts.len(), if cfg!(feature = "bundled-font") { 18 } else { 0 });
    /// ```
    #[must_use]
    pub fn fonts() -> Vec<Cow<'static, [u8]>> {
        #[cfg(feature = "bundled-font")]
        {
            use crate::theme::typography::{
                DECLARED_WEIGHTS, FONT_INTER, FONT_INTER_ITALIC, with_declared_weight,
            };

            let faces = [FONT_INTER, FONT_INTER_ITALIC];
            let mut fonts: Vec<Cow<'static, [u8]>> =
                faces.iter().map(|face| Cow::Borrowed(*face)).collect();

            fonts.extend(faces.iter().flat_map(|face| {
                DECLARED_WEIGHTS
                    .iter()
                    .filter_map(move |weight| with_declared_weight(face, *weight).map(Cow::Owned))
            }));

            fonts
        }
        #[cfg(not(feature = "bundled-font"))]
        {
            Vec::new()
        }
    }
}

impl Default for Luminate {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use iced_animate::curves::SMOOTH;
    use iced_animate::key;

    use super::*;

    #[test]
    fn luminate_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Luminate>();
    }

    #[test]
    fn clones_share_the_engine_and_the_theme() {
        let a = Luminate::with_theme(Theme::DARK);
        let b = a.clone();

        let _ = a.motion().to(key!(), SMOOTH, 1.0_f32);

        assert_eq!(b.motion().track_count(), 1, "one engine behind both clones");
        assert_eq!(b.theme().name, "Luminate Dark");
        assert_eq!(*Luminate::new().theme(), Theme::LIGHT);
    }
}
