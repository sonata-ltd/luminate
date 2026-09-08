//! A single animation track: one property, one curve, up to four components.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::Duration;

pub use iced_core::animation::Easing;

use crate::spring::{Spring, SpringParams};
use crate::value::MAX_COMPONENTS;

/// How a track gets from its current value to its target.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum CurveKind {
    /// Physical spring. Carries velocity, so retargeting mid-flight continues
    /// the motion instead of restarting it. The default.
    Spring(SpringParams),
    /// Fixed-duration easing curve. Restarts from the current value on
    /// retarget, which reads as a cut rather than a redirection.
    Ease {
        /// The easing function.
        easing: Easing,
        /// How long a full transition takes.
        duration: Duration,
    },
}

/// A curve plus an optional lead-in delay.
///
/// Curves are meant to be declared once as constants and shared, see
/// [`crate::curves`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Curve {
    kind: CurveKind,
    delay: Duration,
}

impl Curve {
    /// A spring curve with no delay.
    #[must_use]
    pub const fn spring(params: SpringParams) -> Self {
        Self {
            kind: CurveKind::Spring(params),
            delay: Duration::ZERO,
        }
    }

    /// An easing curve of the given duration, with no delay.
    #[must_use]
    pub const fn ease(easing: Easing, duration: Duration) -> Self {
        Self {
            kind: CurveKind::Ease { easing, duration },
            delay: Duration::ZERO,
        }
    }

    /// Sets (replaces) the lead-in delay of this curve.
    ///
    /// A delay applies when the track is retargeted: the value holds its
    /// current pose for `delay`, then moves. This is what staggers a
    /// sequence. A retarget while a delayed spring is still moving holds it
    /// where it is for the delay and then continues with the velocity it
    /// had. Calling `delayed` twice keeps only the last delay; it does not
    /// accumulate.
    #[must_use]
    pub const fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// The interpolation this curve runs.
    #[must_use]
    pub const fn kind(self) -> CurveKind {
        self.kind
    }

    /// How long the curve waits before it starts moving.
    #[must_use]
    pub const fn delay(self) -> Duration {
        self.delay
    }
}

impl Default for Curve {
    fn default() -> Self {
        crate::curves::SMOOTH
    }
}

/// What a moving track costs to present, and therefore what the host must
/// invalidate when it moves.
///
/// Set by whichever widget binds the value, and only ever raised, a track
/// read from both a `draw` and a `layout` is treated as [`Tier::Layout`]. A
/// track nobody has marked has no tier ([`Anim::tier`] is `None`); the
/// engine treats it as [`Tier::Paint`], which is correct for any value read
/// in `draw`.
///
/// [`Anim::tier`]: crate::Anim::tier
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Tier {
    /// Consumed by the compositor (transform, opacity). Needs a redraw, and
    /// not even a re-record of a cached texture.
    ///
    /// This crate never produces it: only a compositor-tier widget such as
    /// `Cached` in `iced_texture_cache` marks a track `Composite`.
    Composite = 0,
    /// Read during `draw` (colours, radii, border widths). Needs a redraw.
    Paint = 1,
    /// Read during `layout` (sizes, padding, text size). Needs a relayout on
    /// top of the redraw, so it is the expensive tier.
    Layout = 2,
}

/// Stored tier byte for "no widget has marked this track yet".
const UNMARKED: u8 = 0;

impl Tier {
    /// The byte stored for a marked tier; one above the discriminant so that
    /// `0` can mean unmarked.
    const fn mark(self) -> u8 {
        self as u8 + 1
    }

    /// Decodes a stored byte; `None` for [`UNMARKED`].
    fn from_mark(raw: u8) -> Option<Self> {
        match raw {
            UNMARKED => None,
            1 => Some(Tier::Composite),
            2 => Some(Tier::Paint),
            3 => Some(Tier::Layout),
            _ => unreachable!("corrupted tier byte {raw}"),
        }
    }
}

/// Where a keyed element is in its enter/present/exit life.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Phase {
    Present = 0,
    Entering = 1,
    Exiting = 2,
}

impl Phase {
    fn from_u8(raw: u8) -> Self {
        match raw {
            0 => Phase::Present,
            1 => Phase::Entering,
            2 => Phase::Exiting,
            _ => unreachable!("corrupted phase byte {raw}"),
        }
    }
}

/// Solver state. The curve that produced it is stored alongside, so the two
/// can never disagree.
enum Solver {
    Spring {
        params: SpringParams,
        springs: [Spring; MAX_COMPONENTS],
    },
    Ease {
        easing: Easing,
        duration: Duration,
        from: [f32; MAX_COMPONENTS],
        elapsed: f32,
    },
}

impl Solver {
    fn new(curve: Curve, start: &[f32; MAX_COMPONENTS]) -> Self {
        match curve.kind {
            CurveKind::Spring(params) => Solver::Spring {
                params,
                springs: std::array::from_fn(|i| Spring::new(params, start[i])),
            },
            CurveKind::Ease { easing, duration } => Solver::Ease {
                easing,
                duration,
                from: *start,
                elapsed: 0.0,
            },
        }
    }

    fn same_family(&self, kind: CurveKind) -> bool {
        matches!(
            (self, kind),
            (Solver::Spring { .. }, CurveKind::Spring(_))
                | (Solver::Ease { .. }, CurveKind::Ease { .. })
        )
    }

    fn kind(&self) -> CurveKind {
        match self {
            Solver::Spring { params, .. } => CurveKind::Spring(*params),
            Solver::Ease {
                easing, duration, ..
            } => CurveKind::Ease {
                easing: *easing,
                duration: *duration,
            },
        }
    }
}

struct State {
    solver: Solver,
    delay: Duration,
    target: [f32; MAX_COMPONENTS],
    /// Time left before the curve starts moving, in seconds.
    delay_left: f32,
}

/// Replaces every non-finite component of `values` (among the first
/// `components`) with the matching component of `fallback`.
///
/// Returns whether anything was replaced. The fallback is always a value the
/// track has already published, so it is finite by construction.
fn sanitise(
    values: &mut [f32; MAX_COMPONENTS],
    fallback: &[f32; MAX_COMPONENTS],
    components: usize,
) -> bool {
    let mut replaced = false;

    for (value, fallback) in values.iter_mut().zip(fallback).take(components) {
        if !value.is_finite() {
            *value = *fallback;
            replaced = true;
        }
    }

    replaced
}

/// What one [`Track::tick`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    /// The track was already at rest; nothing was published.
    Settled,
    /// The track is alive (in its lead-in delay, or moving by less than one
    /// `f32` step) but published no new value.
    Holding,
    /// A new value was published, including the final value at the target.
    Moved,
}

/// One animated property.
///
/// Reads take no lock: `tick` publishes each component into an atomic, and
/// [`value`] loads them. The mutex guards only the solver, which is touched
/// once per frame by the tick and again when the view retargets it.
///
/// [`value`]: Self::value
pub(crate) struct Track {
    value: [AtomicU32; MAX_COMPONENTS],
    /// The target of [`state`](Self::state), mirrored into atomics so that a
    /// widget can read it in `layout` as cheaply as it reads the value.
    target: [AtomicU32; MAX_COMPONENTS],
    components: usize,
    settled: AtomicBool,
    tier: AtomicU8,
    /// Set by every read or write since the last [`stamp`](Self::stamp).
    touched: AtomicBool,
    /// The last build that touched this track.
    last_touched: AtomicU64,
    phase: AtomicU8,
    warned_non_finite: AtomicBool,
    state: Mutex<State>,
}

impl std::fmt::Debug for Track {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Track")
            .field("value", &self.value())
            .field("components", &self.components)
            .field("settled", &self.is_settled())
            .field("tier", &self.tier())
            .field("phase", &self.phase())
            .finish_non_exhaustive()
    }
}

impl Track {
    /// Creates a track resting at `initial`, holding `components` scalars.
    ///
    /// A non-finite `initial` rests at `0.0` for that component.
    pub(crate) fn new(
        curve: Curve,
        initial: [f32; MAX_COMPONENTS],
        components: usize,
        build: u64,
    ) -> Self {
        let mut start = initial;
        let _ = sanitise(&mut start, &[0.0; MAX_COMPONENTS], components);

        Self {
            value: std::array::from_fn(|i| AtomicU32::new(start[i].to_bits())),
            target: std::array::from_fn(|i| AtomicU32::new(start[i].to_bits())),
            components,
            settled: AtomicBool::new(true),
            tier: AtomicU8::new(UNMARKED),
            touched: AtomicBool::new(true),
            last_touched: AtomicU64::new(build),
            phase: AtomicU8::new(Phase::Present as u8),
            warned_non_finite: AtomicBool::new(false),
            state: Mutex::new(State {
                solver: Solver::new(curve, &start),
                delay: curve.delay,
                target: start,
                delay_left: 0.0,
            }),
        }
    }

    /// Number of scalars this track holds.
    pub(crate) fn components(&self) -> usize {
        self.components
    }

    /// Returns the published value of every component.
    pub(crate) fn value(&self) -> [f32; MAX_COMPONENTS] {
        std::array::from_fn(|i| f32::from_bits(self.value[i].load(Ordering::Relaxed)))
    }

    /// Returns the target of every component: where the track is headed, or
    /// where it rests once it has arrived.
    pub(crate) fn target(&self) -> [f32; MAX_COMPONENTS] {
        std::array::from_fn(|i| f32::from_bits(self.target[i].load(Ordering::Relaxed)))
    }

    fn publish(&self, value: &[f32; MAX_COMPONENTS]) {
        for (slot, component) in self.value.iter().zip(value.iter()) {
            slot.store(component.to_bits(), Ordering::Relaxed);
        }
    }

    /// Returns `true` once the track has reached its target and stopped.
    pub(crate) fn is_settled(&self) -> bool {
        self.settled.load(Ordering::Relaxed)
    }

    /// Records that something read or wrote this track since the last build
    /// ended, keeping it out of the engine's garbage collector.
    pub(crate) fn touch(&self) {
        self.touched.store(true, Ordering::Relaxed);
    }

    /// Called once per build by the engine: turns a pending touch into a
    /// build stamp.
    pub(crate) fn stamp(&self, build: u64) {
        if self.touched.swap(false, Ordering::Relaxed) {
            self.last_touched.store(build, Ordering::Relaxed);
        }
    }

    pub(crate) fn last_touched(&self) -> u64 {
        self.last_touched.load(Ordering::Relaxed)
    }

    /// The presentation cost of this track, or `None` if no widget has
    /// marked it yet.
    pub(crate) fn tier(&self) -> Option<Tier> {
        Tier::from_mark(self.tier.load(Ordering::Relaxed))
    }

    /// Raises the track's tier to at least `tier`.
    pub(crate) fn mark_tier(&self, tier: Tier) {
        self.tier.fetch_max(tier.mark(), Ordering::Relaxed);
    }

    pub(crate) fn phase(&self) -> Phase {
        Phase::from_u8(self.phase.load(Ordering::Relaxed))
    }

    pub(crate) fn set_phase(&self, phase: Phase) {
        self.phase.store(phase as u8, Ordering::Relaxed);
    }

    /// A non-finite target is a programming error in the caller; it is
    /// reported once per track and then degraded to "hold the current value".
    fn report_non_finite(&self) {
        if !self.warned_non_finite.swap(true, Ordering::Relaxed) {
            log::error!(
                "a non-finite animation target was replaced by the track's current value; \
                 check the arithmetic that produced it"
            );
        }
    }

    /// Points the track at a new target, keeping its current motion.
    ///
    /// A no-op when the target and curve are unchanged, so this is safe to
    /// call from `view()` on every rebuild. A change of spring tuning retunes
    /// the springs in place; only a change of *family* (spring ↔ ease)
    /// rebuilds the solver from the current value.
    pub(crate) fn retarget(&self, curve: Curve, target: &[f32; MAX_COMPONENTS]) {
        let components = self.components;

        debug_assert!(
            target[..components].iter().all(|v| v.is_finite()),
            "animation target must be finite"
        );

        let current = self.value();
        let mut target = *target;
        if sanitise(&mut target, &current, components) {
            self.report_non_finite();
        }

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);

        let unchanged = state.solver.kind() == curve.kind
            && state.delay == curve.delay
            && state.target[..components]
                .iter()
                .zip(target[..components].iter())
                .all(|(current, next)| current.to_bits() == next.to_bits());

        if unchanged {
            return;
        }

        if !state.solver.same_family(curve.kind) {
            state.solver = Solver::new(curve, &current);
        }

        state.delay = curve.delay;
        state.target = target;
        state.delay_left = curve.delay.as_secs_f32();

        for (slot, component) in self.target.iter().zip(target.iter()) {
            slot.store(component.to_bits(), Ordering::Relaxed);
        }

        // `target` is `Copy`; taking it out of `state` first keeps the solver
        // borrow below exclusive.
        let goals = state.target;

        // An ease asked to go where it already is has nothing to interpolate:
        // holding for the full delay and duration would only cost frames.
        let already_there = matches!(curve.kind, CurveKind::Ease { .. })
            && goals[..components]
                .iter()
                .zip(&current[..components])
                .all(|(goal, now)| goal.to_bits() == now.to_bits());
        if already_there {
            state.delay_left = 0.0;
        }

        match &mut state.solver {
            Solver::Spring { params, springs } => {
                if let CurveKind::Spring(new_params) = curve.kind
                    && *params != new_params
                {
                    *params = new_params;
                    for spring in springs.iter_mut() {
                        spring.retune(new_params);
                    }
                }

                for (spring, goal) in springs.iter_mut().zip(goals.iter()) {
                    spring.set_target(*goal);
                }
            }
            Solver::Ease {
                easing,
                duration,
                from,
                elapsed,
            } => {
                if let CurveKind::Ease {
                    easing: new_easing,
                    duration: new_duration,
                } = curve.kind
                {
                    *easing = new_easing;
                    *duration = new_duration;
                }

                *from = current;
                *elapsed = 0.0;
            }
        }

        // `Release` pairs with the `Acquire` fast path in `tick`, so a tick
        // that sees "moving" also sees the solver state written above.
        self.settled.store(already_there, Ordering::Release);
    }

    /// Restarts the track from an explicit pose.
    ///
    /// Unlike [`retarget`], this discards current motion, it is how an enter
    /// animation or a one-shot sequence is seeded.
    ///
    /// [`retarget`]: Self::retarget
    pub(crate) fn restart(
        &self,
        curve: Curve,
        from: &[f32; MAX_COMPONENTS],
        target: &[f32; MAX_COMPONENTS],
    ) {
        let components = self.components;

        debug_assert!(
            from[..components]
                .iter()
                .chain(&target[..components])
                .all(|v| v.is_finite()),
            "animation values must be finite"
        );

        let mut start = *from;
        let replaced = sanitise(&mut start, &self.value(), components);
        let mut goal = *target;
        let replaced = sanitise(&mut goal, &start, components) || replaced;
        if replaced {
            self.report_non_finite();
        }

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);

        state.delay = curve.delay;
        state.target = goal;
        state.delay_left = curve.delay.as_secs_f32();
        state.solver = Solver::new(curve, &start);

        let goals = state.target;

        if let Solver::Spring { springs, .. } = &mut state.solver {
            for (spring, goal) in springs.iter_mut().zip(goals.iter()) {
                spring.set_target(*goal);
            }
        }

        self.publish(&start);
        self.settled.store(false, Ordering::Release);
    }

    /// Advances the track by `dt` seconds.
    pub(crate) fn tick(&self, dt: f32) -> Step {
        // `Acquire` pairs with the `Release` stores of `settled`, so a track
        // seen as moving here is seen with the retarget that started it.
        if self.settled.load(Ordering::Acquire) {
            return Step::Settled;
        }

        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);

        // Burn the lead-in delay first; a delayed curve holds its pose rather
        // than easing toward the target early.
        if state.delay_left > 0.0 {
            state.delay_left -= dt;

            if state.delay_left > 0.0 {
                return Step::Holding;
            }

            // Spend only the remainder of this frame on the curve itself.
            let overshoot = -state.delay_left;
            state.delay_left = 0.0;

            return self.advance(&mut state, overshoot);
        }

        self.advance(&mut state, dt)
    }

    fn advance(&self, state: &mut State, dt: f32) -> Step {
        let components = self.components;
        let before = self.value();
        let mut next = before;
        let goals = state.target;

        let settled = match &mut state.solver {
            Solver::Spring { springs, .. } => {
                let mut all_settled = true;

                for (i, spring) in springs.iter_mut().enumerate().take(components) {
                    spring.tick(dt);

                    if spring.is_settled() {
                        spring.snap();
                    } else {
                        all_settled = false;
                    }

                    next[i] = spring.position();
                }

                all_settled
            }
            Solver::Ease {
                easing,
                duration,
                from,
                elapsed,
            } => {
                *elapsed += dt;

                let total = duration.as_secs_f32().max(f32::EPSILON);
                let progress = (*elapsed / total).clamp(0.0, 1.0);
                let eased = easing.value(progress);

                for i in 0..components {
                    next[i] = from[i] + (goals[i] - from[i]) * eased;
                }

                progress >= 1.0
            }
        };

        if settled {
            next[..components].copy_from_slice(&goals[..components]);
        }

        self.publish(&next);
        self.settled.store(settled, Ordering::Release);

        // An entrance is over the moment its track comes to rest; doing it
        // here (once) rather than in the engine's loop (every frame).
        if settled && self.phase() == Phase::Entering {
            self.set_phase(Phase::Present);
        }

        let changed = before[..components]
            .iter()
            .zip(&next[..components])
            .any(|(a, b)| a.to_bits() != b.to_bits());

        if changed {
            Step::Moved
        } else if settled {
            Step::Settled
        } else {
            Step::Holding
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_COMPONENTS, sanitise};

    #[test]
    fn non_finite_components_fall_back_to_the_current_value() {
        let current = [1.0, 2.0, 3.0, 4.0];
        let mut target = [f32::NAN, 5.0, f32::INFINITY, f32::NEG_INFINITY];

        assert!(sanitise(&mut target, &current, MAX_COMPONENTS));
        assert_eq!(target, [1.0, 5.0, 3.0, 4.0]);

        let mut clean = [0.5; MAX_COMPONENTS];
        assert!(!sanitise(&mut clean, &current, MAX_COMPONENTS));
        assert_eq!(clean, [0.5; MAX_COMPONENTS]);
    }

    #[test]
    fn only_declared_components_are_sanitised() {
        let current = [0.0; MAX_COMPONENTS];
        let mut target = [1.0, f32::NAN, f32::NAN, f32::NAN];
        assert!(
            !sanitise(&mut target, &current, 1),
            "slots past `components` are unused"
        );
        assert!(target[1].is_nan());
    }
}
