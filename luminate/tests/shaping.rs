//! The kit's text is shaped at the weight it is drawn at, with kerning.
//!
//! iced's default [`Shaping::Auto`] drops to `Shaping::Basic` for ASCII, and
//! cosmic-text's basic path takes advances from the font's *default* instance
//! and applies no `kern`. On a variable face that means every weight is
//! spaced like Regular and no pair is kerned — glyphs drawn Semibold sitting
//! on Regular spacing. These tests pin the two properties that go missing.
//!
//! The rest of the file is the other half of the same story: `weighted_text`
//! reaches past `iced::font::Weight`'s nine variants to the `wght` axis
//! itself, and these tests pin that a weight between two declared ones is
//! really drawn between them, that a settled animation lands on exactly the
//! width the stock widget would have laid out, and that each layout policy
//! costs the tier it claims.
//!
//! [`Shaping::Auto`]: iced_luminate::iced::widget::text::Shaping::Auto

#![cfg(feature = "bundled-font")]

use std::sync::{Mutex, MutexGuard, Once};

use iced_luminate::animate::testing::FrameClock;
use iced_luminate::animate::{Anim, Motion, MotionKey, Tier, curves::QUICK};
use iced_luminate::iced::advanced::Renderer as _;
use iced_luminate::iced::advanced::clipboard;
use iced_luminate::iced::advanced::graphics::text::cosmic_text;
use iced_luminate::iced::advanced::renderer::{self, Headless};
use iced_luminate::iced::font::Weight;
use iced_luminate::iced::time::Instant;
use iced_luminate::iced::{self, Color, Event, Rectangle, Size, mouse, window};
use iced_luminate::texture::testing::headless_tiny_skia;
use iced_luminate::theme::Theme;
use iced_luminate::theme::typography::{DisplaySize, FAMILY, TextSize, TextStyle, styled_text};
use iced_luminate::widget::weighted_text::{WeightLayout, weighted_text};
use iced_luminate::{Element, Luminate, Renderer};
use iced_test::Simulator;
use iced_test::runtime::user_interface::{self, UserInterface};

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

/// The width the kit lays `content` out at, in `style`, at an arbitrary
/// `wght` value the nine-variant [`Weight`] cannot name.
fn weighted_width(content: &'static str, style: TextStyle, weight: f32) -> f32 {
    let _guard = one_at_a_time();
    load_fonts();

    let root: Element<'_, ()> = weighted_text(content, style)
        .weight(weight)
        // A step of one leaves the requested weight exactly as it is, so the
        // measurement is of the weight the test asked for.
        .step(1.0)
        .into();
    let mut ui: Simulator<'_, (), Theme, Renderer> =
        Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

    ui.find(content)
        .expect("the text is on screen")
        .bounds()
        .width
}

/// The point of the whole exercise: a weight between two declared ones is
/// really drawn between them.
///
/// `iced::font::Weight` can name 400 and 500 and nothing in between, so a 437
/// that came out the same width as 400 would mean the request had been
/// quantised away somewhere and the animation would be a four-step staircase.
/// The bundled face carries a `wght` axis, cosmic-text sets it from the
/// weight in the cache key, and this is what proves the chain end to end.
#[test]
fn an_intermediate_weight_lands_between_the_declared_ones() {
    const CONTENT: &str = "Add External Runtime";

    let style = TextStyle::text(TextSize::Lg, Weight::Normal);

    let light = weighted_width(CONTENT, style, 400.0);
    let between = weighted_width(CONTENT, style, 437.0);
    let heavy = weighted_width(CONTENT, style, 500.0);

    assert!(
        light < between && between < heavy,
        "400 laid out at {light} px, 437 at {between} px and 500 at {heavy} px; \
         437 sharing a width with either end means the `wght` axis never got \
         the value and the weight was matched to a declared face instead"
    );
}

/// A weight that rests on one of the nine named weights goes back through the
/// ordinary paragraph path, and must lay out identically to the stock widget:
/// the handover between the two paths happens at the end of every animation
/// and would show as a jump if the two disagreed.
#[test]
fn a_resting_weight_matches_the_stock_text_widget() {
    const CONTENT: &str = "Add External Runtime";

    let style = TextStyle::text(TextSize::Lg, Weight::Semibold);

    let stock = width(CONTENT, style);
    let resting = weighted_width(CONTENT, style, 600.0);

    assert_eq!(
        stock, resting,
        "the kit's text widget lays `{CONTENT}` out at {stock} px and the \
         weighted one at rest at {resting} px"
    );
}

/// A track sitting at `from` and moving to `to`, plus the clock that advances
/// it.
fn moving(from: f32, to: f32) -> (Motion, Anim<f32>) {
    let motion = Motion::new();
    let key = MotionKey::unique();

    let _ = motion.to(key, QUICK, from);
    let weight = motion.to(key, QUICK, to);

    (motion, weight)
}

/// The width the widget lays `CONTENT` out at right now, under `policy`.
fn moving_width(weight: &Anim<f32>, policy: WeightLayout) -> f32 {
    let _guard = one_at_a_time();
    load_fonts();

    let root: Element<'_, ()> = weighted_text(CONTENT, MOVING_STYLE)
        .weight(weight.clone())
        .weight_layout(policy)
        .into();
    let mut ui: Simulator<'_, (), Theme, Renderer> =
        Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

    ui.find(CONTENT)
        .expect("the text is on screen")
        .bounds()
        .width
}

const CONTENT: &str = "Add External Runtime";
const MOVING_STYLE: TextStyle = TextStyle::text(TextSize::Lg, Weight::Medium);

/// The honest policy: the line is measured at the weight of the moment, so it
/// grows over the animation and ends on the width the target weight occupies.
#[test]
fn a_live_weight_widens_the_line_as_it_moves() {
    let (motion, weight) = moving(500.0, 600.0);
    let mut clock = FrameClock::new(&motion);

    let start = moving_width(&weight, WeightLayout::Live);

    let _ = clock.run(4);
    let middle = moving_width(&weight, WeightLayout::Live);

    let _ = clock.run(120);
    let end = moving_width(&weight, WeightLayout::Live);

    assert!(
        start < middle && middle < end,
        "the line went {start} → {middle} → {end} px; a width that does not \
         move means the weight never reached the shaper"
    );
    assert_eq!(
        end,
        width(CONTENT, TextStyle::text(TextSize::Lg, Weight::Semibold)),
        "a settled animation must land on exactly the width its target weight \
         occupies, or the handover to the resting path shows as a jump"
    );
}

/// The still policy: the line is measured at the weight the animation is
/// heading for and keeps that width from the first frame, so nothing around
/// it moves.
#[test]
fn a_snapped_weight_keeps_the_width_it_is_heading_for() {
    let (motion, weight) = moving(500.0, 600.0);
    let mut clock = FrameClock::new(&motion);

    let start = moving_width(&weight, WeightLayout::Snapped);

    let _ = clock.run(4);
    let middle = moving_width(&weight, WeightLayout::Snapped);

    let _ = clock.run(120);
    let end = moving_width(&weight, WeightLayout::Snapped);

    assert_eq!(start, middle, "the reserved line moved mid-animation");
    assert_eq!(middle, end, "and did not settle on the width it reserved");
    assert_eq!(
        start,
        width(CONTENT, TextStyle::text(TextSize::Lg, Weight::Semibold)),
        "the width reserved is the target weight's, not the starting one's"
    );
}

/// Only the widget knows where it reads the value, and the two policies read
/// it in different passes: the engine must relayout for one and only repaint
/// for the other.
#[test]
fn each_policy_marks_the_tier_it_reads_at() {
    let (_motion, live) = moving(500.0, 600.0);
    let _ = moving_width(&live, WeightLayout::Live);

    assert_eq!(
        live.tier(),
        Some(Tier::Layout),
        "a live weight is read in `layout` and changes the line's width"
    );

    let (_motion, snapped) = moving(500.0, 600.0);
    let _ = moving_width(&snapped, WeightLayout::Snapped);

    assert_eq!(
        snapped.tier(),
        Some(Tier::Paint),
        "a snapped weight never moves the layout, so it costs a redraw"
    );
}

/// Rounding is what keeps the number of font instances an animation asks for
/// finite: a frame whose weight rounds to the same step as the last frame's
/// reuses its `Font` and its glyphs.
///
/// A step as coarse as the whole transition leaves only its two ends, so the
/// line may take exactly two widths however many frames it is sampled over.
#[test]
fn a_moving_weight_is_rounded_to_its_step() {
    let _guard = one_at_a_time();
    load_fonts();

    let (motion, weight) = moving(500.0, 600.0);
    let mut clock = FrameClock::new(&motion);

    let mut widths: Vec<u32> = Vec::new();

    for _ in 0..24 {
        let root: Element<'_, ()> = weighted_text(CONTENT, MOVING_STYLE)
            .weight(weight.clone())
            .step(100.0)
            .into();
        let mut ui: Simulator<'_, (), Theme, Renderer> =
            Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

        let width = ui
            .find(CONTENT)
            .expect("the text is on screen")
            .bounds()
            .width;

        widths.push(width.to_bits());
        let _ = clock.run(1);
    }

    widths.sort_unstable();
    widths.dedup();

    assert!(
        widths.len() <= 2,
        "a 100-unit step over a 100-unit transition produced {} distinct \
         widths, so the weight was not rounded and every frame asked the text \
         stack for a font instance of its own",
        widths.len()
    );
}

/// The ink `content` puts on screen at `weight`: how many pixels the software
/// backend darkened, drawing through `fill_raw`.
fn ink(weight: f32) -> usize {
    const SIZE: Size = Size::new(400.0, 120.0);

    let _guard = one_at_a_time();
    load_fonts();

    let root: Element<'_, ()> =
        weighted_text(CONTENT, TextStyle::display(DisplaySize::Sm, Weight::Normal))
            .weight(weight)
            .step(1.0)
            .into();

    let mut renderer = headless_tiny_skia();
    let mut ui: UserInterface<'_, (), Theme, Renderer> =
        UserInterface::build(root, SIZE, user_interface::Cache::default(), &mut renderer);

    let mut messages = Vec::new();
    let _ = ui.update(
        &[Event::Window(
            window::Event::RedrawRequested(Instant::now()),
        )],
        mouse::Cursor::Unavailable,
        &mut renderer,
        &mut clipboard::Null,
        &mut messages,
    );

    renderer.reset(Rectangle::with_size(SIZE));
    ui.draw(
        &mut renderer,
        &Theme::default(),
        &renderer::Style {
            text_color: Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );

    let rgba = renderer.screenshot(
        Size::new(SIZE.width as u32, SIZE.height as u32),
        1.0,
        Color::WHITE,
    );

    rgba.as_chunks::<4>()
        .0
        .iter()
        .filter(|px| px[..3].iter().any(|channel| *channel < 128))
        .count()
}

/// The raw path has to *draw*, not only measure.
///
/// Every other test here reads a laid-out width, which the widget produces in
/// `layout`; none of them would notice if the `Raw` handed to `fill_raw` were
/// empty, mispositioned or dropped before the renderer got to it, and the
/// text would simply be invisible. This one looks at the pixels: there is ink
/// on the screen, and a weight the nine variants cannot name puts more of it
/// there than the weight below it.
#[test]
fn an_intermediate_weight_is_drawn_and_not_only_measured() {
    let light = ink(400.0);
    let between = ink(437.0);

    assert!(
        light > 0,
        "nothing was drawn at all: the raw buffer never reached the renderer"
    );
    assert!(
        between > light,
        "437 darkened {between} pixels against {light} for 400; no more ink \
         means the rasterizer drew the face's declared instance and not the \
         weight on the axis"
    );
}

/// The family every round hundred has to be shaped in.
///
/// cosmic-text picks a face whose declared `OS/2.usWeightClass` equals the
/// requested weight *exactly*, and only looks for the requested family inside
/// that set (`FontFallbackIter::default_font_match_key`). A weight nothing
/// declares — 437 — leaves the set empty everywhere and the family query
/// wins; a weight some *other* installed family declares exactly takes the
/// text out of Inter altogether. The round hundreds are exactly the weights
/// real fonts declare, which is why the kit has to declare all of them.
///
/// The test needs a competing family installed to fail, so a machine with no
/// fonts but Inter passes it either way. It is still the invariant worth
/// stating: text the kit asked for in its own family must be drawn in it.
#[test]
fn every_round_hundred_is_shaped_in_the_kits_family() {
    let _guard = one_at_a_time();
    load_fonts();

    let mut system = iced::advanced::graphics::text::font_system()
        .write()
        .expect("the font system is not poisoned");
    let font_system = system.raw();

    let mut strays = Vec::new();

    for weight in (100..=900).step_by(100) {
        let mut buffer =
            cosmic_text::Buffer::new(font_system, cosmic_text::Metrics::new(18.0, 28.0));
        buffer.set_size(font_system, Some(600.0), Some(40.0));
        buffer.set_text(
            font_system,
            CONTENT,
            &cosmic_text::Attrs::new()
                .family(cosmic_text::Family::Name(FAMILY))
                .weight(cosmic_text::Weight(weight)),
            cosmic_text::Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(font_system, false);

        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let face = font_system
                    .db()
                    .face(glyph.font_id)
                    .expect("the shaper picked a face the database has");

                if !face.families.iter().any(|(name, _)| name == FAMILY) {
                    let name = face
                        .families
                        .first()
                        .map_or("?", |(name, _)| name.as_str())
                        .to_owned();

                    if !strays.contains(&(weight, name.clone())) {
                        strays.push((weight, name));
                    }
                }
            }
        }
    }

    assert!(
        strays.is_empty(),
        "these weights left the family: {strays:?}; a weight the kit does not \
         declare loses the face to whatever other installed family declares it \
         exactly"
    );
}
