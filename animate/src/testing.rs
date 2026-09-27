//! Drives a [`Motion`] engine through synthetic frames in tests.
//!
//! Hidden from the documentation because it is not part of the animation
//! API, but public and stable so that downstream crates can test their own
//! widgets against the engine without a window.

use std::time::Duration;

use iced_core::time::Instant;

use crate::{Motion, TickStatus};

/// The interval between two frames of a [`FrameClock`]: 60 Hz.
pub const FRAME: Duration = Duration::from_micros(16_667);

/// How many frames [`FrameClock::run_until_settled`] tries before giving up.
const MAX_FRAMES: usize = 10_000;

#[cfg(feature = "geometry")]
thread_local! {
    static PATH_BUILDS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times a path widget has built its geometry on this thread.
///
/// Diagnostics only: tests use it to prove that a frame whose values did not
/// change reuses the cached geometry.
#[cfg(feature = "geometry")]
#[must_use]
pub fn path_geometry_builds() -> u64 {
    PATH_BUILDS.with(std::cell::Cell::get)
}

#[cfg(feature = "geometry")]
pub(crate) fn note_path_geometry_build() {
    PATH_BUILDS.with(|builds| builds.set(builds.get() + 1));
}

#[cfg(all(feature = "wgpu", feature = "geometry"))]
thread_local! {
    static PATH_PROJECTIONS: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

/// How many times a path widget has projected its mesh on this thread.
///
/// Diagnostics only: projecting also re-strokes the plan's strokes along
/// the projected centrelines, so tests use it to prove that a redraw whose
/// values did not change reuses the projected mesh too, not just the flat
/// tessellation that [`path_geometry_builds`] counts.
#[cfg(all(feature = "wgpu", feature = "geometry"))]
#[must_use]
pub fn path_mesh_projections() -> u64 {
    PATH_PROJECTIONS.with(std::cell::Cell::get)
}

#[cfg(all(feature = "wgpu", feature = "geometry"))]
pub(crate) fn note_path_mesh_projection() {
    PATH_PROJECTIONS.with(|projections| projections.set(projections.get() + 1));
}

/// A monotonic 60 Hz clock bound to one engine.
///
/// The engine derives its delta from the timestamps it is handed, so a helper
/// that restarted from `Instant::now()` on each call would feed it timestamps
/// older than its last tick, and a zero delta advances nothing. The first
/// tick of a fresh engine only starts its clock; the second is the first real
/// frame.
///
/// The same holds after any stretch in which nothing moved: an application
/// draws no frames while at rest, so the engine restarts its clock rather
/// than charge that stretch to whatever starts next. A test that retargets a
/// settled track therefore needs one frame to restart the clock and a second
/// one to see movement.
#[derive(Debug)]
pub struct FrameClock {
    motion: Motion,
    now: Instant,
}

impl FrameClock {
    /// Creates a clock for `motion`, starting now. Nothing is ticked yet.
    #[must_use]
    pub fn new(motion: &Motion) -> Self {
        Self {
            motion: motion.clone(),
            now: Instant::now(),
        }
    }

    /// The timestamp the next frame will be one [`FRAME`] after.
    #[must_use]
    pub fn now(&self) -> Instant {
        self.now
    }

    /// Advances the engine by `frames` frames and returns the status of the
    /// last one (`TickStatus::default()` for zero frames).
    #[must_use]
    pub fn run(&mut self, frames: usize) -> TickStatus {
        let mut status = TickStatus::default();

        for _ in 0..frames {
            self.now += FRAME;
            status = self.motion.tick(self.now);
        }

        status
    }

    /// Runs frames until the engine reports nothing animating, returning how
    /// many it took (at most `10_000`).
    #[must_use]
    pub fn run_until_settled(&mut self) -> usize {
        for frame in 1..=MAX_FRAMES {
            if !self.run(1).animating {
                return frame;
            }
        }

        MAX_FRAMES
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::FrameClock;
    use crate::{Curve, Motion, SpringParams, key};

    /// A fast spring for tests; not the shipped `curves::SMOOTH`.
    const FAST: Curve = Curve::spring(SpringParams::new(0.0, Duration::from_millis(300)));

    #[test]
    fn the_frame_clock_reports_the_last_status_and_counts_to_settle() {
        let m = Motion::new();
        let mut clock = FrameClock::new(&m);
        let key = key!();

        let _ = m.to(key, FAST, 0.0_f32);
        let value = m.to(key, FAST, 100.0_f32);

        assert!(
            clock.run(1).animating,
            "the first tick only starts the clock"
        );
        assert!(clock.run(1).animating);

        let frames = clock.run_until_settled();
        assert!(
            frames > 5 && frames < 120,
            "a 300 ms spring settles in well under 2 s: {frames}"
        );
        assert!(!value.is_animating());
        assert!(!clock.run(1).animating, "settled engines report no work");
    }
}
