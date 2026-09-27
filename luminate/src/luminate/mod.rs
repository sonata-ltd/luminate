//! [`Luminate`]: the kit that turns descriptors into elements.

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

    /// Registers the bundled Inter faces with iced's text stack. Does nothing
    /// without the `bundled-font` feature, and nothing on a second call.
    ///
    /// Call it in `main`, before `iced::application(..)`. The faces go
    /// straight into iced's process-wide font system rather than through
    /// `Application::font`, which copies every buffer it is handed and keeps
    /// the original too. Registering there does not bump the font system's
    /// version, so text shaped before the call would not see the faces.
    ///
    /// Besides the two faces, this declares each at the weights in
    #[cfg_attr(
        feature = "bundled-font",
        doc = "[`DECLARED_WEIGHTS`](crate::theme::typography::DECLARED_WEIGHTS),"
    )]
    #[cfg_attr(not(feature = "bundled-font"), doc = "`DECLARED_WEIGHTS`,")]
    /// as entries that read the same bytes, so the text stack selects the
    /// kit's family at every round weight; see `DECLARED_WEIGHTS` for why.
    /// Nothing is copied: the faces are read from the bytes embedded in the
    /// binary.
    ///
    /// ```
    /// use iced_luminate::Luminate;
    ///
    /// Luminate::load_fonts();
    /// Luminate::load_fonts(); // already registered: does nothing
    /// ```
    pub fn load_fonts() {
        #[cfg(feature = "bundled-font")]
        {
            static ONCE: std::sync::Once = std::sync::Once::new();

            ONCE.call_once(|| {
                let mut system = iced::advanced::graphics::text::font_system()
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);

                crate::theme::typography::declare_faces(system.raw().db_mut());
            });
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
