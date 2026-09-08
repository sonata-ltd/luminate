//! Damped-harmonic-oscillator spring, evaluated in closed form.
//!
//! A spring is the default [`Curve`] of the motion engine: unlike a duration
//! curve it carries velocity, so retargeting mid-flight continues from the
//! current motion instead of restarting. Retargeting deliberately leaves the
//! velocity untouched for exactly that reason.
//!
//! The analytic solution is exact for any time step, so a stalled window or a
//! debugger pause advances the spring by wall-clock time without any risk of
//! numerical blow-up.
//!
//! [`Curve`]: crate::Curve

use std::time::Duration;

/// Perceptual tuning for a spring curve.
///
/// Both fields are sanitised by [`new`](Self::new), so two parameter sets
/// built from the same inputs always compare equal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpringParams {
    bounce: f32,
    duration: Duration,
}

impl SpringParams {
    /// Creates spring parameters from a `bounce` amount and a perceptual
    /// `duration`.
    ///
    /// `bounce` runs from `0.0` (no overshoot) toward `1.0` (very bouncy) and
    /// is clamped to `0.0..=0.9`; `NaN` reads as `0.0`.
    ///
    /// `duration` is the *perceptual* duration: how long the motion reads as
    /// taking, not how long the maths keeps producing values. A no-bounce
    /// spring is about 99 % of the way there when it elapses, and the
    /// remaining sliver — sub-pixel, and cut off once nothing visible is
    /// still ahead — takes about 0.6x as long again. It is floored at 1 ms.
    ///
    /// Both parameters mean what they mean in `SwiftUI`'s `Spring(duration:
    /// bounce:)`, down to the coefficients — `stiffness = (2π / duration)²`,
    /// `damping = (1 - bounce) · 4π / duration` — so a value taken from
    /// Apple's documentation, or from a designer's `SwiftUI` prototype, can be
    /// typed in here unchanged.
    #[must_use]
    pub const fn new(bounce: f32, duration: Duration) -> Self {
        let bounce = if bounce.is_nan() || bounce < 0.0 {
            0.0
        } else if bounce > 0.9 {
            0.9
        } else {
            bounce
        };
        let duration = if duration.as_millis() < 1 {
            Duration::from_millis(1)
        } else {
            duration
        };

        Self { bounce, duration }
    }

    /// Overshoot amount, already clamped to `0.0..=0.9`.
    #[must_use]
    pub const fn bounce(self) -> f32 {
        self.bounce
    }

    /// Perceptual duration, at least 1 ms. See [`new`](Self::new).
    #[must_use]
    pub const fn duration(self) -> Duration {
        self.duration
    }

    /// Natural frequency `ω` and damping ratio `ζ` for these parameters.
    ///
    /// One period of the oscillator *is* the duration, which is the
    /// calibration `SwiftUI` uses: `stiffness = (2π / duration)²` and
    /// `damping = (1 - bounce) · 4π / duration`, i.e. `ω = 2π / duration`
    /// and `ζ = 1 - bounce`.
    fn coefficients(self) -> (f32, f32) {
        let zeta = 1.0 - self.bounce;
        let omega = PERCEPTUAL_FACTOR / self.duration.as_secs_f32();

        (omega, zeta)
    }
}

/// `ω · duration` for the perceptual calibration: one period of the
/// oscillator.
///
/// At `t = duration` a critically damped step response is within 1.4 % of its
/// target — visually arrived. Calibrating against the far stricter
/// [`Spring::is_settled`] tolerance instead (which would put this at 10.0)
/// makes the same number describe a spring about 1.6× faster, and puts every
/// preset out of step with the `SwiftUI` values designers quote.
const PERCEPTUAL_FACTOR: f32 = std::f32::consts::TAU;

impl Default for SpringParams {
    fn default() -> Self {
        crate::curves::SMOOTH_PARAMS
    }
}

/// A one-dimensional spring.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Spring {
    omega: f32,
    zeta: f32,
    position: f32,
    velocity: f32,
    target: f32,
}

impl Spring {
    /// Creates a spring resting at `initial`.
    pub(crate) fn new(params: SpringParams, initial: f32) -> Self {
        let (omega, zeta) = params.coefficients();

        Self {
            omega,
            zeta,
            position: initial,
            velocity: 0.0,
            target: initial,
        }
    }

    /// Current position.
    pub(crate) fn position(&self) -> f32 {
        self.position
    }

    /// Current velocity, in units per second.
    #[cfg(test)]
    pub(crate) fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Retargets the spring, preserving its velocity so a change of direction
    /// mid-flight reads as momentum rather than a restart.
    pub(crate) fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Replaces the tuning while keeping position and velocity.
    pub(crate) fn retune(&mut self, params: SpringParams) {
        let (omega, zeta) = params.coefficients();
        self.omega = omega;
        self.zeta = zeta;
    }

    /// Advances the spring by `dt` seconds using the analytic solution of
    /// `x″ + 2ζω x′ + ω² x = 0` about the target. Non-positive or non-finite
    /// `dt` is ignored.
    #[allow(clippy::many_single_char_names)] // the oscillator's own symbols
    pub(crate) fn tick(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }

        let (w, z) = (self.omega, self.zeta);

        // The envelope `e^{-ζωt}` underflows to zero here, so the closed form
        // is exactly the target, and `sin_cos` of a huge argument is `NaN`.
        if z * w * dt > 87.0 {
            self.snap();
            return;
        }

        let x0 = self.position - self.target;
        let v0 = self.velocity;

        // Just below critical damping `ω_d → 0` and `b = (v0 + ζωx0) / ω_d`
        // amplifies rounding; the critically damped form is within rounding
        // of the truth there.
        let (x, v) = if z < 1.0 - 1e-3 {
            // Under-damped.
            let wd = w * (1.0 - z * z).sqrt();
            let e = (-z * w * dt).exp();
            let (s, c) = (wd * dt).sin_cos();
            let a = x0;
            let b = (v0 + z * w * x0) / wd;
            let x = e * (a * c + b * s);
            let v = e * ((b * wd - a * z * w) * c - (a * wd + b * z * w) * s);
            (x, v)
        } else {
            // Critically damped (ζ never exceeds 1 here).
            let e = (-w * dt).exp();
            let b = v0 + w * x0;
            let x = e * (x0 + b * dt);
            let v = e * (b - w * (x0 + b * dt));
            (x, v)
        };

        self.position = self.target + x;
        self.velocity = v;
    }

    /// The largest distance from the target the spring will still reach, over
    /// all the time it has left.
    ///
    /// The trajectory [`tick`](Self::tick) integrates is smooth, so its
    /// largest value lies either at `t = 0` or at its first turning point.
    /// This solves for that turning point rather than taking the envelope at
    /// `t = 0`: the envelope is a bound, but a loose one late in a spring's
    /// life, where it predicts an overshoot several times the one that
    /// actually arrives — and predicting an overshoot that never comes is
    /// what keeps a settling spring running through the reversal this is
    /// meant to avoid.
    #[allow(clippy::many_single_char_names)] // the oscillator's own symbols
    fn peak_excursion(&self) -> f32 {
        let (w, z) = (self.omega, self.zeta);
        let x0 = self.position - self.target;
        let v0 = self.velocity;

        if w <= 0.0 || !x0.is_finite() || !v0.is_finite() {
            return x0.abs();
        }

        // The same split `tick` makes, for the same reason: just below
        // critical damping `ω_d → 0` and `b` amplifies rounding.
        let (turning_point, at) = if z < 1.0 - 1e-3 {
            // `x(t) = e^{-σt}(a cos ω_d t + b sin ω_d t)`; `x'(t) = 0` at
            // `tan ω_d t = (b ω_d - σ a) / (σ b + a ω_d)`.
            let wd = w * (1.0 - z * z).sqrt();
            let s = z * w;
            let (a, b) = (x0, (v0 + s * x0) / wd);
            let t = (b * wd - s * a).atan2(s * b + a * wd) / wd;
            let t = if t > 0.0 {
                t
            } else {
                t + std::f32::consts::PI / wd
            };

            let e = (-s * t).exp();
            let (sin, cos) = (wd * t).sin_cos();
            (t, e * (a * cos + b * sin))
        } else {
            // Critically damped: `x(t) = e^{-ωt}(x0 + b t)`, `x'(t) = 0` at
            // `t = 1/ω - x0/b`.
            let b = v0 + w * x0;
            if b.abs() <= f32::EPSILON {
                return x0.abs();
            }
            let t = 1.0 / w - x0 / b;
            let e = (-w * t).exp();
            (t, e * (x0 + b * t))
        };

        // A turning point in the past or beyond the floating-point horizon
        // says the spring is already on its way down: `t = 0` is the peak.
        if turning_point <= 0.0 || !at.is_finite() {
            return x0.abs();
        }

        x0.abs().max(at.abs())
    }

    /// Returns `true` once nothing the spring has left to do would be visible,
    /// so it can stop animating.
    ///
    /// The tolerance scales with the magnitude being animated so that a
    /// window width settles as reliably as an opacity; whatever residual it
    /// allows is erased by [`snap`](Self::snap).
    ///
    /// The question asked is about the *excursion still ahead*
    /// ([`peak_excursion`](Self::peak_excursion)), not the speed right now.
    /// Those differ exactly where it matters. A fast spring crossing its
    /// target is close in position but has a large excursion ahead, so it
    /// keeps running — the case a bare position test would snap mid-flight.
    /// A bouncy spring on its last approach is the mirror image: it is about
    /// to drift a sliver past the target and come back, and a velocity test
    /// keeps it alive for those frames. That reversal is worse than the
    /// residual it protects, because a texture composited across it changes
    /// size in a frame the eye has just been told the motion is over. Asking
    /// about the excursion covers both: run while something visible remains
    /// ahead, stop when nothing does.
    pub(crate) fn is_settled(&self) -> bool {
        let scale = self.target.abs().max(self.position.abs()).max(1.0);

        self.peak_excursion() < 5e-4 * scale
    }

    /// Places the spring exactly at its target and stops it.
    pub(crate) fn snap(&mut self) {
        self.position = self.target;
        self.velocity = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    fn settle(mut s: Spring, dt: f32, max_frames: usize) -> (Spring, usize) {
        for frame in 0..max_frames {
            s.tick(dt);
            if s.is_settled() {
                return (s, frame + 1);
            }
        }
        (s, max_frames)
    }

    #[test]
    fn stable_for_every_duration_and_frame_rate() {
        for duration in [0.01, 0.02, 0.03, 0.1, 0.22, 0.4, 0.5, 1.0] {
            for dt in [1.0 / 240.0, 1.0 / 60.0, 1.0 / 20.0, 0.5, 2.0] {
                for bounce in [0.0, 1e-6, 5e-4, 0.35, 0.9] {
                    let mut s = Spring::new(
                        SpringParams::new(bounce, Duration::from_secs_f32(duration)),
                        0.0,
                    );
                    s.set_target(100.0);
                    let (s, frames) = settle(s, dt, 10_000);
                    assert!(s.is_settled(), "{duration} {dt} {bounce}: never settled");
                    assert!(frames < 10_000, "{duration} {dt} {bounce}: {frames} frames");
                    assert!(s.position().is_finite(), "{duration} {dt} {bounce}");
                    assert!(s.velocity().is_finite());
                    assert!(
                        s.position() <= 100.0 * (1.0 + bounce + 0.05),
                        "{duration} {dt} {bounce} -> {}",
                        s.position()
                    );
                }
            }
        }
    }

    #[test]
    fn a_no_bounce_spring_never_overshoots() {
        for dt in [1.0 / 240.0, 1.0 / 60.0, 1.0 / 20.0] {
            let mut s = Spring::new(SpringParams::new(0.0, Duration::from_millis(400)), 0.0);
            s.set_target(100.0);
            for _ in 0..1000 {
                s.tick(dt);
                assert!(s.position() <= 100.0 + 1e-3, "dt={dt} pos={}", s.position());
            }
        }
    }

    #[test]
    fn duration_is_the_time_to_visually_arrive() {
        for duration in [0.2, 0.4, 0.8] {
            let mut s = Spring::new(
                SpringParams::new(0.0, Duration::from_secs_f32(duration)),
                0.0,
            );
            s.set_target(100.0);

            // The perceptual calibration: `duration` is when the motion is
            // done to the eye, which for a no-bounce spring is 99 % of the
            // way. One frame of slack at 60 Hz is 8 % of the shortest
            // duration tested, hence the 15 % bound.
            let mut arrival = None;
            for frame in 1..10_000 {
                s.tick(1.0 / 60.0);
                if arrival.is_none() && s.position() >= 99.0 {
                    arrival = Some(frame as f32 / 60.0);
                }
                if s.is_settled() {
                    let settled = frame as f32 / 60.0;
                    let arrival = arrival.expect("99 % comes before settling");
                    assert!(
                        (arrival - duration).abs() <= duration * 0.15,
                        "duration {duration} reached 99 % at {arrival}"
                    );
                    // The invisible remainder: about 0.6x again, and the
                    // reason `duration` is not the settling time. It ends the
                    // frame nothing visible remains ahead; see `is_settled`.
                    let ratio = settled / duration;
                    assert!(
                        (1.5..=1.8).contains(&ratio),
                        "duration {duration} fully settled at {ratio}x"
                    );
                    break;
                }
            }
        }
    }

    /// The reason `is_settled` asks about the excursion ahead rather than the
    /// speed right now.
    ///
    /// A bouncy spring's last approach drifts a sliver past its target and
    /// comes back. Those frames are all inside the settling tolerance, so
    /// nothing in them is worth showing — but they contain a *reversal*, and
    /// a reversal is the one thing the eye reads as a twitch however small it
    /// is: a scaled texture composited across one visibly changes size. A
    /// velocity test keeps the spring alive through them. Asking what
    /// excursion is still ahead stops it before the first of them.
    ///
    /// Crossing the target at speed is the opposite case and must still run:
    /// the error is momentarily small, but a large excursion remains.
    #[test]
    fn a_settling_spring_renders_no_frame_it_could_not_be_seen_in() {
        for bounce in [0.0, 0.2, 0.35, 0.6, 0.9] {
            let mut s = Spring::new(SpringParams::new(bounce, Duration::from_millis(300)), 1.0);
            s.set_target(1.6);

            let tolerance = |s: &Spring| 5e-4 * s.target.abs().max(s.position().abs()).max(1.0);

            // Every frame the spring actually renders, and the error it shows.
            let mut frames = Vec::new();
            for _ in 0..10_000 {
                s.tick(1.0 / 60.0);
                if s.is_settled() {
                    break;
                }
                frames.push((s.position() - s.target, tolerance(&s)));
            }
            assert!(!frames.is_empty(), "bounce {bounce}: settled before moving");

            // The invisible tail: every frame after the last one the eye could
            // have seen. One is the frame that carried the spring in; more
            // than one means it lingered there, which is where it turns
            // around.
            let last_visible = frames
                .iter()
                .rposition(|(error, tolerance)| error.abs() >= *tolerance);
            let tail = match last_visible {
                Some(i) => frames.len() - 1 - i,
                None => frames.len(),
            };
            assert!(
                tail <= 1,
                "bounce {bounce}: {tail} frames rendered below the tolerance, \
                 errors {:?}",
                frames[frames.len() - tail..]
                    .iter()
                    .map(|(e, _)| *e)
                    .collect::<Vec<_>>()
            );
        }
    }

    /// `peak_excursion` is only allowed to over-estimate: it is what lets the
    /// spring stop early, so an under-estimate would cut a visible frame.
    #[test]
    fn the_peak_excursion_bounds_every_frame_still_to_come() {
        for bounce in [0.0, 0.35, 0.9] {
            for velocity in [-40.0, -3.0, 0.0, 3.0, 40.0] {
                let mut s = Spring::new(SpringParams::new(bounce, Duration::from_millis(250)), 7.0);
                s.set_target(0.0);
                s.velocity = velocity;

                let bound = s.peak_excursion();
                for _ in 0..2_000 {
                    s.tick(1.0 / 240.0);
                    let error = (s.position() - s.target).abs();
                    assert!(
                        error <= bound + 1e-3,
                        "bounce {bounce}, v {velocity}: reached {error}, bound said {bound}"
                    );
                }
            }
        }
    }

    #[test]
    fn set_target_and_retune_keep_velocity() {
        let mut s = Spring::new(SpringParams::new(0.0, Duration::from_millis(400)), 0.0);
        s.set_target(100.0);
        for _ in 0..6 {
            s.tick(1.0 / 60.0);
        }
        let v = s.velocity();
        assert!(v > 0.0);
        s.set_target(0.0);
        assert_eq!(s.velocity(), v);
        s.retune(SpringParams::new(0.5, Duration::from_millis(200)));
        assert_eq!(s.velocity(), v);
    }

    #[test]
    fn exact_for_any_step_size() {
        let mut a = Spring::new(SpringParams::new(0.2, Duration::from_millis(400)), 0.0);
        let mut b = a;
        a.set_target(50.0);
        b.set_target(50.0);
        a.tick(0.1);
        for _ in 0..100 {
            b.tick(0.001);
        }
        assert!(
            (a.position() - b.position()).abs() < 0.05,
            "{} vs {}",
            a.position(),
            b.position()
        );
    }

    #[test]
    fn params_are_sanitised_on_construction() {
        let p = SpringParams::new(f32::NAN, Duration::ZERO);
        assert_eq!(p.bounce(), 0.0, "NaN bounce reads as no bounce");
        assert_eq!(
            p.duration(),
            Duration::from_millis(1),
            "duration is floored at 1 ms"
        );
        assert_eq!(SpringParams::new(2.0, Duration::from_secs(1)).bounce(), 0.9);
        assert_eq!(
            SpringParams::new(-1.0, Duration::from_secs(1)).bounce(),
            0.0
        );

        // Sanitised params compare equal, so a rebuild with the same tuning is
        // a no-op for the track.
        assert_eq!(
            SpringParams::new(f32::NAN, Duration::ZERO),
            SpringParams::new(f32::NAN, Duration::ZERO)
        );

        let s = Spring::new(SpringParams::new(f32::NAN, Duration::ZERO), 0.0);
        assert!(s.omega.is_finite() && s.omega > 0.0);
        assert_eq!(s.zeta, 1.0);
        let mut s = Spring::new(SpringParams::new(0.0, Duration::from_millis(400)), 0.0);
        s.set_target(1.0);
        s.tick(f32::NAN);
        s.tick(-1.0);
        assert_eq!(s.position(), 0.0, "invalid dt is ignored");
    }

    #[test]
    fn an_enormous_step_lands_exactly_on_the_target() {
        let mut s = Spring::new(SpringParams::new(0.35, Duration::from_millis(400)), 0.0);
        s.set_target(100.0);
        s.tick(f32::MAX);
        assert_eq!(s.position(), 100.0);
        assert_eq!(s.velocity(), 0.0);
        assert!(s.is_settled());
    }

    #[test]
    fn near_critical_damping_is_continuous_with_critical() {
        let mut nearly = Spring::new(SpringParams::new(5e-4, Duration::from_millis(400)), 0.0);
        let mut exactly = Spring::new(SpringParams::new(0.0, Duration::from_millis(400)), 0.0);
        nearly.set_target(100.0);
        exactly.set_target(100.0);
        for _ in 0..30 {
            nearly.tick(1.0 / 60.0);
            exactly.tick(1.0 / 60.0);
            assert!(
                (nearly.position() - exactly.position()).abs() < 0.5,
                "{} vs {}",
                nearly.position(),
                exactly.position()
            );
        }
    }
}
