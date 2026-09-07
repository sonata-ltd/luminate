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
    /// remaining sliver — sub-pixel, and cut off by the engine's settling
    /// tolerance — takes about as long again. It is floored at 1 ms.
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

    /// Returns `true` once the spring is close enough to its target, in both
    /// position and velocity, to stop animating.
    ///
    /// The tolerances scale with the magnitude being animated so that a
    /// window width settles as reliably as an opacity; whatever residual they
    /// allow is erased by [`snap`](Self::snap).
    ///
    /// The velocity bound is deliberately far below "one position tolerance
    /// per 60 Hz frame". That looser rule is right for a position, and wrong
    /// for everything downstream of one: a track carrying a page index for a
    /// 418 px slide is still 0.2 device pixels from its target when a
    /// per-frame bound would call it settled, and a fifth of a pixel is the
    /// difference between a texture composited at a soft fractional phase and
    /// one composited crisply on the grid. Snapping there ends the transition
    /// with a visible jump in sharpness instead of letting the blur resolve.
    /// The extra frames it costs are sub-pixel: nothing moves in them that
    /// the eye reads as motion, only the last of the blur clearing.
    pub(crate) fn is_settled(&self) -> bool {
        let scale = self.target.abs().max(self.position.abs()).max(1.0);

        (self.position - self.target).abs() < 5e-4 * scale && self.velocity.abs() < 1e-3 * scale
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
                    // The invisible remainder: about as long again, and the
                    // reason `duration` is not the settling time. It is spent
                    // below a pixel, clearing the last of the resampling
                    // blur; see `is_settled` for why the bound is that tight.
                    let ratio = settled / duration;
                    assert!(
                        (1.75..=2.15).contains(&ratio),
                        "duration {duration} fully settled at {ratio}x"
                    );
                    break;
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
