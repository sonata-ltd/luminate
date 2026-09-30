//! The kit's text is shaped at the weight it is drawn at, with kerning.
//!
//! iced's default [`Shaping::Auto`] drops to `Shaping::Basic` for ASCII, and
//! cosmic-text's basic path takes advances from the font's *default* instance
//! and applies no `kern`. On a variable face that means every weight is
//! spaced like Regular and no pair is kerned — glyphs drawn Semibold sitting
//! on Regular spacing. These tests pin the two properties that go missing.
//!
//! The rest of the file is the other half of the same story: `animated_text`
//! reaches past `iced::font::Weight`'s nine variants to the `wght` axis
//! itself, and these tests pin that a weight between two declared ones is
//! really drawn between them, that a settled animation lands on exactly the
//! width the stock widget would have laid out, and that each layout policy
//! costs the tier it claims. The size tests pin the same for a size: a live
//! one starts and ends on the stock widget's boxes, and a scaled one keeps
//! its target's box and is still drawn at the size of the moment.
//!
//! [`Shaping::Auto`]: iced_luminate::iced::widget::text::Shaping::Auto

#![cfg(feature = "bundled-font")]

use std::sync::{Mutex, MutexGuard};

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
use iced_luminate::widget::animated_text::{AnimatedText, SizeLayout, WeightLayout, animated_text};
use iced_luminate::{Element, Luminate, Renderer};
use iced_test::Simulator;
use iced_test::runtime::user_interface::{self, UserInterface};

/// The bundled faces, in the process-wide font system the simulator draws
/// with. Without them every measurement below is of a fallback font.
fn load_fonts() {
    Luminate::load_fonts();
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

    let root: Element<'_, ()> = animated_text(content, style)
        .weight(weight)
        // A step of one leaves the requested weight exactly as it is, so the
        // measurement is of the weight the test asked for.
        .weight_step(1.0)
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

    let root: Element<'_, ()> = animated_text(CONTENT, MOVING_STYLE)
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
        let root: Element<'_, ()> = animated_text(CONTENT, MOVING_STYLE)
            .weight(weight.clone())
            .weight_step(100.0)
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
    ink_of(
        animated_text(CONTENT, TextStyle::display(DisplaySize::Sm, Weight::Normal))
            .weight(weight)
            .weight_step(1.0)
            .into(),
    )
}

/// How many pixels the software backend darkened drawing `root`.
fn ink_of(root: Element<'_, ()>) -> usize {
    const SIZE: Size = Size::new(400.0, 120.0);

    let _guard = one_at_a_time();
    load_fonts();

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

/// The box the kit lays `CONTENT` out in, in `style`, through the stock
/// widget.
fn stock_box(style: TextStyle) -> Size {
    let _guard = one_at_a_time();
    load_fonts();

    let root: Element<'_, ()> = styled_text(CONTENT, style).into();
    let mut ui: Simulator<'_, (), Theme, Renderer> =
        Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

    ui.find(CONTENT)
        .expect("the text is on screen")
        .bounds()
        .size()
}

/// The box the widget lays `CONTENT` out in right now, with `size` bound
/// under `policy`.
fn sized_box(size: &Anim<f32>, policy: SizeLayout) -> Size {
    let _guard = one_at_a_time();
    load_fonts();

    let root: Element<'_, ()> = animated_text(CONTENT, SMALL)
        .size(size.clone())
        .size_layout(policy)
        .into();
    let mut ui: Simulator<'_, (), Theme, Renderer> =
        Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

    ui.find(CONTENT)
        .expect("the text is on screen")
        .bounds()
        .size()
}

const SMALL: TextStyle = TextStyle::text(TextSize::Sm, Weight::Medium);
const LARGE: TextStyle = TextStyle::text(TextSize::Lg, Weight::Medium);

/// A live size grows the line in both dimensions, and starts and ends on
/// exactly the boxes the stock widget gives the two steps of the scale: the
/// handover to and from a resting label would show as a jump otherwise.
#[test]
fn a_live_size_grows_the_line_between_two_steps() {
    let (motion, size) = moving(SMALL.size, LARGE.size);
    let mut clock = FrameClock::new(&motion);

    let start = sized_box(&size, SizeLayout::Live);

    let _ = clock.run(4);
    let middle = sized_box(&size, SizeLayout::Live);

    let _ = clock.run(120);
    let end = sized_box(&size, SizeLayout::Live);

    assert_eq!(start, stock_box(SMALL), "the line starts at 14 px");
    assert!(
        start.width < middle.width && middle.width < end.width,
        "the line went {} → {} → {} px wide",
        start.width,
        middle.width,
        end.width
    );
    assert!(
        start.height <= middle.height && middle.height <= end.height,
        "the line box went {} → {} → {} px tall",
        start.height,
        middle.height,
        end.height
    );
    assert_eq!(
        end,
        stock_box(LARGE),
        "and settles on exactly the box `TextSize::Lg` has, line height included"
    );
}

/// Between the two steps the line box follows the size to a fraction of a
/// pixel. Rounded, it would stand still for several frames and then jump a
/// whole pixel, and move everything laid out after it by as much.
#[test]
fn a_live_size_moves_its_line_box_by_fractions_of_a_pixel() {
    let (motion, size) = moving(SMALL.size, LARGE.size);
    let mut clock = FrameClock::new(&motion);

    let heights: Vec<f32> = (0..12)
        .map(|_| {
            let _ = clock.run(1);
            sized_box(&size, SizeLayout::Live).height
        })
        .collect();

    assert!(
        heights.iter().any(|height| height.fract() != 0.0),
        "every frame sat on a whole pixel: {heights:?}"
    );
}

/// A scaled size keeps the box it is heading for from the first frame.
#[test]
fn a_scaled_size_keeps_the_box_it_is_heading_for() {
    let (motion, size) = moving(SMALL.size, LARGE.size);
    let mut clock = FrameClock::new(&motion);

    let start = sized_box(&size, SizeLayout::Scaled);

    let _ = clock.run(4);
    let middle = sized_box(&size, SizeLayout::Scaled);

    let _ = clock.run(120);
    let end = sized_box(&size, SizeLayout::Scaled);

    assert_eq!(start, middle, "the reserved box moved mid-animation");
    assert_eq!(middle, end, "and did not settle on the box it reserved");
    assert_eq!(start, stock_box(LARGE), "the box is the target size's");
}

#[test]
fn each_size_policy_marks_the_tier_it_reads_at() {
    let (_motion, live) = moving(SMALL.size, LARGE.size);
    let _ = sized_box(&live, SizeLayout::Live);

    assert_eq!(live.tier(), Some(Tier::Layout));

    let (_motion, scaled) = moving(SMALL.size, LARGE.size);
    let _ = sized_box(&scaled, SizeLayout::Scaled);

    assert_eq!(
        scaled.tier(),
        Some(Tier::Paint),
        "a scaled size never moves the layout, so it costs a redraw"
    );
}

/// A scaled line is laid out at its target but must be *drawn* at the size
/// of the moment. A line heading down from 30 px to 18 px draws, on its
/// first frame, as much ink as a line resting at 30 px — and far more than
/// its laid-out 18 px box holds — on both paths: the paragraph's while the
/// weight rests, and the raw buffer's while it moves.
#[test]
fn a_scaled_size_is_drawn_at_the_size_of_the_moment() {
    let style = TextStyle::text(TextSize::Lg, Weight::Normal);

    let at = |size: f32| ink_of(animated_text(CONTENT, style).size(size).into());
    let (large, small) = (at(30.0), at(18.0));

    let (_motion, size) = moving(30.0, 18.0);
    let paragraph = ink_of(
        animated_text(CONTENT, style)
            .size(size.clone())
            .size_layout(SizeLayout::Scaled)
            .into(),
    );

    let (_weights, weight) = moving(400.0, 500.0);
    let raw = ink_of(
        animated_text(CONTENT, style)
            .size(size)
            .size_layout(SizeLayout::Scaled)
            .weight(weight)
            .into(),
    );

    assert!(
        large > small,
        "30 px darkened {large} pixels and 18 px {small}"
    );

    for (path, ink) in [("paragraph", paragraph), ("raw", raw)] {
        assert!(
            ink.abs_diff(large) < ink.abs_diff(small),
            "the scaled {path} path darkened {ink} pixels, closer to 18 px's \
             {small} than to 30 px's {large}: the scale never reached the \
             renderer"
        );
    }
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

/// The window the composited tests draw into.
const WINDOW: Size = Size::new(400.0, 120.0);

/// Draws `root` on the software backend, keeping the widget state across
/// calls in `cache`, and returns the pixels with the cache to pass on.
fn frame(
    root: Element<'_, ()>,
    cache: user_interface::Cache,
    renderer: &mut Renderer,
) -> (Vec<u8>, user_interface::Cache) {
    let mut ui: UserInterface<'_, (), Theme, Renderer> =
        UserInterface::build(root, WINDOW, cache, renderer);

    renderer.reset(Rectangle::with_size(WINDOW));
    ui.draw(
        renderer,
        &Theme::default(),
        &renderer::Style {
            text_color: Color::BLACK,
        },
        mouse::Cursor::Unavailable,
    );

    let rgba = renderer.screenshot(
        Size::new(WINDOW.width as u32, WINDOW.height as u32),
        1.0,
        Color::WHITE,
    );

    (rgba, ui.into_cache())
}

/// `root` at a fractional position, as text sits after an animated icon:
/// off the device grid on both axes, so a texture that ignored the live
/// text's sub-pixel phase would land its glyphs in the wrong place.
fn off_the_grid(root: Element<'static, ()>) -> Element<'static, ()> {
    use iced_luminate::iced::widget::{Space, column, row};

    column![
        Space::new().height(5.61),
        row![Space::new().width(10.37), root]
    ]
    .into()
}

/// The largest difference between two screenshots in any channel.
fn largest_difference(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap_or(0)
}

fn composited(size: &Anim<f32>) -> Element<'static, ()> {
    off_the_grid(
        animated_text(CONTENT, SMALL)
            .size(size.clone())
            .size_layout(SizeLayout::Composited)
            .into(),
    )
}

/// The seam at either end of a composited transition: its first frame is
/// the text as it stood and its last is the text as it will stand, to the
/// pixel. A texture a shade off 1:1 or off the pixel grid would turn from
/// soft to sharp in the frame the live text takes over, the snap this mode
/// exists to avoid.
#[test]
fn a_composited_transition_starts_and_ends_on_the_resting_text() {
    let _guard = one_at_a_time();
    load_fonts();

    let motion = Motion::new();
    let key = MotionKey::unique();
    let mut renderer = headless_tiny_skia();

    let resting = motion.to(key, QUICK, SMALL.size);
    let (small, cache) = frame(
        composited(&resting),
        user_interface::Cache::default(),
        &mut renderer,
    );

    let size = motion.to(key, QUICK, LARGE.size);
    assert!(size.is_animating(), "the retarget starts a transition");
    let (first, mut cache) = frame(composited(&size), cache, &mut renderer);

    assert!(
        largest_difference(&first, &small) <= 2,
        "the first composited frame is not the resting 14 px text"
    );

    // The tail: close enough to 18 px to be drawn 1:1, not yet settled.
    let mut clock = FrameClock::new(&motion);
    let mut last = None;
    while size.is_animating() {
        let _ = clock.run(1);
        let (pixels, next) = frame(composited(&size), cache, &mut renderer);
        cache = next;

        if size.is_animating() && (size.get() - LARGE.size).abs() < 0.01 {
            last = Some(pixels);
        }
    }
    let last = last.expect("the spring passed through its tail");

    let (large, _) = frame(
        off_the_grid(animated_text(CONTENT, SMALL).size(LARGE.size).into()),
        user_interface::Cache::default(),
        &mut renderer,
    );

    assert!(
        largest_difference(&last, &large) <= 2,
        "the last composited frame is not the resting 18 px text"
    );
    assert!(
        largest_difference(&small, &large) > 100,
        "the two ends look alike, so the test proves nothing"
    );
}

/// In between, the line box follows the size, as under `Live`: what is laid
/// out after the text moves with it. (Each measurement here starts a fresh
/// widget, whose transition starts where the size has got to; that the box
/// is unrounded between the ends is pinned where the state survives, in the
/// widget's own tests.)
#[test]
fn a_composited_size_grows_the_line_box_between_two_steps() {
    let (motion, size) = moving(SMALL.size, LARGE.size);
    let mut clock = FrameClock::new(&motion);

    let _ = clock.run(4);
    let middle = sized_box(&size, SizeLayout::Composited);

    let _ = clock.run(120);
    let end = sized_box(&size, SizeLayout::Composited);

    let start = stock_box(SMALL);
    assert!(
        start.width < middle.width && middle.width < end.width,
        "the line went {} → {} → {} px wide",
        start.width,
        middle.width,
        end.width
    );
    assert!(
        start.height < middle.height && middle.height < end.height,
        "the line box went {} → {} → {} px tall",
        start.height,
        middle.height,
        end.height
    );
    assert_eq!(end, stock_box(LARGE), "and rests on the stock widget's box");
}

#[test]
fn a_composited_line_reads_both_tracks_at_the_layout_tier() {
    let (_motion, size) = moving(SMALL.size, LARGE.size);
    let _ = sized_box(&size, SizeLayout::Composited);

    assert_eq!(
        size.tier(),
        Some(Tier::Layout),
        "the box moves with the size"
    );
}

/// The same seam for a weight: the crossfade starts on the weight the text
/// stood at and ends on the one it will stand at, to the pixel.
#[test]
fn a_composited_weight_transition_starts_and_ends_on_the_resting_text() {
    let _guard = one_at_a_time();
    load_fonts();

    let label = |weight: &Anim<f32>| -> Element<'static, ()> {
        off_the_grid(
            animated_text(CONTENT, MOVING_STYLE)
                .weight(weight.clone())
                .size_layout(SizeLayout::Composited)
                .into(),
        )
    };

    let motion = Motion::new();
    let key = MotionKey::unique();
    let mut renderer = headless_tiny_skia();

    let resting = motion.to(key, QUICK, 500.0);
    let (medium, cache) = frame(
        label(&resting),
        user_interface::Cache::default(),
        &mut renderer,
    );

    let weight = motion.to(key, QUICK, 600.0);
    let (first, mut cache) = frame(label(&weight), cache, &mut renderer);
    assert!(
        largest_difference(&first, &medium) <= 2,
        "the first composited frame is not the resting weight"
    );

    let mut clock = FrameClock::new(&motion);
    let mut last = None;
    while weight.is_animating() {
        let _ = clock.run(1);
        let (pixels, next) = frame(label(&weight), cache, &mut renderer);
        cache = next;

        if weight.is_animating() && (weight.get() - 600.0).abs() < 0.5 {
            last = Some(pixels);
        }
    }
    let last = last.expect("the spring passed through its tail");

    let (semibold, _) = frame(
        off_the_grid(animated_text(CONTENT, MOVING_STYLE).weight(600.0).into()),
        user_interface::Cache::default(),
        &mut renderer,
    );
    assert!(
        largest_difference(&last, &semibold) <= 2,
        "the last composited frame is not the resting weight"
    );
    assert!(
        largest_difference(&medium, &semibold) > 100,
        "the two weights look alike, so the test proves nothing"
    );
}

/// The seam as the application meets it: size and weight on tracks of their
/// own, the text off the device grid, at the display scales people run. The
/// very last frame of the transition, whatever it happens to be, is the
/// resting text; so is the first.
///
/// This is what catches a texture drawn 1:1 but recorded at a slightly
/// different sub-pixel phase than it lands at: the filter then resamples the
/// whole line, and it turns sharp in the frame the live text takes over.
#[test]
fn a_composited_transition_is_seamless_at_every_scale() {
    let _guard = one_at_a_time();
    load_fonts();

    let composited = |weight: &Anim<f32>, size: &Anim<f32>| -> Element<'static, ()> {
        animated_text(CONTENT, SMALL)
            .weight(weight.clone())
            .size(size.clone())
            .size_layout(SizeLayout::Composited)
            .into()
    };

    for scale in [1.0, 1.25, 1.5, 2.0] {
        for (from, to) in [
            ((400.0, 16.0), (500.0, 20.0)),
            ((500.0, 20.0), (400.0, 16.0)),
        ] {
            let (first, last) = seams(&mut headless_tiny_skia(), scale, composited, from, to);

            assert!(
                first <= 2,
                "{scale}x, {from:?} → {to:?}: the first frame is not the text as it stood"
            );
            assert!(
                last <= 2,
                "{scale}x, {from:?} → {to:?}: the last frame is not the text as it stands, \
                 off by {last}"
            );
        }
    }
}

/// The ink-weighted centre of what `rgba` darkened, `width` pixels a row.
fn ink_centre(rgba: &[u8], width: usize) -> (f32, f32) {
    let (mut x, mut y, mut mass) = (0.0_f64, 0.0_f64, 0.0_f64);

    for (i, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let ink = 255.0 - f64::from(px[0]);

        if ink > 8.0 {
            x += ink * (i % width) as f64;
            y += ink * (i / width) as f64;
            mass += ink;
        }
    }

    ((x / mass) as f32, (y / mass) as f32)
}

/// Live text sits on whole device pixels vertically: cosmic-text truncates
/// a line's vertical position ("hinting in Y axis"). A composited frame
/// that glides where the live text steps would part from it by up to a
/// pixel while the layout around it moves, and close the gap with a jump
/// the moment the live text takes over. So every frame of a composited
/// transition sits where a live frame of the same moment would.
#[test]
fn a_composited_line_steps_vertically_with_the_live_text() {
    use iced_luminate::animate::curves::SMOOTH;
    use iced_luminate::animate::widget::sized;
    use iced_luminate::iced::widget::{Space, column};

    const WIDE: Size = Size::new(900.0, 200.0);
    let scale = 1.25;

    let _guard = one_at_a_time();
    load_fonts();

    let draw = |root: Element<'_, ()>, cache: user_interface::Cache, renderer: &mut Renderer| {
        let logical = Size::new(WIDE.width / scale, WIDE.height / scale);
        let mut ui: UserInterface<'_, (), Theme, Renderer> =
            UserInterface::build(root, logical, cache, renderer);

        renderer.reset(Rectangle::with_size(logical));
        ui.draw(
            renderer,
            &Theme::default(),
            &renderer::Style {
                text_color: Color::BLACK,
            },
            mouse::Cursor::Unavailable,
        );

        let rgba = renderer.screenshot(
            Size::new(WIDE.width as u32, WIDE.height as u32),
            scale,
            Color::WHITE,
        );
        (rgba, ui.into_cache())
    };

    // The layout above the text moves on a slow curve of its own, the way a
    // row above collapses, while the text changes size and weight.
    let root = |policy: SizeLayout,
                gap: &Anim<f32>,
                size: &Anim<f32>,
                weight: &Anim<f32>|
     -> Element<'static, ()> {
        column![
            sized(Space::new().width(10.0)).height(gap),
            animated_text(CONTENT, MOVING_STYLE)
                .size(size.clone())
                .weight(weight.clone())
                .size_layout(policy)
                // The live text at exactly the size of the moment, not at
                // the nearest step: the reference is where cosmic-text puts
                // that size, rounding and all.
                .size_step(1e-4),
        ]
        .into()
    };

    let mut worst = 0.0_f32;
    let mut renderers = [headless_tiny_skia(), headless_tiny_skia()];
    let mut caches = [
        user_interface::Cache::default(),
        user_interface::Cache::default(),
    ];
    let policies = [SizeLayout::Composited, SizeLayout::Live];

    let motion = Motion::new();
    let keys = (
        MotionKey::unique(),
        MotionKey::unique(),
        MotionKey::unique(),
    );
    let resting = (
        motion.to(keys.0, SMOOTH, 40.0),
        motion.to(keys.1, QUICK, 20.0),
        motion.to(keys.2, QUICK, 500.0),
    );
    for _ in 0..2 {
        for (i, policy) in policies.into_iter().enumerate() {
            let cache = std::mem::take(&mut caches[i]);
            let (_, cache) = draw(
                root(policy, &resting.0, &resting.1, &resting.2),
                cache,
                &mut renderers[i],
            );
            caches[i] = cache;
        }
    }

    let gap = motion.to(keys.0, SMOOTH, 18.3);
    let size = motion.to(keys.1, QUICK, 16.0);
    let weight = motion.to(keys.2, QUICK, 400.0);
    let mut clock = FrameClock::new(&motion);

    while size.is_animating() || weight.is_animating() {
        let _ = clock.run(1);

        let mut centres = [(0.0, 0.0); 2];
        for (i, policy) in policies.into_iter().enumerate() {
            let cache = std::mem::take(&mut caches[i]);
            let (pixels, cache) =
                draw(root(policy, &gap, &size, &weight), cache, &mut renderers[i]);
            caches[i] = cache;
            centres[i] = ink_centre(&pixels, WIDE.width as usize);
        }

        worst = worst.max((centres[0].1 - centres[1].1).abs());
    }

    // A scaled texture and glyphs rasterised at the size of the moment differ
    // a little in shape, and so in where their ink centres; a line placed a
    // pixel off is what this is after.
    assert!(
        worst < 0.75,
        "a composited frame sat {worst} px away from the live text vertically"
    );
}

/// How many times, frame to frame, a line of text moved against what is
/// beside it: `offsets` are its position relative to that, one a frame.
fn slips(offsets: &[f32]) -> usize {
    offsets
        .windows(2)
        .filter(|pair| (pair[1] - pair[0]).abs() > 0.05)
        .count()
}

/// Asserts what snapping a moving layout promises, given where a line of
/// text sat against what is beside it, frame by frame, from a frame at rest
/// through a motion: that it left exactly from where it rested, and that it
/// moved against its neighbour once at most — the one fraction of a pixel
/// the two ends of the motion differ by, which snapping puts in the middle,
/// where the motion is fastest — while unsnapped it slips again and again.
fn assert_snapped_in_step(what: &str, glided: &[f32], snapped: &[f32]) {
    assert!(
        slips(glided) > 1,
        "{what}: unsnapped, the text should slip against its neighbour; offsets {glided:?}"
    );
    assert!(
        (snapped[1] - snapped[0]).abs() <= 0.05,
        "{what}: the first frame of the motion jumped from where it rested: {snapped:?}"
    );
    assert!(
        slips(snapped) <= 1,
        "{what}: snapped, the text slipped {} times: {snapped:?}",
        slips(snapped)
    );
}

/// The ink-weighted vertical centre of what `rgba` darkened between two
/// columns, `width` pixels a row.
fn ink_row(rgba: &[u8], width: usize, columns: std::ops::Range<usize>) -> f32 {
    let (mut y, mut mass) = (0.0_f64, 0.0_f64);

    for (i, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let ink = 255.0 - f64::from(px[1]);

        if columns.contains(&(i % width)) && ink > 8.0 {
            y += ink * (i / width) as f64;
            mass += ink;
        }
    }

    (y / mass) as f32
}

/// A layout that moves its text vertically: a box above collapses on a slow
/// curve, and a bar and a label beside it ride along. Text sits on whole
/// device pixels vertically and the bar does not, so on a gliding layout the
/// label trails the bar and catches up a pixel at a time. Snapped to whole
/// device pixels, the layout moves both by the same steps, and the label
/// never moves against its bar.
#[test]
fn a_pixel_snapped_layout_moves_text_with_what_is_beside_it() {
    use iced_luminate::animate::curves::SMOOTH;
    use iced_luminate::animate::widget::{shape, sized};
    use iced_luminate::iced::widget::{Space, column, row};
    use iced_luminate::iced::{Alignment, Length};

    const WIDE: Size = Size::new(600.0, 160.0);
    let scale = 1.25;

    let _guard = one_at_a_time();
    load_fonts();

    let drift = |snap: bool| {
        let mut renderer = headless_tiny_skia();
        let mut cache = user_interface::Cache::default();
        // The first frame lays out before any screenshot at this scale; the
        // factor is process-wide, and the last test may have left another.
        iced_luminate::animate::set_scale_factor(scale);

        let motion = Motion::new();
        let key = MotionKey::unique();
        let mut gap = motion.to(key, SMOOTH, 40.0);
        let mut clock = FrameClock::new(&motion);

        // A frame at rest, then the motion: a retargeted track takes one
        // frame to restart the clock and a second to move.
        let mut offsets = Vec::new();
        for frame in 0.. {
            if frame == 1 {
                gap = motion.to(key, SMOOTH, 7.3);
            }
            if frame >= 1 {
                let _ = clock.run(if frame == 1 { 2 } else { 1 });
                if !gap.is_animating() {
                    break;
                }
            }

            let root: Element<'_, ()> = column![
                sized(Space::new().width(10.0))
                    .height(gap.clone())
                    .pixel_snap(snap),
                row![
                    shape()
                        .width(40.0)
                        .height(3.0)
                        .fill(Color::from_rgb(1.0, 0.0, 1.0))
                        // A shape snaps itself to the grid whenever its
                        // bounds stand still, which a layout moving in
                        // whole steps does every other frame; the bar
                        // here stands for any quad the layout moves.
                        .pixel_snap(iced_luminate::animate::widget::PixelSnap::Never),
                    Space::new().width(Length::Fixed(20.0)),
                    styled_text(CONTENT, MOVING_STYLE),
                ]
                .align_y(Alignment::Center),
            ]
            .into();

            let logical = Size::new(WIDE.width / scale, WIDE.height / scale);
            let mut ui: UserInterface<'_, (), Theme, Renderer> =
                UserInterface::build(root, logical, cache, &mut renderer);
            renderer.reset(Rectangle::with_size(logical));
            ui.draw(
                &mut renderer,
                &Theme::default(),
                &renderer::Style {
                    text_color: Color::BLACK,
                },
                mouse::Cursor::Unavailable,
            );
            let rgba = renderer.screenshot(
                Size::new(WIDE.width as u32, WIDE.height as u32),
                scale,
                Color::WHITE,
            );
            cache = ui.into_cache();

            let width = WIDE.width as usize;
            // The bar spans the first 40 logical pixels; the text starts at 60.
            let bar = ink_row(&rgba, width, 0..(40.0 * scale) as usize);
            let text = ink_row(&rgba, width, (65.0 * scale) as usize..width);
            // An empty crop is NaN, which the drift below would swallow.
            assert!(
                bar.is_finite() && text.is_finite(),
                "the bar or the text is out of its crop"
            );
            offsets.push(text - bar);
        }

        offsets
    };

    assert_snapped_in_step("a collapsing box", &drift(false), &drift(true));
}

/// The rows below a label that changes size ride on its line box. Grown by
/// fractions of a pixel, the box moves them smoothly and their text in
/// whole-pixel jumps, against its own background; snapped, the box moves
/// them by whole device pixels, and their text keeps its place beside them.
#[test]
fn a_pixel_snapped_label_moves_the_rows_below_it_in_step() {
    use iced_luminate::animate::widget::{PixelSnap, shape};
    use iced_luminate::iced::widget::{Space, column, row};
    use iced_luminate::iced::{Alignment, Length};

    const WIDE: Size = Size::new(600.0, 200.0);
    let scale = 1.25;

    let _guard = one_at_a_time();
    load_fonts();

    let drift = |policy: SizeLayout, snap: bool| {
        let mut renderer = headless_tiny_skia();
        let mut cache = user_interface::Cache::default();
        // The first frame lays out before any screenshot at this scale; the
        // factor is process-wide, and the last test may have left another.
        iced_luminate::animate::set_scale_factor(scale);

        let motion = Motion::new();
        let key = MotionKey::unique();
        let mut size = motion.to(key, QUICK, 20.0);
        let mut clock = FrameClock::new(&motion);

        // A frame at rest, then the motion: a retargeted track takes one
        // frame to restart the clock and a second to move.
        let mut offsets = Vec::new();
        for frame in 0.. {
            if frame == 1 {
                size = motion.to(key, QUICK, 16.0);
            }
            if frame >= 1 {
                let _ = clock.run(if frame == 1 { 2 } else { 1 });
                if !size.is_animating() {
                    break;
                }
            }

            let root: Element<'_, ()> = column![
                animated_text(CONTENT, MOVING_STYLE)
                    .size(size.clone())
                    .size_layout(policy)
                    .pixel_snap(snap),
                // Room to crop between the label, at most 30 px tall, and the
                // row, which starts 20 px under it.
                Space::new().height(20.0),
                row![
                    shape()
                        .width(40.0)
                        .height(3.0)
                        .fill(Color::from_rgb(1.0, 0.0, 1.0))
                        .pixel_snap(PixelSnap::Never),
                    Space::new().width(Length::Fixed(20.0)),
                    styled_text(CONTENT, MOVING_STYLE),
                ]
                .align_y(Alignment::Center),
            ]
            .into();

            let logical = Size::new(WIDE.width / scale, WIDE.height / scale);
            let mut ui: UserInterface<'_, (), Theme, Renderer> =
                UserInterface::build(root, logical, cache, &mut renderer);
            renderer.reset(Rectangle::with_size(logical));
            ui.draw(
                &mut renderer,
                &Theme::default(),
                &renderer::Style {
                    text_color: Color::BLACK,
                },
                mouse::Cursor::Unavailable,
            );
            let rgba = renderer.screenshot(
                Size::new(WIDE.width as u32, WIDE.height as u32),
                scale,
                Color::WHITE,
            );
            cache = ui.into_cache();

            // Only the lower row: below the label's tallest box, above
            // where the row can rise to.
            let width = WIDE.width as usize;
            let below = &rgba[width * 4 * (40.0 * scale) as usize..];
            let bar = ink_row(below, width, 0..(40.0 * scale) as usize);
            let text = ink_row(below, width, (65.0 * scale) as usize..width);
            // An empty crop is NaN, which the drift below would swallow.
            assert!(
                bar.is_finite() && text.is_finite(),
                "the bar or the text is out of its crop"
            );
            offsets.push(text - bar);
        }

        offsets
    };

    for policy in [SizeLayout::Composited, SizeLayout::Live] {
        assert_snapped_in_step(
            &format!("{policy:?} label"),
            &drift(policy, false),
            &drift(policy, true),
        );
    }
}

/// How far the first and the last frame of a transition from `from` to `to`
/// — `(weight, size)`, each on a track of its own, the text off the device
/// grid — are from the resting text before and after, in the largest
/// channel difference of any pixel.
fn seams(
    renderer: &mut Renderer,
    scale: f32,
    label: impl Fn(&Anim<f32>, &Anim<f32>) -> Element<'static, ()>,
    from: (f32, f32),
    to: (f32, f32),
) -> (u8, u8) {
    const WIDE: Size = Size::new(900.0, 200.0);

    let draw = |root: Element<'_, ()>, cache: user_interface::Cache, renderer: &mut Renderer| {
        let logical = Size::new(WIDE.width / scale, WIDE.height / scale);
        let mut ui: UserInterface<'_, (), Theme, Renderer> =
            UserInterface::build(root, logical, cache, renderer);

        renderer.reset(Rectangle::with_size(logical));
        ui.draw(
            renderer,
            &Theme::default(),
            &renderer::Style {
                text_color: Color::BLACK,
            },
            mouse::Cursor::Unavailable,
        );

        let rgba = renderer.screenshot(
            Size::new(WIDE.width as u32, WIDE.height as u32),
            scale,
            Color::WHITE,
        );
        (rgba, ui.into_cache())
    };

    let motion = Motion::new();
    let (weight_key, size_key) = (MotionKey::unique(), MotionKey::unique());
    let resting = (
        motion.to(weight_key, QUICK, from.0),
        motion.to(size_key, QUICK, from.1),
    );

    // A screenshot adopts its scale for the frames after it.
    let (_, cache) = draw(
        off_the_grid(label(&resting.0, &resting.1)),
        user_interface::Cache::default(),
        renderer,
    );
    let (before, cache) = draw(off_the_grid(label(&resting.0, &resting.1)), cache, renderer);

    let weight = motion.to(weight_key, QUICK, to.0);
    let size = motion.to(size_key, QUICK, to.1);
    let (first, mut cache) = draw(off_the_grid(label(&weight, &size)), cache, renderer);

    let mut clock = FrameClock::new(&motion);
    let mut last = None;
    while weight.is_animating() || size.is_animating() {
        let _ = clock.run(1);
        let (pixels, next) = draw(off_the_grid(label(&weight, &size)), cache, renderer);
        cache = next;

        if weight.is_animating() || size.is_animating() {
            last = Some(pixels);
        }
    }
    let (after, _) = draw(off_the_grid(label(&weight, &size)), cache, renderer);
    let last = last.expect("the transition drew at least one frame");

    (
        largest_difference(&first, &before),
        largest_difference(&last, &after),
    )
}

/// The same seam for the live text, which every frame shapes and rasterises
/// again at the size and weight of the moment: its first frame is the text
/// as it stood and its last the text as it stands, at every scale, with the
/// kit's default steps and with a continuous weight, on the scale and off
/// it.
///
/// The grids every animated value is rounded to are anchored at the target,
/// so the end is exact by construction; the start is exact only because
/// each step is fitted to the distance it covers, and because the line
/// height, unrounded in between, is rounded again at both ends.
#[test]
fn a_live_transition_is_seamless_at_every_scale() {
    let _guard = one_at_a_time();
    load_fonts();

    for weight_step in [None, Some(1.0)] {
        let live = |weight: &Anim<f32>, size: &Anim<f32>| -> Element<'static, ()> {
            let label = animated_text(CONTENT, SMALL)
                .weight(weight.clone())
                .size(size.clone());

            match weight_step {
                Some(step) => label.weight_step(step).into(),
                None => label.into(),
            }
        };

        for scale in [1.0, 1.25, 1.5, 2.0] {
            for (from, to) in [
                ((400.0, 16.0), (500.0, 20.0)),
                ((500.0, 20.0), (400.0, 16.0)),
                ((500.0, 17.3), (400.0, 14.0)),
                ((400.0, 13.1), (600.0, 21.7)),
            ] {
                let (first, last) = seams(&mut headless_tiny_skia(), scale, live, from, to);

                assert!(
                    first <= 2 && last <= 2,
                    "{scale}x, {from:?} → {to:?}, weight step {weight_step:?}: \
                     the first frame is {first} off the text as it stood, the last \
                     {last} off the text as it stands"
                );
            }
        }
    }
}

/// How much `root` darkens a white window: every channel's shortfall from
/// white, summed. Proportional to how opaque black text is drawn.
fn darkness(root: Element<'_, ()>) -> u64 {
    let (rgba, _) = frame(
        root,
        user_interface::Cache::default(),
        &mut headless_tiny_skia(),
    );

    rgba.as_chunks::<4>()
        .0
        .iter()
        .map(|px| u64::from(255 - px[0]))
        .sum()
}

/// An opacity fades the text on every path it can be drawn by: the stock
/// paragraph (a named weight at rest), the shaped line (a weight between the
/// named ones), and the textures of a composited transition. `1.0` is the
/// text as it is, `0.0` nothing, and half is about half the ink.
#[test]
fn an_opacity_fades_the_text_on_every_path() {
    type Label = AnimatedText<'static, Theme, Renderer>;

    let _guard = one_at_a_time();
    load_fonts();

    let motion = Motion::new();
    let key = MotionKey::unique();
    let _ = motion.to(key, QUICK, 16.0);
    let moving = motion.to(key, QUICK, 20.0);
    let mut clock = FrameClock::new(&motion);
    let _ = clock.run(3);
    assert!(
        moving.is_animating(),
        "the composited label is mid-transition"
    );

    let paths: [(&str, &dyn Fn() -> Label); 3] = [
        ("paragraph", &|| animated_text(CONTENT, SMALL)),
        ("shaped", &|| animated_text(CONTENT, SMALL).weight(437.0)),
        ("composited", &|| {
            animated_text(CONTENT, SMALL)
                .size(moving.clone())
                .size_layout(SizeLayout::Composited)
        }),
    ];

    for (path, label) in paths {
        let plain = darkness(label().into());
        let opaque = darkness(label().opacity(1.0).into());
        let half = darkness(label().opacity(0.5).into());
        let hidden = darkness(label().opacity(0.0).into());

        assert!(plain > 0, "{path}: nothing was drawn at all");
        assert_eq!(opaque, plain, "{path}: an opacity of 1 changed the text");
        assert_eq!(hidden, 0, "{path}: an opacity of 0 still drew");

        let share = half as f64 / plain as f64;
        assert!(
            (0.4..0.6).contains(&share),
            "{path}: half opacity drew {share} of the ink"
        );
    }
}

/// A fade is a redraw, not a relayout: the opacity is read where the text is
/// drawn, and nothing about the line box depends on it.
#[test]
fn an_opacity_costs_a_redraw_and_no_relayout() {
    use iced_luminate::animate::curves::FADE;

    let motion = Motion::new();
    let key = MotionKey::unique();
    let _ = motion.to(key, FADE, 1.0);
    let opacity = motion.to(key, FADE, 0.0);

    let label: Element<'_, ()> = animated_text(CONTENT, SMALL)
        .opacity(opacity.clone())
        .into();
    let _ = darkness(label);

    assert_eq!(opacity.tier(), Some(Tier::Paint));

    let mut clock = FrameClock::new(&motion);
    let status = clock.run(2);
    assert!(status.animating, "the fade is still running");
    assert!(!status.layout_invalid, "a fade asked for a relayout");
}

/// `iced`'s own renderer cannot render into a texture. A composited label on
/// it is laid out and drawn as a live one rather than refused: the same box,
/// frame for frame.
#[test]
fn a_composited_line_without_textures_is_drawn_live() {
    let _guard = one_at_a_time();
    load_fonts();

    let (motion, size) = moving(SMALL.size, LARGE.size);
    let mut clock = FrameClock::new(&motion);
    let _ = clock.run(3);
    assert!(size.is_animating(), "mid-transition");

    let bounds = |size_layout: SizeLayout| {
        let root: iced::Element<'_, (), iced::Theme, iced::Renderer> =
            animated_text(CONTENT, SMALL)
                .size(size.clone())
                .size_layout(size_layout)
                .into();
        let mut ui: Simulator<'_, (), iced::Theme, iced::Renderer> =
            Simulator::with_size(iced::Settings::default(), Size::new(600.0, 400.0), root);

        let bounds = ui.find(CONTENT).expect("the text is on screen").bounds();
        let _ = ui.snapshot(&iced::Theme::Light).expect("it draws");
        bounds
    };

    assert_eq!(bounds(SizeLayout::Composited), bounds(SizeLayout::Live));
}
