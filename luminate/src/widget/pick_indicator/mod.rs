//! A ring with a dot inside, for showing a pick and the wait for its confirmation.

use std::fmt;
use std::time::Duration;

use iced::advanced::widget::{Tree, tree};
use iced::advanced::{Layout, Widget, layout, mouse, renderer};
use iced::{Background, Border, Color, Length, Rectangle, Size, border};
use iced_animate::{Anim, Curve, Easing, Motion, MotionKey, Repeat, curves};

use crate::theme::PickIndicatorTheme;
use crate::{Element, Renderer, Theme};

/// The state a pick indicator shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum Pick {
    /// Not picked: an empty ring.
    #[default]
    Off,
    /// Picked, but not confirmed yet.
    Pending,
    /// Picked.
    On,
}

impl Pick {
    /// The state of a row from three facts about it.
    ///
    /// While any row is pending, the previous pick goes dark, so that two
    /// rows never look picked at once.
    #[must_use]
    pub fn resolve(selected: bool, pending: bool, any_pending: bool) -> Self {
        match (pending, selected && !any_pending) {
            (true, _) => Self::Pending,
            (false, true) => Self::On,
            _ => Self::Off,
        }
    }

    fn dot_scale(self) -> f32 {
        match self {
            Self::Off => 0.0,
            Self::Pending => PENDING_SCALE,
            Self::On => 1.0,
        }
    }

    fn fill_amount(self) -> f32 {
        match self {
            Self::Off | Self::Pending => 0.0,
            Self::On => 1.0,
        }
    }
}

const PENDING_SCALE: f32 = 0.55;

const PENDING_ALPHA: f32 = 0.35;

/// How long a pending state lasts before the dot starts to breathe.
pub const BREATHE_DELAY: Duration = Duration::from_millis(300);

/// Played from 1.0 to `PENDING_ALPHA` and back, so the alpha rests at 1.0 and
/// breathes between the two.
/// The delay keeps it still at first: a fast operation must not flash.
const BREATHE: Curve = Curve::ease(Easing::EaseInOut, Duration::from_millis(700))
    .repeat(Repeat::forever().alternate())
    .delayed(BREATHE_DELAY);

const FILL_CURVE: Curve = curves::sharp::QUICK;

/// Colors of the indicator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Color of the ring.
    pub ring: Color,
    /// Color of the dot.
    pub dot: Color,
    /// Color of the fill.
    pub fill: Color,
}

/// Dimensions of the indicator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Metrics {
    /// Diameter of the outer circle.
    pub size: f32,
    /// Diameter of the inner circle.
    pub dot_size: f32,
    /// Width of the outline.
    pub ring_width: f32,
}

impl From<PickIndicatorTheme> for Metrics {
    fn from(tokens: PickIndicatorTheme) -> Self {
        Self {
            size: tokens.size,
            dot_size: tokens.dot_size,
            ring_width: tokens.ring_width,
        }
    }
}

/// The metrics of the light theme. Every look shares them, so the tokens stay
/// the only place the numbers are written down.
impl Default for Metrics {
    fn default() -> Self {
        Theme::LIGHT.pick_indicator.into()
    }
}

/// Animations of an indicator. Lives in the tree of whatever draws it.
#[derive(Debug)]
pub struct State {
    key: MotionKey,
    pick: Pick,
    scale: Anim<f32>,
    alpha: Anim<f32>,
    fill: Anim<f32>,
}

impl State {
    /// An indicator that stands in `pick` at once, without an appearing animation.
    #[must_use]
    pub fn new(pick: Pick, motion: Option<&Motion>) -> Self {
        let key = MotionKey::unique();

        // `Motion::to` on a new key starts the track at its target, so a row
        // scrolled into view does not fill up before the eyes.
        let (scale, alpha, fill) = match motion {
            Some(motion) => {
                let scale = motion.to(key.with("scale"), curves::QUICK, pick.dot_scale());
                let alpha = if pick == Pick::Pending {
                    // `to` on a new key would stand still, and `play` restarts the
                    // track on every call: fine here, `new` runs once per state.
                    motion.play(key.with("alpha"), BREATHE, 1.0, PENDING_ALPHA)
                } else {
                    motion.to(key.with("alpha"), curves::QUICK, 1.0)
                };

                let fill = motion.to(key.with("fill"), FILL_CURVE, pick.fill_amount());

                (scale, alpha, fill)
            }
            None => (
                Anim::constant(pick.dot_scale()),
                Anim::constant(1.0),
                Anim::constant(pick.fill_amount()),
            ),
        };

        Self {
            key,
            pick,
            scale,
            alpha,
            fill,
        }
    }

    /// Moves the animations to a new state. Does nothing for the same `pick`, so it can run on every rebuild.
    pub fn retarget(&mut self, pick: Pick, motion: Option<&Motion>) {
        if pick == self.pick {
            return;
        }

        self.pick = pick;

        let Some(motion) = motion else {
            self.scale = Anim::constant(pick.dot_scale());
            self.alpha = Anim::constant(1.0);
            self.fill = Anim::constant(pick.fill_amount());
            return;
        };

        self.scale = motion.to(self.key.with("scale"), curves::QUICK, pick.dot_scale());

        self.alpha = if pick == Pick::Pending {
            motion.to(self.key.with("alpha"), BREATHE, PENDING_ALPHA)
        } else {
            motion.to(self.key.with("alpha"), curves::QUICK, 1.0)
        };

        self.fill = motion.to(self.key.with("fill"), FILL_CURVE, pick.fill_amount());
    }

    /// The state the animations head to.
    #[must_use]
    pub fn pick(&self) -> Pick {
        self.pick
    }
}

/// Draws the indicator centered in `bounds`.
pub fn draw(
    renderer: &mut Renderer,
    bounds: Rectangle,
    state: &State,
    metrics: Metrics,
    style: Style,
) {
    use iced::advanced::Renderer as _;

    let size = metrics.size.min(bounds.width).min(bounds.height);
    let center = bounds.center();
    let square = |side: f32| Rectangle {
        x: center.x - side / 2.0,
        y: center.y - side / 2.0,
        width: side,
        height: side,
    };

    let fill = style.fill.scale_alpha(state.fill.get().clamp(0.0, 1.0));
    renderer.fill_quad(
        renderer::Quad {
            bounds: square(size),
            border: Border {
                color: style.ring,
                width: metrics.ring_width,
                radius: (size / 2.0).into(),
            },
            snap: false,
            ..renderer::Quad::default()
        },
        Background::Color(fill),
    );

    let diameter = (metrics.dot_size * state.scale.get().max(0.0)).min(size);

    if diameter > 0.5 {
        renderer.fill_quad(
            renderer::Quad {
                bounds: square(diameter),
                border: border::rounded(diameter / 2.0),
                snap: false,
                ..renderer::Quad::default()
            },
            Background::Color(style.dot.scale_alpha(state.alpha.get().clamp(0.0, 1.0))),
        );
    }
}

catalog!(status: Pick, |theme, pick| {
    let palette = theme.extended_palette();
    let accent = palette.primary.base.color;

    Style {
        ring: if pick == Pick::Off { palette.background.strong.color } else { accent },
        dot: Color::WHITE,
        fill: accent
    }
});

/// A standalone pick indicator.
pub struct PickIndicator<'a> {
    pick: Pick,
    metrics: Metrics,
    motion: Option<Motion>,
    class: <Theme as Catalog>::Class<'a>,
}

impl<'a> PickIndicator<'a> {
    /// An indicator in state `pick`.
    #[must_use]
    pub fn new(pick: Pick) -> Self {
        Self {
            pick,
            metrics: Metrics::default(),
            motion: None,
            class: <Theme as Catalog>::default(),
        }
    }

    /// Dimensions of the indicator.
    #[must_use]
    pub fn metrics(mut self, metrics: Metrics) -> Self {
        self.metrics = metrics;
        self
    }

    /// Animates changes of state through `motion`. Without it, changes are instant.
    #[must_use]
    pub fn motion(mut self, motion: Motion) -> Self {
        self.motion = Some(motion);
        self
    }

    /// Style function in place of the theme's style.
    #[must_use]
    pub fn style(mut self, style: impl Fn(&Theme, Pick) -> Style + 'a) -> Self {
        self.class = Box::new(style);
        self
    }
}

impl<Message> Widget<Message, Theme, Renderer> for PickIndicator<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::new(self.pick, self.motion.as_ref()))
    }

    fn diff(&self, tree: &mut Tree) {
        tree.state
            .downcast_mut::<State>()
            .retarget(self.pick, self.motion.as_ref());
    }

    fn size(&self) -> Size<Length> {
        Size::new(
            Length::Fixed(self.metrics.size),
            Length::Fixed(self.metrics.size),
        )
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::Node::new(limits.resolve(
            Length::Fixed(self.metrics.size),
            Length::Fixed(self.metrics.size),
            Size::ZERO,
        ))
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_ref::<State>();
        let style = theme.style(&self.class, self.pick);

        draw(renderer, layout.bounds(), state, self.metrics, style);
    }
}

impl fmt::Debug for PickIndicator<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PickIndicator")
            .field("pick", &self.pick)
            .field("metrics", &self.metrics)
            .finish_non_exhaustive()
    }
}

impl<'a, Message> From<PickIndicator<'a>> for Element<'a, Message> {
    fn from(indicator: PickIndicator<'a>) -> Self {
        Element::new(indicator)
    }
}

/// An indicator in state `pick`.
#[must_use]
pub fn pick_indicator<'a>(pick: Pick) -> PickIndicator<'a> {
    PickIndicator::new(pick)
}

#[cfg(test)]
mod tests {
    use iced_animate::Motion;
    use iced_animate::testing::FrameClock;

    use super::{BREATHE_DELAY, Metrics, PENDING_ALPHA, PENDING_SCALE, Pick, State};
    use crate::Theme;

    #[test]
    fn pending_takes_the_dot_from_the_selected_row() {
        assert_eq!(Pick::resolve(true, false, false), Pick::On);
        assert_eq!(Pick::resolve(true, false, true), Pick::Off);
        assert_eq!(Pick::resolve(false, true, true), Pick::Pending);
        assert_eq!(Pick::resolve(true, true, true), Pick::Pending);
        assert_eq!(Pick::resolve(false, false, false), Pick::Off);
    }

    #[test]
    fn pending_grows_the_dot_at_once_and_breathes_only_after_the_delay() {
        let motion = Motion::new();
        let mut clock = FrameClock::new(&motion);
        let mut state = State::new(Pick::Off, Some(&motion));
        let _ = clock.run(1);

        state.retarget(Pick::Pending, Some(&motion));
        let _ = clock.run(13);

        assert!(
            (state.scale.get() - PENDING_SCALE).abs() < 0.05,
            "scale = {}",
            state.scale.get()
        );
        assert_eq!(state.alpha.get(), 1.0);

        let frames = (BREATHE_DELAY.as_millis() / 16) as usize + 15;
        let _ = clock.run(frames);
        let alpha = state.alpha.get();
        assert!((PENDING_ALPHA..1.0).contains(&alpha), "alpha = {alpha}");

        assert_eq!(state.fill.get(), 0.0);

        state.retarget(Pick::On, Some(&motion));
        let _ = clock.run(120);
        assert!((state.scale.get() - 1.0).abs() < 0.01);
        assert!((state.alpha.get() - 1.0).abs() < 0.01);
        assert!((state.fill.get() - 1.0).abs() < 0.01);
    }

    #[test]
    fn default_metrics_come_from_the_tokens() {
        let tokens = Theme::LIGHT.pick_indicator;
        let metrics = Metrics::default();

        assert_eq!(metrics.size, tokens.size);
        assert_eq!(metrics.dot_size, tokens.dot_size);
        assert_eq!(metrics.ring_width, tokens.ring_width);
        assert_eq!(Theme::DARK.pick_indicator.size, tokens.size);
    }
}
