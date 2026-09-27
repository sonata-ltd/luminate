//! Curves that run more than once.

use std::time::Duration;

/// How many times a curve runs, whether every other run goes back, and how
/// long it pauses in between. Attach one with
/// [`Curve::repeat`](crate::Curve::repeat).
///
/// ```
/// use std::time::Duration;
/// use iced_animate::{Curve, Easing, Repeat};
///
/// // A pulse: out and back, for ever.
/// const PULSE: Curve = Curve::ease(Easing::EaseInOut, Duration::from_millis(600))
///     .repeat(Repeat::forever().alternate());
/// ```
///
/// **A run** goes from the start of the cycle to its end. For
/// [`Motion::play`](crate::Motion::play) those are `from` and `to`; for
/// [`Motion::to`](crate::Motion::to) the start is the value at the moment of
/// the retarget. A key seen for the first time is created *at* its target,
/// so `to` on a new key has nothing to cycle between and rests: start a
/// pulse that should run from the first frame with `play`.
///
/// **Counting.** [`times(n)`](Self::times) is the number of runs in total,
/// not of repeats after the first: `times(1)` is a plain curve. CSS
/// (`animation-iteration-count`) counts the same way; anime.js's `loop: n`
/// is `times(n + 1)`.
///
/// **Alternate** runs the even runs backwards, in reversed time: an ease-out
/// going out is an ease-in coming back, as with CSS `alternate`. Without it
/// the value jumps back to the start after every run.
///
/// **Springs** end a run when they settle, so a repeating spring overshoots
/// at every turn; the time a frame runs past that point is not carried into
/// the next run. **Eases** carry it, so a thousand runs end on time.
///
/// **Cost.** A track repeating [`forever`](Self::forever) asks for a frame
/// on every frame while it lives, and one read during `layout` relayouts on
/// every frame (the engine logs a warning once). It is collected like a
/// settled track: once nothing holds a handle and no build has touched it
/// for a few builds.
///
/// `enter` and `retire` ignore a repeat, with a warning: an entrance and an
/// exit must end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Repeat {
    /// Total runs; `0` for ever.
    runs: u32,
    alternate: bool,
    gap: Duration,
}

impl Repeat {
    /// One run: no repeat.
    pub const ONCE: Self = Self {
        runs: 1,
        alternate: false,
        gap: Duration::ZERO,
    };

    /// `runs` runs in total; `0` counts as `1`.
    #[must_use]
    pub const fn times(runs: u32) -> Self {
        Self {
            runs: if runs == 0 { 1 } else { runs },
            ..Self::ONCE
        }
    }

    /// Runs until the track is retargeted or collected.
    #[must_use]
    pub const fn forever() -> Self {
        Self {
            runs: 0,
            ..Self::ONCE
        }
    }

    /// Runs every other run backwards.
    #[must_use]
    pub const fn alternate(mut self) -> Self {
        self.alternate = true;
        self
    }

    /// Holds the end of each run for `pause` before the next one starts.
    #[must_use]
    pub const fn gap(mut self, pause: Duration) -> Self {
        self.gap = pause;
        self
    }

    /// The number of runs, or `None` for ever.
    #[must_use]
    pub const fn runs(self) -> Option<u32> {
        if self.runs == 0 {
            None
        } else {
            Some(self.runs)
        }
    }

    /// `true` when it never stops.
    #[must_use]
    pub const fn is_forever(self) -> bool {
        self.runs == 0
    }

    /// `true` when even runs go backwards.
    #[must_use]
    pub const fn is_alternate(self) -> bool {
        self.alternate
    }

    /// The pause between runs.
    #[must_use]
    pub const fn pause(self) -> Duration {
        self.gap
    }

    pub(crate) const fn is_once(self) -> bool {
        self.runs == 1
    }

    /// Whether the last run ends back at the start: an even number of
    /// alternating runs.
    pub(crate) const fn rests_at_start(self) -> bool {
        self.alternate && self.runs != 0 && self.runs.is_multiple_of(2)
    }

    /// Whether run `run` (counted from 1) goes from the end to the start.
    pub(crate) const fn is_reversed(self, run: u32) -> bool {
        self.alternate && run.is_multiple_of(2)
    }

    /// Whether another run follows run `run`.
    pub(crate) const fn has_run_after(self, run: u32) -> bool {
        self.runs == 0 || run < self.runs
    }
}

impl Default for Repeat {
    fn default() -> Self {
        Self::ONCE
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::Repeat;
    use crate::engine::GC_IDLE_BUILDS;
    use crate::testing::{FRAME, FrameClock};
    use crate::{Curve, Easing, Motion, Presence, SpringParams, key};

    const MS_100: Duration = Duration::from_millis(100);

    fn linear() -> Curve {
        Curve::ease(Easing::Linear, MS_100)
    }

    #[test]
    fn a_thousand_eased_runs_end_on_time_and_stay_in_phase() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let value = m.play(key!(), linear().repeat(Repeat::times(1000)), 0.0_f32, 1.0);

        let _ = clock.run(1); // starts the clock
        let _ = clock.run(3001);
        let t = 3001.0 * FRAME.as_secs_f64();
        let expected = (t / 0.1).fract() as f32;
        assert!(
            (value.get() - expected).abs() < 2e-3,
            "after {t:.3} s: {} vs {expected}",
            value.get()
        );

        // 100 s at 60 Hz ends on frame 6000; the next frame reports rest.
        let frames = 3001 + clock.run_until_settled();
        assert!(
            (5999..=6002).contains(&frames),
            "settled after {frames} frames"
        );
        assert_eq!(value.get(), 1.0);
    }

    #[test]
    fn many_very_short_runs_stay_in_phase_and_end_on_time() {
        // One 16.7 ms frame swallows a hundred-odd 100 µs runs at once:
        // whole pairs of runs are dropped, not the elapsed time.
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let curve = Curve::ease(Easing::Linear, Duration::from_micros(100));
        let value = m.play(key!(), curve.repeat(Repeat::times(1000)), 0.0_f32, 1.0);

        let _ = clock.run(1); // starts the clock
        let _ = clock.run(1);
        let expected = (FRAME.as_secs_f64() / 1e-4).fract() as f32;
        assert!(
            (value.get() - expected).abs() < 2e-2,
            "{} vs {expected}",
            value.get()
        );

        // 0.1 s in all: over within seven frames, not 1000 / 64.
        assert!(clock.run_until_settled() <= 7);
        assert_eq!(value.get(), 1.0);
    }

    #[test]
    fn an_alternate_run_comes_back_in_reversed_time() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let curve = Curve::ease(Easing::EaseOut, MS_100).repeat(Repeat::times(2).alternate());
        let value = m.play(key!(), curve, 0.0_f32, 1.0);

        let _ = clock.run(1);
        let _ = clock.run(9);
        let t = 9.0 * FRAME.as_secs_f32();
        let expected = Easing::EaseOut.value(1.0 - (t - 0.1) / 0.1);
        assert!(
            (value.get() - expected).abs() < 2e-3,
            "{} vs {expected}",
            value.get()
        );
    }

    #[test]
    fn a_finite_repeat_rests_where_its_last_run_ends_and_says_so_up_front() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let even = m.play(
            key!(),
            linear().repeat(Repeat::times(2).alternate()),
            0.0_f32,
            10.0,
        );
        let odd = m.play(
            key!(),
            linear().repeat(Repeat::times(3).alternate()),
            0.0_f32,
            10.0,
        );
        let plain = m.play(key!(), linear().repeat(Repeat::times(2)), 0.0_f32, 10.0);

        assert_eq!(even.target(), 0.0);
        assert_eq!(odd.target(), 10.0);
        assert_eq!(plain.target(), 10.0);

        let _ = clock.run_until_settled();
        assert_eq!((even.get(), odd.get(), plain.get()), (0.0, 10.0, 10.0));
    }

    #[test]
    fn a_repeating_spring_overshoots_at_both_turns() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let bouncy = Curve::spring(SpringParams::new(0.3, Duration::from_millis(300)));
        let once = m.play(key!(), bouncy, 0.0_f32, 100.0);
        let twice = m.play(
            key!(),
            bouncy.repeat(Repeat::times(2).alternate()),
            0.0_f32,
            100.0,
        );

        let _ = clock.run(1);
        let (mut peak, mut trough) = (f32::MIN, f32::MAX);
        let (mut once_done, mut frames) = (None, 0);
        while twice.is_animating() && frames < 2_000 {
            let _ = clock.run(1);
            frames += 1;
            peak = peak.max(twice.get());
            trough = trough.min(twice.get());
            if once_done.is_none() && !once.is_animating() {
                once_done = Some(frames);
            }
        }

        assert!(peak > 100.0, "overshoots on the way out: {peak}");
        assert!(trough < 0.0, "and on the way back: {trough}");
        assert_eq!(twice.get(), 0.0);
        let once_done = once_done.expect("a single run settles first") as f32;
        let ratio = frames as f32 / once_done;
        assert!(
            (1.6..2.4).contains(&ratio),
            "two runs take twice as long: {ratio}"
        );
    }

    #[test]
    fn a_gap_holds_the_end_of_a_run_without_relayout() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let value = m.play(
            key!(),
            linear().repeat(Repeat::times(2).gap(MS_100)),
            0.0_f32,
            1.0,
        );

        let _ = clock.run(1);
        let _ = clock.run(6); // 100.002 ms: the first run is over
        for frame in 7..=11 {
            let status = clock.run(1);
            assert_eq!(value.get(), 1.0, "held at frame {frame}");
            assert!(
                status.animating && !status.layout_invalid,
                "frame {frame}: {status:?}"
            );
        }
        let _ = clock.run(2); // 216.7 ms: 16.7 ms into the second run
        assert!((value.get() - 0.1667).abs() < 2e-3, "{}", value.get());
    }

    #[test]
    fn restating_a_repeat_keeps_it_going_and_a_plain_curve_stops_it() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();
        let pulse = linear().repeat(Repeat::forever().alternate());

        let _ = m.to(key, linear(), 0.0_f32);
        let value = m.to(key, pulse, 10.0_f32);
        let _ = clock.run(1);
        let _ = clock.run(8); // 133 ms: a third of the way back
        let mut last = value.get();
        for _ in 0..2 {
            let _ = m.to(key, pulse, 10.0_f32);
            let _ = clock.run(1);
            assert!(
                value.get() < last,
                "still coming back: {} after {last}",
                value.get()
            );
            last = value.get();
        }

        let _ = m.to(key, linear(), 10.0_f32);
        let _ = clock.run_until_settled();
        assert_eq!(value.get(), 10.0);
        assert!(!value.is_animating());
    }

    #[test]
    fn a_cycle_between_a_value_and_itself_settles_at_once() {
        // Review Focus 1.
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let forever = linear().repeat(Repeat::forever().alternate());

        let fresh = m.to(key!(), forever, 5.0_f32);
        assert!(!fresh.is_animating(), "a new key rests at its target");
        assert!(!clock.run(2).animating);

        let still = m.play(key!(), forever, 3.0_f32, 3.0);
        assert!(
            clock.run_until_settled() <= 10,
            "one run of nothing, then rest"
        );
        assert!(!still.is_animating());
    }

    #[test]
    fn an_abandoned_endless_track_is_collected_and_a_held_one_is_not() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let forever = linear().repeat(Repeat::forever());

        let held = m.play(key!(), forever, 0.0_f32, 1.0);
        drop(m.play(key!(), forever, 0.0_f32, 1.0));
        let _ = clock.run(2);

        for _ in 0..=GC_IDLE_BUILDS {
            m.end_build();
            m.collect();
        }
        assert_eq!(m.track_count(), 1);

        drop(held);
        for _ in 0..=GC_IDLE_BUILDS {
            m.end_build();
            m.collect();
        }
        assert_eq!(m.track_count(), 0);
        assert!(!clock.run(2).animating, "nothing left to ask for frames");
    }

    #[test]
    fn entrances_and_exits_run_once_whatever_the_curve() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let forever = linear().repeat(Repeat::forever());

        let arriving = key!();
        let _ = m.enter(arriving, forever, 0.0_f32, 1.0);
        assert_eq!(m.presence(arriving), Presence::Entering);

        let leaving = key!();
        let _ = m.to(leaving, linear(), 0.0_f32);
        let _ = m.retire(leaving, forever, 1.0_f32);
        assert_eq!(m.presence(leaving), Presence::Exiting);

        assert!(clock.run_until_settled() < 20);
        assert_eq!(m.presence(arriving), Presence::Present);
        assert_eq!(m.presence(leaving), Presence::Gone);
    }
}
