//! The kit's text is shaped at the weight it is drawn at, with kerning.
//!
//! iced's default [`Shaping::Auto`] drops to `Shaping::Basic` for ASCII, and
//! cosmic-text's basic path takes advances from the font's *default* instance
//! and applies no `kern`. On a variable face that means every weight is
//! spaced like Regular and no pair is kerned — glyphs drawn Semibold sitting
//! on Regular spacing. These tests pin the two properties that go missing.
//!
//! [`Shaping::Auto`]: iced_luminate::iced::widget::text::Shaping::Auto

#![cfg(feature = "bundled-font")]

use std::sync::{Mutex, MutexGuard, Once};

use iced_luminate::iced::font::Weight;
use iced_luminate::iced::{self, Size};
use iced_luminate::theme::Theme;
use iced_luminate::theme::typography::{TextSize, TextStyle, styled_text};
use iced_luminate::{Element, Luminate, Renderer};
use iced_test::Simulator;

/// The bundled faces, in the process-wide font system the simulator draws
/// with. Without them every measurement below is of a fallback font.
fn load_fonts() {
    static ONCE: Once = Once::new();

    ONCE.call_once(|| {
        let mut system = iced::advanced::graphics::text::font_system()
            .write()
            .expect("the font system is not poisoned");

        for font in Luminate::fonts() {
            system.load_font(font);
        }
    });
}

/// Building a [`Simulator`] builds a renderer, and two threads reaching a
/// cold graphics driver at once segfault inside it. The tests here are the
/// only ones in this binary that measure, so serialising them costs nothing.
fn one_at_a_time() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());

    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The width the kit lays `content` out at, in `style`.
fn width(content: &'static str, style: TextStyle) -> f32 {
    let _guard = one_at_a_time();
    load_fonts();

    let root: Element<'_, ()> = styled_text(content, style).into();
    let mut ui: Simulator<'_, (), Theme, Renderer> =
        Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

    ui.find(content)
        .expect("the text is on screen")
        .bounds()
        .width
}

/// The regression: a variable face declares one instance, and basic shaping
/// measures every weight against it. Semibold text then occupies exactly the
/// Regular width while its glyphs are drawn heavier — visibly tighter than
/// the same string set anywhere else.
#[test]
fn a_heavier_style_is_laid_out_wider() {
    const CONTENT: &str = "Add External Runtime";

    let regular = width(CONTENT, TextStyle::text(TextSize::Lg, Weight::Normal));
    let semibold = width(CONTENT, TextStyle::text(TextSize::Lg, Weight::Semibold));

    assert!(
        semibold > regular,
        "Semibold laid out at {semibold} px and Regular at {regular} px; equal \
         widths mean the advances came from the face's default instance and \
         not from the requested weight"
    );
}

/// Kerning is a `GPOS` feature, and the basic shaper never runs it. `To` is
/// one of the pairs Inter kerns hardest, so the pair must come out narrower
/// than the two letters measured apart.
#[test]
fn a_kerned_pair_is_tighter_than_its_letters_apart() {
    let style = TextStyle::text(TextSize::Lg, Weight::Normal);

    let pair = width("To", style);
    let apart = width("T", style) + width("o", style);

    assert!(
        pair < apart,
        "`To` laid out at {pair} px against {apart} px for `T` and `o` apart; \
         no tightening means the `kern` feature never ran"
    );
}
