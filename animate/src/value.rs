//! What the engine can interpolate, and how a widget reads it back.

use std::marker::PhantomData;
use std::sync::Arc;

use iced_core::{Color, Padding, Pixels, Point, Radians, Rectangle, Size, Vector, border::Radius};

use crate::track::{Tier, Track};

/// Maximum number of scalar components an [`Animatable`] may decompose into.
///
/// Four covers every type this crate animates (a colour, a padding, a corner
/// radius, a rectangle); a track stores exactly this many slots inline, so it
/// never allocates. An implementation declaring more fails to compile at its
/// first use with the engine.
pub const MAX_COMPONENTS: usize = 4;

/// A value the motion engine can interpolate.
///
/// Implementors decompose into at most [`MAX_COMPONENTS`] scalars that share
/// a single curve, so the parts of a colour or a padding move together.
/// Components are interpolated without range knowledge: a bouncy spring can
/// overshoot a colour channel past `1.0` or a length below `0.0`, so
/// consumers clamp where the type demands it.
///
/// Implementations write and read only their first [`COMPONENTS`] slots;
/// the rest are ignored.
///
/// [`COMPONENTS`]: Self::COMPONENTS
pub trait Animatable: Copy + 'static {
    /// How many scalars this type occupies; at most [`MAX_COMPONENTS`].
    const COMPONENTS: usize;

    /// Writes this value's scalars into the first [`Self::COMPONENTS`] slots
    /// of `out`.
    fn write(self, out: &mut [f32; MAX_COMPONENTS]);

    /// Rebuilds a value from the first [`Self::COMPONENTS`] slots of `src`.
    fn read(src: &[f32; MAX_COMPONENTS]) -> Self;
}

impl Animatable for f32 {
    const COMPONENTS: usize = 1;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        src[0]
    }
}

impl Animatable for Pixels {
    const COMPONENTS: usize = 1;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.0;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Pixels(src[0])
    }
}

impl Animatable for Vector {
    const COMPONENTS: usize = 2;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.x;
        out[1] = self.y;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Vector::new(src[0], src[1])
    }
}

impl Animatable for Size {
    const COMPONENTS: usize = 2;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.width;
        out[1] = self.height;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Size::new(src[0], src[1])
    }
}

impl Animatable for Color {
    const COMPONENTS: usize = 4;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.r;
        out[1] = self.g;
        out[2] = self.b;
        out[3] = self.a;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Color {
            r: src[0],
            g: src[1],
            b: src[2],
            a: src[3],
        }
    }
}

impl Animatable for Padding {
    const COMPONENTS: usize = 4;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.top;
        out[1] = self.right;
        out[2] = self.bottom;
        out[3] = self.left;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Padding {
            top: src[0],
            right: src[1],
            bottom: src[2],
            left: src[3],
        }
    }
}

impl Animatable for Radius {
    const COMPONENTS: usize = 4;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.top_left;
        out[1] = self.top_right;
        out[2] = self.bottom_right;
        out[3] = self.bottom_left;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Radius {
            top_left: src[0],
            top_right: src[1],
            bottom_right: src[2],
            bottom_left: src[3],
        }
    }
}

impl Animatable for Point {
    const COMPONENTS: usize = 2;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.x;
        out[1] = self.y;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Point::new(src[0], src[1])
    }
}

impl Animatable for Rectangle {
    const COMPONENTS: usize = 4;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.x;
        out[1] = self.y;
        out[2] = self.width;
        out[3] = self.height;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Rectangle {
            x: src[0],
            y: src[1],
            width: src[2],
            height: src[3],
        }
    }
}

impl Animatable for Radians {
    const COMPONENTS: usize = 1;

    fn write(self, out: &mut [f32; MAX_COMPONENTS]) {
        out[0] = self.0;
    }

    fn read(src: &[f32; MAX_COMPONENTS]) -> Self {
        Radians(src[0])
    }
}

/// A value that may be animated, resolved at the moment it is used.
///
/// `T` converts into `Anim<T>` for free, so a call site reads identically
/// whether the value is constant or animated:
///
/// ```
/// use iced_animate::Anim;
/// let constant: Anim<f32> = 16.0.into();
/// assert_eq!(constant.get(), 16.0);
/// ```
///
/// The distinction that matters is *when* [`get`] is called. A value read
/// while building the view freezes until the next rebuild; a value read inside
/// a widget's `layout` or `draw` follows the animation frame by frame. Widgets
/// therefore store the `Anim<T>` and resolve it themselves.
///
/// # Lifetime
///
/// A handle is a reference: the engine never collects a track while an
/// `Anim` for it exists, and a track re-declared with [`Motion::to`] (or
/// read with [`get`]) in a build is kept for a few builds after. A key that
/// is neither declared nor held is collected once its track has settled; the
/// next `to` for that key then creates a fresh track sitting at its target.
/// So either store the handle (a widget's `tree::State`, for instance) or
/// re-declare the key on every rebuild, both are fine, holding nothing is
/// not. A read ([`get`]) touches the track, so a track whose last handle is
/// dropped still gets a grace period of a few builds (`GC_IDLE_BUILDS`,
/// currently 3) after the build in which it was last read.
///
/// A handle made with [`map`](Self::map) holds the same track and counts
/// as a handle on it.
///
/// [`get`]: Self::get
/// [`Motion::to`]: crate::Motion::to
#[derive(Debug, Clone)]
pub struct Anim<T> {
    inner: Inner<T>,
}

/// A projection from a track's raw slots to the value a derived handle reads.
type Project<T> = Arc<dyn Fn(&[f32; MAX_COMPONENTS]) -> T + Send + Sync>;

#[derive(Clone)]
enum Inner<T> {
    Const(T),
    Live(Arc<Track>, PhantomData<fn() -> T>),
    /// A value computed from another track's, every time it is read: a point
    /// on a path from a progress, say. See [`Anim::map`].
    Derived(Arc<Track>, Project<T>),
}

impl<T: std::fmt::Debug> std::fmt::Debug for Inner<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Const(value) => f.debug_tuple("Const").field(value).finish(),
            Self::Live(track, _) => f.debug_tuple("Live").field(track).finish(),
            Self::Derived(track, _) => f
                .debug_tuple("Derived")
                .field(track)
                .finish_non_exhaustive(),
        }
    }
}

impl<T: Animatable> Anim<T> {
    /// A value that never moves.
    #[must_use]
    pub const fn constant(value: T) -> Self {
        Self {
            inner: Inner::Const(value),
        }
    }

    pub(crate) fn live(track: Arc<Track>) -> Self {
        Self {
            inner: Inner::Live(track, PhantomData),
        }
    }

    /// Re-types a single-component handle. Both types must read and write
    /// slot `0` only, so the track is shared as-is.
    pub(crate) fn retype<U: Animatable>(
        self,
        convert: impl Fn(T) -> U + Send + Sync + 'static,
    ) -> Anim<U> {
        const { assert!(T::COMPONENTS == 1 && U::COMPONENTS == 1) };

        match self.inner {
            Inner::Const(value) => Anim::constant(convert(value)),
            Inner::Live(track, _) => Anim::live(track),
            Inner::Derived(track, project) => Anim {
                inner: Inner::Derived(track, Arc::new(move |slots| convert(project(slots)))),
            },
        }
    }

    /// A handle whose value is `f` of this one's, computed on every read.
    ///
    /// This is how one track drives a value of another type: a progress
    /// becomes a point on a path, a pose, a colour from a ramp.
    ///
    /// ```
    /// use iced_animate::Anim;
    /// let progress: Anim<f32> = 0.5.into();
    /// let width = progress.map(|t| 40.0 + 200.0 * t);
    /// assert_eq!(width.get(), 140.0);
    /// ```
    ///
    /// The mapped handle *is* a handle on the same track: it keeps the track
    /// alive exactly as the original does, it animates while the track
    /// moves, and [`mark_tier`](Self::mark_tier) on it raises the track's
    /// tier. A track read through two handles pays for the most expensive of
    /// them — map a progress into a `Cached::translate` and also into a
    /// width, and the whole track is `Tier::Layout`.
    ///
    /// `f` runs on every [`get`](Self::get) and [`target`](Self::target),
    /// inside `draw` or `layout`: keep it cheap, and precompute anything
    /// heavy (a path's length table) outside it.
    #[must_use]
    pub fn map<U: Animatable>(&self, f: impl Fn(T) -> U + Send + Sync + 'static) -> Anim<U> {
        match &self.inner {
            Inner::Const(value) => Anim::constant(f(*value)),
            Inner::Live(track, _) => Anim {
                inner: Inner::Derived(Arc::clone(track), Arc::new(move |slots| f(T::read(slots)))),
            },
            Inner::Derived(track, project) => {
                let project = Arc::clone(project);
                Anim {
                    inner: Inner::Derived(
                        Arc::clone(track),
                        Arc::new(move |slots| f(project(slots))),
                    ),
                }
            }
        }
    }

    /// Resolves the current value.
    ///
    /// Call this inside `layout` or `draw`, not while building the view.
    /// Reads take no lock: the tick publishes each component into an atomic.
    ///
    /// **Not for cross-thread use.** `Anim` is `Send + Sync` so that every
    /// engine method can take `&self`, but a multi-component value read from
    /// another thread while the engine ticks may tear (one channel from this
    /// frame, another from the next). The engine is single-threaded by
    /// design.
    #[must_use]
    pub fn get(&self) -> T {
        match &self.inner {
            Inner::Const(value) => *value,
            Inner::Live(track, _) => {
                track.touch();
                T::read(&track.value())
            }
            Inner::Derived(track, project) => {
                track.touch();
                project(&track.value())
            }
        }
    }

    /// Resolves the value this handle is heading for: the target the view
    /// last gave [`Motion::to`], or the resting value once it has arrived.
    ///
    /// Read it where a widget needs the end of an animation before the
    /// animation gets there — laying text out at its final font weight, say,
    /// so the line does not change width while the weight moves. Like
    /// [`get`], the read takes no lock.
    ///
    /// [`get`]: Self::get
    /// [`Motion::to`]: crate::Motion::to
    #[must_use]
    pub fn target(&self) -> T {
        match &self.inner {
            Inner::Const(value) => *value,
            Inner::Live(track, _) => {
                track.touch();
                T::read(&track.target())
            }
            Inner::Derived(track, project) => {
                track.touch();
                project(&track.target())
            }
        }
    }

    /// Returns `true` if this value is currently in motion.
    #[must_use]
    pub fn is_animating(&self) -> bool {
        match &self.inner {
            Inner::Const(_) => false,
            Inner::Live(track, _) | Inner::Derived(track, _) => !track.is_settled(),
        }
    }

    /// Returns `true` if this value is bound to an engine track (whether or
    /// not it is moving right now).
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self.inner, Inner::Live(..) | Inner::Derived(..))
    }

    /// Raises the presentation cost of the underlying track to at least
    /// `tier`. A no-op for a constant.
    ///
    /// Called by the widget that binds the value, since only the consumer
    /// knows whether it reads the value in `layout`, in `draw`, or hands it
    /// to the compositor.
    pub fn mark_tier(&self, tier: Tier) {
        if let Inner::Live(track, _) | Inner::Derived(track, _) = &self.inner {
            track.mark_tier(tier);
        }
    }

    /// The presentation tier of the underlying track: `None` for a constant
    /// and for a live value no widget has marked yet (the engine then treats
    /// it as [`Tier::Paint`]).
    #[must_use]
    pub fn tier(&self) -> Option<Tier> {
        match &self.inner {
            Inner::Const(_) => None,
            Inner::Live(track, _) | Inner::Derived(track, _) => track.tier(),
        }
    }
}

impl<T: Animatable> From<T> for Anim<T> {
    fn from(value: T) -> Self {
        Self::constant(value)
    }
}

impl<T: Animatable> From<&Anim<T>> for Anim<T> {
    fn from(value: &Anim<T>) -> Self {
        value.clone()
    }
}

impl<T: Animatable + Default> Default for Anim<T> {
    fn default() -> Self {
        Self::constant(T::default())
    }
}

#[cfg(test)]
mod tests {
    use iced_core::{Point, Radians, Rectangle};

    use super::{Animatable, MAX_COMPONENTS};
    use std::time::Duration;

    use iced::Color;

    use crate::testing::FrameClock;
    use crate::{Anim, Curve, Motion, SpringParams, Tier, key};

    fn round_trip<T: Animatable + PartialEq + std::fmt::Debug>(value: T) {
        let mut slots = [f32::NAN; MAX_COMPONENTS];
        value.write(&mut slots);
        assert!(
            slots[..T::COMPONENTS].iter().all(|v| v.is_finite()),
            "every declared component is written"
        );
        assert_eq!(T::read(&slots), value);
    }

    #[test]
    fn iced_geometry_types_round_trip() {
        round_trip(Point::new(1.0, -2.0));
        round_trip(Rectangle::new(
            Point::new(1.0, 2.0),
            iced_core::Size::new(3.0, 4.0),
        ));
        round_trip(Radians(1.5));
    }

    #[test]
    fn a_handle_converts_from_a_reference_and_defaults_to_the_type_default() {
        let a: crate::Anim<f32> = 3.0.into();
        let b = crate::Anim::from(&a);
        assert_eq!(b.get(), 3.0);
        assert_eq!(
            crate::Anim::<iced_core::Padding>::default().get(),
            iced_core::Padding::ZERO
        );
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // pins the public constant
    fn the_limit_is_public_and_four() {
        assert_eq!(MAX_COMPONENTS, 4);
        assert!(<Rectangle as Animatable>::COMPONENTS <= MAX_COMPONENTS);
    }

    /// A fast spring for tests; not the shipped `curves::SMOOTH`.
    const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

    #[test]
    fn multi_component_values_move_together() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, Color::from_rgba(0.0, 0.0, 0.0, 0.0));
        let color = m.to(key, FAST, Color::from_rgba(1.0, 0.5, 0.25, 1.0));

        let _ = clock.run(120);

        let settled = color.get();

        assert_eq!(settled.r, 1.0);
        assert_eq!(settled.g, 0.5);
        assert_eq!(settled.b, 0.25);
        assert_eq!(settled.a, 1.0);
    }

    #[test]
    fn anim_marks_tier_without_exposing_the_track() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let live = m.to(key, FAST, 10.0_f32);
        live.mark_tier(Tier::Layout);
        assert_eq!(live.tier(), Some(Tier::Layout));
        assert!(live.is_live());

        let constant: Anim<f32> = 3.0.into();
        constant.mark_tier(Tier::Layout);
        assert_eq!(constant.tier(), None, "a constant has no track and no tier");
        assert!(!constant.is_animating());
        assert!(!constant.is_live());
    }

    #[test]
    fn anim_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Motion>();
        assert_send_sync::<Anim<Color>>();
    }

    #[test]
    fn an_unmarked_track_is_unknown_to_its_handle_and_paint_to_the_engine() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 1.0_f32);

        assert_eq!(
            value.tier(),
            None,
            "nobody has said where this value is read"
        );

        let _ = clock.run(1);
        let status = clock.run(1);
        assert!(status.animating);
        assert!(
            !status.layout_invalid,
            "an unmarked track costs a redraw, never a relayout"
        );
    }

    #[test]
    fn a_target_is_readable_before_the_track_arrives() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 500.0_f32);
        let value = m.to(key, FAST, 600.0_f32);

        assert_eq!(
            value.target(),
            600.0,
            "the target is known the moment the view retargets the track"
        );

        let _ = clock.run(1);
        assert!(value.is_animating(), "the track is still on its way");
        assert!(
            value.get() < 600.0,
            "and its current value has not arrived yet"
        );
        assert_eq!(value.target(), 600.0, "which does not move the target");
    }

    #[test]
    fn an_entrance_targets_where_it_is_going_not_where_it_starts() {
        let m = Motion::new();
        let key = key!();

        let value = m.enter(key, FAST, 400.0_f32, 0.0_f32);

        assert_eq!(value.get(), 400.0, "an entrance starts at `from`");
        assert_eq!(
            value.target(),
            0.0,
            "and is known to be headed for `to` from the start"
        );
    }

    #[test]
    fn a_settled_track_targets_where_it_rests() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 1.0_f32);

        let _ = clock.run(120);
        assert!(!value.is_animating(), "120 frames settle a fast spring");
        assert_eq!(value.target(), value.get());
    }

    #[test]
    fn a_constant_targets_itself() {
        let value: Anim<f32> = 16.0.into();

        assert_eq!(value.target(), 16.0);
        assert_eq!(value.target(), value.get());
    }

    const LINEAR_100: Curve = Curve::ease(crate::Easing::Linear, Duration::from_millis(100));

    #[test]
    fn a_mapped_constant_is_a_constant_and_a_mapped_track_follows_it() {
        let constant: Anim<f32> = 3.0.into();
        let doubled = constant.map(|v| v * 2.0);
        assert_eq!(doubled.get(), 6.0);
        assert!(!doubled.is_live());

        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let base = m.play(key!(), LINEAR_100, 0.0_f32, 10.0);
        let point = base.map(|v| Point::new(v, -v));
        assert!(point.is_live());
        assert_eq!(
            point.target(),
            Point::new(10.0, -10.0),
            "target goes through the map too"
        );

        let _ = clock.run(4);
        assert_eq!(point.get(), Point::new(base.get(), -base.get()));
        assert!(point.is_animating());
        let _ = clock.run_until_settled();
        assert!(!point.is_animating());
        assert_eq!(point.get(), Point::new(10.0, -10.0));
    }

    #[test]
    fn two_maps_compose_into_one() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let base = m.play(key!(), LINEAR_100, 0.0_f32, 10.0);
        let twice = base.map(|v| v + 1.0).map(|v| v * 3.0);
        let _ = clock.run(3);
        assert_eq!(twice.get(), (base.get() + 1.0) * 3.0);
    }

    #[test]
    fn a_mapped_handle_marks_the_source_track_and_keeps_it_alive() {
        let m = Motion::new();
        let base = m.to(key!(), LINEAR_100, 1.0_f32);
        let mapped = base.map(|v| v * 2.0);

        mapped.mark_tier(Tier::Composite);
        assert_eq!(base.tier(), Some(Tier::Composite));
        mapped.mark_tier(Tier::Layout);
        assert_eq!(base.tier(), Some(Tier::Layout), "the highest tier wins");

        drop(base);
        for _ in 0..=crate::engine::GC_IDLE_BUILDS {
            m.end_build();
            m.collect();
        }
        assert_eq!(m.track_count(), 1, "the mapped handle holds the track");

        drop(mapped);
        m.collect();
        assert_eq!(m.track_count(), 0, "and releases it when dropped");
    }

    #[test]
    fn composite_is_reachable_and_marks_only_ever_raise() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 1.0_f32);

        value.mark_tier(Tier::Composite);
        assert_eq!(
            value.tier(),
            Some(Tier::Composite),
            "a compositor widget opts in"
        );

        value.mark_tier(Tier::Layout);
        assert_eq!(value.tier(), Some(Tier::Layout), "the max over marks wins");

        value.mark_tier(Tier::Composite);
        assert_eq!(
            value.tier(),
            Some(Tier::Layout),
            "and cannot be lowered again"
        );
    }
}
