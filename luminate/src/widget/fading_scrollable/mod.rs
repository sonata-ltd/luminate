//! A scrollable whose bars stay out of the way until they are wanted.

use iced::advanced::widget::{Operation, Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, text};
use iced::border::Radius;
use iced::time::{Duration, Instant};
use iced::widget::container;
use iced::widget::scrollable::{self, AbsoluteOffset, Direction, Scrollable, Scrollbar};
use iced::{
    Background, Border, Color, Element, Event, Length, Padding, Rectangle, Size, Vector, border,
    window,
};
use iced_animate::{Anim, Curve, CurveKind, Motion, MotionKey, Spring, SpringParams, curves};

use crate::theme::palette::mix;

/// Colours a [`FadingScrollable`]'s bars are drawn with.
///
/// The three pill colours are the ends of the two blends the widget runs:
/// `scroller` to `scroller_hover` as the cursor takes the gutter, and
/// whatever that gives to `scroller_dragged` as the pill is grabbed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Fill of the rail behind the pill. `None` leaves the rail unpainted.
    pub rail: Option<Color>,
    /// Fill of the rail while the cursor is in its gutter. `None` leaves
    /// the rail at [`rail`](Self::rail) however the cursor moves; setting
    /// this alone, with `rail` at `None`, gives a background that arrives
    /// with the cursor and nothing at all the rest of the time.
    pub rail_hover: Option<Color>,
    /// Fill of the pill while the cursor is elsewhere.
    pub scroller: Color,
    /// Fill of the pill while the cursor is in its gutter.
    pub scroller_hover: Color,
    /// Fill of the pill while it is being dragged.
    pub scroller_dragged: Color,
    /// Corner radius of rail and pill.
    pub radius: Radius,
}

catalog!(|theme| {
    let palette = theme.extended_palette();

    Style {
        rail: None,
        rail_hover: None,
        scroller: palette.background.strong.color,
        scroller_hover: palette.background.base.text,
        scroller_dragged: palette.primary.base.color,
        radius: Radius::new(u32::MAX),
    }
});

/// How thick the pill is at this much `expand`, between the two ends the
/// builder set.
fn thickness(rest: f32, hover: f32, expand: f32) -> f32 {
    rest + (hover - rest) * expand.clamp(0.0, 1.0)
}

/// The pill's fill: blended toward hover and drag, then faded by `reveal`.
///
/// A pill with no reveal left comes back as exactly [`Color::TRANSPARENT`],
/// which is the one value `scrollable`'s `draw` tests for before it paints
/// a quad — anything else has it painting an invisible one every frame.
fn fill(style: &Style, reveal: f32, expand: f32, press: f32) -> Color {
    let reveal = reveal.clamp(0.0, 1.0);

    // Below half a percent nothing is visible anyway, and landing on the
    // exact value is what buys the skipped quad.
    if reveal < 0.005 {
        return Color::TRANSPARENT;
    }

    let engaged = mix(style.scroller, style.scroller_hover, expand);

    mix(engaged, style.scroller_dragged, press).scale_alpha(reveal)
}

/// One of the two scrollbars a [`FadingScrollable`] can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The bar down the trailing edge, scrolling the content vertically.
    Vertical,
    /// The bar along the bottom edge, scrolling the content horizontally.
    Horizontal,
}

/// A value held once per [`Axis`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Axes<T> {
    vertical: T,
    horizontal: T,
}

impl<T> Axes<T> {
    /// The value belonging to `axis`.
    fn of(&self, axis: Axis) -> &T {
        match axis {
            Axis::Vertical => &self.vertical,
            Axis::Horizontal => &self.horizontal,
        }
    }

    /// The value belonging to `axis`, mutably.
    fn of_mut(&mut self, axis: Axis) -> &mut T {
        match axis {
            Axis::Vertical => &mut self.vertical,
            Axis::Horizontal => &mut self.horizontal,
        }
    }
}

impl Axis {
    /// Both axes, vertical first.
    const BOTH: [Self; 2] = [Self::Vertical, Self::Horizontal];

    /// The insets at the start and the end of this axis' track.
    fn ends(self, padding: Padding) -> (f32, f32) {
        match self {
            Self::Vertical => (padding.top, padding.bottom),
            Self::Horizontal => (padding.left, padding.right),
        }
    }

    /// The insets on either side of this axis' pill, across its rail.
    fn across(self, padding: Padding) -> (f32, f32) {
        match self {
            Self::Vertical => (padding.left, padding.right),
            Self::Horizontal => (padding.top, padding.bottom),
        }
    }

    /// The extent of `rect` along this axis — the length of a bar's track.
    fn length(self, rect: Rectangle) -> f32 {
        match self {
            Self::Vertical => rect.height,
            Self::Horizontal => rect.width,
        }
    }

    /// This axis' component of an offset, where `None` means the offset
    /// leaves the axis alone.
    fn component(self, offset: AbsoluteOffset<Option<f32>>) -> Option<f32> {
        match self {
            Self::Vertical => offset.y,
            Self::Horizontal => offset.x,
        }
    }

    /// Asks `offset` to take this axis to `to`.
    fn aim_at(self, offset: &mut AbsoluteOffset<Option<f32>>, to: f32) {
        match self {
            Self::Vertical => offset.y = Some(to),
            Self::Horizontal => offset.x = Some(to),
        }
    }
}

/// Whether the offset a scrollable took from `event` is worth travelling to
/// rather than cutting to.
///
/// A wheel's `Lines` are the one delta with nothing behind them: a notch is a
/// discrete step, and something has to carry the eye across it. A touchpad's
/// `Pixels` arrive smoothed by the hand that sent them, and a drag — of a
/// pill or of the content under a finger — *is* the hand, where any lag reads
/// as the widget slipping out from under it. Those land where they say.
fn travels(event: &Event) -> bool {
    matches!(
        event,
        Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { .. }
        })
    )
}

/// The rail's fill, blended toward hover and faded by `reveal`.
///
/// `None` where nothing would show: `scrollable`'s `draw` skips the rail
/// entirely for a background of `None` and paints an invisible quad every
/// frame for a transparent `Some`.
fn rail(style: &Style, reveal: f32, expand: f32) -> Option<Color> {
    let colour = match (style.rail, style.rail_hover) {
        (None, None) => return None,
        (Some(rest), None) => rest.scale_alpha(reveal),
        (Some(rest), Some(hovered)) => mix(rest, hovered, expand).scale_alpha(reveal),
        // Nothing at rest and something on hover: the rail belongs to the
        // cursor, so it rides in on the hover blend as well as the fade.
        (None, Some(hovered)) => hovered.scale_alpha(reveal * expand.clamp(0.0, 1.0)),
    };

    (colour.a >= 0.005).then_some(colour)
}

/// One bar's whole appearance at this frame's three values.
///
/// The join between the numbers the frame published and the `Rail` iced
/// draws, kept out of the style closure so it can be looked at directly.
fn bar_style(style: &Style, reveal: f32, expand: f32, press: f32) -> scrollable::Rail {
    scrollable::Rail {
        background: rail(style, reveal, expand).map(Background::Color),
        border: border::rounded(style.radius),
        scroller: scrollable::Scroller {
            background: Background::Color(fill(style, reveal, expand, press)),
            border: border::rounded(style.radius),
        },
    }
}

/// Where in the pill a press landed, 0 at its leading edge and 1 at its
/// trailing one.
///
/// A press on the rail rather than the pill comes back as half, which takes
/// the pill's middle to the cursor instead of jumping it by an edge — the
/// same thing iced does with a press on its own rail.
fn grip(bar: Bar, axis: Axis, gutter: &Rectangle, reach: Reach, cursor: mouse::Cursor) -> f32 {
    let Some(at) = cursor.position() else {
        return 0.5;
    };

    let Some((start, length)) = pill_span(
        bar,
        axis,
        axis.length(*gutter),
        reach.viewport,
        reach.content,
        reach.offset,
    ) else {
        return 0.5;
    };

    let along = match axis {
        Axis::Vertical => at.y - gutter.y,
        Axis::Horizontal => at.x - gutter.x,
    };

    if length <= 0.0 || along < start || along > start + length {
        0.5
    } else {
        (along - start) / length
    }
}

/// Where a drag with the cursor here takes the content.
fn dragged_to(
    bar: Bar,
    axis: Axis,
    gutter: Rectangle,
    reach: Reach,
    grabbed_at: f32,
    cursor: iced::Point,
) -> AbsoluteOffset<Option<f32>> {
    let track = axis.length(gutter);

    let length = pill_span(
        bar,
        axis,
        track,
        reach.viewport,
        reach.content,
        reach.offset,
    )
    .map_or(0.0, |(_, length)| length);

    let along = match axis {
        Axis::Vertical => cursor.y - gutter.y,
        Axis::Horizontal => cursor.x - gutter.x,
    };

    let offset = offset_for(
        bar,
        axis,
        track,
        reach.viewport,
        reach.content,
        length,
        grabbed_at,
        along,
    );

    match axis {
        Axis::Vertical => AbsoluteOffset {
            x: None,
            y: Some(offset),
        },
        Axis::Horizontal => AbsoluteOffset {
            x: Some(offset),
            y: None,
        },
    }
}

/// How much reveal a pill needs before a press may grab it.
///
/// A bar nobody can see must not be grabbable, or a press near the trailing
/// edge of the content jumps the scroll for no visible reason. The cursor
/// entering the gutter reveals the bar, so the only presses this turns away
/// are the ones inside the first frames of that fade.
const GRABBABLE: f32 = 0.35;

/// The axis a press lands on, or `None` if the press belongs to the content.
fn grabbed(
    gutters: Axes<Option<Rectangle>>,
    reveal: Axes<f32>,
    cursor: mouse::Cursor,
) -> Option<Axis> {
    let at = cursor.position()?;

    Axis::BOTH.into_iter().find(|axis| {
        *reveal.of(*axis) >= GRABBABLE
            && gutters.of(*axis).is_some_and(|gutter| gutter.contains(at))
    })
}

/// What each axis takes from one event.
///
/// Split out of the widget so the whole trigger set — a wheel turn, the
/// cursor entering a gutter, a pill under the button — can be tested
/// against plain geometry, with no runtime in the way.
fn cues(
    bar: Bar,
    axes: Axes<bool>,
    dragging: Option<Axis>,
    event: &Event,
    layout: Layout<'_>,
    cursor: mouse::Cursor,
) -> Axes<Cue> {
    let bounds = layout.bounds();
    let content = layout
        .children()
        .next()
        .map_or(bounds, |content| content.bounds());

    let gutters = gutters(bar, axes, bounds, content);
    let zones = zones(bar, gutters, bounds);
    let mut cues = Axes::<Cue>::default();

    for axis in Axis::BOTH {
        let cue = cues.of_mut(axis);

        cue.hovered = zones
            .of(axis)
            .zip(cursor.position())
            .is_some_and(|(gutter, at)| gutter.contains(at));
        cue.dragging = dragging == Some(axis);
    }

    // A wheel turn reaches every widget the pointer's path crosses, so it
    // only counts where the pointer is actually over this one.
    if let Event::Mouse(mouse::Event::WheelScrolled { delta }) = event
        && cursor.is_over(bounds)
    {
        let (x, y) = match delta {
            mouse::ScrollDelta::Lines { x, y } | mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
        };

        cues.vertical.scrolled = y != 0.0 && gutters.vertical.is_some();
        cues.horizontal.scrolled = x != 0.0 && gutters.horizontal.is_some();
    }

    cues
}

/// `padding` with every side below zero taken back to zero.
fn non_negative(padding: Padding) -> Padding {
    Padding {
        top: padding.top.max(0.0),
        right: padding.right.max(0.0),
        bottom: padding.bottom.max(0.0),
        left: padding.left.max(0.0),
    }
}

/// The fixed geometry both bars are laid out with.
///
/// Nothing in it animates: the rail is sized for the thickest pill and the
/// grab zone for the gutter, so neither moves while `rest` and `hover` move
/// the pill inside them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Bar {
    /// The narrowest the grab zone gets across a bar.
    gutter: f32,
    /// The thickest the pill gets. The rail is sized for it, so a pill
    /// growing under the cursor never moves the rail.
    thickest: f32,
    /// The inset inside each rail, around the lane its pill runs in: see
    /// [`Axis::ends`] and [`Axis::across`] for which side does what to
    /// which bar.
    track_padding: Padding,
    /// The inset outside the rails, between them and the viewport's
    /// edges: the bars live in the viewport shrunk by it.
    margin: Padding,
    /// The shortest the pill may get. Without it a long page shrinks the
    /// pill to a few pixels nobody can grab.
    min_pill: f32,
}

/// The rail of each bar the content overflows: the box its background is
/// painted in and the cursor is answered in, padding included.
///
/// This widget's own geometry, not iced's: the inner scrollable's bar is
/// zero-width and grabs nothing, so nothing here has to agree with it. A
/// bar exists only where the content is longer than the viewport, lies
/// against the edge it runs along of the viewport shrunk by the margin,
/// and gives way to the other bar in the corner.
fn gutters(
    bar: Bar,
    axes: Axes<bool>,
    bounds: Rectangle,
    content: Rectangle,
) -> Axes<Option<Rectangle>> {
    let scrolls_y = axes.vertical && content.height > bounds.height;
    let scrolls_x = axes.horizontal && content.width > bounds.width;

    let thick = |axis: Axis| {
        let (near, far) = axis.across(bar.track_padding);
        near + bar.thickest + far
    };
    let right = thick(Axis::Vertical);
    let bottom = thick(Axis::Horizontal);

    // The margin is outside the rails: they are laid out in what it leaves.
    let bounds = Rectangle {
        x: bounds.x + bar.margin.left,
        y: bounds.y + bar.margin.top,
        width: (bounds.width - bar.margin.left - bar.margin.right).max(0.0),
        height: (bounds.height - bar.margin.top - bar.margin.bottom).max(0.0),
    };

    // Where both bars are up, each stops short of the other's whole rail,
    // so the corner belongs to neither and one hit-test cannot answer for
    // two axes.
    let vertical = scrolls_y.then(|| Rectangle {
        x: bounds.x + bounds.width - right,
        y: bounds.y,
        width: right,
        height: (bounds.height - if scrolls_x { bottom } else { 0.0 }).max(0.0),
    });

    let horizontal = scrolls_x.then(|| Rectangle {
        x: bounds.x,
        y: bounds.y + bounds.height - bottom,
        width: (bounds.width - if scrolls_y { right } else { 0.0 }).max(0.0),
        height: bottom,
    });

    Axes {
        vertical,
        horizontal,
    }
}

/// The zone each bar answers the cursor in: its rail, carried across to
/// the edge it runs along and never narrower than the gutter.
///
/// A rail held off its edge by the margin should still be caught by a
/// cursor thrown against that edge — it is the easiest target on the
/// screen, and leaving a dead strip there would waste it.
fn zones(bar: Bar, rails: Axes<Option<Rectangle>>, bounds: Rectangle) -> Axes<Option<Rectangle>> {
    // Out to the edge, and in from it by at least the gutter.
    let vertical = rails.vertical.map(|rail| {
        let x = rail.x.min(rail.x + rail.width - bar.gutter);
        Rectangle {
            x,
            width: bounds.x + bounds.width - x,
            ..rail
        }
    });
    let horizontal = rails.horizontal.map(|rail| {
        let y = rail.y.min(rail.y + rail.height - bar.gutter);
        Rectangle {
            y,
            height: bounds.y + bounds.height - y,
            ..rail
        }
    });

    // Widened, the two could meet in the corner: each stops where the
    // other begins, so one press is only ever one axis.
    match (vertical, horizontal) {
        (Some(v), Some(h)) => Axes {
            vertical: Some(Rectangle {
                height: v.height.min((h.y - v.y).max(0.0)),
                ..v
            }),
            horizontal: Some(Rectangle {
                width: h.width.min((v.x - h.x).max(0.0)),
                ..h
            }),
        },
        _ => Axes {
            vertical,
            horizontal,
        },
    }
}

/// What the inner scrollable last reported about one axis.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Reach {
    /// How much of the content the viewport shows.
    viewport: f32,
    /// How much there is.
    content: f32,
    /// How far in it has been taken.
    offset: f32,
}

/// Reads the inner scrollable's offset, and optionally moves it.
///
/// The bar's geometry is this widget's own, so the drag has to write the
/// offset through the same door the scrollable opens to any other
/// operation rather than guess at its state. `traverse` deliberately does
/// nothing: the answer is at the scrollable itself, and walking the whole
/// page below it every frame would be paid for nothing.
struct Scroll {
    /// Filled in by the scrollable as the operation reaches it.
    reach: Option<(Rectangle, Rectangle, Vector)>,
    /// Where to take it, if anywhere.
    to: Option<AbsoluteOffset<Option<f32>>>,
}

impl<T> Operation<T> for Scroll {
    fn traverse(&mut self, _operate: &mut dyn FnMut(&mut dyn Operation<T>)) {}

    fn scrollable(
        &mut self,
        _id: Option<&iced::advanced::widget::Id>,
        bounds: Rectangle,
        content_bounds: Rectangle,
        translation: Vector,
        state: &mut dyn iced::advanced::widget::operation::Scrollable,
    ) {
        self.reach = Some((bounds, content_bounds, translation));

        if let Some(to) = self.to {
            state.scroll_to(to);
        }
    }
}

impl Scroll {
    /// What the operation learned, per axis.
    fn reaches(&self) -> Axes<Reach> {
        let Some((bounds, content, translation)) = self.reach else {
            return Axes::default();
        };

        Axes {
            vertical: Reach {
                viewport: bounds.height,
                content: content.height,
                offset: translation.y,
            },
            horizontal: Reach {
                viewport: bounds.width,
                content: content.width,
                offset: translation.x,
            },
        }
    }
}

/// The pill's rectangle: `span` along `gutter`, `thickness` across it.
///
/// Centred across the gutter, so a pill that grows under the cursor grows
/// from the middle and never slides out from under it.
fn pill(bar: Bar, axis: Axis, gutter: Rectangle, span: (f32, f32), thickness: f32) -> Rectangle {
    let (start, length) = span;
    let (near, _) = axis.across(bar.track_padding);

    match axis {
        Axis::Vertical => Rectangle {
            x: gutter.x + near + (bar.thickest - thickness) / 2.0,
            y: gutter.y + start,
            width: thickness,
            height: length,
        },
        Axis::Horizontal => Rectangle {
            x: gutter.x + start,
            y: gutter.y + near + (bar.thickest - thickness) / 2.0,
            width: length,
            height: thickness,
        },
    }
}

/// How far through its scrollable range the content has been taken, 0 to 1.
fn progress(viewport: f32, content: f32, offset: f32) -> f32 {
    let scrollable = content - viewport;

    if scrollable <= 0.0 {
        0.0
    } else {
        (offset / scrollable).clamp(0.0, 1.0)
    }
}

/// Where the pill sits along its track, as `(start, length)` measured from
/// the start of the gutter.
///
/// `None` when the content fits, or when the padding has eaten the track.
/// The mapping is deliberately the plain one — the pill's travel is
/// `track - length`, spread over the content's scrollable range — because
/// [`offset_for`] has to invert it exactly or a dragged pill drifts out
/// from under the cursor.
fn pill_span(
    bar: Bar,
    axis: Axis,
    gutter: f32,
    viewport: f32,
    content: f32,
    offset: f32,
) -> Option<(f32, f32)> {
    let (lead, trail) = axis.ends(bar.track_padding);
    let track = gutter - lead - trail;

    if content <= viewport || track <= 0.0 {
        return None;
    }

    let length = (track * (viewport / content)).max(bar.min_pill).min(track);

    Some((
        lead + (track - length) * progress(viewport, content, offset),
        length,
    ))
}

/// The offset that puts the `grabbed_at` point of a pill of `length` under
/// a cursor `along` the gutter from its start. The inverse of [`pill_span`].
#[allow(clippy::too_many_arguments)]
fn offset_for(
    bar: Bar,
    axis: Axis,
    gutter: f32,
    viewport: f32,
    content: f32,
    length: f32,
    grabbed_at: f32,
    along: f32,
) -> f32 {
    let (lead, trail) = axis.ends(bar.track_padding);
    let travel = gutter - lead - trail - length;
    let scrollable = (content - viewport).max(0.0);

    if travel <= 0.0 {
        return 0.0;
    }

    let start = along - length * grabbed_at;

    ((start - lead) / travel).clamp(0.0, 1.0) * scrollable
}

/// What one axis is being asked for on this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Cue {
    /// The offset moved, or the wheel turned over the content.
    scrolled: bool,
    /// The cursor is inside this axis' gutter.
    hovered: bool,
    /// This axis' pill is being dragged.
    dragging: bool,
}

impl Cue {
    /// Whether anything at all wants the bar on screen.
    fn wants_bar(self) -> bool {
        self.scrolled || self.hovered || self.dragging
    }

    /// The part of a cue that is a pose rather than an edge.
    ///
    /// A wheel turn happens and is over; the cursor, by contrast, is
    /// somewhere and stays there. Only the pose can be compared against the
    /// last frame's to tell whether anything has actually changed.
    fn pose(self) -> (bool, bool) {
        (self.hovered, self.dragging)
    }
}

/// What [`Phase::settle`] asks the widget to do.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Change {
    /// Retarget the reveal track to this value.
    Retarget(f32),
    /// Ask for one frame at this instant, when the hold runs out. A plain
    /// `Curve::delayed` would do the same thing by spinning every frame of
    /// the hold, which is a lot of frames to spend holding still.
    WakeAt(Instant),
    /// Nothing to do this frame.
    None,
}

/// Whether one axis' bar is on screen, and when it is due to leave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct Phase {
    shown: bool,
    hide_at: Option<Instant>,
}

impl Phase {
    /// Folds this frame's [`Cue`] in and says what the reveal track needs.
    fn settle(&mut self, cue: Cue, now: Instant, hold: Duration) -> Change {
        if cue.wants_bar() {
            // The hide the last quiet frame booked is off: the bar is
            // wanted again, and the hold restarts from whenever it next
            // stops being wanted.
            self.hide_at = None;

            return if std::mem::replace(&mut self.shown, true) {
                Change::None
            } else {
                Change::Retarget(1.0)
            };
        }

        if self.hide_at.is_some_and(|hide_at| now >= hide_at) {
            self.shown = false;
            self.hide_at = None;
            return Change::Retarget(0.0);
        }

        if let Some(hide_at) = self.hide_at {
            // A booked wake-up lasts only until some pass asks for the next
            // frame — the fade still in flight does, and wipes it. Asking
            // again on every quiet frame keeps it on the last pass before
            // the window sleeps, at the cost of no frame at all.
            return Change::WakeAt(hide_at);
        }

        if self.shown {
            let hide_at = now + hold;
            self.hide_at = Some(hide_at);
            return Change::WakeAt(hide_at);
        }

        Change::None
    }
}

#[cfg(test)]
mod tests {
    use iced::advanced::clipboard;
    use iced::advanced::layout::{self, Node};
    use iced::advanced::renderer::Headless;
    use iced::mouse::ScrollDelta;
    use iced::{Point, Size};

    use super::*;

    /// The layout a `scrollable` builds: the viewport, with the content it
    /// scrolls as its one child.
    fn nodes(bounds: Rectangle, content: Rectangle) -> Node {
        Node::with_children(bounds.size(), vec![Node::new(content.size())])
            .move_to(bounds.position())
    }

    fn wheel(x: f32, y: f32) -> Event {
        Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: ScrollDelta::Lines { x, y },
        })
    }

    fn at(point: Point) -> mouse::Cursor {
        mouse::Cursor::Available(point)
    }

    // --- the widget, end to end -----------------------------------------

    /// What the page under test publishes when iced actually scrolls it.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Scrolled;

    type Page<'a> = FadingScrollable<'a, Scrolled, iced::Theme, crate::Renderer>;

    /// The viewport every widget test lays out into.
    const FRAME: Size = Size::new(200.0, 100.0);

    fn headless() -> crate::Renderer {
        iced_test::futures::futures::executor::block_on(<crate::Renderer as Headless>::new(
            iced::Font::DEFAULT,
            iced::Pixels(16.0),
            Some("tiny-skia"),
        ))
        .expect("tiny_skia needs no GPU")
    }

    /// A page taller than the frame, so a vertical bar exists at all.
    fn page(motion: &Motion) -> Page<'static> {
        fading_scrollable(iced::widget::container(iced::widget::text("a long page")).height(900.0))
            .motion(motion.clone())
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(|_| Scrolled)
    }

    /// What one delivered event produced.
    struct Delivered {
        messages: Vec<Scrolled>,
        /// Whether the widget asked for a frame. Nothing moves without one.
        asked_for_a_frame: bool,
        /// Exactly what it asked the runtime for.
        request: iced::window::RedrawRequest,
    }

    /// Hands `widget` one event and reports what came back.
    fn deliver(
        widget: &mut Page<'_>,
        tree: &mut Tree,
        renderer: &crate::Renderer,
        node: &Node,
        event: &Event,
        cursor: mouse::Cursor,
    ) -> Delivered {
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);

        widget.update(
            tree,
            event,
            Layout::new(node),
            cursor,
            renderer,
            &mut clipboard::Null,
            &mut shell,
            &Rectangle::with_size(FRAME),
        );

        Delivered {
            asked_for_a_frame: shell.redraw_request() != iced::window::RedrawRequest::Wait,
            request: shell.redraw_request(),
            messages,
        }
    }

    /// Advances the clock and hands `widget` the frame that follows.
    fn frame(
        widget: &mut Page<'_>,
        tree: &mut Tree,
        renderer: &crate::Renderer,
        node: &Node,
        motion: &Motion,
        now: Instant,
        cursor: mouse::Cursor,
    ) {
        let _ = motion.tick(now);
        let _ = deliver(
            widget,
            tree,
            renderer,
            node,
            &Event::Window(window::Event::RedrawRequested(now)),
            cursor,
        );
    }

    /// Everything the widget owns lives in one place; a test that drives it
    /// wants the widget, its tree and the layout together.
    struct Rig {
        widget: Page<'static>,
        tree: Tree,
        renderer: crate::Renderer,
        node: Node,
        motion: Motion,
        start: Instant,
        /// Whether a frame has been asked for and not yet produced. The
        /// runtime keeps exactly this book; [`Rig::pump`] spends it.
        pending: bool,
    }

    impl Rig {
        fn new() -> Self {
            Self::with(page)
        }

        fn with(build: fn(&Motion) -> Page<'static>) -> Self {
            let motion = Motion::new();
            let renderer = headless();
            let mut widget = build(&motion);
            let mut tree =
                Tree::new(&widget as &dyn Widget<Scrolled, iced::Theme, crate::Renderer>);
            let node = widget.layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, FRAME),
            );

            Self {
                widget,
                tree,
                renderer,
                node,
                motion,
                start: Instant::now(),
                pending: false,
            }
        }

        /// The gutter the vertical bar grabs in.
        fn vertical_gutter(&self) -> Rectangle {
            let layout = Layout::new(&self.node);

            gutters(
                self.widget.bar,
                self.widget.axes,
                layout.bounds(),
                layout.children().next().expect("content").bounds(),
            )
            .vertical
            .expect("the page overflows")
        }

        fn deliver(&mut self, event: &Event, cursor: mouse::Cursor) -> Delivered {
            let delivered = deliver(
                &mut self.widget,
                &mut self.tree,
                &self.renderer,
                &self.node,
                event,
                cursor,
            );

            self.pending |= delivered.asked_for_a_frame;

            delivered
        }

        /// Runs `steps` frames of 16 ms from `at`, holding the cursor still.
        fn run(&mut self, from: Duration, steps: u32, cursor: mouse::Cursor) -> Duration {
            for step in 0..steps {
                let now = self.start + from + Duration::from_millis(16 * u64::from(step));
                let _ = self.motion.tick(now);
                let _ = self.deliver(&Event::Window(window::Event::RedrawRequested(now)), cursor);
            }

            from + Duration::from_millis(16 * u64::from(steps))
        }

        /// What the widget last read back about the vertical axis.
        fn reach(&self) -> Reach {
            self.tree.state.downcast_ref::<State>().reach.vertical
        }

        /// Where the widget would draw the vertical pill right now.
        fn pill(&self) -> Rectangle {
            let gutter = self.vertical_gutter();
            let reach = self.reach();
            let span = pill_span(
                self.widget.bar,
                Axis::Vertical,
                Axis::Vertical.length(gutter),
                reach.viewport,
                reach.content,
                reach.offset,
            )
            .expect("the page overflows");

            pill(
                self.widget.bar,
                Axis::Vertical,
                gutter,
                span,
                self.widget.hover,
            )
        }

        /// Which axis, if any, the widget thinks is being dragged.
        fn dragging(&self) -> Option<Axis> {
            self.tree.state.downcast_ref::<State>().dragging
        }

        /// Runs frames the way the runtime does: only while something is
        /// still asking for one, either the bar itself or the engine with a
        /// track still in flight (which is what `Host` watches for).
        ///
        /// [`run`](Self::run) forces its frames and so cannot see a bar that
        /// has stopped asking for them; this can.
        fn pump(&mut self, from: Duration, cursor: mouse::Cursor) -> Duration {
            let mut at = from;

            for _ in 0..300 {
                // No frame was asked for, so the runtime produces none —
                // not even one. A bar that forgot to ask stops here.
                if !self.pending {
                    break;
                }

                self.pending = false;

                let now = self.start + at;
                let status = self.motion.tick(now);
                let _ = self.deliver(&Event::Window(window::Event::RedrawRequested(now)), cursor);

                // `Host` asks for the next frame while anything is in
                // flight; this stands in for it.
                self.pending |= status.animating;
                at += Duration::from_millis(16);
            }

            at
        }

        /// Runs the window the way `iced_winit` does, from the request the
        /// last pass left behind, until the window has nothing to wake for.
        ///
        /// A pass's request lives for that pass only: `NextFrame` draws the
        /// next frame and forgets any booked wake-up, `At` books one, `Wait`
        /// keeps whatever is booked. `Host` asks for the next frame, ahead
        /// of its content, while a track is in flight — and in the merge the
        /// sooner request wins. With no frame coming and nothing booked the
        /// window sleeps, and so does this.
        fn live(
            &mut self,
            from: Duration,
            mut request: iced::window::RedrawRequest,
            cursor: mouse::Cursor,
        ) -> Duration {
            use iced::window::RedrawRequest;

            let mut at = from;
            let mut booked: Option<Instant> = None;

            for _ in 0..1000 {
                let next_frame = match request {
                    RedrawRequest::NextFrame => {
                        booked = None;
                        true
                    }
                    RedrawRequest::At(wake) => {
                        booked = Some(wake);
                        false
                    }
                    RedrawRequest::Wait => false,
                };

                let now = if next_frame {
                    self.start + at + Duration::from_millis(16)
                } else if let Some(wake) = booked.take() {
                    wake.max(self.start + at)
                } else {
                    break;
                };
                at = now - self.start;

                let status = self.motion.tick(now);
                let delivered =
                    self.deliver(&Event::Window(window::Event::RedrawRequested(now)), cursor);

                let host = if status.animating {
                    RedrawRequest::NextFrame
                } else {
                    RedrawRequest::Wait
                };
                request = host.min(delivered.request);
            }

            at
        }

        /// This frame's `(reveal, expand, press)` on the vertical bar.
        fn vertical(&self) -> (f32, f32, f32) {
            self.tree
                .state
                .downcast_ref::<State>()
                .values(Axis::Vertical)
        }
    }

    #[test]
    fn the_cursor_in_the_gutter_fades_the_bar_in_and_thickens_it() {
        let mut rig = Rig::new();
        let inside = at(rig.vertical_gutter().center());

        let _ = rig.run(Duration::ZERO, 30, inside);

        let (reveal, expand, _) = rig.vertical();
        assert!(reveal > 0.9, "faded in: {reveal}");
        assert!(expand > 0.9, "thickened: {expand}");
    }

    #[test]
    fn the_bar_leaves_again_once_the_cursor_does() {
        let mut rig = Rig::new();
        let gutter = rig.vertical_gutter();
        let away = at(Point::new(20.0, 20.0));

        let settled = rig.run(Duration::ZERO, 30, at(gutter.center()));
        assert!(rig.vertical().0 > 0.9);

        // Long enough for the hold to run out and the fade to finish.
        let _ = rig.run(settled, 80, away);

        assert_eq!(rig.vertical().0, 0.0, "gone again");
    }

    /// The thickening is what the grab zone must *not* follow: a pill that
    /// moved out from under the cursor as it grew would be unusable.
    #[test]
    fn the_grab_zone_holds_still_while_the_pill_grows() {
        let mut rig = Rig::new();
        let thin = rig.vertical_gutter();

        let _ = rig.run(Duration::ZERO, 30, at(thin.center()));

        assert!(rig.vertical().1 > 0.9, "the pill did grow");
        assert_eq!(rig.vertical_gutter(), thin);
    }

    /// A page that overflows both ways, with both bars turned on.
    fn wide_page(motion: &Motion) -> Page<'static> {
        fading_scrollable(
            iced::widget::container(iced::widget::text("a long wide page"))
                .width(900.0)
                .height(900.0),
        )
        .motion(motion.clone())
        .horizontal()
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(|_| Scrolled)
    }

    /// Each bar answers for its own gutter: the corner belongs to neither,
    /// and hovering one must leave the other where it was.
    #[test]
    fn the_two_bars_fade_in_one_at_a_time() {
        let mut rig = Rig::with(wide_page);
        let layout = Layout::new(&rig.node);
        let zones = gutters(
            rig.widget.bar,
            rig.widget.axes,
            layout.bounds(),
            layout.children().next().expect("content").bounds(),
        );
        let in_horizontal = at(zones.horizontal.expect("overflowing").center());

        let _ = rig.run(Duration::ZERO, 30, in_horizontal);

        let horizontal = rig
            .tree
            .state
            .downcast_ref::<State>()
            .values(Axis::Horizontal);
        assert!(
            horizontal.0 > 0.9,
            "the hovered bar is up: {}",
            horizontal.0
        );
        assert_eq!(rig.vertical().0, 0.0, "the other one stayed away");
    }

    /// The kit theme draws the bars from its own token group, so an
    /// application retunes them with struct update syntax rather than by
    /// writing a style closure.
    #[test]
    fn the_kit_theme_draws_the_bars_from_its_tokens() {
        use crate::Theme;
        use crate::theme::FadingScrollableTheme;

        let marker = Color::from_rgb(0.0, 1.0, 1.0);
        let theme = Theme {
            fading_scrollable: FadingScrollableTheme {
                rail_hover: Some(marker),
                ..Theme::LIGHT.fading_scrollable
            },
            ..Theme::LIGHT
        };

        let style = <Theme as Catalog>::style(&theme, &<Theme as Catalog>::default());

        assert_eq!(style.rail_hover, Some(marker));
        assert_eq!(style.scroller, Theme::LIGHT.fading_scrollable.scroller);
    }

    /// The two looks have to differ, or the dark one is drawing the light
    /// one's bars.
    #[test]
    fn the_two_kit_looks_give_the_bars_different_colours() {
        use crate::Theme;

        let class = <Theme as Catalog>::default();
        let light = <Theme as Catalog>::style(&Theme::LIGHT, &class);
        let dark = <Theme as Catalog>::style(&Theme::DARK, &class);

        assert_ne!(light.scroller, dark.scroller);
        assert_ne!(light.rail_hover, dark.rail_hover);
        assert!(light.rail.is_none(), "an overlay bar has no resting rail");
    }

    #[test]
    fn the_iced_theme_catalog_follows_the_palette() {
        let class = <iced::Theme as Catalog>::default();
        let light = <iced::Theme as Catalog>::style(&iced::Theme::Light, &class);
        let dark = <iced::Theme as Catalog>::style(&iced::Theme::Dark, &class);

        assert_ne!(light.scroller, dark.scroller);
        assert_ne!(light.scroller, light.scroller_dragged);
    }

    /// A curve slow enough that a fifth of a second into it the track is
    /// unmistakably still on its way.
    fn crawl() -> Curve {
        Curve::ease(iced_animate::Easing::Linear, Duration::from_secs(2))
    }

    /// `Motion::to` starts a track it has never seen *at* its target, so a
    /// bar whose tracks were never seeded would cut in rather than fade.
    /// Endpoints alone cannot tell the two apart; this looks mid-flight.
    #[test]
    fn the_bar_fades_in_rather_than_cutting_in() {
        let mut rig = Rig::new();
        let hovered = at(rig.vertical_gutter().center());

        let _ = rig.run(Duration::ZERO, 3, hovered);

        let (reveal, expand, _) = rig.vertical();
        assert!(reveal > 0.0 && reveal < 0.9, "part way in: {reveal}");
        assert!(expand > 0.0 && expand < 0.9, "part way out: {expand}");
    }

    #[test]
    fn a_slower_expand_curve_takes_longer_to_thicken() {
        let mut quick = Rig::new();
        let mut slow = Rig::with(|motion| page(motion).expand_curve(crawl()));

        let hovered = at(quick.vertical_gutter().center());
        let _ = quick.run(Duration::ZERO, 20, hovered);
        let _ = slow.run(Duration::ZERO, 20, hovered);

        assert!(quick.vertical().1 > 0.9, "default: {}", quick.vertical().1);
        assert!(slow.vertical().1 < 0.5, "slowed: {}", slow.vertical().1);
    }

    #[test]
    fn a_slower_fade_curve_takes_longer_to_arrive() {
        let mut quick = Rig::new();
        let mut slow = Rig::with(|motion| page(motion).fade_curve(crawl()));

        let hovered = at(quick.vertical_gutter().center());
        let _ = quick.run(Duration::ZERO, 20, hovered);
        let _ = slow.run(Duration::ZERO, 20, hovered);

        assert!(quick.vertical().0 > 0.9, "default: {}", quick.vertical().0);
        assert!(slow.vertical().0 < 0.5, "slowed: {}", slow.vertical().0);
    }

    #[test]
    fn a_slower_press_curve_takes_longer_to_darken() {
        let mut quick = Rig::new();
        let mut slow = Rig::with(|motion| page(motion).press_curve(crawl()));

        let hovered = at(quick.vertical_gutter().center());

        for rig in [&mut quick, &mut slow] {
            let settled = rig.run(Duration::ZERO, 30, hovered);
            let _ = rig.deliver(&press(), hovered);
            let _ = rig.run(settled, 20, hovered);
        }

        assert!(quick.vertical().2 > 0.9, "default: {}", quick.vertical().2);
        assert!(slow.vertical().2 < 0.5, "slowed: {}", slow.vertical().2);
    }

    fn release() -> Event {
        Event::Mouse(iced::mouse::Event::ButtonReleased(
            iced::mouse::Button::Left,
        ))
    }

    fn moved(to: Point) -> Event {
        Event::Mouse(iced::mouse::Event::CursorMoved { position: to })
    }

    /// The reported bug, end to end: driven by real events alone and given
    /// only the frames those events ask for, the bar has to thicken under
    /// the cursor and settle again once the pill is let go — without
    /// waiting for an unrelated scroll to produce a frame.
    #[test]
    fn the_bar_thickens_and_settles_on_real_events_alone() {
        let mut rig = Rig::new();
        let away = Point::new(20.0, 20.0);
        let mut when = rig.pump(Duration::ZERO, at(away));

        let inside = rig.vertical_gutter().center();
        let _ = rig.deliver(&moved(inside), at(inside));
        when = rig.pump(when, at(inside));

        assert!(rig.vertical().1 > 0.9, "thickened: {}", rig.vertical().1);

        let _ = rig.deliver(&press(), at(inside));
        when = rig.pump(when, at(inside));

        assert!(rig.vertical().2 > 0.9, "darkened: {}", rig.vertical().2);

        let _ = rig.deliver(&release(), at(inside));
        let _ = rig.deliver(&moved(away), at(away));
        let _ = rig.pump(when, at(away));

        assert_eq!(rig.vertical().2, 0.0, "let go of");
        assert_eq!(rig.vertical().1, 0.0, "and thinned again");
    }

    /// A frame only happens if something asks for one, and nothing else on
    /// the page need be moving when the cursor reaches the gutter. Without
    /// this the bar lights up only when an unrelated redraw happens along —
    /// which is to say, not reliably.
    #[test]
    fn the_cursor_reaching_the_gutter_asks_for_a_frame() {
        let mut rig = Rig::new();
        let _ = rig.run(Duration::ZERO, 2, at(Point::new(20.0, 20.0)));

        let inside = rig.vertical_gutter().center();

        assert!(rig.deliver(&moved(inside), at(inside)).asked_for_a_frame);
    }

    #[test]
    fn the_cursor_leaving_the_gutter_asks_for_a_frame() {
        let mut rig = Rig::new();
        let inside = rig.vertical_gutter().center();
        let _ = rig.run(Duration::ZERO, 30, at(inside));

        let away = Point::new(20.0, 20.0);

        assert!(rig.deliver(&moved(away), at(away)).asked_for_a_frame);
    }

    /// Letting go has to settle the pill back, and settling needs a frame.
    #[test]
    fn letting_go_of_the_pill_asks_for_a_frame() {
        let mut rig = Rig::new();
        let inside = rig.vertical_gutter().center();
        let _ = rig.run(Duration::ZERO, 30, at(inside));
        let _ = rig.deliver(&press(), at(inside));
        let settled = rig.run(Duration::from_millis(480), 20, at(inside));
        let _ = settled;

        assert!(rig.deliver(&release(), at(inside)).asked_for_a_frame);
    }

    /// The builders have to reach the geometry, not just the struct: a
    /// padding nobody reads is a padding nobody sees.
    #[test]
    fn the_track_padding_builder_moves_the_drawn_pill() {
        let mut bare = Rig::new();
        let mut padded = Rig::with(|motion| page(motion).track_padding(20.0).min_pill(40.0));

        for rig in [&mut bare, &mut padded] {
            let _ = rig.run(Duration::ZERO, 30, at(rig.vertical_gutter().center()));
        }

        let gutter = padded.vertical_gutter();
        assert_eq!(padded.pill().y, gutter.y + 20.0);
        assert_eq!(padded.pill().height, 40.0, "the floor is read too");
        assert_eq!(bare.pill().y, gutter.y + PADDING);
    }

    /// Every side given to `.track_padding` lands where it says: `top` ahead
    /// of the pill, `left` and `right` inside the rail on either side of it.
    #[test]
    fn the_track_padding_builder_places_the_pill_side_by_side() {
        let mut rig = Rig::with(|motion| {
            page(motion)
                .track_padding(Padding {
                    top: 20.0,
                    right: 7.0,
                    bottom: 0.0,
                    left: 5.0,
                })
                .min_pill(0.0)
        });
        let _ = rig.run(Duration::ZERO, 30, at(rig.vertical_gutter().center()));

        let bounds = Layout::new(&rig.node).bounds();
        let rail = rig.vertical_gutter();

        assert_eq!(rail.x + rail.width, bounds.x + bounds.width);
        assert_eq!(rail.width, 5.0 + HOVER.max(REST) + 7.0);
        assert_eq!(rig.pill().y, rail.y + 20.0);
        assert_eq!(
            rig.pill().center_x(),
            bounds.x + bounds.width - 7.0 - HOVER.max(REST) / 2.0
        );
    }

    /// A single number is the same inset on every side.
    #[test]
    fn a_single_track_padding_goes_on_every_side() {
        let rig = Rig::with(|motion| page(motion).track_padding(5.0));

        assert_eq!(rig.widget.bar.track_padding, Padding::new(5.0));
    }

    /// `.margin` holds the drawn bar off the edges, and leaves the track
    /// padding inside the rail as it was.
    #[test]
    fn the_margin_builder_holds_the_drawn_bar_off_the_edges() {
        let mut rig = Rig::with(|motion| {
            page(motion)
                .margin(Padding {
                    top: 6.0,
                    right: 4.0,
                    bottom: 0.0,
                    left: 0.0,
                })
                .min_pill(0.0)
        });
        let _ = rig.run(Duration::ZERO, 30, at(rig.vertical_gutter().center()));

        let bounds = Layout::new(&rig.node).bounds();
        let rail = rig.vertical_gutter();

        assert_eq!(rail.x + rail.width, bounds.x + bounds.width - 4.0);
        assert_eq!(rail.y, bounds.y + 6.0);
        assert_eq!(rail.width, PADDING + HOVER.max(REST) + PADDING);
        assert_eq!(rig.pill().y, rail.y + PADDING);
    }

    /// `.pill` sizes the rail too: it is as thick as the thicker pill.
    #[test]
    fn the_pill_builder_sizes_the_rail() {
        let rig = Rig::with(|motion| page(motion).pill(2.0, 8.0).track_padding(0.0));

        assert_eq!(rig.vertical_gutter().width, 8.0);
    }

    /// The whole point of owning the bar: the drawn pill and the drag are
    /// the same geometry, so the pill goes exactly where the cursor takes
    /// it and the content follows.
    #[test]
    fn dragging_the_pill_scrolls_the_content() {
        // Its own padding: a long default leaves a track too short for the
        // 30 px drag below, which would clamp and prove nothing.
        let mut rig = Rig::with(|motion| page(motion).track_padding(3.0));
        let gutter = rig.vertical_gutter();
        let _ = rig.run(Duration::ZERO, 30, at(gutter.center()));

        let grabbed = rig.pill().center();
        let _ = rig.deliver(&press(), at(grabbed));
        assert_eq!(rig.dragging(), Some(Axis::Vertical));

        let down = Point::new(grabbed.x, grabbed.y + 30.0);
        let _ = rig.deliver(&moved(down), at(down));

        assert!(rig.reach().offset > 0.0, "scrolled: {}", rig.reach().offset);
        let moved_pill = rig.pill();
        assert!(
            (moved_pill.center().y - down.y).abs() < 0.5,
            "the pill followed the cursor: {} vs {}",
            moved_pill.center().y,
            down.y
        );
    }

    fn press() -> Event {
        Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left))
    }

    #[test]
    fn dragging_the_pill_takes_it_to_the_dragged_colour() {
        let mut rig = Rig::new();
        let inside = at(rig.vertical_gutter().center());

        // The pill has to be visible before it can be grabbed.
        let settled = rig.run(Duration::ZERO, 30, inside);
        let _ = rig.deliver(&press(), inside);
        assert_eq!(
            rig.dragging(),
            Some(Axis::Vertical),
            "a visible pill answers"
        );

        let _ = rig.run(settled, 20, inside);

        assert!(rig.vertical().2 > 0.9, "darkened: {}", rig.vertical().2);
    }

    /// A press on a pill nobody can see belongs to the content, or the
    /// scroll jumps for a reason the eye cannot connect it to.
    #[test]
    fn a_press_on_a_faded_out_pill_never_reaches_the_scrollable() {
        let mut rig = Rig::new();
        let inside = at(rig.vertical_gutter().center());

        // One frame, so nothing has had time to fade in yet.
        let _ = rig.run(Duration::ZERO, 1, at(Point::new(20.0, 20.0)));

        assert!(
            rig.deliver(&press(), inside).messages.is_empty(),
            "the press never reached the scrollable, so nothing scrolled"
        );
        assert_eq!(rig.dragging(), None, "and nothing was grabbed");
    }

    #[test]
    fn a_wheel_turn_fades_the_bar_in() {
        let motion = Motion::new();
        let renderer = headless();
        let mut widget = page(&motion);
        let mut tree = Tree::new(&widget as &dyn Widget<Scrolled, iced::Theme, crate::Renderer>);
        let node = widget.layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, FRAME),
        );
        let start = Instant::now();
        let away = at(Point::new(20.0, 20.0));

        frame(
            &mut widget,
            &mut tree,
            &renderer,
            &node,
            &motion,
            start,
            away,
        );
        assert_eq!(
            tree.state.downcast_ref::<State>().values(Axis::Vertical).0,
            0.0,
            "nothing has called for the bar yet"
        );

        deliver(
            &mut widget,
            &mut tree,
            &renderer,
            &node,
            &wheel(0.0, -3.0),
            away,
        );

        for step in 1..30 {
            frame(
                &mut widget,
                &mut tree,
                &renderer,
                &node,
                &motion,
                start + Duration::from_millis(16 * step),
                away,
            );
        }

        assert!(
            tree.state.downcast_ref::<State>().values(Axis::Vertical).0 > 0.9,
            "faded in: {}",
            tree.state.downcast_ref::<State>().values(Axis::Vertical).0
        );
    }

    // --- smooth scrolling ------------------------------------------------

    fn pixels(x: f32, y: f32) -> Event {
        Event::Mouse(iced::mouse::Event::WheelScrolled {
            delta: ScrollDelta::Pixels { x, y },
        })
    }

    /// One notch of the wheel in pixels: what iced spends a `Lines` delta of
    /// one as.
    const NOTCH: f32 = 60.0;

    /// A page with no engine behind it. Every track is a constant, so
    /// nothing may animate — including the scroll.
    fn plain(_: &Motion) -> Page<'static> {
        fading_scrollable(iced::widget::container(iced::widget::text("a long page")).height(900.0))
            .width(Length::Fill)
            .height(Length::Fill)
            .on_scroll(|_| Scrolled)
    }

    /// A turn of the wheel is somewhere to go, not somewhere to be: the
    /// content sets off in the frame the wheel turns and arrives over the
    /// frames after it.
    #[test]
    fn a_wheel_turn_is_travelled_rather_than_jumped() {
        let mut rig = Rig::new();
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -1.0), away);

        assert_eq!(
            rig.reach().offset,
            0.0,
            "still where it was in the frame the wheel turned"
        );

        let under_way = rig.run(Duration::from_millis(16), 3, away);
        let part = rig.reach().offset;

        assert!(part > 0.0 && part < NOTCH, "under way: {part}");

        let _ = rig.run(under_way, 60, away);

        assert!(
            (rig.reach().offset - NOTCH).abs() <= 1.0,
            "arrived: {}",
            rig.reach().offset
        );
    }

    /// A second turn while the first is still travelling adds to it. Reading
    /// the delta off the content instead would lose whatever the first turn
    /// had left to travel.
    ///
    /// Acceleration off: two notches this close are a series, and this is
    /// about what a notch adds, not about what a series makes of it.
    #[test]
    fn a_second_turn_inside_the_travel_counts_too() {
        let mut rig = Rig::with(|motion| page(motion).scroll_acceleration(1.0));
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -1.0), away);
        let under_way = rig.run(Duration::from_millis(16), 3, away);
        let _ = rig.deliver(&wheel(0.0, -1.0), away);
        let _ = rig.run(under_way, 60, away);

        assert!(
            (rig.reach().offset - 2.0 * NOTCH).abs() <= 1.0,
            "both turns counted: {}",
            rig.reach().offset
        );
    }

    /// A turn the other way sets off from where the content is, not from
    /// where it was going: a reversal has to answer the hand at once rather
    /// than spend what is left of a travel going the wrong way.
    #[test]
    fn a_turn_the_other_way_starts_from_where_the_content_is() {
        let mut rig = Rig::new();
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -3.0), away);
        let under_way = rig.run(Duration::from_millis(16), 3, away);
        let reached = rig.reach().offset;

        assert!(reached < 3.0 * NOTCH, "still travelling: {reached}");

        let _ = rig.deliver(&wheel(0.0, 1.0), away);
        let _ = rig.run(under_way, 60, away);

        assert!(
            rig.reach().offset < reached,
            "turned back from {reached}, where it was, not from where it was going: {}",
            rig.reach().offset
        );
    }

    /// A touchpad's pixels arrive smoothed already, and a second smoothing
    /// is only lag under the fingers.
    #[test]
    fn a_pixel_delta_lands_at_once() {
        let mut rig = Rig::new();
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&pixels(0.0, -40.0), away);

        assert_eq!(rig.reach().offset, 40.0, "there in the frame it arrived");
    }

    /// However much is asked for, the travel ends at the end of the content.
    #[test]
    fn a_flurry_of_turns_stops_at_the_end_of_the_content() {
        let mut rig = Rig::new();
        let away = at(Point::new(20.0, 20.0));

        for _ in 0..20 {
            let _ = rig.deliver(&wheel(0.0, -3.0), away);
        }

        let _ = rig.run(Duration::ZERO, 120, away);

        assert_eq!(
            rig.reach().offset,
            800.0,
            "the content is 900 tall in a frame of 100"
        );
    }

    /// The travel ends. A widget that kept writing an offset would keep the
    /// window awake for as long as the page was open.
    #[test]
    fn a_travel_that_has_arrived_lets_the_window_sleep() {
        let mut rig = Rig::new();
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -1.0), away);
        let _ = rig.pump(Duration::ZERO, away);

        assert!(!rig.pending, "nothing is asking for another frame");
        assert!(
            (rig.reach().offset - NOTCH).abs() <= 1.0,
            "and the content arrived: {}",
            rig.reach().offset
        );
    }

    /// A page far too long to reach the end of, so a flurry of turns is
    /// measured rather than clamped.
    fn tall(motion: &Motion) -> Page<'static> {
        fading_scrollable(
            iced::widget::container(iced::widget::text("a long page")).height(20_000.0),
        )
        .motion(motion.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(|_| Scrolled)
    }

    /// Turns delivered one after another with no frame between them: a wheel
    /// spun hard enough that the events outrun the display.
    fn flurry(rig: &mut Rig, turns: u32, cursor: mouse::Cursor) {
        // One frame first: a series is measured against the clock the last
        // frame left behind, and before any frame there is none.
        let _ = rig.run(Duration::ZERO, 1, cursor);

        for _ in 0..turns {
            let _ = rig.deliver(&wheel(0.0, -1.0), cursor);
        }
    }

    /// The wheel spun hard carries further per notch than the same notches
    /// spread out, which is the whole of what acceleration is.
    #[test]
    fn a_fast_series_carries_further_than_the_same_turns_spread_out() {
        let away = at(Point::new(20.0, 20.0));

        let mut spread = Rig::with(tall);
        let mut when = Duration::ZERO;

        for _ in 0..4 {
            let _ = spread.deliver(&wheel(0.0, -1.0), away);
            // 320 ms: well past the window, and still inside the travel.
            when = spread.run(when, 20, away);
        }

        let _ = spread.run(when, 60, away);

        assert!(
            (spread.reach().offset - 4.0 * NOTCH).abs() <= 1.0,
            "four unhurried notches are four notches: {}",
            spread.reach().offset
        );

        let mut fast = Rig::with(tall);
        flurry(&mut fast, 4, away);
        let _ = fast.run(Duration::from_millis(16), 60, away);

        assert!(
            fast.reach().offset > spread.reach().offset,
            "the same four, spun: {} against {}",
            fast.reach().offset,
            spread.reach().offset
        );
    }

    /// However long the series, a notch is worth at most the ceiling the
    /// builder set.
    #[test]
    fn the_ceiling_holds_the_fastest_notch_down() {
        let away = at(Point::new(20.0, 20.0));
        let mut rig = Rig::with(|motion| tall(motion).scroll_acceleration(2.0));

        flurry(&mut rig, 14, away);
        let _ = rig.run(Duration::from_millis(16), 90, away);

        let travelled = rig.reach().offset;

        assert!(
            travelled > 14.0 * NOTCH,
            "it did accelerate: {travelled} against {}",
            14.0 * NOTCH
        );
        assert!(
            travelled <= 14.0 * 2.0 * NOTCH,
            "but no notch was worth more than twice its own: {travelled} against {}",
            14.0 * 2.0 * NOTCH
        );
    }

    /// The series is over once the hand stops: the next notch is a notch
    /// again.
    #[test]
    fn a_turn_after_a_pause_is_worth_one_notch_again() {
        let away = at(Point::new(20.0, 20.0));
        let mut rig = Rig::with(tall);

        flurry(&mut rig, 6, away);

        // Long enough for the travel to end, which is longer than the window.
        let settled = rig.run(Duration::from_millis(16), 60, away);
        let spun = rig.reach().offset;

        let _ = rig.deliver(&wheel(0.0, -1.0), away);
        let _ = rig.run(settled, 60, away);

        assert!(
            (rig.reach().offset - (spun + NOTCH)).abs() <= 1.0,
            "one notch on from {spun}: {}",
            rig.reach().offset
        );
    }

    /// A turn the other way starts the series over. Without that, working the
    /// wheel back and forth would wind the multiplier up to its ceiling and
    /// leave it there.
    #[test]
    fn a_turn_the_other_way_starts_the_series_again() {
        let away = at(Point::new(20.0, 20.0));
        let mut rig = Rig::with(tall);

        flurry(&mut rig, 6, away);

        // Part way along, so there is somewhere above to turn back to.
        let under_way = rig.run(Duration::from_millis(16), 12, away);
        let before = rig.reach().offset;

        assert!(before > NOTCH, "far enough down to turn back: {before}");

        let _ = rig.deliver(&wheel(0.0, 1.0), away);
        let _ = rig.run(under_way, 60, away);

        assert!(
            (rig.reach().offset - (before - NOTCH)).abs() <= 1.0,
            "one notch back up from {before}, not a spun one: {}",
            rig.reach().offset
        );
    }

    /// A ceiling of one is acceleration off, and every notch is a notch.
    #[test]
    fn a_ceiling_of_one_counts_every_notch_the_same() {
        let away = at(Point::new(20.0, 20.0));
        let mut rig = Rig::with(|motion| tall(motion).scroll_acceleration(1.0));

        flurry(&mut rig, 4, away);
        let _ = rig.run(Duration::from_millis(16), 60, away);

        assert!(
            (rig.reach().offset - 4.0 * NOTCH).abs() <= 1.0,
            "four notches, spun or not: {}",
            rig.reach().offset
        );
    }

    /// A page holding a scrollable of its own, with the inner one under the
    /// cursor every test below puts there.
    fn nested(motion: &Motion) -> Page<'static> {
        fading_scrollable(
            iced::widget::container(
                iced::widget::scrollable(
                    iced::widget::container(iced::widget::text("a long page")).height(900.0),
                )
                .height(300.0),
            )
            .height(900.0),
        )
        .motion(motion.clone())
        .width(Length::Fill)
        .height(Length::Fill)
        .on_scroll(|_| Scrolled)
    }

    /// The wheel is read back off the scrollable rather than taken from it,
    /// and that is what leaves a scrollable inside this one working: it
    /// answers the wheel first, and this one never sees the offset move.
    #[test]
    fn a_scrollable_inside_takes_the_wheel_first() {
        let mut rig = Rig::with(nested);
        let inside = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -3.0), inside);
        let _ = rig.run(Duration::ZERO, 60, inside);

        assert_eq!(
            rig.reach().offset,
            0.0,
            "the inner scrollable took the turn"
        );
    }

    /// Turned off, the widget is iced's own scrollable again.
    #[test]
    fn a_scrollable_with_no_smooth_scroll_lands_at_once() {
        let mut rig = Rig::with(|motion| page(motion).no_smooth_scroll());
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -3.0), away);

        assert_eq!(rig.reach().offset, 3.0 * NOTCH, "there at once");
    }

    /// A spring curve is run by the widget itself: the offset lives in its
    /// own state, so the wheel glides with no engine to tick it, and the
    /// window goes back to sleep once it has arrived.
    #[test]
    fn a_spring_travel_needs_no_engine() {
        let mut rig = Rig::with(plain);
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -3.0), away);
        assert_eq!(rig.reach().offset, 0.0, "not there at once");

        let _ = rig.pump(Duration::from_millis(16), away);

        assert_eq!(rig.reach().offset, 3.0 * NOTCH, "arrived exactly");
        assert!(!rig.pending, "and stopped asking for frames");
    }

    /// Any other curve needs the engine to run it. Without one the content
    /// is where the wheel put it, exactly as the other tracks jump.
    #[test]
    fn a_curve_only_the_engine_can_run_lands_at_once_without_one() {
        let mut rig = Rig::with(|motion| {
            plain(motion).scroll_curve(Curve::ease(
                iced_animate::Easing::EaseOut,
                Duration::from_millis(200),
            ))
        });
        let away = at(Point::new(20.0, 20.0));

        let _ = rig.deliver(&wheel(0.0, -3.0), away);

        assert_eq!(rig.reach().offset, 3.0 * NOTCH, "there at once");
    }

    const BAR: Bar = Bar {
        gutter: 10.0,
        thickest: 4.0,
        track_padding: Padding::ZERO,
        margin: Padding::ZERO,
        min_pill: 0.0,
    };

    /// A bar with uneven ends and a floor under the pill. The ends differ
    /// on purpose: equal ones would hide a track that took its start from
    /// its end.
    const PADDED: Bar = Bar {
        gutter: 10.0,
        thickest: 4.0,
        track_padding: Padding {
            top: 4.0,
            right: 6.0,
            bottom: 9.0,
            left: 11.0,
        },
        margin: Padding::ZERO,
        min_pill: 20.0,
    };

    /// A bar held off every edge, by different amounts so a test can tell
    /// which side went where.
    const EDGED: Bar = Bar {
        gutter: 10.0,
        thickest: 4.0,
        track_padding: Padding::ZERO,
        margin: Padding {
            top: 2.0,
            right: 3.0,
            bottom: 5.0,
            left: 7.0,
        },
        min_pill: 0.0,
    };

    const BOTH: Axes<bool> = Axes {
        vertical: true,
        horizontal: true,
    };

    const DOWN_ONLY: Axes<bool> = Axes {
        vertical: true,
        horizontal: false,
    };

    const HOLD: Duration = Duration::from_millis(700);

    const SCROLLED: Cue = Cue {
        scrolled: true,
        hovered: false,
        dragging: false,
    };

    fn both_ways(bounds: Rectangle) -> Rectangle {
        Rectangle::new(bounds.position(), Size::new(600.0, 400.0))
    }

    fn viewport() -> Rectangle {
        Rectangle::new(Point::new(5.0, 7.0), Size::new(200.0, 100.0))
    }

    /// A bar that cannot scroll anything must not claim the grab zone: a
    /// press near the trailing edge belongs to the content.
    #[test]
    fn a_scroll_reveals_the_bar() {
        let mut phase = Phase::default();

        let change = phase.settle(SCROLLED, Instant::now(), HOLD);

        assert_eq!(change, Change::Retarget(1.0));
        assert!(phase.shown);
    }

    /// The bar does not leave the moment the wheel stops; it waits out the
    /// hold, and the widget is told when to come back and look.
    #[test]
    fn a_quiet_frame_books_a_wake_up_at_the_end_of_the_hold() {
        let start = Instant::now();
        let mut phase = Phase::default();
        let _ = phase.settle(SCROLLED, start, HOLD);

        let change = phase.settle(Cue::default(), start, HOLD);

        assert_eq!(change, Change::WakeAt(start + HOLD));
        assert!(phase.shown, "still on screen while the hold runs");
    }

    /// The runtime keeps a wake-up only until a pass asks for the next
    /// frame, so every quiet frame inside the hold has to ask for it again.
    #[test]
    fn every_quiet_frame_inside_the_hold_asks_again_for_the_same_wake_up() {
        let start = Instant::now();
        let mut phase = Phase::default();
        let _ = phase.settle(SCROLLED, start, HOLD);
        let _ = phase.settle(Cue::default(), start, HOLD);

        let change = phase.settle(Cue::default(), start + HOLD / 2, HOLD);

        assert_eq!(change, Change::WakeAt(start + HOLD));
    }

    /// The first quiet frame falls while the fade-in is still in flight, so
    /// `Host`'s `NextFrame` wipes the wake-up that frame booked. A bar that
    /// booked its hide only then would sleep on screen until something else
    /// woke the window.
    #[test]
    fn a_bar_left_alone_leaves_on_its_own() {
        let mut rig = Rig::new();
        let over_content = at(Point::new(10.0, 10.0));

        let scrolled = rig.deliver(&wheel(0.0, -3.0), over_content);
        let slept_at = rig.live(Duration::ZERO, scrolled.request, over_content);

        assert!(
            rig.vertical().0 < 0.01,
            "the window slept at {slept_at:?} with the bar still up: {}",
            rig.vertical().0
        );
        assert!(slept_at >= rig.widget.hold, "it waited out the hold first");
    }

    #[test]
    fn the_bar_leaves_once_the_hold_has_run_out() {
        let start = Instant::now();
        let mut phase = Phase::default();
        let _ = phase.settle(SCROLLED, start, HOLD);
        let _ = phase.settle(Cue::default(), start, HOLD);

        let change = phase.settle(Cue::default(), start + HOLD, HOLD);

        assert_eq!(change, Change::Retarget(0.0));
        assert!(!phase.shown);
    }

    /// A second scroll inside the hold must call the hide off rather than
    /// let it fire on the instant it was booked for.
    #[test]
    fn scrolling_again_inside_the_hold_calls_the_hide_off() {
        let start = Instant::now();
        let mut phase = Phase::default();
        let _ = phase.settle(SCROLLED, start, HOLD);
        let _ = phase.settle(Cue::default(), start, HOLD);

        let resumed = start + HOLD / 2;
        let _ = phase.settle(SCROLLED, resumed, HOLD);
        let change = phase.settle(Cue::default(), resumed, HOLD);

        assert_eq!(change, Change::WakeAt(resumed + HOLD));
        assert_eq!(
            phase.settle(Cue::default(), start + HOLD, HOLD),
            Change::WakeAt(resumed + HOLD),
            "the hold the first scroll booked must not still fire: only the new one is asked for"
        );
        assert!(
            phase.shown,
            "still on screen when the first hold would have ended"
        );
    }

    /// Nothing is moving and nothing is on screen: the widget must not keep
    /// booking frames it has no use for.
    #[test]
    fn a_hidden_bar_left_alone_asks_for_nothing() {
        let mut phase = Phase::default();

        let change = phase.settle(Cue::default(), Instant::now(), HOLD);

        assert_eq!(change, Change::None);
    }

    #[test]
    fn resting_the_cursor_in_the_gutter_reveals_the_bar() {
        let mut phase = Phase::default();

        let change = phase.settle(
            Cue {
                hovered: true,
                ..Cue::default()
            },
            Instant::now(),
            HOLD,
        );

        assert_eq!(change, Change::Retarget(1.0));
    }

    /// A drag can outlast both the scrolling and the cursor leaving the
    /// gutter, and the bar has to stay up for all of it.
    #[test]
    fn a_drag_alone_keeps_the_bar_up() {
        let start = Instant::now();
        let mut phase = Phase::default();
        let dragging = Cue {
            dragging: true,
            ..Cue::default()
        };

        let change = phase.settle(dragging, start, HOLD);

        assert_eq!(change, Change::Retarget(1.0));
        assert_eq!(phase.settle(dragging, start + HOLD * 2, HOLD), Change::None);
        assert!(phase.shown);
    }

    const RAIL: Color = Color::from_rgb(0.0, 0.0, 1.0);
    const RAIL_HOVER: Color = Color::from_rgb(1.0, 1.0, 0.0);

    fn style() -> Style {
        Style {
            rail: None,
            rail_hover: None,
            scroller: Color::from_rgb(0.0, 0.0, 0.0),
            scroller_hover: Color::from_rgb(1.0, 0.0, 0.0),
            scroller_dragged: Color::from_rgb(0.0, 1.0, 0.0),
            radius: Radius::new(0),
        }
    }

    #[test]
    fn the_pill_grows_from_the_resting_thickness_to_the_hovered_one() {
        assert_eq!(thickness(4.0, 10.0, 0.0), 4.0);
        assert_eq!(thickness(4.0, 10.0, 1.0), 10.0);
        assert_eq!(thickness(4.0, 10.0, 0.5), 7.0);
    }

    /// `scrollable::draw` skips the quad only for exactly `TRANSPARENT`, so
    /// a faded-out pill has to land on that value and not merely near it.
    #[test]
    fn a_pill_with_no_reveal_left_is_exactly_transparent() {
        assert_eq!(fill(&style(), 0.0, 0.0, 0.0), Color::TRANSPARENT);
        assert_eq!(fill(&style(), 0.0, 1.0, 1.0), Color::TRANSPARENT);
    }

    #[test]
    fn a_bar_given_no_rail_colours_paints_no_rail() {
        assert_eq!(rail(&style(), 1.0, 1.0), None);
    }

    #[test]
    fn a_rail_with_no_reveal_left_is_unpainted() {
        let style = Style {
            rail: Some(RAIL),
            ..style()
        };

        assert_eq!(rail(&style, 0.0, 1.0), None);
    }

    #[test]
    fn a_rail_with_no_hover_colour_holds_the_resting_one() {
        let style = Style {
            rail: Some(RAIL),
            ..style()
        };

        assert_eq!(rail(&style, 1.0, 0.0), Some(RAIL));
        assert_eq!(rail(&style, 1.0, 1.0), Some(RAIL));
    }

    #[test]
    fn a_hovered_rail_takes_the_hover_colour() {
        let style = Style {
            rail: Some(RAIL),
            rail_hover: Some(RAIL_HOVER),
            ..style()
        };

        assert_eq!(rail(&style, 1.0, 0.0), Some(RAIL));
        assert_eq!(rail(&style, 1.0, 1.0), Some(RAIL_HOVER));
    }

    /// A hover colour with no resting one is the "background only under the
    /// cursor" case: it must be absent until the cursor arrives, and it
    /// rides the same fade in as the pill.
    #[test]
    fn a_rail_that_only_exists_on_hover_arrives_with_the_cursor() {
        let style = Style {
            rail_hover: Some(RAIL_HOVER),
            ..style()
        };

        assert_eq!(rail(&style, 1.0, 0.0), None);
        assert_eq!(rail(&style, 1.0, 1.0), Some(RAIL_HOVER));
        assert_eq!(rail(&style, 0.0, 1.0), None, "and only while the bar is up");
    }

    /// The rail and the pill are drawn from the same three numbers; this is
    /// the join that hands both to iced.
    #[test]
    fn a_hovered_bar_carries_its_rail_and_its_pill_together() {
        let style = Style {
            rail_hover: Some(RAIL_HOVER),
            ..style()
        };

        let bar = bar_style(&style, 1.0, 1.0, 0.0);

        assert_eq!(bar.background, Some(Background::Color(RAIL_HOVER)));
        assert_eq!(
            bar.scroller.background,
            Background::Color(style.scroller_hover)
        );
    }

    #[test]
    fn a_bar_nobody_called_for_paints_neither() {
        let style = Style {
            rail: Some(RAIL),
            ..style()
        };

        let bar = bar_style(&style, 0.0, 0.0, 0.0);

        assert_eq!(bar.background, None);
        assert_eq!(
            bar.scroller.background,
            Background::Color(Color::TRANSPARENT)
        );
    }

    #[test]
    fn a_dragged_pill_takes_the_dragged_colour() {
        assert_eq!(fill(&style(), 1.0, 1.0, 1.0), style().scroller_dragged);
    }

    #[test]
    fn a_revealed_pill_left_alone_takes_the_resting_colour() {
        assert_eq!(fill(&style(), 1.0, 0.0, 0.0), style().scroller);
    }

    /// An axis the builder never turned on cannot scroll, so it must not
    /// claim a grab zone however the content measures.
    #[test]
    fn a_vertical_only_scrollable_grows_no_horizontal_gutter() {
        let bounds = viewport();

        let gutters = gutters(BAR, DOWN_ONLY, bounds, both_ways(bounds));

        assert_eq!(gutters.horizontal, None);
        assert!(gutters.vertical.is_some());
    }

    #[test]
    fn a_wheel_turn_over_the_content_cues_the_axis_it_scrolls() {
        let bounds = viewport();
        let content = both_ways(bounds);
        let node = nodes(bounds, content);

        let cues = cues(
            BAR,
            BOTH,
            None,
            &wheel(0.0, -3.0),
            Layout::new(&node),
            at(bounds.center()),
        );

        assert!(cues.vertical.scrolled);
        assert!(!cues.horizontal.scrolled);
    }

    /// A wheel turn is delivered to every widget under the pointer's path,
    /// so the bar must check that the pointer is actually over it.
    #[test]
    fn a_wheel_turn_away_from_the_widget_cues_nothing() {
        let bounds = viewport();
        let node = nodes(bounds, both_ways(bounds));

        let cues = cues(
            BAR,
            BOTH,
            None,
            &wheel(0.0, -3.0),
            Layout::new(&node),
            at(Point::new(bounds.x - 40.0, bounds.y - 40.0)),
        );

        assert!(!cues.vertical.scrolled);
        assert!(!cues.horizontal.scrolled);
    }

    #[test]
    fn the_cursor_in_one_gutter_cues_that_axis_alone() {
        let bounds = viewport();
        let node = nodes(bounds, both_ways(bounds));
        let layout = Layout::new(&node);
        let in_vertical = gutters(BAR, BOTH, bounds, both_ways(bounds))
            .vertical
            .expect("overflowing")
            .center();

        let cues = cues(BAR, BOTH, None, &wheel(0.0, 0.0), layout, at(in_vertical));

        assert!(cues.vertical.hovered);
        assert!(!cues.horizontal.hovered);
    }

    #[test]
    fn a_grabbed_axis_is_cued_as_dragging() {
        let bounds = viewport();
        let node = nodes(bounds, both_ways(bounds));

        let cues = cues(
            BAR,
            BOTH,
            Some(Axis::Horizontal),
            &wheel(0.0, 0.0),
            Layout::new(&node),
            mouse::Cursor::Unavailable,
        );

        assert!(cues.horizontal.dragging);
        assert!(!cues.vertical.dragging);
    }

    fn shown() -> Axes<f32> {
        Axes {
            vertical: 1.0,
            horizontal: 1.0,
        }
    }

    #[test]
    fn a_press_in_the_vertical_gutter_grabs_that_axis() {
        let bounds = viewport();
        let zones = zones(BAR, gutters(BAR, BOTH, bounds, both_ways(bounds)), bounds);
        let inside = zones.vertical.expect("overflowing").center();

        assert_eq!(grabbed(zones, shown(), at(inside)), Some(Axis::Vertical));
    }

    #[test]
    fn a_press_on_the_content_grabs_nothing() {
        let bounds = viewport();
        let zones = zones(BAR, gutters(BAR, BOTH, bounds, both_ways(bounds)), bounds);

        assert_eq!(grabbed(zones, shown(), at(bounds.center())), None);
    }

    /// The pill is invisible until something calls for it, and what cannot
    /// be seen must not answer the button.
    #[test]
    fn a_press_on_an_invisible_pill_is_left_to_the_content() {
        let bounds = viewport();
        let zones = zones(BAR, gutters(BAR, BOTH, bounds, both_ways(bounds)), bounds);
        let inside = zones.vertical.expect("overflowing").center();

        assert_eq!(grabbed(zones, Axes::default(), at(inside)), None);
    }

    // --- where the pill sits ---------------------------------------------

    /// A 200-long gutter over a viewport of 100 showing content of 400.
    const GUTTER_LEN: f32 = 200.0;
    const VIEWPORT: f32 = 100.0;
    const CONTENT: f32 = 400.0;

    #[test]
    fn the_pill_starts_after_the_padding() {
        let (start, _) = pill_span(PADDED, Axis::Vertical, GUTTER_LEN, VIEWPORT, CONTENT, 0.0)
            .expect("overflows");

        assert_eq!(start, PADDED.track_padding.top);
    }

    #[test]
    fn the_pill_stops_before_the_padding_at_the_far_end() {
        let scrollable = CONTENT - VIEWPORT;
        let (start, length) = pill_span(
            PADDED,
            Axis::Vertical,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            scrollable,
        )
        .expect("overflows");

        assert_eq!(start + length, GUTTER_LEN - PADDED.track_padding.bottom);
    }

    #[test]
    fn the_pill_never_shrinks_below_its_minimum() {
        let (_, length) = pill_span(PADDED, Axis::Vertical, GUTTER_LEN, VIEWPORT, 100_000.0, 0.0)
            .expect("overflows");

        assert_eq!(length, PADDED.min_pill);
    }

    /// A floor taller than the track it has to sit in gives way to the
    /// track: a pill longer than its rail would draw outside the gutter.
    #[test]
    fn a_minimum_that_cannot_fit_gives_way_to_the_track() {
        // A 24-long gutter leaves an 11-long track once the top and bottom
        // have had theirs, shorter than the 20 the bar asks for as a floor.
        let (_, length) =
            pill_span(PADDED, Axis::Vertical, 24.0, VIEWPORT, 100_000.0, 0.0).expect("overflows");

        assert_eq!(
            length,
            24.0 - PADDED.track_padding.top - PADDED.track_padding.bottom
        );
    }

    #[test]
    fn a_content_that_fits_has_no_pill() {
        assert_eq!(
            pill_span(PADDED, Axis::Vertical, GUTTER_LEN, VIEWPORT, VIEWPORT, 0.0),
            None
        );
    }

    /// The drag has to be the exact inverse of the drawing, or the pill
    /// slides out from under the cursor as it is dragged.
    #[test]
    fn dragging_a_pill_to_where_it_already_is_moves_nothing() {
        let offset = 137.0;
        let (start, length) = pill_span(
            PADDED,
            Axis::Vertical,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            offset,
        )
        .expect("overflows");

        let grabbed_at = 0.25;
        let under_cursor = start + length * grabbed_at;
        let back = offset_for(
            PADDED,
            Axis::Vertical,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            length,
            grabbed_at,
            under_cursor,
        );

        assert!((back - offset).abs() < 0.001, "{back} vs {offset}");
    }

    #[test]
    fn a_drag_past_the_end_stops_at_the_end() {
        let (_, length) = pill_span(PADDED, Axis::Vertical, GUTTER_LEN, VIEWPORT, CONTENT, 0.0)
            .expect("overflows");

        let far = offset_for(
            PADDED,
            Axis::Vertical,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            length,
            0.5,
            GUTTER_LEN * 4.0,
        );

        assert_eq!(far, CONTENT - VIEWPORT);
    }

    #[test]
    fn the_vertical_pill_runs_down_its_gutter_and_is_centred_across_it() {
        let gutter = Rectangle::new(Point::new(190.0, 7.0), Size::new(10.0, GUTTER_LEN));

        let bar = Bar {
            thickest: 10.0,
            ..BAR
        };
        let pill = pill(bar, Axis::Vertical, gutter, (4.0, 60.0), 6.0);

        assert_eq!(pill.y, gutter.y + 4.0);
        assert_eq!(pill.height, 60.0);
        assert_eq!(pill.width, 6.0);
        assert_eq!(pill.center_x(), gutter.center_x());
    }

    #[test]
    fn the_horizontal_pill_runs_along_its_gutter_and_is_centred_across_it() {
        let gutter = Rectangle::new(Point::new(5.0, 97.0), Size::new(GUTTER_LEN, 10.0));

        let bar = Bar {
            thickest: 10.0,
            ..BAR
        };
        let pill = pill(bar, Axis::Horizontal, gutter, (4.0, 60.0), 6.0);

        assert_eq!(pill.x, gutter.x + 4.0);
        assert_eq!(pill.width, 60.0);
        assert_eq!(pill.height, 6.0);
        assert_eq!(pill.center_y(), gutter.center_y());
    }

    /// The padding lives inside the rail: across the bar it widens the box
    /// the rail is painted in, and the box itself stays against its edge.
    #[test]
    fn the_padding_widens_the_rail_across_the_bar() {
        let bounds = viewport();
        let rails = gutters(PADDED, BOTH, bounds, both_ways(bounds));

        let vertical = rails.vertical.expect("overflowing");
        let horizontal = rails.horizontal.expect("overflowing");

        assert_eq!(vertical.x + vertical.width, bounds.x + bounds.width);
        assert_eq!(
            vertical.width,
            11.0 + PADDED.thickest + 6.0,
            "left + gutter + right"
        );
        assert_eq!(horizontal.y + horizontal.height, bounds.y + bounds.height);
        assert_eq!(
            horizontal.height,
            4.0 + PADDED.thickest + 9.0,
            "top + gutter + bottom"
        );
    }

    /// Across its rail the pill sits in what the padding leaves: after
    /// `left` for the vertical bar, after `top` for the horizontal one —
    /// uneven sides, so a pill centred on the whole rail is caught.
    #[test]
    fn the_pill_sits_between_the_padding_across_its_rail() {
        let vertical_rail = Rectangle::new(Point::new(190.0, 7.0), Size::new(21.0, GUTTER_LEN));
        let horizontal_rail = Rectangle::new(Point::new(5.0, 97.0), Size::new(GUTTER_LEN, 17.0));

        let vertical = pill(PADDED, Axis::Vertical, vertical_rail, (4.0, 60.0), 2.0);
        let horizontal = pill(PADDED, Axis::Horizontal, horizontal_rail, (4.0, 60.0), 2.0);

        assert_eq!(vertical.center_x(), 190.0 + 11.0 + PADDED.thickest / 2.0);
        assert_eq!(horizontal.center_y(), 97.0 + 4.0 + PADDED.thickest / 2.0);
    }

    /// With no track padding there is nothing around the pill: the rail is
    /// the pill, across and at the start of the track alike.
    #[test]
    fn a_rail_with_no_padding_hugs_the_pill() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(200.0, 400.0));
        let rail = gutters(BAR, BOTH, bounds, content)
            .vertical
            .expect("overflowing");

        let pill = pill(BAR, Axis::Vertical, rail, (0.0, 30.0), BAR.thickest);

        assert_eq!(rail.width, BAR.thickest);
        assert_eq!((pill.x, pill.width), (rail.x, rail.width));
        assert_eq!(pill.y, rail.y);
    }

    /// An even track padding is even: the thickest pill sits as far from
    /// the rail's sides as from the start of its track.
    #[test]
    fn an_even_track_padding_is_even_on_every_side() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(200.0, 400.0));
        let bar = Bar {
            track_padding: Padding::new(3.0),
            ..BAR
        };
        let rail = gutters(bar, BOTH, bounds, content)
            .vertical
            .expect("overflowing");
        let span =
            pill_span(bar, Axis::Vertical, rail.height, VIEWPORT, CONTENT, 0.0).expect("overflows");

        let pill = pill(bar, Axis::Vertical, rail, span, bar.thickest);

        assert_eq!(pill.x - rail.x, 3.0, "left");
        assert_eq!((rail.x + rail.width) - (pill.x + pill.width), 3.0, "right");
        assert_eq!(pill.y - rail.y, 3.0, "top");
    }

    /// A rail drawn thin must not be a thin target: the grab zone keeps
    /// the gutter's width in from the edge whatever the rail measures.
    #[test]
    fn a_thin_rail_is_still_grabbed_across_the_whole_gutter() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(200.0, 400.0));
        let rails = gutters(BAR, BOTH, bounds, content);

        let zone = zones(BAR, rails, bounds).vertical.expect("overflowing");

        assert!(rails.vertical.expect("overflowing").width < BAR.gutter);
        assert_eq!(zone.x, bounds.x + bounds.width - BAR.gutter);
        assert_eq!(zone.width, BAR.gutter);
    }

    /// Widened past their rails, the two zones would meet in the corner:
    /// each stops where the other begins, so one press is one axis.
    #[test]
    fn the_widened_zones_never_share_the_corner() {
        let bounds = viewport();
        let zones = zones(BAR, gutters(BAR, BOTH, bounds, both_ways(bounds)), bounds);

        let v = zones.vertical.expect("overflowing");
        let h = zones.horizontal.expect("overflowing");
        let overlap = v.x < h.x + h.width
            && h.x < v.x + v.width
            && v.y < h.y + h.height
            && h.y < v.y + v.height;

        assert!(!overlap, "{v:?} and {h:?}");
    }

    /// The margin sits outside the rails: they live in the viewport shrunk
    /// by it, each against its own inset edge, and still give the corner up.
    #[test]
    fn the_margin_holds_the_rails_off_the_edges() {
        let bounds = viewport();
        let rails = gutters(EDGED, BOTH, bounds, both_ways(bounds));

        assert_eq!(
            rails.vertical,
            Some(Rectangle {
                x: bounds.x + bounds.width - 3.0 - EDGED.thickest,
                y: bounds.y + 2.0,
                width: EDGED.thickest,
                height: bounds.height - 2.0 - 5.0 - EDGED.thickest,
            })
        );
        assert_eq!(
            rails.horizontal,
            Some(Rectangle {
                x: bounds.x + 7.0,
                y: bounds.y + bounds.height - 5.0 - EDGED.thickest,
                width: bounds.width - 7.0 - 3.0 - EDGED.thickest,
                height: EDGED.thickest,
            })
        );
    }

    /// Margin and track padding are two different boxes: one moves the rail,
    /// the other stays inside it.
    #[test]
    fn the_margin_leaves_the_rail_its_track_padding() {
        let bounds = viewport();
        let bar = Bar {
            margin: EDGED.margin,
            ..PADDED
        };

        let rail = gutters(bar, BOTH, bounds, both_ways(bounds))
            .vertical
            .expect("overflowing");

        assert_eq!(rail.x + rail.width, bounds.x + bounds.width - 3.0);
        assert_eq!(rail.width, 11.0 + PADDED.thickest + 6.0);
    }

    /// Holding a rail off its edge must not leave a dead strip there.
    #[test]
    fn a_rail_held_off_its_edge_still_answers_at_the_edge() {
        let bounds = viewport();
        let rails = gutters(EDGED, BOTH, bounds, both_ways(bounds));
        let zones = zones(EDGED, rails, bounds);

        let vertical = zones.vertical.expect("overflowing");
        let horizontal = zones.horizontal.expect("overflowing");
        let rail = rails.vertical.expect("overflowing");

        assert_eq!(vertical.x + vertical.width, bounds.x + bounds.width);
        assert_eq!(vertical.y, rail.y);
        assert_eq!(horizontal.y + horizontal.height, bounds.y + bounds.height);
    }

    /// A lone bar has its whole inset edge: the margin ends it, not a corner.
    #[test]
    fn a_lone_rail_runs_its_edge_between_the_margins() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(200.0, 400.0));

        let rail = gutters(EDGED, BOTH, bounds, content)
            .vertical
            .expect("overflowing");

        assert_eq!(rail.height, bounds.height - 2.0 - 5.0);
    }

    #[test]
    fn a_content_that_fits_grows_no_gutter() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), bounds.size());

        let gutters = gutters(BAR, BOTH, bounds, content);

        assert_eq!(gutters.vertical, None);
        assert_eq!(gutters.horizontal, None);
    }

    /// Neither bar may claim the corner: each stops short of the other's
    /// whole rail, padding included.
    #[test]
    fn each_gutter_gives_the_corner_up_to_the_other() {
        let bounds = viewport();

        let gutters = gutters(PADDED, BOTH, bounds, both_ways(bounds));

        assert_eq!(
            gutters.vertical.expect("overflowing").height,
            bounds.height - (4.0 + PADDED.thickest + 9.0)
        );
        assert_eq!(
            gutters.horizontal.expect("overflowing").width,
            bounds.width - (11.0 + PADDED.thickest + 6.0)
        );
    }

    /// The horizontal bar's track runs left to right, so it takes its ends
    /// from `left` and `right` and not from the vertical bar's pair.
    #[test]
    fn the_horizontal_track_takes_its_ends_from_left_and_right() {
        let (start, _) = pill_span(PADDED, Axis::Horizontal, GUTTER_LEN, VIEWPORT, CONTENT, 0.0)
            .expect("overflows");
        let (far_start, far_length) = pill_span(
            PADDED,
            Axis::Horizontal,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            CONTENT - VIEWPORT,
        )
        .expect("overflows");

        assert_eq!(start, PADDED.track_padding.left);
        assert_eq!(
            far_start + far_length,
            GUTTER_LEN - PADDED.track_padding.right
        );
    }

    #[test]
    fn a_horizontal_drag_with_uneven_ends_lands_where_it_started() {
        let offset = 211.0;
        let (start, length) = pill_span(
            PADDED,
            Axis::Horizontal,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            offset,
        )
        .expect("overflows");

        let back = offset_for(
            PADDED,
            Axis::Horizontal,
            GUTTER_LEN,
            VIEWPORT,
            CONTENT,
            length,
            0.75,
            start + length * 0.75,
        );

        assert!((back - offset).abs() < 0.001, "{back} vs {offset}");
    }

    /// One bar alone has the whole edge: there is no corner to give up.
    #[test]
    fn a_lone_gutter_runs_the_whole_edge() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(200.0, 400.0));

        let gutters = gutters(BAR, BOTH, bounds, content);

        assert_eq!(gutters.vertical.expect("overflowing").height, bounds.height);
    }

    #[test]
    fn a_wider_content_puts_the_horizontal_gutter_along_the_bottom_edge() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(600.0, 100.0));

        let gutters = gutters(BAR, BOTH, bounds, content);

        assert_eq!(
            gutters.horizontal,
            Some(Rectangle {
                x: bounds.x,
                y: bounds.y + bounds.height - BAR.thickest,
                width: bounds.width,
                height: BAR.thickest,
            })
        );
    }

    #[test]
    fn a_taller_content_puts_the_vertical_gutter_on_the_trailing_edge() {
        let bounds = viewport();
        let content = Rectangle::new(bounds.position(), Size::new(200.0, 400.0));

        let gutters = gutters(BAR, BOTH, bounds, content);

        assert_eq!(
            gutters.vertical,
            Some(Rectangle {
                x: bounds.x + bounds.width - BAR.thickest,
                y: bounds.y,
                width: BAR.thickest,
                height: bounds.height,
            })
        );
    }
}

/// A scrollable whose bars stay out of the way until they are wanted.
///
/// With nothing scrolling and the cursor elsewhere, the bars are not there:
/// the pill fades to nothing and `scrollable` skips its quad. A wheel turn,
/// or the cursor entering a bar's gutter, fades it back in; the cursor
/// resting in the gutter also grows the pill from its resting thickness to
/// its hovered one, and dragging the pill takes it to the dragged colour.
/// After the last of that, the bar waits out a hold and leaves again.
///
/// All the scrolling itself is iced's: this wraps `scrollable` rather than
/// replacing it, so offsets, touch, the keyboard, auto-scroll and
/// `scrollable`'s operations behave exactly as they do everywhere else.
///
/// Bind it to a [`Motion`] for any of that to move; without one the bars
/// jump between their poses.
///
/// # Example
///
/// ```
/// use iced_luminate::animate::Motion;
/// use iced_luminate::iced::widget::text;
/// use iced_luminate::iced::{Element, Theme};
/// use iced_luminate::widget::fading_scrollable::fading_scrollable;
///
/// let motion = Motion::new();
/// let page: Element<'_, (), Theme, iced_luminate::Renderer> =
///     fading_scrollable(text("a long page"))
///         .motion(motion.clone())
///         .into();
/// ```
pub struct FadingScrollable<'a, Message, Theme = iced::Theme, Renderer = crate::Renderer>
where
    Theme: Catalog + scrollable::Catalog,
    Renderer: iced::advanced::text::Renderer,
{
    /// `Some` except while a builder has it out: `Scrollable`'s own builders
    /// take it by value, and re-applying `direction` per frame is what moves
    /// the pill's thickness.
    inner: Option<Scrollable<'a, Message, Theme, Renderer>>,
    class: <Theme as Catalog>::Class<'a>,
    motion: Option<Motion>,
    bar: Bar,
    rest: f32,
    hover: f32,
    hold: Duration,
    axes: Axes<bool>,
    curves: Curves,
    /// Whether a wheel turn is travelled rather than jumped.
    smooth: bool,
    /// The most a turn of the wheel can grow to inside a fast series.
    fastest: f32,
}

/// Default narrowest grab zone across a bar.
const GUTTER: f32 = 12.0;
/// Default thickness of a pill nobody is pointing at.
const REST: f32 = 4.0;
/// Default thickness of a pill whose gutter holds the cursor.
const HOVER: f32 = 4.0;
/// Default inset at both ends of a bar's track.
const PADDING: f32 = 3.0;
/// Default floor under the pill's length.
const MIN_PILL: f32 = 24.0;
/// Default wait between the last call for a bar and the bar leaving.
const HOLD: Duration = Duration::from_millis(700);
/// Default ceiling on what one turn of the wheel can grow to.
const FASTEST: f32 = 4.0;
/// What each turn inside a series multiplies the one before it by.
///
/// Tuned against [`FASTEST`]: the fourth notch of a series is worth about
/// one and a half, the eighth about three, and the ceiling arrives around
/// the tenth — far enough in that an ordinary pair of notches is untouched,
/// near enough that a page can be thrown past in one gesture.
const GROWTH: f32 = 1.15;
/// The longest gap between two turns that still reads as one gesture.
const SERIES: Duration = Duration::from_millis(200);

/// How close a glide has to be to its target to be over, in pixels: the
/// offset is written in whole pixels, so anything nearer rounds onto it.
const GLIDE_TOLERANCE: f32 = 0.5;

/// The curve each of a bar's three tracks moves on, and the one the content
/// travels on under them.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Curves {
    /// The bar arriving and leaving.
    fade: Curve,
    /// The pill thickening under the cursor, and taking the hover colour
    /// with it.
    expand: Curve,
    /// The pill darkening as it is dragged.
    press: Curve,
    /// The content travelling to where a turn of the wheel asked for.
    scroll: Curve,
}

impl Default for Curves {
    fn default() -> Self {
        Self {
            fade: curves::FADE,
            expand: curves::QUICK,
            press: curves::QUICK,
            scroll: Curve::spring(SpringParams::new(0.0, Duration::from_millis(200))),
        }
    }
}

impl<Message, Theme, Renderer> std::fmt::Debug for FadingScrollable<'_, Message, Theme, Renderer>
where
    Theme: Catalog + scrollable::Catalog,
    Renderer: iced::advanced::text::Renderer,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FadingScrollable")
            .field("motion", &self.motion)
            .field("bar", &self.bar)
            .field("rest", &self.rest)
            .field("hover", &self.hover)
            .field("hold", &self.hold)
            .field("axes", &self.axes)
            .field("curves", &self.curves)
            .finish_non_exhaustive()
    }
}

impl<'a, Message, Theme, Renderer> FadingScrollable<'a, Message, Theme, Renderer>
where
    Theme: Catalog + scrollable::Catalog + 'a,
    <Theme as scrollable::Catalog>::Class<'a>: From<scrollable::StyleFn<'a, Theme>>,
    Renderer: text::Renderer,
{
    /// Wraps `content` in a vertical scrollable with fading bars.
    #[must_use]
    pub fn new(content: impl Into<Element<'a, Message, Theme, Renderer>>) -> Self {
        let axes = Axes {
            vertical: true,
            horizontal: false,
        };
        let bar = Bar {
            gutter: GUTTER,
            thickest: HOVER.max(REST),
            track_padding: Padding::new(PADDING),
            margin: Padding::ZERO,
            min_pill: MIN_PILL,
        };

        let mut scrollable = Self {
            inner: Some(blank(Scrollable::new(content))),
            class: <Theme as Catalog>::default(),
            motion: None,
            bar,
            rest: REST,
            hover: HOVER,
            hold: HOLD,
            axes,
            curves: Curves::default(),
            smooth: true,
            fastest: FASTEST,
        };
        scrollable.reapply_direction();

        scrollable
    }

    /// Rebuilds the inner scrollable's `Direction` from the axes and bar
    /// geometry as they now stand, with both pills at rest.
    ///
    /// Every builder that moves either has to call this: which axes scroll
    /// decides what limits the content is laid out against, so setting the
    /// flag alone would leave a bar that never has anything to scroll.
    fn reapply_direction(&mut self) {
        let direction = self.direction();

        self.map_inner(|inner| inner.direction(direction));
    }

    /// Also scrolls the content sideways, with a bar along the bottom edge.
    #[must_use]
    pub fn horizontal(mut self) -> Self {
        self.axes.horizontal = true;
        self.reapply_direction();
        self
    }

    /// Stops the content scrolling vertically, leaving only the bar the
    /// other axis asked for.
    #[must_use]
    pub fn no_vertical(mut self) -> Self {
        self.axes.vertical = false;
        self.reapply_direction();
        self
    }

    /// The width of the scrollable.
    #[must_use]
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.map_inner(|inner| inner.width(width));
        self
    }

    /// The height of the scrollable.
    #[must_use]
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.map_inner(|inner| inner.height(height));
        self
    }

    /// The [`widget::Id`](iced::advanced::widget::Id) of the scrollable, so
    /// `scrollable`'s operations can reach it.
    #[must_use]
    pub fn id(mut self, id: impl Into<iced::advanced::widget::Id>) -> Self {
        self.map_inner(|inner| inner.id(id));
        self
    }

    /// A message produced whenever the content is scrolled.
    #[must_use]
    pub fn on_scroll(mut self, f: impl Fn(scrollable::Viewport) -> Message + 'a) -> Self {
        self.map_inner(|inner| inner.on_scroll(f));
        self
    }

    /// Applies one of `Scrollable`'s own by-value builders in place.
    fn map_inner(
        &mut self,
        f: impl FnOnce(
            Scrollable<'a, Message, Theme, Renderer>,
        ) -> Scrollable<'a, Message, Theme, Renderer>,
    ) {
        let inner = self.inner.take().expect("the scrollable is only out here");
        self.inner = Some(f(inner));
    }

    /// Binds every bar animation to `motion`. Without one, the bars jump.
    ///
    /// The wheel's travel does not need it on a spring curve, the default:
    /// see [`scroll_curve`](Self::scroll_curve).
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// The narrowest the grab zone gets across a bar, measured in from its
    /// edge. The rail is drawn only as thick as the pill plus the
    /// [`track_padding`](Self::track_padding); a thinner rail is still
    /// answered this far in. Defaults to 12 px.
    #[must_use]
    pub fn gutter(mut self, gutter: impl Into<iced::Pixels>) -> Self {
        self.bar.gutter = gutter.into().0.max(0.0);
        self.reapply_direction();
        self
    }

    /// The two thicknesses the pill moves between: the first for a pill
    /// nobody is pointing at, the second for one whose gutter holds the
    /// cursor. Pass the same value twice for a pill that only fades. The
    /// rail is sized for the thicker of the two, so it never moves.
    #[must_use]
    pub fn pill(mut self, rest: impl Into<iced::Pixels>, hover: impl Into<iced::Pixels>) -> Self {
        self.rest = rest.into().0.max(0.0);
        self.hover = hover.into().0.max(0.0);
        self.bar.thickest = self.rest.max(self.hover);
        self.reapply_direction();
        self
    }

    /// The curve a bar arrives and leaves on. Defaults to
    /// [`curves::FADE`].
    #[must_use]
    pub fn fade_curve(mut self, curve: Curve) -> Self {
        self.curves.fade = curve;
        self
    }

    /// The curve the pill thickens on as the cursor takes its gutter, and
    /// with it the blend to the hovered colours. Defaults to
    /// [`curves::QUICK`]; a longer one reads as calmer.
    #[must_use]
    pub fn expand_curve(mut self, curve: Curve) -> Self {
        self.curves.expand = curve;
        self
    }

    /// The curve the pill darkens on as it is dragged. Defaults to
    /// [`curves::QUICK`].
    #[must_use]
    pub fn press_curve(mut self, curve: Curve) -> Self {
        self.curves.press = curve;
        self
    }

    /// The curve the content travels on after a turn of the wheel.
    ///
    /// Defaults to [`curves::SMOOTH`], a spring: a turn arriving while the
    /// last one is still travelling continues that motion rather than
    /// restarting it, so a flurry of notches reads as one gesture. A spring
    /// without a delay is run by the widget itself and needs no
    /// [`motion`](Self::motion). A fixed-duration curve works through the
    /// engine, but restarts on every notch.
    #[must_use]
    pub fn scroll_curve(mut self, curve: Curve) -> Self {
        self.curves.scroll = curve;
        self
    }

    /// The most one turn of the wheel can grow to while the wheel is being
    /// spun. Defaults to `4.0`; `1.0` is no acceleration at all.
    ///
    /// Turns arriving within 200 ms of one another are one gesture, and each
    /// is worth a little more than the last until it reaches this. A turn
    /// after a longer pause, or one the other way, starts the count again.
    /// Only the wheel accelerates: a touchpad's pixels arrive with the hand's
    /// own acceleration already in them.
    #[must_use]
    pub fn scroll_acceleration(mut self, fastest: f32) -> Self {
        self.fastest = fastest.max(1.0);
        self
    }

    /// Lands every scroll in the frame it arrives, the way iced does on its
    /// own.
    ///
    /// The travel is otherwise on. On a spring curve — the default — the
    /// widget runs it itself; any other [`scroll_curve`](Self::scroll_curve)
    /// needs a [`motion`](Self::motion) to travel through, and jumps
    /// without one.
    #[must_use]
    pub fn no_smooth_scroll(mut self) -> Self {
        self.smooth = false;
        self
    }

    /// The inset inside each rail, around the pill.
    ///
    /// Like a container's padding: the vertical bar's pill runs from `top`
    /// to `bottom` with `left` and `right` on either side of it, the
    /// horizontal bar's from `left` to `right` with `top` and `bottom`
    /// above and below. The rail is the thicker [`pill`](Self::pill) plus
    /// this, so an even padding is even on every side and none leaves the
    /// rail hugging the pill. A single number puts the same inset on every
    /// side.
    #[must_use]
    pub fn track_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.bar.track_padding = non_negative(padding.into());
        self
    }

    /// The inset outside the rails, between them and the viewport's edges.
    ///
    /// The bars are laid out in the viewport shrunk by it: `right` holds
    /// the vertical bar off the right edge and `bottom` the horizontal one
    /// off the bottom, while `top` and `bottom` end the vertical rail short
    /// and `left` and `right` the horizontal one. The rail and the
    /// [`track_padding`](Self::track_padding) inside it keep their size;
    /// the cursor is still answered right up to the edge. The content is
    /// never moved. Defaults to none.
    #[must_use]
    pub fn margin(mut self, margin: impl Into<Padding>) -> Self {
        self.bar.margin = non_negative(margin.into());
        self
    }

    /// The shortest the pill may get. Without a floor a long page shrinks
    /// it to a sliver nobody can grab. Defaults to 24 px.
    #[must_use]
    pub fn min_pill(mut self, min_pill: impl Into<iced::Pixels>) -> Self {
        self.bar.min_pill = min_pill.into().0.max(0.0);
        self
    }

    /// How long a bar stays after the last thing that called for it.
    #[must_use]
    pub fn hold(mut self, hold: Duration) -> Self {
        self.hold = hold;
        self
    }

    /// Sets the style of the bars.
    #[must_use]
    pub fn style(self, style: impl Fn(&Theme) -> Style + 'a) -> Self
    where
        <Theme as Catalog>::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class(Box::new(style) as StyleFn<'a, Theme>)
    }

    /// Sets the style class of the bars.
    #[must_use]
    pub fn class(mut self, class: impl Into<<Theme as Catalog>::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }
}

/// Silences the inner scrollable's own painting.
///
/// Its bar is already zero-width and so never drawn, but the container
/// background and the auto-scroll overlay come from the application's
/// theme, and this widget draws its own surface. A closure with nothing
/// captured, built once.
fn blank<'a, Message, Theme, Renderer>(
    inner: Scrollable<'a, Message, Theme, Renderer>,
) -> Scrollable<'a, Message, Theme, Renderer>
where
    Theme: Catalog + scrollable::Catalog + 'a,
    <Theme as scrollable::Catalog>::Class<'a>: From<scrollable::StyleFn<'a, Theme>>,
    Renderer: text::Renderer,
{
    inner.style(|_theme, _status| {
        let rail = scrollable::Rail {
            background: None,
            border: Border::default(),
            scroller: scrollable::Scroller {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
            },
        };

        scrollable::Style {
            container: container::Style::default(),
            vertical_rail: rail,
            horizontal_rail: rail,
            gap: None,
            auto_scroll: scrollable::AutoScroll {
                background: Background::Color(Color::TRANSPARENT),
                border: Border::default(),
                shadow: iced::Shadow::default(),
                icon: Color::TRANSPARENT,
            },
        }
    })
}

/// Everything one [`FadingScrollable`] remembers between frames.
#[derive(Debug)]
struct State {
    keys: Axes<MotionKey>,
    reveal: Axes<Anim<f32>>,
    expand: Axes<Anim<f32>>,
    press: Axes<Anim<f32>>,
    phase: Axes<Phase>,
    dragging: Option<Axis>,
    /// Where in the pill it was grabbed, 0 at its leading edge and 1 at its
    /// trailing one. Half means the press landed on the rail instead, which
    /// takes the pill to the cursor rather than jumping it by its edge.
    grabbed_at: f32,
    /// What the inner scrollable last reported, per axis.
    reach: Axes<Reach>,
    /// The pose both bars were in when a frame was last asked for. A pose
    /// that has moved since is one nothing has acted on yet.
    pose: Axes<(bool, bool)>,
    /// Wheel turns seen since the last frame. A scroll is an edge, not a
    /// pose, so it is collected here and spent when the frame arrives with
    /// the clock the hold needs.
    scrolled: Axes<bool>,
    /// The offset the content is drawn at, on its way to [`target`], when
    /// the travel runs through the engine: a curve that is not a plain
    /// spring. At rest it holds where the content is.
    ///
    /// [`target`]: Self::target
    scroll: Axes<Anim<f32>>,
    /// The travel on a spring curve, owned here rather than by the engine,
    /// or `None` when there is none. The offset lives in this widget's
    /// state, so it needs no host to tick it: the wheel glides with or
    /// without a [`motion`](FadingScrollable::motion).
    glide: Axes<Option<Spring>>,
    /// The frame each glide was last ticked on, `None` before its first:
    /// a glide sets off on the frame after the turn that started it, not
    /// from however long ago the last frame was drawn.
    glided_at: Axes<Option<Instant>>,
    /// Where that travel ends.
    target: Axes<f32>,
    /// The last frame's clock, which is the only one a wheel turn has: the
    /// event carries no time of its own, and a widget that asked the system
    /// for one would keep a clock the tests cannot drive.
    clock: Option<Instant>,
    /// When the last turn of the wheel came, by that clock. `None` until one
    /// has, and until a frame has been drawn to time it by — a flurry that
    /// beats the very first frame is simply not accelerated.
    turned_at: Axes<Option<Instant>>,
    /// What the turns of the current series have grown a notch to. `1.0` is
    /// a notch worth exactly itself.
    gain: Axes<f32>,
    /// The offset this widget last left the inner scrollable at, so an
    /// offset that has moved on its own — the wheel it has just spent, a
    /// finger, an application's `scroll_to` — can be told from one this
    /// widget wrote itself.
    left_at: Axes<f32>,
}

impl State {
    /// A state resting with both bars away.
    ///
    /// Every track is *declared* at rest here rather than left to the first
    /// retarget. `Motion::to` starts a track it has never seen at whatever
    /// it is first asked for, so a bar whose first call was the one that
    /// reveals it would cut in at full strength instead of fading.
    fn new(motion: Option<&Motion>, curves: Curves) -> Self {
        let keys = Axes {
            vertical: MotionKey::unique(),
            horizontal: MotionKey::unique(),
        };

        let seed = |name: &str, curve| {
            let at_rest = |axis: Axis| {
                let mut slot = Anim::constant(0.0);
                drive(motion, keys.of(axis).with(name), curve, 0.0, &mut slot);
                slot
            };

            Axes {
                vertical: at_rest(Axis::Vertical),
                horizontal: at_rest(Axis::Horizontal),
            }
        };

        Self {
            reveal: seed("reveal", curves.fade),
            expand: seed("expand", curves.expand),
            press: seed("press", curves.press),
            scroll: seed("scroll", curves.scroll),
            keys,
            phase: Axes::default(),
            dragging: None,
            grabbed_at: 0.5,
            pose: Axes::default(),
            reach: Axes::default(),
            scrolled: Axes::default(),
            target: Axes::default(),
            glide: Axes::default(),
            glided_at: Axes::default(),
            left_at: Axes::default(),
            clock: None,
            turned_at: Axes::default(),
            gain: Axes {
                vertical: 1.0,
                horizontal: 1.0,
            },
        }
    }

    /// This frame's `(reveal, expand, press)` on `axis`.
    fn values(&self, axis: Axis) -> (f32, f32, f32) {
        (
            self.reveal.of(axis).get(),
            self.expand.of(axis).get(),
            self.press.of(axis).get(),
        )
    }

    /// Where the content is drawn on `axis` this frame.
    fn offset(&self, axis: Axis) -> f32 {
        self.glide
            .of(axis)
            .map_or_else(|| self.scroll.of(axis).get(), |glide| glide.position())
    }

    /// Where the content on `axis` is headed.
    fn heading(&self, axis: Axis) -> f32 {
        self.glide
            .of(axis)
            .map_or_else(|| self.scroll.of(axis).target(), |glide| glide.target())
    }

    /// This frame's reveal on both axes, as [`grabbed`] wants it.
    fn revealed(&self) -> Axes<f32> {
        Axes {
            vertical: self.reveal.vertical.get(),
            horizontal: self.reveal.horizontal.get(),
        }
    }
}

/// Points `slot` at `target`, animating there through `motion` if there is
/// one and jumping if there is not.
fn drive(
    motion: Option<&Motion>,
    key: MotionKey,
    curve: iced_animate::Curve,
    target: f32,
    slot: &mut Anim<f32>,
) {
    *slot = match motion {
        Some(motion) => motion.to(key, curve, target),
        None => Anim::constant(target),
    };
}

/// Puts `slot` at `at` with no motion left in it.
///
/// Through the engine's own track rather than over it with a constant: the
/// track outlives the handle, so a constant would leave the next [`drive`]
/// setting off from wherever the track was abandoned instead of from here.
fn jump(
    motion: Option<&Motion>,
    key: MotionKey,
    curve: iced_animate::Curve,
    at: f32,
    slot: &mut Anim<f32>,
) {
    *slot = match motion {
        Some(motion) => motion.play(key, curve, at, at),
        None => Anim::constant(at),
    };
}

impl<'a, Message, Theme, Renderer> FadingScrollable<'a, Message, Theme, Renderer>
where
    Theme: Catalog + scrollable::Catalog,
    Renderer: text::Renderer,
{
    /// The `Direction` the inner scrollable is configured with.
    ///
    /// Always [`Scrollbar::hidden`]: a zero-width bar makes iced's
    /// `total_bounds` empty, so it neither draws a bar nor answers the
    /// button, while the wheel, touch, the keyboard and every `scrollable`
    /// operation go on working. The visible bar is this widget's own, drawn
    /// in `draw` from geometry it can inset and floor as it likes.
    fn direction(&self) -> Direction {
        match (self.axes.vertical, self.axes.horizontal) {
            (_, false) => Direction::Vertical(Scrollbar::hidden()),
            (false, true) => Direction::Horizontal(Scrollbar::hidden()),
            (true, true) => Direction::Both {
                vertical: Scrollbar::hidden(),
                horizontal: Scrollbar::hidden(),
            },
        }
    }

    /// The wrapped scrollable. It is only ever out while a builder holds it.
    fn inner(&self) -> &Scrollable<'a, Message, Theme, Renderer> {
        self.inner.as_ref().expect("the scrollable is in")
    }

    /// The wrapped scrollable, mutably.
    fn inner_mut(&mut self) -> &mut Scrollable<'a, Message, Theme, Renderer> {
        self.inner.as_mut().expect("the scrollable is in")
    }

    /// The wrapped scrollable as the widget it is, for `Tree`.
    fn as_widget(&self) -> &dyn Widget<Message, Theme, Renderer> {
        self.inner()
    }

    /// Runs a [`Scroll`] against the inner scrollable, reading its offset
    /// back and optionally taking it somewhere first.
    fn reach(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        to: Option<AbsoluteOffset<Option<f32>>>,
    ) -> Axes<Reach> {
        // The scrollable hands the operation the translation it had on the
        // way in, so a pass that writes reports the offset from before its
        // own write. A drag needs the truth this frame, not last frame's,
        // and the read costs nothing: `Scroll` never traverses.
        if to.is_some() {
            let mut write = Scroll { reach: None, to };

            self.inner_mut()
                .operate(&mut tree.children[0], layout, renderer, &mut write);
        }

        let mut read = Scroll {
            reach: None,
            to: None,
        };

        self.inner_mut()
            .operate(&mut tree.children[0], layout, renderer, &mut read);

        read.reaches()
    }

    /// The lanes of both bars for this layout.
    fn lanes(&self, layout: Layout<'_>) -> Axes<Option<Rectangle>> {
        let bounds = layout.bounds();
        let content = layout
            .children()
            .next()
            .map_or(bounds, |content| content.bounds());

        gutters(self.bar, self.axes, bounds, content)
    }

    /// The spring a wheel travel glides on, when the curve is a plain one:
    /// a spring with no lead-in delay. Anything else goes through the
    /// engine, which knows how to run it.
    fn glide_spring(&self) -> Option<SpringParams> {
        match self.curves.scroll.kind() {
            CurveKind::Spring(params) if self.curves.scroll.delay().is_zero() => Some(params),
            _ => None,
        }
    }

    /// Advances every glide to `now`, and lands the ones that have arrived.
    fn glide(&self, state: &mut State, now: Instant, shell: &mut Shell<'_, Message>) {
        for axis in Axis::BOTH {
            let Some(mut glide) = *state.glide.of(axis) else {
                continue;
            };

            if let Some(then) = *state.glided_at.of(axis) {
                glide.tick(now.saturating_duration_since(then).as_secs_f32());
            }
            *state.glided_at.of_mut(axis) = Some(now);

            if glide.is_settled_within(GLIDE_TOLERANCE) {
                // Handed back to the resting track, exactly on target, so
                // the next jump or engine travel starts from here.
                *state.glide.of_mut(axis) = None;
                jump(
                    self.motion.as_ref(),
                    state.keys.of(axis).with("scroll"),
                    self.curves.scroll,
                    glide.target(),
                    state.scroll.of_mut(axis),
                );
            } else {
                *state.glide.of_mut(axis) = Some(glide);
                shell.request_redraw();
            }
        }
    }

    /// Spends one frame: settles both phases against the clock it carries
    /// and points every track at what this frame asks for. The values
    /// themselves are read again in `draw`, off the same tracks.
    fn spend(
        &mut self,
        state: &mut State,
        cues: Axes<Cue>,
        now: Instant,
        shell: &mut Shell<'_, Message>,
    ) {
        let motion = self.motion.clone();

        self.glide(state, now, shell);

        // The only clock the widget has. A wheel turn between two frames is
        // timed by the one before it, which is a frame out at worst and so
        // nowhere near the window a series is measured against.
        state.clock = Some(now);

        for axis in Axis::BOTH {
            let mut cue = *cues.of(axis);
            cue.scrolled |= *state.scrolled.of(axis);

            match state.phase.of_mut(axis).settle(cue, now, self.hold) {
                Change::Retarget(target) => {
                    drive(
                        motion.as_ref(),
                        state.keys.of(axis).with("reveal"),
                        self.curves.fade,
                        target,
                        state.reveal.of_mut(axis),
                    );
                    // The host only asks for frames while a track is already
                    // moving, and this one was retargeted after its tick.
                    shell.request_redraw();
                }
                Change::WakeAt(at) => shell.request_redraw_at(at),
                Change::None => {}
            }

            // Thickness and colour follow the cursor with no hold: a pill
            // the cursor has left should thin out while it fades, not after.
            let engaged = f32::from(u8::from(cue.hovered || cue.dragging));
            let held = f32::from(u8::from(cue.dragging));

            if state.expand.of(axis).target() != engaged {
                drive(
                    motion.as_ref(),
                    state.keys.of(axis).with("expand"),
                    self.curves.expand,
                    engaged,
                    state.expand.of_mut(axis),
                );
                shell.request_redraw();
            }

            if state.press.of(axis).target() != held {
                drive(
                    motion.as_ref(),
                    state.keys.of(axis).with("press"),
                    self.curves.press,
                    held,
                    state.press.of_mut(axis),
                );
                shell.request_redraw();
            }
        }

        state.scrolled = Axes::default();
    }

    /// Points both axes at what `event` asked for, and reports the offset
    /// the content should be drawn at now — `None` when that is where the
    /// scrollable already is.
    ///
    /// The scrollable below has spent the event by the time this runs, so
    /// the offset it sits at *is* the request: whatever moved it — the wheel
    /// it has just spent, a finger, an application's own `scroll_to`, a
    /// nested scrollable declining the wheel and leaving it to this one —
    /// arrives here as a distance from wherever this widget last left it.
    /// Reading it back rather than taking the wheel away from the widgets
    /// below is what keeps a scrollable inside this one scrolling.
    fn aim(
        &self,
        state: &mut State,
        seen: Axes<Reach>,
        dragged: Option<AbsoluteOffset<Option<f32>>>,
        event: &Event,
        shell: &mut Shell<'_, Message>,
    ) -> Option<AbsoluteOffset<Option<f32>>> {
        let mut to = AbsoluteOffset { x: None, y: None };

        for axis in Axis::BOTH {
            let reach = *seen.of(axis);
            let limit = (reach.content - reach.viewport).max(0.0);
            let moved = reach.offset - *state.left_at.of(axis);
            let travelling = self.smooth && travels(event);
            let key = state.keys.of(axis).with("scroll");
            let at = state.offset(axis);

            // A drag is the pill under the cursor and nothing else. A turn of
            // the wheel is a distance to add to wherever the last one was
            // headed. Anything else is already where it belongs.
            let aim = match dragged.and_then(|dragged| axis.component(dragged)) {
                Some(to) => Some((to, false)),
                None if moved == 0.0 => None,
                None if travelling => {
                    // Adding to the target is what makes a flurry of notches
                    // add up rather than each one losing what the last had
                    // left to travel. That only holds while the two agree on
                    // a direction: a turn back the other way has to answer
                    // from where the content is, not spend the rest of a
                    // travel going the wrong way first.
                    let reversed = (*state.target.of(axis) - at) * moved < 0.0;
                    let from = if reversed { at } else { *state.target.of(axis) };

                    // A turn that came in on the heels of the last one is the
                    // same gesture, and worth a little more than it was. One
                    // after a pause, or one the other way, starts again.
                    let series =
                        !reversed
                            && state.clock.zip(*state.turned_at.of(axis)).is_some_and(
                                |(now, last)| now.saturating_duration_since(last) <= SERIES,
                            );

                    let gain = if series {
                        (*state.gain.of(axis) * GROWTH).min(self.fastest)
                    } else {
                        1.0
                    };

                    *state.gain.of_mut(axis) = gain;
                    *state.turned_at.of_mut(axis) = state.clock;

                    Some((from + moved * gain, true))
                }
                None => Some((reach.offset, false)),
            };

            if let Some((target, travel)) = aim {
                let target = target.clamp(0.0, limit);

                if travel {
                    if state.heading(axis) != target {
                        match self.glide_spring() {
                            // Retargeted in flight, a glide keeps its
                            // velocity: a flurry of notches is one motion.
                            Some(params) => match state.glide.of_mut(axis) {
                                Some(glide) => glide.set_target(target),
                                glide @ None => {
                                    let mut spring = Spring::new(params, at);
                                    spring.set_target(target);
                                    *glide = Some(spring);
                                    *state.glided_at.of_mut(axis) = None;
                                }
                            },
                            None => drive(
                                self.motion.as_ref(),
                                key,
                                self.curves.scroll,
                                target,
                                state.scroll.of_mut(axis),
                            ),
                        }

                        // A glide ticks only on the frames it asks for, and
                        // the host only produces frames while a track is
                        // already moving: this one was retargeted after its
                        // tick — or started from nothing.
                        shell.request_redraw();
                    }
                } else if at != target || state.heading(axis) != target {
                    *state.glide.of_mut(axis) = None;
                    jump(
                        self.motion.as_ref(),
                        key,
                        self.curves.scroll,
                        target,
                        state.scroll.of_mut(axis),
                    );
                }

                *state.target.of_mut(axis) = target;
            }

            // Whole pixels: the scrollable rounds its translation anyway, and
            // writing what it will round to is what lets the next pass tell
            // its own offset from one somebody else moved.
            let at = state.offset(axis).clamp(0.0, limit).round();

            if at != reach.offset {
                axis.aim_at(&mut to, at);
            }
        }

        (to.x.is_some() || to.y.is_some()).then_some(to)
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for FadingScrollable<'_, Message, Theme, Renderer>
where
    Theme: Catalog + scrollable::Catalog,
    Renderer: text::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(self.motion.as_ref(), self.curves))
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(self.as_widget())]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.children[0].diff(self.as_widget());
    }

    fn size(&self) -> Size<Length> {
        self.inner().size()
    }

    fn size_hint(&self) -> Size<Length> {
        self.inner().size_hint()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.inner_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.inner().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );

        let Some(visible) = layout.bounds().intersection(viewport) else {
            return;
        };

        let state = tree.state.downcast_ref::<State>();
        let colours = <Theme as Catalog>::style(theme, &self.class);
        let lanes = self.lanes(layout);

        renderer.with_layer(visible, |renderer| {
            for axis in Axis::BOTH {
                let Some(gutter) = *lanes.of(axis) else {
                    continue;
                };

                let (reveal, expand, press) = state.values(axis);
                let look = bar_style(&colours, reveal, expand, press);

                if let Some(background) = look.background {
                    renderer.fill_quad(
                        renderer::Quad {
                            bounds: gutter,
                            border: look.border,
                            ..renderer::Quad::default()
                        },
                        background,
                    );
                }

                let reach = *state.reach.of(axis);
                let Some(span) = pill_span(
                    self.bar,
                    axis,
                    axis.length(gutter),
                    reach.viewport,
                    reach.content,
                    reach.offset,
                ) else {
                    continue;
                };

                let bounds = pill(
                    self.bar,
                    axis,
                    gutter,
                    span,
                    thickness(self.rest, self.hover, expand),
                );

                renderer.fill_quad(
                    renderer::Quad {
                        bounds,
                        border: look.scroller.border,
                        ..renderer::Quad::default()
                    },
                    look.scroller.background,
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let lanes = self.lanes(layout);
        let zones = zones(self.bar, lanes, layout.bounds());
        let state = tree.state.downcast_mut::<State>();

        let mut swallowed = false;
        let mut take_to = None;

        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.dragging = grabbed(zones, state.revealed(), cursor);

                if let Some(axis) = state.dragging {
                    let gutter = lanes.of(axis).expect("grabbed means a lane");
                    let reach = *state.reach.of(axis);
                    state.grabbed_at = grip(self.bar, axis, &gutter, reach, cursor);

                    // A press on the rail takes the pill there at once, not
                    // on the first movement after it: a click with a still
                    // mouse has to scroll too. On the pill itself this is
                    // where it already is.
                    take_to = cursor
                        .position()
                        .map(|at| dragged_to(self.bar, axis, gutter, reach, state.grabbed_at, at));
                }

                // The bar is ours: iced's own is zero-width and never grabs,
                // so a press anywhere in a gutter is this widget's to keep —
                // and one on a pill too faint to see belongs to nobody.
                swallowed = cursor.position().is_some_and(|at| {
                    Axis::BOTH
                        .into_iter()
                        .any(|axis| zones.of(axis).is_some_and(|zone| zone.contains(at)))
                });

                if swallowed {
                    shell.capture_event();
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.dragging = None;
            }
            Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(axis) = state.dragging
                    && let Some(gutter) = lanes.of(axis)
                {
                    take_to = Some(dragged_to(
                        self.bar,
                        axis,
                        *gutter,
                        *state.reach.of(axis),
                        state.grabbed_at,
                        *position,
                    ));
                    swallowed = true;
                    shell.capture_event();
                }
            }
            _ => {}
        }

        let cues = cues(self.bar, self.axes, state.dragging, event, layout, cursor);

        for axis in Axis::BOTH {
            *state.scrolled.of_mut(axis) |= cues.of(axis).scrolled;
        }

        // Nothing moves without a frame, and nothing else on the page need
        // be producing them: the cursor arriving at a gutter, or a pill
        // being let go, changes what the bar should be doing and so has to
        // ask for the frame that will do it. Without this the bar waits for
        // an unrelated redraw — a scroll, someone else's animation — and so
        // lights up and settles only sometimes.
        let moved = Axis::BOTH
            .into_iter()
            .any(|axis| cues.of(axis).pose() != *state.pose.of(axis));

        if moved {
            for axis in Axis::BOTH {
                *state.pose.of_mut(axis) = cues.of(axis).pose();
            }

            shell.request_redraw();
        }

        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            self.spend(tree.state.downcast_mut::<State>(), cues, *now, shell);
        }

        if !swallowed {
            self.inner_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        // Last, so it sees whatever this event did to the offset — the
        // wheel the scrollable just spent, or the drag taken above.
        let seen = self.reach(tree, layout, renderer, None);
        let to = self.aim(
            tree.state.downcast_mut::<State>(),
            seen,
            take_to,
            event,
            shell,
        );

        let reach = match to {
            Some(to) => self.reach(tree, layout, renderer, Some(to)),
            None => seen,
        };

        let state = tree.state.downcast_mut::<State>();
        state.reach = reach;

        for axis in Axis::BOTH {
            *state.left_at.of_mut(axis) = reach.of(axis).offset;
        }

        // A write the scrollable has taken is one it has not published yet:
        // it notices a viewport that has moved on the frame after, so the
        // frame after is what has to come. Without this the offset the travel
        // ends on is the one `on_scroll` never hears about.
        if to.is_some() {
            shell.request_redraw();
        }
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        self.inner_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.inner()
            .mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.inner_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a, Message, Theme, Renderer> From<FadingScrollable<'a, Message, Theme, Renderer>>
    for Element<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: Catalog + scrollable::Catalog + 'a,
    Renderer: text::Renderer + 'a,
{
    fn from(value: FadingScrollable<'a, Message, Theme, Renderer>) -> Self {
        Element::new(value)
    }
}

/// Wraps `content` in a [`FadingScrollable`].
#[must_use]
pub fn fading_scrollable<'a, Message, Theme, Renderer>(
    content: impl Into<Element<'a, Message, Theme, Renderer>>,
) -> FadingScrollable<'a, Message, Theme, Renderer>
where
    Theme: Catalog + scrollable::Catalog + 'a,
    <Theme as scrollable::Catalog>::Class<'a>: From<scrollable::StyleFn<'a, Theme>>,
    Renderer: text::Renderer,
{
    FadingScrollable::new(content)
}
