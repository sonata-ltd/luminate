//! The [`Motion`] handle and its engine.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use iced_core::time::Instant;

use crate::key::MotionKey;
use crate::set::MotionSet;
use crate::track::{Curve, Phase, Step, Tier, Track};
use crate::value::{Anim, Animatable, MAX_COMPONENTS};

/// How many view builds a settled, unreferenced track survives after the
/// last one that touched it.
///
/// Counted in *builds*, not frames, because that is the unit in which a track
/// can be referenced at all: a value the view holds without binding it into a
/// widget, such as an exit fade on a row that has not left yet, is touched once per
/// `view()` and never in between. A frame-based clock collects it during any
/// idle stretch long enough (a couple of seconds of cursor movement is plenty)
/// and the animation it was holding open then starts from the wrong pose.
///
/// Two would do; three leaves room for a build that bails out early.
pub(crate) const GC_IDLE_BUILDS: u64 = 3;

/// Identifies one [`Host`] widget for the "two hosts" diagnostic.
///
/// A host is rebuilt with every `view()`, so the id changes per build; that
/// is fine, because the diagnostic asks whether two *different* hosts ticked
/// within one build.
///
/// [`Host`]: crate::widget::Host
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct HostId(u64);

impl HostId {
    /// Allocates the next id. Ids start at 1; `0` means "no host yet".
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Sentinel for "no animation has started yet".
const NEVER: u64 = u64::MAX;

/// The largest delta one frame may spend, in seconds: about four frames of a
/// 60 Hz display.
///
/// A frame is charged the wall-clock time since the previous one, and that is
/// right up to the point where there *was* no previous frame in any
/// meaningful sense — a stall while a pipeline compiled, a window that came
/// back from behind another one. Passing such a gap through would advance an
/// animation to its end in a single step, which reads as a teleport rather
/// than motion. Capping it makes the animation lose time instead of position:
/// it resumes from where it stopped and still arrives, a little later than
/// wall-clock would say.
///
/// The cap is deliberately generous. A machine genuinely running at 20 FPS is
/// under it, and keeps animating in real time; only an actual hitch is
/// clipped.
pub(crate) const MAX_FRAME: f32 = 1.0 / 15.0;

/// What one [`Motion::tick`] changed, and therefore what the host must
/// invalidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct TickStatus {
    /// At least one track is still moving; another frame is needed.
    pub animating: bool,
    /// A track read during `layout` published a new value this frame, so the
    /// layout is stale. `false` while a delayed track is only holding its
    /// pose.
    ///
    /// Kept separate from `animating` so a pure transform or opacity
    /// animation does not drag a relayout along with it every frame.
    pub layout_invalid: bool,
}

/// Whether a keyed element is on screen, and how.
///
/// Used for keyed collections: an element removed from the application's data
/// cannot animate out, because the `Element` that would draw it no longer
/// exists. The application therefore keeps the item while its key reports
/// [`Presence::Exiting`], and drops it on [`Presence::Gone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Presence {
    /// Playing its enter animation.
    Entering,
    /// Settled and on screen.
    Present,
    /// Playing its exit animation; keep rendering it.
    Exiting,
    /// Finished leaving, or never seen. Safe to drop.
    Gone,
}

struct Engine {
    tracks: Mutex<HashMap<MotionKey, Arc<Track>>>,
    /// Counts `view()` builds. This is the garbage collector's clock. See
    /// [`GC_IDLE_BUILDS`].
    build: AtomicU64,
    last_tick: Mutex<Option<Instant>>,
    ticked: AtomicBool,
    /// Whether the previous tick left anything moving. Frames are produced on
    /// demand, so this is what separates "the last frame was one frame ago"
    /// from "there were no frames at all until now". See [`MAX_FRAME`].
    was_moving: AtomicBool,
    /// The build during which the first animation started, or [`NEVER`].
    first_start_build: AtomicU64,
    warned_never_ticked: AtomicBool,
    /// The build at which [`Motion::collect`] last ran.
    last_gc_build: AtomicU64,
    /// The host that ticked last (`0` = none) and the build it ticked in.
    last_host: AtomicU64,
    last_host_build: AtomicU64,
    warned_two_hosts: AtomicBool,
}

/// A handle onto the animation engine.
///
/// Cheap to clone. Keep one in application state and clone it wherever the
/// view needs it. Every method takes `&self`, so it works from `view()`.
#[derive(Clone)]
pub struct Motion(Arc<Engine>);

impl std::fmt::Debug for Motion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_struct("Motion");
        match self.0.tracks.try_lock() {
            Ok(tracks) => out.field("tracks", &tracks.len()),
            Err(_) => out.field("tracks", &"<locked>"),
        };
        out.field("build", &self.0.build.load(Ordering::Relaxed))
            .finish()
    }
}

impl Default for Motion {
    fn default() -> Self {
        Self::new()
    }
}

impl Motion {
    /// Creates an engine with no tracks.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(Engine {
            tracks: Mutex::new(HashMap::new()),
            build: AtomicU64::new(0),
            last_tick: Mutex::new(None),
            ticked: AtomicBool::new(false),
            was_moving: AtomicBool::new(false),
            first_start_build: AtomicU64::new(NEVER),
            warned_never_ticked: AtomicBool::new(false),
            last_gc_build: AtomicU64::new(0),
            last_host: AtomicU64::new(0),
            last_host_build: AtomicU64::new(0),
            warned_two_hosts: AtomicBool::new(false),
        }))
    }

    /// Remembers the build in which the first animation started, so a missing
    /// host can be reported once a few builds have passed without a frame.
    ///
    /// Forgetting to wrap the view in a [`Host`] otherwise fails silently:
    /// targets are set, nothing advances, and every animated value simply
    /// holds its first pose. Warning immediately would misfire on a message
    /// handled before the first frame (a startup task result), so the check
    /// is deferred to [`end_build`].
    ///
    /// [`Host`]: crate::widget::Host
    /// [`end_build`]: Self::end_build
    fn note_started(&self) {
        if self.0.ticked.load(Ordering::Relaxed) {
            return;
        }

        let build = self.0.build.load(Ordering::Relaxed);
        let _ = self.0.first_start_build.compare_exchange(
            NEVER,
            build,
            Ordering::Relaxed,
            Ordering::Relaxed,
        );
    }

    /// `true` when an animation started three or more builds ago and the
    /// engine has still never been ticked (one build is a startup task result
    /// arriving before the first frame; three is a missing host).
    pub(crate) fn never_ticked_stale(&self) -> bool {
        if self.0.ticked.load(Ordering::Relaxed) {
            return false;
        }

        let first = self.0.first_start_build.load(Ordering::Relaxed);
        let build = self.0.build.load(Ordering::Relaxed);

        first != NEVER && build.saturating_sub(first) > 2
    }

    /// Whether two distinct hosts have ever ticked this engine in one build.
    #[cfg(test)]
    pub(crate) fn two_hosts_seen(&self) -> bool {
        self.0.warned_two_hosts.load(Ordering::Relaxed)
    }

    fn note_host(&self, host: HostId) {
        let build = self.0.build.load(Ordering::Relaxed);
        let previous = self.0.last_host.swap(host.0, Ordering::Relaxed);
        let previous_build = self.0.last_host_build.swap(build, Ordering::Relaxed);

        if previous != 0
            && previous != host.0
            && previous_build == build
            && !self.0.warned_two_hosts.swap(true, Ordering::Relaxed)
        {
            log::warn!(
                "two `Host` widgets ticked one `Motion` in the same view build; use exactly \
                 one host per view and one `Motion` per window"
            );
        }
    }

    /// Looks up the track at `key`, creating it at `initial` if absent.
    ///
    /// Returns the track and whether this call created it.
    fn track_for(
        &self,
        key: MotionKey,
        curve: Curve,
        initial: [f32; MAX_COMPONENTS],
        components: usize,
    ) -> (Arc<Track>, bool) {
        let mut tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(track) = tracks.get(&key) {
            debug_assert_eq!(
                track.components(),
                components,
                "{key:?} was reused with a different `Animatable` type"
            );
            return (Arc::clone(track), false);
        }

        let track = Arc::new(Track::new(
            curve,
            initial,
            components,
            self.0.build.load(Ordering::Relaxed),
        ));

        let _ = tracks.insert(key, Arc::clone(&track));

        (track, true)
    }

    /// Animates one property toward `target`.
    ///
    /// Idempotent: safe to call from `view()` on every rebuild. A track seen
    /// for the first time starts *at* `target` rather than animating in from
    /// nowhere. Use [`enter`] for that.
    ///
    /// Re-declaring a key with `to` cancels a pending exit ([`retire`]) and
    /// ends an entrance ([`enter`]) for the purposes of [`presence`].
    ///
    /// The returned handle is the only thing that keeps the track alive
    /// between builds without re-declaring it: see [`Anim`] for the
    /// lifetime contract.
    ///
    /// [`enter`]: Self::enter
    /// [`retire`]: Self::retire
    /// [`presence`]: Self::presence
    #[must_use = "the handle is what a widget reads; discarding it leaves a track nothing binds"]
    pub fn to<T: Animatable>(&self, key: MotionKey, curve: Curve, target: T) -> Anim<T> {
        const { assert!(T::COMPONENTS <= MAX_COMPONENTS) };
        let mut buffer = [0.0; MAX_COMPONENTS];
        target.write(&mut buffer);

        let (track, _) = self.track_for(key, curve, buffer, T::COMPONENTS);

        track.touch();
        track.retarget(curve, &buffer);
        track.set_phase(Phase::Present);

        if !track.is_settled() {
            self.note_started();
        }

        Anim::live(track)
    }

    /// Animates a whole named property set toward `target`.
    ///
    /// Each field of the set gets its own track under a key derived from
    /// `key` with [`MotionKey::salted`], and they all share `curve`, so the
    /// properties move together as one transition. Idempotent like [`to`].
    ///
    /// [`to`]: Self::to
    #[must_use = "the animated twin is what widgets read; discarding it leaves tracks nothing binds"]
    pub fn to_set<S: MotionSet>(&self, key: MotionKey, curve: Curve, target: S) -> S::Animated {
        target.bind(self, key, curve)
    }

    /// Restarts a property at `from` and animates it to `to`, discarding any
    /// motion in progress.
    ///
    /// This is the one-shot form, the equivalent of a web
    /// `animate(el, { opacity: [0, 1] })`. Combine with [`Curve::delayed`] to
    /// stagger a sequence.
    ///
    /// Unlike [`to`] it is **not** idempotent: every call restarts the
    /// sequence. Call it in response to the event that should trigger it,
    /// not from `view()`, where every rebuild would replay it. The handle it
    /// returns is often discarded on purpose; fetch it again in `view()` with
    /// [`get`].
    ///
    /// [`to`]: Self::to
    /// [`get`]: Self::get
    pub fn play<T: Animatable>(&self, key: MotionKey, curve: Curve, from: T, to: T) -> Anim<T> {
        const { assert!(T::COMPONENTS <= MAX_COMPONENTS) };
        let mut start = [0.0; MAX_COMPONENTS];
        let mut goal = [0.0; MAX_COMPONENTS];
        from.write(&mut start);
        to.write(&mut goal);

        let (track, _) = self.track_for(key, curve, start, T::COMPONENTS);

        track.touch();
        track.restart(curve, &start, &goal);
        track.set_phase(Phase::Present);
        self.note_started();

        Anim::live(track)
    }

    /// Animates a property in the first time `key` is seen, then behaves like
    /// [`to`].
    ///
    /// This is how a newly appended list item slides and fades into place
    /// without the application tracking which items are new.
    ///
    /// [`to`]: Self::to
    #[must_use = "the handle is what a widget reads; discarding it leaves a track nothing binds"]
    pub fn enter<T: Animatable>(&self, key: MotionKey, curve: Curve, from: T, to: T) -> Anim<T> {
        let requested = curve;
        let curve = curve.once();
        const { assert!(T::COMPONENTS <= MAX_COMPONENTS) };
        let mut start = [0.0; MAX_COMPONENTS];
        let mut goal = [0.0; MAX_COMPONENTS];
        from.write(&mut start);
        to.write(&mut goal);

        let (track, created) = self.track_for(key, curve, start, T::COMPONENTS);
        if !requested.repetition().is_once() {
            track.warn_ignored_repeat();
        }

        track.touch();

        if created {
            track.set_phase(Phase::Entering);
            track.restart(curve, &start, &goal);
        } else {
            track.retarget(curve, &goal);
            // A rebuild mid-entrance keeps `Entering`; only an exit is cancelled.
            if track.phase() == Phase::Exiting {
                track.set_phase(Phase::Present);
            }
        }

        if !track.is_settled() {
            self.note_started();
        }

        Anim::live(track)
    }

    /// Marks `key` as leaving and animates it toward `to`.
    ///
    /// [`presence`] reports [`Presence::Exiting`] until the animation settles,
    /// then [`Presence::Gone`]. The application must keep rendering the
    /// element until then. The engine animates values; it cannot resurrect an
    /// `Element` the view no longer builds.
    ///
    /// A key the engine has never seen is created *at* `to`, already settled,
    /// so it reports [`Presence::Gone`] at once.
    ///
    /// [`presence`]: Self::presence
    #[must_use = "the handle is what a widget reads; discarding it leaves a track nothing binds"]
    pub fn retire<T: Animatable>(&self, key: MotionKey, curve: Curve, to: T) -> Anim<T> {
        let requested = curve;
        let curve = curve.once();
        const { assert!(T::COMPONENTS <= MAX_COMPONENTS) };
        let mut goal = [0.0; MAX_COMPONENTS];
        to.write(&mut goal);

        let (track, _) = self.track_for(key, curve, goal, T::COMPONENTS);
        if !requested.repetition().is_once() {
            track.warn_ignored_repeat();
        }

        track.touch();
        track.set_phase(Phase::Exiting);
        track.retarget(curve, &goal);

        // An unseen key rests at `to` already; that is not a missing host.
        if !track.is_settled() {
            self.note_started();
        }

        Anim::live(track)
    }

    /// Looks a track up without retargeting it.
    ///
    /// The `view()`-side half of a one-shot: call [`play`] from `update` in
    /// response to the triggering event, then bind the handle in `view` with
    /// `get`, which touches the track (keeping it out of the garbage
    /// collector) but never restarts it. Returns `None` for a key the engine
    /// has not seen.
    ///
    /// ```
    /// use iced_animate::{curves::FADE, key, Motion};
    ///
    /// let m = Motion::new();
    /// let flash = key!();
    ///
    /// // update():
    /// let _ = m.play(flash, FADE, 1.0_f32, 0.0_f32);
    ///
    /// // view(), on every rebuild:
    /// let opacity = m.get::<f32>(flash).unwrap_or_else(|| 0.0.into());
    /// assert_eq!(opacity.get(), 1.0);
    /// ```
    ///
    /// [`play`]: Self::play
    #[must_use]
    pub fn get<T: Animatable>(&self, key: MotionKey) -> Option<Anim<T>> {
        const { assert!(T::COMPONENTS <= MAX_COMPONENTS) };

        let tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let track = tracks.get(&key)?;

        debug_assert_eq!(
            track.components(),
            T::COMPONENTS,
            "{key:?} was reused with a different `Animatable` type"
        );

        track.touch();

        Some(Anim::live(Arc::clone(track)))
    }

    /// Reports whether a keyed element is entering, settled, leaving, or done.
    #[must_use]
    pub fn presence(&self, key: MotionKey) -> Presence {
        let tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let Some(track) = tracks.get(&key) else {
            return Presence::Gone;
        };

        track.touch();

        match (track.phase(), track.is_settled()) {
            (Phase::Exiting, true) => Presence::Gone,
            (Phase::Exiting, false) => Presence::Exiting,
            (Phase::Entering, false) => Presence::Entering,
            (Phase::Entering, true) | (Phase::Present, _) => Presence::Present,
        }
    }

    /// Advances every track to `now`.
    ///
    /// Called once per frame by [`Host`], before the event reaches the rest
    /// of the tree, so every widget reads this frame's values.
    ///
    /// The real elapsed time is used; springs are evaluated in closed form,
    /// so wall-clock and animation time agree frame by frame. A timestamp
    /// equal to or older than the last one advances nothing and only reports
    /// whether frames are still wanted.
    ///
    /// Two gaps are not animation time and are not spent as such. A frame
    /// that follows a tick which left everything at rest starts the clock and
    /// advances nothing, exactly as the very first tick of an engine does:
    /// frames are produced on demand, so the interval before an animation
    /// began is however long the interface sat still, not motion anybody
    /// missed. Beyond that, a single frame spends at most 1/15 s, so a stall
    /// — a pipeline compiling, a window coming back from behind another —
    /// resumes the animation instead of teleporting it to the end.
    ///
    /// [`Host`]: crate::widget::Host
    #[must_use = "the status says whether to request a redraw and invalidate the layout"]
    pub fn tick(&self, now: Instant) -> TickStatus {
        self.tick_from(now, None)
    }

    /// [`tick`](Self::tick) as called by a [`Host`], which identifies itself
    /// so two hosts in one view can be reported.
    ///
    /// [`Host`]: crate::widget::Host
    pub(crate) fn tick_from(&self, now: Instant, host: Option<HostId>) -> TickStatus {
        if let Some(host) = host {
            self.note_host(host);
        }

        let dt = {
            let mut last = self
                .0
                .last_tick
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);

            match *last {
                // A repeated or older timestamp advances nothing. Two redraws
                // can legitimately share an instant; it is not a diagnostic.
                Some(previous) if now <= previous => return self.pending_status(),
                Some(previous) => {
                    *last = Some(now);
                    now.saturating_duration_since(previous).as_secs_f32()
                }
                None => {
                    *last = Some(now);
                    0.0
                }
            }
        };

        self.0.ticked.store(true, Ordering::Relaxed);

        // Nothing was moving when the last frame was drawn, so no frames have
        // been produced since: the gap is idle time, not a long frame. The
        // clock restarts, the same as on the engine's first tick ever, and
        // this frame's own delta is the next one's to spend.
        let dt = if self.0.was_moving.load(Ordering::Relaxed) {
            dt.min(MAX_FRAME)
        } else {
            0.0
        };

        if dt <= 0.0 {
            // The clock starts (or restarts); nothing advances. A track that
            // was just retargeted reports as animating, so the host asks for
            // the frame that will move it.
            let status = self.pending_status();
            self.0.was_moving.store(status.animating, Ordering::Relaxed);
            return status;
        }

        let tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut status = TickStatus::default();

        for track in tracks.values() {
            match track.tick(dt) {
                Step::Settled => {}
                Step::Holding => status.animating = true,
                Step::Moved => {
                    status.animating = true;
                    let layout = track.tier() == Some(Tier::Layout);
                    // Unmarked reads as Paint: a redraw, never a relayout.
                    status.layout_invalid |= layout;
                    if layout && track.is_forever() {
                        track.warn_forever_layout();
                    }
                }
            }
        }

        self.0.was_moving.store(status.animating, Ordering::Relaxed);

        status
    }

    /// The status for a frame that advanced nothing: whether tracks are still
    /// waiting for frames, never a stale layout.
    fn pending_status(&self) -> TickStatus {
        let tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        TickStatus {
            animating: tracks.values().any(|track| !track.is_settled()),
            layout_invalid: false,
        }
    }

    /// Records that a `view()` build has finished.
    ///
    /// This is the garbage collector's clock. It ticks per build rather than
    /// per frame because a build is the unit in which the application
    /// declares which tracks it still wants. [`Host::new`] calls it once per
    /// `view()`, so an application using the host never calls it directly;
    /// a custom host or a headless test drives the engine with `end_build`
    /// and [`collect`].
    ///
    /// [`Host::new`]: crate::widget::Host::new
    /// [`collect`]: Self::collect
    pub fn end_build(&self) {
        let ended = self.0.build.fetch_add(1, Ordering::Relaxed);
        let tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        for track in tracks.values() {
            track.stamp(ended);
        }
        drop(tracks);

        if self.never_ticked_stale() && !self.0.warned_never_ticked.swap(true, Ordering::Relaxed) {
            log::warn!(
                "an animation started several view builds ago on a `Motion` that has never been \
                 ticked; wrap the view in `Host` (`motion.host(view)`) or nothing will move"
            );
        }
    }

    /// Drops tracks that have settled (a track repeating for ever counts as
    /// settled here), that no [`Anim`] handle references, and that no view
    /// build has touched for a few builds.
    ///
    /// Keys are derived from call sites and runtime data, so a long-running
    /// application would otherwise accumulate a track per element it ever
    /// showed. [`Host`] runs this once per view build; call it yourself only
    /// when driving the engine without a host.
    ///
    /// [`Host`]: crate::widget::Host
    pub fn collect(&self) {
        let build = self.0.build.load(Ordering::Relaxed);
        let mut tracks = self
            .0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        // A track is collectable only when nobody but this map holds it, it
        // has stopped, and no build has touched it for a while. The handle
        // count is what makes a stored `Anim` a real reference.
        tracks.retain(|_, track| {
            Arc::strong_count(track) > 1
                || (!track.is_settled() && !track.is_forever())
                || build.saturating_sub(track.last_touched()) < GC_IDLE_BUILDS
        });
    }

    /// Runs [`collect`](Self::collect) once per view build rather than once
    /// per frame.
    pub(crate) fn gc_if_built(&self) {
        let build = self.0.build.load(Ordering::Relaxed);
        if self.0.last_gc_build.swap(build, Ordering::Relaxed) != build {
            self.collect();
        }
    }

    /// Returns the number of live tracks. Intended for demos and diagnostics.
    #[must_use]
    pub fn track_count(&self) -> usize {
        self.0
            .tracks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use iced::Color;
    use iced::time::Instant;

    use super::{GC_IDLE_BUILDS, HostId, MAX_FRAME};
    use crate::testing::FrameClock;
    use crate::{Curve, Motion, Presence, SpringParams, Tier, key};

    /// A fast spring for tests; not the shipped `curves::SMOOTH`.
    const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

    #[test]
    fn track_settles_at_its_target() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        // First sight starts *at* the target, so move it afterwards.
        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 100.0_f32);

        assert!(value.is_animating(), "a retarget should start the track");

        let _ = clock.run(120);

        assert!(!value.is_animating(), "0.3s spring should settle within 2s");
        assert_eq!(value.get(), 100.0, "a settled track sits exactly on target");
    }

    #[test]
    fn first_sight_does_not_animate() {
        let m = Motion::new();
        let value = m.to(key!(), FAST, 42.0_f32);

        assert_eq!(value.get(), 42.0);
        assert!(
            !value.is_animating(),
            "a value seen for the first time should not fly in from zero"
        );
    }

    #[test]
    fn retargeting_the_same_value_is_a_no_op() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 0.0_f32);
        let _ = m.to(key, FAST, 100.0_f32);

        let _ = clock.run(8);

        let mid = m.to(key, FAST, 100.0_f32).get();

        // Re-declaring the same target on every rebuild must not restart the
        // curve, or an animation would never finish while the view is rebuilding.
        for _ in 0..8 {
            let _ = m.to(key, FAST, 100.0_f32);
        }

        let _ = clock.run(8);

        let later = m.to(key, FAST, 100.0_f32).get();

        assert!(
            later > mid,
            "repeated identical targets should keep advancing ({mid} -> {later})"
        );
    }

    #[test]
    fn retargeting_mid_flight_preserves_velocity() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 0.0_f32);
        let _ = m.to(key, FAST, 100.0_f32);

        let _ = clock.run(6);

        let at_reversal = m.to(key, FAST, 0.0_f32).get();
        assert!(at_reversal > 0.0, "the spring should have left the origin");

        // A reference spring that starts *at rest* from the same point.
        let fresh = Motion::new();
        let mut fresh_clock = FrameClock::new(&fresh);
        let _ = fresh.to(key, FAST, at_reversal);
        let from_rest = fresh.to(key, FAST, 0.0_f32);
        // The first tick of an engine only starts its clock; the second is the
        // one real frame.
        let _ = fresh_clock.run(2);

        let _ = clock.run(1);
        let after = m.to(key, FAST, 0.0_f32).get();

        assert!(
            after > from_rest.get(),
            "a spring reversed mid-flight keeps its outward momentum \
             ({at_reversal} -> {after}, from rest it would be {}); \
             being no higher than a restarted spring means velocity was lost",
            from_rest.get()
        );
    }

    #[test]
    fn ease_respects_its_duration() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let curve = Curve::ease(iced::animation::Easing::Linear, Duration::from_millis(500));

        let _ = m.to(key, curve, 0.0_f32);
        let value = m.to(key, curve, 100.0_f32);

        let _ = clock.run(16); // ~0.25s

        let halfway = value.get();
        assert!(
            (40.0..60.0).contains(&halfway),
            "a linear 500ms ease should be near half way at 250ms, got {halfway}"
        );

        let _ = clock.run(20); // past 500ms

        assert!(!value.is_animating());
        assert_eq!(value.get(), 100.0);
    }

    #[test]
    fn a_delayed_curve_holds_its_pose_first() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let curve = Curve::ease(iced::animation::Easing::Linear, Duration::from_millis(200))
            .delayed(Duration::from_millis(300));

        let _ = m.to(key, curve, 0.0_f32);
        let value = m.to(key, curve, 100.0_f32);

        let _ = clock.run(12); // ~0.2s, still inside the delay

        assert_eq!(value.get(), 0.0, "a delayed curve must not move early");
        assert!(value.is_animating(), "but it is still a pending animation");

        let _ = clock.run(40);

        assert_eq!(value.get(), 100.0);
    }

    #[test]
    fn layout_tier_is_reported_separately_from_paint() {
        let m = Motion::new();
        let paint = key!();
        let layout = key!();

        let _ = m.to(paint, FAST, 0.0_f32);
        let _ = m.to(layout, FAST, 0.0_f32);

        let painted = m.to(paint, FAST, 1.0_f32);
        painted.mark_tier(Tier::Paint);

        let start = Instant::now();
        let status = m.tick(start + Duration::from_millis(16));

        assert!(status.animating);
        assert!(
            !status.layout_invalid,
            "a paint-tier animation must not force a relayout every frame"
        );

        let sized = m.to(layout, FAST, 1.0_f32);
        sized.mark_tier(Tier::Layout);

        let status = m.tick(start + Duration::from_millis(32));

        assert!(
            status.layout_invalid,
            "a layout-tier animation must relayout"
        );
    }

    #[test]
    fn gc_collects_settled_tracks_but_keeps_moving_ones() {
        let m = Motion::new();

        let settled = key!();
        let moving = key!();

        let _ = m.to(settled, FAST, 1.0_f32);
        let _ = m.to(moving, FAST, 0.0_f32);
        let _ = m.to(moving, FAST, 1000.0_f32);

        assert_eq!(m.track_count(), 2);

        let mut clock = FrameClock::new(&m);

        // A few frames in, the settled track is already collectable by value but
        // not yet by age. No rebuild has dropped it from the view.
        let _ = clock.run(10);
        m.collect();
        assert_eq!(m.track_count(), 2, "recently touched tracks are kept");

        // Let the moving track finish, then rebuild the view repeatedly without
        // mentioning either key. Builds, not frames, are what age a track out.
        let _ = clock.run(200);

        for _ in 0..GC_IDLE_BUILDS {
            m.end_build();
            m.collect();
        }

        assert_eq!(
            m.track_count(),
            0,
            "both tracks have settled and the view stopped referencing them"
        );
    }

    #[test]
    fn gc_keeps_tracks_the_view_still_reads() {
        let m = Motion::new();
        let key = key!();

        // A widget reads the value during build 0, then the handle is gone.
        let value = m.to(key, FAST, 1.0_f32);
        let _ = value.get();
        drop(value);

        // The read stamps build 0 at `end_build`; the track then survives
        // fewer than `GC_IDLE_BUILDS` idle builds…
        for build in 1..GC_IDLE_BUILDS {
            m.end_build();
            m.collect();
            assert_eq!(
                m.track_count(),
                1,
                "still in the grace period at build {build}"
            );
        }

        // …and is collected once the grace period has passed.
        m.end_build();
        m.collect();
        assert_eq!(
            m.track_count(),
            0,
            "unreferenced and idle past the grace period"
        );
    }

    #[test]
    fn presence_follows_the_enter_and_exit_lifecycle() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        assert_eq!(m.presence(key), Presence::Gone, "an unseen key is Gone");

        let _ = m.enter(key, FAST, 0.0_f32, 1.0_f32);
        assert_eq!(m.presence(key), Presence::Entering);

        let _ = clock.run(120);
        assert_eq!(m.presence(key), Presence::Present);

        let _ = m.retire(key, FAST, 0.0_f32);
        assert_eq!(m.presence(key), Presence::Exiting);

        let _ = clock.run(120);
        assert_eq!(
            m.presence(key),
            Presence::Gone,
            "a settled exit frees the application to drop the item"
        );
    }

    #[test]
    fn enter_only_replays_on_first_sight() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let value = m.enter(key, FAST, 0.0_f32, 100.0_f32);
        let _ = clock.run(120);
        assert_eq!(value.get(), 100.0);

        // A rebuild re-declares the same enter; it must not snap back to 0.
        let value = m.enter(key, FAST, 0.0_f32, 100.0_f32);
        assert_eq!(value.get(), 100.0, "enter must not replay on every rebuild");
    }

    #[test]
    fn animation_time_tracks_wall_clock_at_any_frame_rate() {
        // Two engines run the same animation over the same wall-clock span, one at
        // 60 FPS and one at 20 FPS, and must land in the same place: springs are
        // evaluated in closed form, so the frame interval does not matter.
        let smooth = Motion::new();
        let choppy = Motion::new();
        let key = key!();

        for m in [&smooth, &choppy] {
            let _ = m.to(key, FAST, 0.0_f32);
            let _ = m.to(key, FAST, 100.0_f32);
        }

        let start = Instant::now();

        for frame in 1..=30_u64 {
            let _ = smooth.tick(start + Duration::from_micros(16_667 * frame));
        }

        for frame in 1..=10_u64 {
            let _ = choppy.tick(start + Duration::from_micros(50_000 * frame));
        }

        let a = smooth.to(key, FAST, 100.0_f32).get();
        let b = choppy.to(key, FAST, 100.0_f32).get();

        assert!(
            (a - b).abs() < 2.0,
            "same elapsed time must give the same progress, got {a} at 60 FPS vs {b} at 20 FPS"
        );
    }

    #[test]
    fn a_composite_tier_animation_never_invalidates_the_layout() {
        // Opacity is consumed by the compositor, so a fade must cost a redraw and
        // nothing more. It does not run layout or record the cached texture again.
        let m = Motion::new();
        let key = key!();

        let _ = m.to(key, FAST, 1.0_f32);
        let fade = m.to(key, FAST, 0.0_f32);

        fade.mark_tier(Tier::Composite);

        let start = Instant::now();
        let status = m.tick(start + Duration::from_millis(16));

        assert!(status.animating, "the fade still needs frames");
        assert!(
            !status.layout_invalid,
            "a fade must not drag a relayout along with it"
        );
    }

    #[test]
    fn a_track_only_the_view_references_survives_idle_frames() {
        let m = Motion::new();
        let key = key!();

        // One view build. The track exists, but nothing binds it into a widget.
        // The view holds it for a state it is not currently in (an exit fade on a
        // row that is still present, say), so no `layout` or `draw` reads it.
        m.end_build();
        let _ = m.to(key, FAST, 1.0_f32);
        m.end_build();

        // The window keeps redrawing anyway: the cursor moves over it, a
        // neighbouring animation runs. None of that rebuilds the view.
        let mut clock = FrameClock::new(&m);
        for _ in 0..300 {
            let _ = clock.run(1);
            m.collect();
        }

        assert_eq!(
            m.track_count(),
            1,
            "the view still references this track; only a rebuild without it may collect it"
        );
    }

    #[test]
    fn an_exit_animates_after_the_view_has_been_idle() {
        let m = Motion::new();
        let fade = key!();

        // The row is on screen and opaque.
        m.end_build();
        let _ = m.to(fade, FAST, 1.0_f32);
        m.end_build();

        // Long idle with no rebuild.
        let mut clock = FrameClock::new(&m);
        for _ in 0..300 {
            let _ = clock.run(1);
            m.collect();
        }

        // Now the user removes the row.
        let value = m.retire(fade, FAST, 0.0_f32);

        assert_eq!(
            m.presence(fade),
            Presence::Exiting,
            "a collected track would be recreated already sitting on its exit \
             value, and the row would blink out instead of fading"
        );
        assert_eq!(value.get(), 1.0, "the exit starts from the pose it had");
    }

    #[test]
    fn a_retired_key_can_come_back() {
        let m = Motion::new();
        let key = key!();
        let mut clock = FrameClock::new(&m);
        let _ = m.enter(key, FAST, 0.0_f32, 1.0_f32);
        let _ = clock.run(120);
        assert_eq!(m.presence(key), Presence::Present);

        let _ = m.retire(key, FAST, 0.0_f32);
        assert_eq!(m.presence(key), Presence::Exiting);

        // Undo: the application re-declares the key with `to`.
        let _ = m.to(key, FAST, 1.0_f32);
        assert_eq!(
            m.presence(key),
            Presence::Present,
            "re-declaring cancels the exit"
        );
        let _ = clock.run(120);
        assert_eq!(m.presence(key), Presence::Present);
    }

    #[test]
    fn changing_spring_params_mid_flight_keeps_velocity() {
        let m = Motion::new();
        let key = key!();
        let mut clock = FrameClock::new(&m);
        let slow = Curve::spring(SpringParams::new(0.0, Duration::from_millis(600)));
        let fast = Curve::spring(SpringParams::new(0.0, Duration::from_millis(200)));

        let _ = m.to(key, slow, 0.0_f32);
        let _ = m.to(key, slow, 100.0_f32);
        let _ = clock.run(6);
        let before = m.to(key, slow, 100.0_f32).get();
        assert!(before > 0.0 && before < 100.0);

        // Same target, different tuning: must not snap velocity to zero.
        let retuned = m.to(key, fast, 100.0_f32);
        let _ = clock.run(1);
        let after = retuned.get();
        assert!(
            after > before,
            "the value kept moving toward the target: {before} -> {after}"
        );
    }

    #[test]
    fn a_backwards_timestamp_does_not_rewind_the_clock() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 100.0_f32);

        let start = Instant::now();
        let _ = m.tick(start);
        let _ = m.tick(start + Duration::from_millis(16));
        let after_one_frame = value.get();
        // An older timestamp must be ignored, not stored…
        let _ = m.tick(start);
        assert_eq!(value.get(), after_one_frame);
        // …so the next real frame is a normal 16 ms step, not a 32 ms jump.
        let _ = m.tick(start + Duration::from_millis(32));
        let after_two = value.get();
        let m2 = Motion::new();
        let _ = m2.to(key, FAST, 0.0_f32);
        let reference = m2.to(key, FAST, 100.0_f32);
        let _ = m2.tick(start);
        let _ = m2.tick(start + Duration::from_millis(16));
        let _ = m2.tick(start + Duration::from_millis(32));
        assert!((after_two - reference.get()).abs() < 1e-3);
    }

    #[test]
    fn a_rebuild_during_an_enter_keeps_reporting_entering() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let _ = m.enter(key, FAST, 0.0_f32, 1.0_f32);
        let _ = clock.run(3);
        assert_eq!(m.presence(key), Presence::Entering);
        let _ = m.enter(key, FAST, 0.0_f32, 1.0_f32); // view rebuilt
        assert_eq!(
            m.presence(key),
            Presence::Entering,
            "a rebuild does not end the entrance"
        );
        let _ = clock.run(120);
        assert_eq!(m.presence(key), Presence::Present);
    }

    #[test]
    fn play_restarts_the_sequence_on_every_call() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let value = m.play(key, FAST, 0.0_f32, 100.0_f32);
        let _ = clock.run(10);
        assert!(value.get() > 0.0);
        let _ = m.play(key, FAST, 0.0_f32, 100.0_f32);
        assert_eq!(value.get(), 0.0, "restarted from `from`");
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "different `Animatable` type")]
    fn reusing_a_key_with_another_type_is_caught() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 1.0_f32);
        let _ = m.to(key, FAST, Color::BLACK);
    }

    #[test]
    fn switching_curve_family_mid_flight_keeps_the_value_continuous() {
        use crate::curves::FADE;
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let _ = m.to(key, FAST, 100.0_f32);
        let _ = clock.run(6);
        let before = m.to(key, FAST, 100.0_f32).get();
        // Same target, ease instead of spring: rebuilt from the current value.
        let value = m.to(key, FADE, 100.0_f32);
        assert!((value.get() - before).abs() < 1e-3, "no jump at the switch");
        let _ = clock.run(1);
        assert!(value.get() > before && value.get() < 100.0);
    }

    #[test]
    fn retiring_an_unseen_key_is_immediately_gone() {
        let m = Motion::new();
        let key = key!();

        let value = m.retire(key, FAST, 0.0_f32);

        assert_eq!(
            value.get(),
            0.0,
            "an unseen key is created at its exit pose"
        );
        assert!(!value.is_animating());
        assert_eq!(m.presence(key), Presence::Gone);
    }

    #[test]
    fn redeclaring_with_to_ends_an_entrance() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.enter(key, FAST, 0.0_f32, 1.0_f32);
        let _ = clock.run(3);
        assert_eq!(m.presence(key), Presence::Entering);

        let _ = m.to(key, FAST, 1.0_f32);
        assert_eq!(
            m.presence(key),
            Presence::Present,
            "`to` cancels an entrance for the purposes of `presence`"
        );
    }

    /// In a debug build a non-finite target is a caught programming error.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "must be finite")]
    fn a_non_finite_target_is_caught_in_debug() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 10.0_f32);
        let _ = m.to(key, FAST, f32::NAN);
    }

    /// In a release build a bad division in application code must degrade to
    /// "the value stays where it is", never to a track that spins forever.
    #[cfg(not(debug_assertions))]
    #[test]
    fn a_non_finite_target_is_replaced_and_the_track_settles() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 10.0_f32);
        let value = m.to(key, FAST, f32::NAN);

        assert_eq!(value.get(), 10.0, "the current value stands in for NaN");
        assert!(!value.is_animating());
        assert!(clock.run_until_settled() < 3);

        let played = m.play(key, FAST, f32::INFINITY, 20.0_f32);
        assert_eq!(played.get(), 10.0, "`from` falls back to the current value");
        let frames = clock.run_until_settled();
        assert!(frames < 200, "settles normally: {frames}");
        assert_eq!(played.get(), 20.0);
    }

    #[test]
    fn a_held_handle_keeps_its_track_alive_and_driven() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        // A widget stores the handle in its `tree::State` and never re-declares
        // the key; the view rebuilds idly many times.
        let held = m.to(key, FAST, 1.0_f32);
        for _ in 0..(GC_IDLE_BUILDS * 3) {
            m.end_build();
            m.collect();
        }
        assert_eq!(m.track_count(), 1, "a referenced track is never collected");

        // The next retarget must animate from the held pose, not restart.
        let again = m.to(key, FAST, 0.0_f32);
        assert!(again.is_animating());
        let _ = clock.run(3);
        let mid = held.get();
        assert!(
            mid > 0.0 && mid < 1.0,
            "the old handle follows the same track: {mid}"
        );

        drop(held);
        drop(again);
        let _ = clock.run_until_settled();
        for _ in 0..GC_IDLE_BUILDS {
            m.end_build();
            m.collect();
        }
        assert_eq!(
            m.track_count(),
            0,
            "unreferenced, settled and idle: collected"
        );
    }

    #[test]
    fn a_holding_delay_does_not_invalidate_the_layout() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let curve = FAST.delayed(Duration::from_millis(300));

        let _ = m.to(key, curve, 0.0_f32);
        let value = m.to(key, curve, 100.0_f32);
        value.mark_tier(Tier::Layout);

        let _ = clock.run(1); // starts the clock
        let holding = clock.run(1);
        assert!(holding.animating, "still waiting for the delay");
        assert!(
            !holding.layout_invalid,
            "nothing moved, so nothing to relayout"
        );

        let _ = clock.run(20); // past the 300 ms delay
        let moving = clock.run(1);
        assert!(moving.animating && moving.layout_invalid);
    }

    #[test]
    fn layout_invalid_returns_to_false_once_the_track_settles() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 100.0_f32);
        value.mark_tier(Tier::Layout);

        let _ = clock.run_until_settled();
        let after = clock.run(1);
        assert!(!after.animating);
        assert!(!after.layout_invalid);
    }

    #[test]
    fn a_repeated_timestamp_is_ignored_without_complaint() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 100.0_f32);

        let start = Instant::now();
        let _ = m.tick(start);
        let _ = m.tick(start + Duration::from_millis(16));
        let after = value.get();
        let status = m.tick(start + Duration::from_millis(16));

        assert_eq!(value.get(), after);
        assert!(status.animating);
        assert!(!m.two_hosts_seen(), "one timestamp twice is not two hosts");
    }

    #[test]
    fn two_hosts_in_one_build_are_detected() {
        let m = Motion::new();
        let start = Instant::now();

        let a = HostId::next();
        let b = HostId::next();

        m.end_build();
        let _ = m.tick_from(start, Some(a));
        let _ = m.tick_from(start + Duration::from_millis(16), Some(a));
        assert!(!m.two_hosts_seen(), "one host per build is fine");

        let _ = m.tick_from(start + Duration::from_millis(32), Some(b));
        assert!(m.two_hosts_seen(), "a second host in the same build is not");
    }

    #[test]
    fn a_new_host_per_build_is_not_two_hosts() {
        let m = Motion::new();
        let start = Instant::now();

        for frame in 0..5_u64 {
            m.end_build();
            let host = HostId::next();
            let _ = m.tick_from(start + Duration::from_millis(16 * frame), Some(host));
        }

        assert!(!m.two_hosts_seen());
    }

    #[test]
    fn the_never_ticked_warning_waits_two_builds() {
        let m = Motion::new();
        let key = key!();

        let _ = m.to(key, FAST, 0.0_f32);
        let _ = m.to(key, FAST, 1.0_f32); // animation starts in build 0
        m.end_build();
        assert!(
            !m.never_ticked_stale(),
            "one build is a startup task result"
        );
        m.end_build();
        assert!(!m.never_ticked_stale());
        m.end_build();
        assert!(
            m.never_ticked_stale(),
            "three builds without a frame is a missing host"
        );

        let _ = m.tick(Instant::now());
        assert!(!m.never_ticked_stale(), "a tick clears it");
    }

    #[test]
    fn get_looks_up_a_track_without_retargeting_it() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        assert!(m.get::<f32>(key).is_none(), "nothing declared yet");

        // `play` in `update`…
        let _ = m.play(key, FAST, 0.0_f32, 100.0_f32);
        // …`get` in `view`, on every rebuild, without restarting the sequence.
        let _ = clock.run(10);
        let seen = m.get::<f32>(key).expect("the track exists");
        let before = seen.get();
        assert!(before > 0.0);
        let again = m.get::<f32>(key).expect("still there");
        assert_eq!(again.get(), before, "a lookup does not restart or retarget");

        let _ = clock.run_until_settled();
        assert_eq!(seen.get(), 100.0);
    }

    #[test]
    fn a_retarget_during_a_delayed_spring_holds_then_resumes_with_velocity() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let curve = FAST.delayed(Duration::from_millis(100));

        let _ = m.to(key, curve, 0.0_f32);
        let value = m.to(key, curve, 100.0_f32);
        let _ = clock.run(1);
        let _ = clock.run(12); // ~100 ms delay burnt, then ~100 ms of motion
        let moving = value.get();
        assert!(moving > 0.0 && moving < 100.0, "in flight: {moving}");

        // A reference spring at rest at the same pose, under the same delayed
        // curve, retargeted at the same moment.
        let rest = Motion::new();
        let mut rest_clock = FrameClock::new(&rest);
        let _ = rest.to(key, curve, moving);
        let _ = rest_clock.run(1); // start the clock first, so both see the same elapsed time
        let from_rest = rest.to(key, curve, 0.0_f32);
        // This engine has been sitting at rest, so its next frame only
        // restarts the clock, while `m` has been animating without a break.
        // Spending that frame here leaves both advancing frame for frame.
        let _ = rest_clock.run(1);

        // Retarget mid-flight: the pose holds for the delay…
        let _ = m.to(key, curve, 0.0_f32);
        let _ = clock.run(3);
        let _ = rest_clock.run(3);
        assert_eq!(value.get(), moving, "held during the new delay");

        // …then continues with the outward velocity it had, so it stays further
        // from the new target than the spring that started from rest.
        let _ = clock.run(4);
        let _ = rest_clock.run(4);
        assert!(
            value.get() > from_rest.get(),
            "momentum carried across the delay: {} vs {} from rest",
            value.get(),
            from_rest.get()
        );
        let _ = clock.run_until_settled();
        assert_eq!(value.get(), 0.0);
    }

    #[test]
    fn a_custom_easing_is_idempotent_across_rebuilds() {
        fn quadratic(x: f32) -> f32 {
            x * x
        }
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let curve = Curve::ease(crate::Easing::Custom(quadratic), Duration::from_millis(500));

        let _ = m.to(key, curve, 0.0_f32);
        let value = m.to(key, curve, 100.0_f32);
        let _ = clock.run(1);
        let _ = clock.run(8);
        let mid = value.get();
        assert!(mid > 0.0 && mid < 100.0);

        // The same fn pointer compares equal, so the rebuild is a no-op.
        let _ = m.to(
            key,
            Curve::ease(crate::Easing::Custom(quadratic), Duration::from_millis(500)),
            100.0_f32,
        );
        let _ = clock.run(1);
        assert!(value.get() > mid, "not restarted from zero");
    }

    #[test]
    fn a_zero_duration_ease_completes_on_its_first_frame() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let curve = Curve::ease(crate::Easing::Linear, Duration::ZERO);

        let _ = m.to(key, curve, 0.0_f32);
        let value = m.to(key, curve, 100.0_f32);
        let _ = clock.run(1);
        let _ = clock.run(1);
        assert_eq!(value.get(), 100.0);
        assert!(!value.is_animating());
    }

    #[test]
    fn a_stalled_frame_resumes_the_animation_instead_of_teleporting_it() {
        let m = Motion::new();
        let key = key!();
        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 100.0_f32);

        // Two real frames, so the clock is running and the spring is moving.
        let start = Instant::now();
        let _ = m.tick(start);
        let _ = m.tick(start + Duration::from_millis(16));
        let before = value.get();
        assert!(before > 0.0 && before < 100.0, "in flight: {before}");

        // Then the window is occluded (or a pipeline compiles) for half a
        // minute. The frame that follows spends `MAX_FRAME`, not the stall.
        let status = m.tick(start + Duration::from_secs(30));
        let after = value.get();
        assert!(status.animating, "the spring is still on its way");
        assert!(
            after > before && after < 100.0,
            "a stall resumes the spring rather than landing it: \
             {before} -> {after}"
        );

        // A capped frame is worth at most `MAX_FRAME` of animation, and the
        // spring still arrives under its own steam.
        let reference = Motion::new();
        let _ = reference.to(key, FAST, 0.0_f32);
        let paced = reference.to(key, FAST, 100.0_f32);
        let mut at = start;
        for _ in 0..2 {
            at += Duration::from_millis(16);
            let _ = reference.tick(at);
        }
        at += Duration::from_secs_f32(MAX_FRAME);
        let _ = reference.tick(at);
        assert_eq!(
            after,
            paced.get(),
            "the stalled frame advanced exactly one `MAX_FRAME`"
        );
    }

    #[test]
    fn a_pause_before_an_animation_is_not_charged_to_its_first_frame() {
        // The interface sits still with the cursor resting on a button: it is
        // drawing no frames at all. The click then starts an animation, and
        // the frame it arrives on must not be handed the pause — which, for a
        // spring this fast, would be its whole flight. How long the pause was
        // must make no difference whatsoever.
        let key = key!();

        let after_a_pause = |pause: Duration| {
            let m = Motion::new();
            let mut at = Instant::now();
            let _ = m.to(key, FAST, 0.0_f32);

            // Settled frames: a hover, a cursor crossing the button.
            for _ in 0..3 {
                let _ = m.tick(at);
                at += Duration::from_millis(16);
            }

            // Nothing moves, so nothing is drawn, for however long.
            at += pause;

            let clicked = m.to(key, FAST, 100.0_f32);
            let mut trajectory = Vec::new();
            for _ in 0..6 {
                let _ = m.tick(at);
                trajectory.push(clicked.get());
                at += Duration::from_millis(16);
            }
            trajectory
        };

        let brief = after_a_pause(Duration::from_millis(16));
        assert_eq!(
            brief[0], 0.0,
            "the frame that restarts the clock advances nothing"
        );
        assert!(
            brief[1] > 0.0 && brief[1] < 40.0,
            "the frame after it is worth one frame: {brief:?}"
        );
        assert!(
            brief.windows(2).all(|w| w[1] >= w[0]),
            "monotonic: {brief:?}"
        );

        for pause in [200, 400, 5_000] {
            assert_eq!(
                after_a_pause(Duration::from_millis(pause)),
                brief,
                "a {pause} ms pause changed the animation that followed it"
            );
        }
    }

    #[test]
    fn presence_keeps_a_track_out_of_the_collector() {
        let m = Motion::new();
        let key = key!();
        let _ = m.retire(key, FAST, 0.0_f32); // settled at once, handle dropped

        for _ in 0..(GC_IDLE_BUILDS * 2) {
            let _ = m.presence(key); // the view asks every build
            m.end_build();
            m.collect();
        }
        assert_eq!(m.track_count(), 1, "asking about a key touches it");
    }
}
