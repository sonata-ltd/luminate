//! Rounded progress bars with the classic macOS indeterminate animation.
//!
//! Like iced's bar, this widget accepts a range and value, `length`, `girth`,
//! `vertical`, `style` and `class`. Text belongs to the Luminate wrapper.

use std::{fmt, ops::RangeInclusive};

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, renderer};
use iced::{Background, Border, Color, Element, Event, Length, Rectangle, Size, gradient, window};
use iced_animate::{Anim, Curve, Motion, MotionKey, curves};

/// Paint used by a progress bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Unfilled track.
    pub background: Background,
    /// Normal fill.
    pub bar: Background,
    /// Error fill.
    pub error: Background,
    /// Opaque midpoint of each moving highlight.
    pub highlight: Color,
    /// Track outline and corner radius.
    pub border: Border,
}

catalog!(|theme| {
    let palette = theme.extended_palette();
    Style {
        background: palette.background.strong.color.into(),
        bar: palette.primary.base.color.into(),
        error: palette.danger.base.color.into(),
        highlight: palette.primary.weak.color,
        border: iced::border::rounded(3),
    }
});

/// A progress bar. Defaults to a horizontal, determinate bar.
pub struct ProgressBar<'a, Theme = crate::Theme>
where
    Theme: Catalog,
{
    range: RangeInclusive<f32>,
    value: f32,
    length: Length,
    girth: Length,
    vertical: bool,
    indeterminate: bool,
    errored: bool,
    smoothed: bool,
    smoothing_curve: Curve,
    motion: Option<Motion>,
    class: Theme::Class<'a>,
}

impl<'a, Theme: Catalog> ProgressBar<'a, Theme> {
    /// Default thickness in logical pixels.
    pub const DEFAULT_GIRTH: f32 = 6.0;

    /// Creates a bar with the given inclusive range and current value.
    /// Values outside the range are clamped; invalid ranges are empty.
    #[must_use]
    pub fn new(range: RangeInclusive<f32>, value: f32) -> Self {
        Self {
            range,
            value,
            length: Length::Fill,
            girth: Self::DEFAULT_GIRTH.into(),
            vertical: false,
            indeterminate: false,
            errored: false,
            smoothed: false,
            smoothing_curve: curves::QUICK,
            motion: None,
            class: Theme::default(),
        }
    }

    /// Sets the length along the direction of progress (default `Fill`).
    #[must_use]
    pub fn length(mut self, length: impl Into<Length>) -> Self {
        self.length = length.into();
        self
    }

    /// Sets the thickness.
    #[must_use]
    pub fn girth(mut self, girth: impl Into<Length>) -> Self {
        self.girth = girth.into();
        self
    }

    /// Makes progress travel from bottom to top.
    #[must_use]
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// Enables the CSS reference's three highlights (2 s, delays 0/1/2 s).
    /// Uses iced's redraw clock; no application subscription is needed.
    #[must_use]
    pub fn indeterminate(mut self, enabled: bool) -> Self {
        self.indeterminate = enabled;
        self
    }

    /// Uses the error fill and stops indeterminate animation.
    /// The filled fraction still comes from the range and value.
    #[must_use]
    pub fn errored(mut self, enabled: bool) -> Self {
        self.errored = enabled;
        self
    }

    /// Smooths changes of the filled fraction using [`curves::QUICK`], a brisk
    /// spring without bounce. Requires [`Self::motion`] and its host.
    /// The initial value is immediate; retargeting preserves spring velocity.
    #[must_use]
    pub fn smoothed(mut self) -> Self {
        self.smoothed = true;
        self
    }

    /// Sets the curve used by [`Self::smoothed`]. Does not enable smoothing.
    #[must_use]
    pub fn smoothing_curve(mut self, curve: Curve) -> Self {
        self.smoothing_curve = curve;
        self
    }

    /// Supplies the animation engine. Wrap the root in its [`Motion::host`].
    /// The Luminate wrapper supplies its shared engine automatically.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    fn sync(&self, state: &mut State) {
        if !self.animated() {
            state.started = None;
            state.elapsed = 0.0;
        }
        let motion = self
            .motion
            .as_ref()
            .filter(|_| self.smoothed && !self.animated());
        state.retarget(
            fraction(&self.range, self.value),
            motion,
            self.smoothing_curve,
        );
    }

    /// Sets a custom style function, as in iced's progress bar.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self
    where
        Theme::Class<'a>: From<StyleFn<'a, Theme>>,
    {
        self.class = (Box::new(style) as StyleFn<'a, Theme>).into();
        self
    }

    /// Sets the theme's style class.
    #[must_use]
    pub fn class(mut self, class: impl Into<Theme::Class<'a>>) -> Self {
        self.class = class.into();
        self
    }

    fn animated(&self) -> bool {
        self.indeterminate && !self.errored
    }

    fn dimensions(&self) -> Size<Length> {
        if self.vertical {
            Size::new(self.girth, self.length)
        } else {
            Size::new(self.length, self.girth)
        }
    }
}

#[derive(Debug)]
struct State {
    started: Option<iced::time::Instant>,
    elapsed: f32,
    progress: Anim<f32>,
    progress_key: MotionKey,
    smoothing: bool,
}

impl State {
    fn new() -> Self {
        Self {
            started: None,
            elapsed: 0.0,
            progress: Anim::constant(0.0),
            progress_key: MotionKey::unique(),
            smoothing: false,
        }
    }

    fn retarget(&mut self, target: f32, motion: Option<&Motion>, curve: Curve) {
        if let Some(motion) = motion {
            if !self.smoothing {
                // A fresh track starts at the received value, including when
                // smoothing is enabled again after an instant/indeterminate bar.
                self.progress_key = MotionKey::unique();
            }
            self.progress = motion.to(self.progress_key, curve, target);
            self.smoothing = true;
        } else {
            self.progress = Anim::constant(target);
            self.smoothing = false;
        }
    }
}

/// Fraction shared by the bar and the wrapper's percentage label.
pub(crate) fn fraction(range: &RangeInclusive<f32>, value: f32) -> f32 {
    let (start, end) = (*range.start(), *range.end());
    if !start.is_finite() || !end.is_finite() || start >= end || value.is_nan() {
        return 0.0;
    }
    // Compute in f64 so even a finite range spanning f32's extremes works.
    ((f64::from(value.clamp(start, end)) - f64::from(start)) / (f64::from(end) - f64::from(start)))
        as f32
}

// Before a CSS animation's positive delay expires, left:auto leaves the
// absolutely positioned layer at its static position, covering the track.
fn offset(elapsed: f32, layer: u8) -> f32 {
    let elapsed = elapsed - f32::from(layer);
    if elapsed < 0.0 {
        0.0
    } else {
        elapsed.rem_euclid(2.0) - 1.0
    }
}

// Express the translated gradient on the track's bounds. This preserves the
// parent's rounded clipping without a texture or a rectangular scissor seam.
fn highlight(offset: f32, color: Color, vertical: bool) -> Background {
    let alpha = |position: f32| (1.0 - ((position - offset) * 2.0 - 1.0).abs()).max(0.0);
    let mut gradient = gradient::Linear::new(if vertical {
        0.0
    } else {
        std::f32::consts::FRAC_PI_2
    });
    let mut stops = [0.0, offset, offset + 0.5, offset + 1.0, 1.0];
    stops.sort_by(f32::total_cmp);
    for position in stops {
        if (0.0..=1.0).contains(&position) {
            gradient = gradient.add_stop(position, color.scale_alpha(alpha(position)));
        }
    }
    Background::Gradient(gradient.into())
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for ProgressBar<'_, Theme>
where
    Theme: Catalog,
    Renderer: iced::advanced::Renderer,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        let mut state = State::new();
        self.sync(&mut state);
        tree::State::new(state)
    }

    fn diff(&self, tree: &mut Tree) {
        self.sync(tree.state.downcast_mut::<State>());
    }

    fn size(&self) -> Size<Length> {
        self.dimensions()
    }

    fn layout(&mut self, _: &mut Tree, _: &Renderer, limits: &layout::Limits) -> layout::Node {
        let size = self.dimensions();
        layout::atomic(limits, size.width, size.height)
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        _: mouse::Cursor,
        _: &Renderer,
        _: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        if !self.animated() || layout.bounds().intersection(viewport).is_none() {
            return;
        }
        if let Event::Window(window::Event::RedrawRequested(now)) = event {
            let state = tree.state.downcast_mut::<State>();
            let start = *state.started.get_or_insert(*now);
            state.elapsed = now.saturating_duration_since(start).as_secs_f32();
        }
        shell.request_redraw();
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _: &renderer::Style,
        layout: Layout<'_>,
        _: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();
        let Some(visible) = bounds.intersection(viewport) else {
            return;
        };
        let style = theme.style(&self.class);
        let quad = |bounds, border| renderer::Quad {
            bounds,
            border,
            snap: false,
            ..renderer::Quad::default()
        };
        renderer.with_layer(visible, |renderer| {
            renderer.fill_quad(quad(bounds, style.border), style.background);
            let amount = if self.animated() {
                1.0
            } else {
                tree.state
                    .downcast_ref::<State>()
                    .progress
                    .get()
                    .clamp(0.0, 1.0)
            };
            if amount <= 0.0 {
                return;
            }
            let fill = if self.vertical {
                Rectangle {
                    y: bounds.y + bounds.height * (1.0 - amount),
                    height: bounds.height * amount,
                    ..bounds
                }
            } else {
                Rectangle {
                    width: bounds.width * amount,
                    ..bounds
                }
            };
            let border = Border {
                width: 0.0,
                ..style.border
            };
            renderer.fill_quad(
                quad(fill, border),
                if self.errored { style.error } else { style.bar },
            );
            if self.animated() {
                let elapsed = tree.state.downcast_ref::<State>().elapsed;
                for layer in 0..3 {
                    renderer.fill_quad(
                        quad(bounds, border),
                        highlight(offset(elapsed, layer), style.highlight, self.vertical),
                    );
                }
            }
        });
    }
}

impl<Theme: Catalog> fmt::Debug for ProgressBar<'_, Theme> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressBar")
            .field("range", &self.range)
            .field("value", &self.value)
            .field("indeterminate", &self.indeterminate)
            .field("errored", &self.errored)
            .finish_non_exhaustive()
    }
}

impl<'a, Message: 'a, Theme: Catalog + 'a, Renderer: iced::advanced::Renderer + 'a>
    From<ProgressBar<'a, Theme>> for Element<'a, Message, Theme, Renderer>
{
    fn from(bar: ProgressBar<'a, Theme>) -> Self {
        Self::new(bar)
    }
}

/// Creates a determinate progress bar, as in iced's helper.
#[must_use]
pub fn progress_bar<'a, Theme: Catalog>(
    range: RangeInclusive<f32>,
    value: f32,
) -> ProgressBar<'a, Theme> {
    ProgressBar::new(range, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_animate::testing::FrameClock;

    #[test]
    fn smoothing_starts_at_the_initial_value_and_retargets_mid_flight() {
        let motion = Motion::new();
        let mut clock = FrameClock::new(&motion);
        let mut state = State::new();
        state.retarget(0.3, Some(&motion), curves::QUICK);
        assert_eq!(state.progress.get(), 0.3);
        let _ = clock.run(1);
        state.retarget(0.6, Some(&motion), curves::QUICK);
        assert_eq!(state.progress.get(), 0.3);
        let _ = clock.run(4);
        let halfway = state.progress.get();
        assert!(halfway > 0.3 && halfway < 0.6, "{halfway}");

        // An unchanged rebuild must not restart the spring.
        state.retarget(0.6, Some(&motion), curves::QUICK);
        assert_eq!(state.progress.get(), halfway);
        // An update before settling starts from the currently painted value.
        state.retarget(0.9, Some(&motion), curves::QUICK);
        assert_eq!(state.progress.get(), halfway);
        let _ = clock.run(30);
        assert!((state.progress.get() - 0.9).abs() < 0.001);

        state.retarget(0.1, Some(&motion), curves::QUICK);
        let _ = clock.run(4);
        assert!(state.progress.get() > 0.1 && state.progress.get() < 0.9);
        let _ = clock.run_until_settled();
        assert!((state.progress.get() - 0.1).abs() < 0.001);
    }

    #[test]
    fn custom_curve_controls_timing_and_disabling_smoothing_snaps() {
        let motion = Motion::new();
        let mut clock = FrameClock::new(&motion);
        let curve = Curve::ease(
            iced_animate::Easing::Linear,
            std::time::Duration::from_secs(1),
        );
        let mut state = State::new();
        state.retarget(0.3, Some(&motion), curve);
        let _ = clock.run(1);
        state.retarget(0.6, Some(&motion), curve);
        let _ = clock.run(16);
        assert!((state.progress.get() - 0.375).abs() < 0.01);

        state.retarget(0.8, None, curve);
        assert_eq!(state.progress.get(), 0.8);
        // Enabling again must not revive the previous track at its old value.
        state.retarget(0.8, Some(&motion), curve);
        assert_eq!(state.progress.get(), 0.8);
        state.retarget(0.0, Some(&motion), curve);
        let _ = clock.run_until_settled();
        assert_eq!(state.progress.get(), 0.0);
    }

    #[test]
    fn smoothing_is_opt_in_and_indeterminate_mode_does_not_start_a_spring() {
        let motion = Motion::new();
        let mut state = State::new();
        let make =
            |value| ProgressBar::<crate::Theme>::new(0.0..=100.0, value).motion(motion.clone());
        make(30.0).sync(&mut state);
        make(60.0).sync(&mut state);
        assert_eq!(state.progress.get(), 0.6);
        assert!(!state.smoothing);
        make(60.0).smoothed().indeterminate(true).sync(&mut state);
        assert!(!state.smoothing);
        make(60.0).smoothed().sync(&mut state);
        assert_eq!(state.progress.get(), 0.6);
        make(90.0).smoothed().errored(true).sync(&mut state);
        assert_eq!(state.progress.get(), 0.6);
        assert!(state.smoothing);
    }

    #[test]
    fn animation_survives_rebuilds_and_only_visible_running_bars_request_frames() {
        use iced::advanced::clipboard;
        use iced::time::{Duration, Instant};
        type Bar = ProgressBar<'static, crate::Theme>;
        type TestWidget = dyn Widget<(), crate::Theme, crate::Renderer>;

        let renderer = iced_texture_cache::testing::headless_tiny_skia();
        let mut bar = Bar::new(0.0..=100.0, 30.0).indeterminate(true);
        let mut tree = Tree::new(&bar as &TestWidget);
        let node = layout::Node::new(Size::new(300.0, 6.0));
        let viewport = Rectangle::with_size(Size::new(300.0, 30.0));
        let start = Instant::now();
        let deliver = |bar: &mut Bar, tree: &mut Tree, now, viewport: &Rectangle| {
            let mut messages: Vec<()> = Vec::new();
            let mut shell = Shell::new(&mut messages);
            bar.update(
                tree,
                &Event::Window(window::Event::RedrawRequested(now)),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut clipboard::Null,
                &mut shell,
                viewport,
            );
            assert!(!shell.is_event_captured());
            shell.redraw_request()
        };
        assert_eq!(
            deliver(&mut bar, &mut tree, start, &viewport),
            window::RedrawRequest::NextFrame
        );
        let rebuilt = Bar::new(0.0..=100.0, 50.0).indeterminate(true);
        <Bar as Widget<(), crate::Theme, crate::Renderer>>::diff(&rebuilt, &mut tree);
        bar = rebuilt;
        let _ = deliver(
            &mut bar,
            &mut tree,
            start + Duration::from_millis(750),
            &viewport,
        );
        assert_eq!(tree.state.downcast_ref::<State>().elapsed, 0.75);
        let hidden = Rectangle {
            y: 50.0,
            ..viewport
        };
        assert_eq!(
            deliver(&mut bar, &mut tree, start, &hidden),
            window::RedrawRequest::Wait
        );
        bar = bar.errored(true);
        <Bar as Widget<(), crate::Theme, crate::Renderer>>::diff(&bar, &mut tree);
        assert_eq!(
            deliver(&mut bar, &mut tree, start, &viewport),
            window::RedrawRequest::Wait
        );
        assert!(tree.state.downcast_ref::<State>().started.is_none());
        bar = bar.errored(false).indeterminate(false);
        assert_eq!(
            deliver(&mut bar, &mut tree, start, &viewport),
            window::RedrawRequest::Wait
        );
    }

    #[test]
    fn values_are_normalized_without_overflow_or_invalid_geometry() {
        assert_eq!(fraction(&(20.0..=60.0), 30.0), 0.25);
        assert_eq!(fraction(&(20.0..=60.0), -100.0), 0.0);
        assert_eq!(fraction(&(20.0..=60.0), f32::INFINITY), 1.0);
        assert_eq!(fraction(&(20.0..=20.0), 20.0), 0.0);
        assert_eq!(fraction(&RangeInclusive::new(60.0, 20.0), 30.0), 0.0);
        assert_eq!(fraction(&(0.0..=100.0), f32::NAN), 0.0);
        assert_eq!(fraction(&(-f32::MAX..=f32::MAX), 0.0), 0.5);
    }

    #[test]
    fn css_delays_and_linear_two_second_cycles_include_the_startup_pose() {
        assert_eq!(
            [offset(0.0, 0), offset(0.0, 1), offset(0.0, 2)],
            [-1.0, 0.0, 0.0]
        );
        assert_eq!(
            [offset(0.5, 0), offset(0.5, 1), offset(0.5, 2)],
            [-0.5, 0.0, 0.0]
        );
        assert_eq!(
            [offset(1.0, 0), offset(1.0, 1), offset(1.0, 2)],
            [0.0, -1.0, 0.0]
        );
        assert_eq!(
            [offset(2.0, 0), offset(2.0, 1), offset(2.0, 2)],
            [-1.0, 0.0, -1.0]
        );
        for layer in 0..3 {
            assert_eq!(offset(3.25, layer), offset(5.25, layer));
        }
    }
}
