//! What each wrapper around a Luminate widget costs, per widget, per frame.
//!
//! The kit builds its widgets by wrapping an `iced` one: a button is a
//! [`MultiBorder`] around `iced::widget::button`, and picks up an
//! [`InteractionOverride`] and a `container` when the descriptor asks for
//! them; an input is an [`ErrorBubble`] around a `MultiBorder` around
//! `iced::widget::text_input`. Every one of those is a boxed `Element` the
//! runtime walks four times a frame, so the question this answers is what a
//! wrapper costs and whether the count is worth managing.
//!
//! The four phases of a frame are timed where each happens rather than by
//! subtracting one run from another. `build` is the application's `view`.
//! `layout` is the diff of the new tree against the cached one plus the
//! layout pass. `draw` is the draw pass. `update` is one `RedrawRequested`,
//! the event an animating application delivers every frame — it is what
//! makes `MultiBorder` retrack its hover and its `Focus::Custom` probe walk
//! the content's state tree. Nothing here measures GPU work: the renderer is
//! the headless software one, and `draw` stops at the primitives it records.
//!
//! The cache is threaded from frame to frame, as a running application keeps
//! it. Rebuilding it every iteration would re-shape every paragraph and
//! report a cold first frame as if it were the steady state — on this
//! machine that alone is the difference between 75 ns and 3 µs of layout per
//! button.
//!
//! Each case builds `WIDGETS` copies of one widget in a column, and the
//! numbers are reported per widget, so cases are read against each other
//! rather than in absolute terms. Every group is a ladder that adds one
//! thing per rung, so a rung is read as the difference from the one above
//! it: the first two add wrappers to an inner `iced` widget, and the last
//! takes the column and the two captions an [`Input`] grows when it is given
//! a label or a hint, and adds them one at a time.
//!
//! [`Input`]: iced_luminate::descriptor::Input
//!
//! Each ladder is checked against the descriptor API: `luminate button`,
//! `luminate input` and `… through the API` build the same thing the public
//! way and should land on the rung with the same parts. That they do is what
//! says the ladder is measuring the real widget rather than a lookalike.
//!
//! ```text
//! cargo bench -p iced_luminate --bench widget_cost
//! ```
//!
//! [`MultiBorder`]: iced_luminate::widget::multi_border::MultiBorder
//! [`InteractionOverride`]: iced_luminate::widget::interaction_override::InteractionOverride
//! [`ErrorBubble`]: iced_luminate::widget::error_bubble::ErrorBubble

use std::hint::black_box;
use std::time::{Duration, Instant};

use iced::advanced::renderer::Headless;
use iced::advanced::widget::{Tree, tree};
use iced::widget::{Column, button, container, text, text_input};
use iced::{Length, Point, Size, mouse};
use iced_luminate::descriptor::{Button, ButtonContent, ButtonHierarchy, ButtonSize, Input};
use iced_luminate::theme::typography::styled_text;
use iced_luminate::theme::{ButtonClass, InputClass, TextClass, Theme};
use iced_luminate::widget::interaction_override::interaction_override;
use iced_luminate::widget::multi_border::{Focus, Ring, Style, multi_border};
use iced_luminate::{Element, Luminate, Renderer};
use iced_runtime::UserInterface;
use iced_runtime::user_interface::Cache;

/// Widgets per case. Large enough that per-widget work dominates the column
/// around it, small enough that a case still fits in cache.
const WIDGETS: usize = 200;

/// Batches per case; the reported figure is the median of these.
const SAMPLES: usize = 101;

/// Batches run and discarded before the first sample.
const WARMUP: usize = 25;

/// The surface every case is laid out and drawn into.
///
/// Tall enough to hold every widget: `draw` culls what falls outside the
/// viewport, and a case whose widgets were half off-screen would report half
/// the draw cost.
const BOUNDS: Size = Size::new(900.0, 40_000.0);

/// The width the column is fixed to, so a `Fill` widget inside it lays out
/// to a form-sized box rather than collapsing to nothing.
const FORM_WIDTH: f32 = 320.0;

/// The application message. The kit only ever clones it.
type Message = ();

/// A widget's label, borrowed for the life of the process.
const LABEL: &str = "Press";

/// Parked over the first widget, so the hover and interaction paths run.
const CURSOR: mouse::Cursor = mouse::Cursor::Available(Point::new(40.0, 20.0));

fn main() {
    let mut bench = Bench::new();
    let kit = Luminate::new();

    println!(
        "widget_cost: {WIDGETS} widgets per case, median of {SAMPLES} batches, \
         ns per widget benchmark\n"
    );

    header("button — the same inner iced button under 0, 1, 2 and 3 wrappers");
    bench.run("iced button", &kit, inner_button);
    bench.run("+ multi_border", &kit, ringed);
    bench.run("+ interaction_override", &kit, |kit| {
        interaction_override(ringed(kit)).into()
    });
    bench.run("+ container", &kit, |kit| {
        container(interaction_override(ringed(kit))).into()
    });

    header("button — through the public descriptor API");
    bench.run("luminate button", &kit, |kit| {
        kit.button(Button::new(LABEL).on_press(()))
    });
    bench.run("… default_interaction", &kit, |kit| {
        kit.button(Button::new(LABEL).on_press(()).force_default_interaction())
    });
    bench.run("… disabled", &kit, |kit| kit.button(Button::new(LABEL)));

    header("input — the same inner text_input under the kit's two wrappers");
    bench.run("iced text_input", &kit, |_| inner_input());
    bench.run("+ multi_border (Click)", &kit, |kit| {
        field(kit, Focus::Click)
    });
    bench.run("+ multi_border (Custom)", &kit, |kit| {
        field(kit, Focus::Custom(Box::new(text_input_focused)))
    });
    bench.run("luminate input", &kit, |kit| {
        kit.input(Input::new("Placeholder", "value").on_input(|_| ()))
    });
    bench.run("… error", &kit, |kit| {
        kit.input(
            Input::new("Placeholder", "value")
                .on_input(|_| ())
                .error(Some("Something went wrong")),
        )
    });

    header("label and hint — the column and the two captions, one at a time");
    bench.run("iced text", &kit, |_| text("Label").into());
    bench.run("luminate input", &kit, |kit| {
        kit.input(Input::new("Placeholder", "value").on_input(|_| ()))
    });
    bench.run("+ column", &kit, |kit| input_column(kit, None, None));
    bench.run("+ label", &kit, |kit| {
        input_column(kit, Some(caption(kit, Caption::Label)), None)
    });
    bench.run("+ label + hint", &kit, |kit| {
        input_column(
            kit,
            Some(caption(kit, Caption::Label)),
            Some(caption(kit, Caption::Hint)),
        )
    });
    bench.run("… as plain text", &kit, |kit| {
        input_column(
            kit,
            Some(caption(kit, Caption::Plain)),
            Some(caption(kit, Caption::Plain)),
        )
    });
    bench.run("… through the API", &kit, |kit| {
        kit.input(
            Input::new("Placeholder", "value")
                .on_input(|_| ())
                .label("Label")
                .hint("Hint"),
        )
    });
}

/// The `iced` button the kit's button wraps, built exactly the way
/// [`Luminate::button`] builds it, so the ladder above it measures wrappers
/// and nothing else.
fn inner_button(kit: &Luminate) -> Element<'static, Message> {
    let tokens = kit.theme().button;
    let content = ButtonContent::Text(LABEL);

    button(
        styled_text::<Theme, Renderer>(LABEL, tokens.label)
            .align_x(iced::Alignment::Center)
            .width(Length::Fill),
    )
    .padding(tokens.padding(ButtonSize::Small).for_content(&content))
    .width(Length::Shrink)
    .height(Length::Shrink)
    .on_press(())
    .class(ButtonClass::Hierarchy(ButtonHierarchy::Primary))
    .into()
}

/// [`inner_button`] under the pressed ring the kit puts around it.
fn ringed(kit: &Luminate) -> Element<'static, Message> {
    multi_border(inner_button(kit))
        .style(move |theme: &Theme, status| {
            if !status.is_pressed || status.is_disabled {
                return Style::new();
            }

            let t = theme.button;
            let variant = t.variant(ButtonHierarchy::Primary);

            Style::new().ring(
                Ring::outer(t.ring_width, variant.ring)
                    .offset(t.ring_offset)
                    .radius(t.ring_radius)
                    .overflowing(),
            )
        })
        .into()
}

/// The `iced` text input the kit's input wraps.
fn inner_input() -> Element<'static, Message> {
    text_input("Placeholder", "value")
        .on_input(|_| ())
        .class(InputClass::Normal)
        .into()
}

/// [`inner_input`] under the focus ring, with the focus detection the case
/// names. The kit uses [`Focus::Custom`], which walks the content's state
/// tree on every event that could move focus — the per-frame redraw included.
fn field(kit: &Luminate, focus: Focus<'static>) -> Element<'static, Message> {
    let tokens = kit.theme().input;

    multi_border(inner_input())
        .focus(focus)
        .height(tokens.height)
        .style(move |theme: &Theme, status| {
            if status.is_disabled || !status.is_focused {
                return Style::new();
            }

            let t = theme.input;

            Style::new().ring(
                Ring::outer(t.ring_width, t.ring)
                    .offset(t.ring_offset)
                    .radius(t.ring_radius)
                    .overflowing(),
            )
        })
        .into()
}

/// Which caption a rung of the label-and-hint ladder pushes.
#[derive(Clone, Copy)]
enum Caption {
    /// The kit's label: [`styled_text`] under [`TextClass::Label`].
    Label,
    /// The kit's hint: [`styled_text`] under [`TextClass::Hint`].
    Hint,
    /// A bare `iced::widget::text`, to separate the cost of a text widget
    /// from the cost of the type style and class the kit puts on it.
    Plain,
}

/// One caption above or below the field.
fn caption(kit: &Luminate, which: Caption) -> Element<'static, Message> {
    let tokens = kit.theme().input;

    match which {
        Caption::Label => styled_text::<Theme, Renderer>("Label", tokens.label_style)
            .class(TextClass::Label)
            .into(),
        Caption::Hint => styled_text::<Theme, Renderer>("Hint", tokens.hint_style)
            .class(TextClass::Hint)
            .into(),
        Caption::Plain => text("Label").into(),
    }
}

/// The column [`Luminate::input`] wraps the field in as soon as it has a
/// label or a hint, with the captions it was given pushed around it.
///
/// With neither, this is the column and nothing else — the rung the API
/// cannot reach, because it returns the bare field in that case.
fn input_column(
    kit: &Luminate,
    above: Option<Element<'static, Message>>,
    below: Option<Element<'static, Message>>,
) -> Element<'static, Message> {
    let tokens = kit.theme().input;
    let field = kit.input(Input::new("Placeholder", "value").on_input(|_| ()));

    let mut column = Column::new().width(Length::Fill);

    if let Some(above) = above {
        column = column.push(above);
    }

    column = column.push(field);

    if let Some(below) = below {
        column = column.push(below);
    }

    column.spacing(tokens.spacing).into()
}

/// Whether any `text_input` in `tree` is focused: the probe the kit hands to
/// [`Focus::Custom`].
fn text_input_focused(tree: &Tree) -> bool {
    type Paragraph = <Renderer as iced::advanced::text::Renderer>::Paragraph;

    if tree.tag == tree::Tag::of::<text_input::State<Paragraph>>() {
        return tree
            .state
            .downcast_ref::<text_input::State<Paragraph>>()
            .is_focused();
    }

    tree.children.iter().any(text_input_focused)
}

fn header(title: &str) {
    println!("{title}");
    println!(
        "  {:<24} {:>9} {:>9} {:>9} {:>9} {:>9}",
        "", "build", "layout", "draw", "update", "frame"
    );
}

/// One frame's cost, split by phase.
#[derive(Clone, Copy, Default)]
struct Phases {
    build: Duration,
    layout: Duration,
    draw: Duration,
    update: Duration,
}

/// Holds the renderer every case is measured against.
struct Bench {
    renderer: Renderer,
}

impl Bench {
    fn new() -> Self {
        // The software backend, as `ci.sh` sets for `iced_test`: it needs no
        // adapter and costs the same on every machine.
        let backend = std::env::var("ICED_TEST_BACKEND").unwrap_or_else(|_| "tiny-skia".into());
        let renderer = iced::futures::executor::block_on(Renderer::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some(&backend),
        ))
        .expect("headless renderer");

        Self { renderer }
    }

    /// Times one case and prints its row.
    fn run(
        &mut self,
        name: &str,
        kit: &Luminate,
        widget: impl Fn(&Luminate) -> Element<'static, Message>,
    ) {
        // One cache for the whole case, threaded from frame to frame. A fresh
        // cache would re-shape every paragraph on every iteration and report a
        // cold first frame as if it were the steady state; the warm-up frames
        // below are what put it in the state a running application keeps it.
        let mut cache = Cache::default();
        let mut samples: Vec<Phases> = Vec::with_capacity(SAMPLES);

        for frame in 0..WARMUP + SAMPLES {
            let phases = self.frame(kit, &widget, &mut cache);

            if frame >= WARMUP {
                samples.push(phases);
            }
        }

        let build = median(&samples, |p| p.build);
        let layout = median(&samples, |p| p.layout);
        let draw = median(&samples, |p| p.draw);
        let update = median(&samples, |p| p.update);

        // A widget that lays out to nothing still costs build, layout and
        // update time but paints no pixels, which is how a case whose width
        // collapsed reads as cheap rather than as broken. `Fill` inside a
        // `Shrink` column is the way that happens.
        assert!(
            draw > 0.0,
            "{name} drew nothing: the case lays out to an empty box"
        );

        println!(
            "  {name:<24} {build:>9.0} {layout:>9.0} {draw:>9.0} {update:>9.0} {:>9.0}",
            build + layout + draw + update
        );
    }

    /// One frame over `WIDGETS` copies of the case's widget, each phase timed
    /// where it happens rather than by subtracting one run from another.
    fn frame(
        &mut self,
        kit: &Luminate,
        widget: &impl Fn(&Luminate) -> Element<'static, Message>,
        cache: &mut Cache,
    ) -> Phases {
        let start = Instant::now();
        let column: Element<'static, Message> =
            Column::from_vec((0..WIDGETS).map(|_| widget(kit)).collect())
                .width(Length::Fixed(FORM_WIDTH))
                .spacing(4)
                .into();
        let build = start.elapsed();

        // `UserInterface::build` is the diff of the new tree against the
        // cached one plus the layout pass; the runtime runs them together.
        let start = Instant::now();
        let mut ui = UserInterface::build(
            black_box(column),
            BOUNDS,
            std::mem::take(cache),
            &mut self.renderer,
        );
        let layout = start.elapsed();

        let start = Instant::now();
        ui.draw(
            &mut self.renderer,
            &Theme::LIGHT,
            &iced::advanced::renderer::Style {
                text_color: iced::Color::BLACK,
            },
            CURSOR,
        );
        let draw = start.elapsed();

        // The redraw every animation frame delivers. It is what makes the
        // input's `Focus::Custom` probe walk the tree, so it belongs in the
        // frame cost rather than in an event-only measurement.
        let mut messages = Vec::new();
        let events = [iced::Event::Window(iced::window::Event::RedrawRequested(
            iced::time::Instant::now(),
        ))];

        let start = Instant::now();
        let outcome = ui.update(
            &events,
            CURSOR,
            &mut self.renderer,
            &mut iced::advanced::clipboard::Null,
            &mut messages,
        );
        let update = start.elapsed();
        let _ = black_box(outcome);

        *cache = ui.into_cache();

        Phases {
            build,
            layout,
            draw,
            update,
        }
    }
}

/// The median of one phase across the samples, in nanoseconds per widget.
fn median(samples: &[Phases], phase: impl Fn(&Phases) -> Duration) -> f64 {
    let mut timings: Vec<Duration> = samples.iter().map(phase).collect();
    timings.sort_unstable();

    timings[timings.len() / 2].as_nanos() as f64 / WIDGETS as f64
}
