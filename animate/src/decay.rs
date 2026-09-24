//! Exponential friction, evaluated in closed form.
//!
//! What a flung list does after the finger lets go: it keeps the velocity it
//! was given and loses a fixed fraction of it every second, so it glides to a
//! rest point that follows from the fling rather than from a target. Like
//! [`Spring`](crate::Spring), the solution is exact for any time step.

/// A one-dimensional value slowing down under exponential friction.
///
/// `x(t) = x₀ + v₀/k · (1 - e^{-kt})` and `v(t) = v₀ · e^{-kt}`, where `k` is
/// the friction [`rate`](Self::new). It comes to rest at [`rest`](Self::rest),
/// `x₀ + v₀/k`, which is known from the start.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Decay {
    position: f32,
    velocity: f32,
    rate: f32,
}

impl Decay {
    /// The friction rate of `UIScrollView.DecelerationRate.normal`, per
    /// second: that keeps 0.998 of the velocity every millisecond, and
    /// `-ln(0.998) · 1000 ≈ 2.0`.
    pub const NORMAL_RATE: f32 = 2.002;

    /// Starts at `position`, moving at `velocity` units per second, and slows
    /// by `rate` per second (see [`NORMAL_RATE`](Self::NORMAL_RATE)).
    ///
    /// A `rate` that is not a positive finite number reads as
    /// [`NORMAL_RATE`](Self::NORMAL_RATE); a velocity that is not finite reads
    /// as `0.0`.
    #[must_use]
    pub fn new(position: f32, velocity: f32, rate: f32) -> Self {
        let rate = if rate.is_finite() && rate > 0.0 {
            rate
        } else {
            Self::NORMAL_RATE
        };
        let velocity = if velocity.is_finite() { velocity } else { 0.0 };

        Self {
            position,
            velocity,
            rate,
        }
    }

    /// Current position.
    #[must_use]
    pub const fn position(&self) -> f32 {
        self.position
    }

    /// Current velocity, in units per second.
    #[must_use]
    pub const fn velocity(&self) -> f32 {
        self.velocity
    }

    /// Where the value comes to rest.
    #[must_use]
    pub fn rest(&self) -> f32 {
        self.position + self.velocity / self.rate
    }

    /// Advances by `dt` seconds. Non-positive or non-finite `dt` is ignored.
    pub fn tick(&mut self, dt: f32) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }

        let rest = self.rest();
        let e = (-self.rate * dt).exp();

        // Measured from the rest point, the travel left shrinks by `e` too.
        self.position = rest - (rest - self.position) * e;
        self.velocity *= e;
    }

    /// Returns `true` once less than `tolerance` of travel is left.
    #[must_use]
    pub fn is_settled_within(&self, tolerance: f32) -> bool {
        (self.velocity / self.rate).abs() < tolerance
    }

    /// Places the value at its rest point and stops it.
    pub fn snap(&mut self) {
        self.position = self.rest();
        self.velocity = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comes_to_rest_where_it_said_it_would() {
        let mut d = Decay::new(10.0, 1000.0, Decay::NORMAL_RATE);
        let rest = d.rest();
        for _ in 0..600 {
            d.tick(1.0 / 60.0);
        }
        assert!(
            (d.position() - rest).abs() < 0.01,
            "{} vs {rest}",
            d.position()
        );
        assert!(d.is_settled_within(0.5));
    }

    #[test]
    fn frame_rate_does_not_change_the_path() {
        let mut coarse = Decay::new(0.0, -800.0, 3.0);
        let mut fine = coarse;
        coarse.tick(0.5);
        for _ in 0..120 {
            fine.tick(0.5 / 120.0);
        }
        assert!((coarse.position() - fine.position()).abs() < 1e-2);
        assert!((coarse.velocity() - fine.velocity()).abs() < 1e-2);
    }

    #[test]
    fn nonsense_inputs_fall_back() {
        let mut d = Decay::new(5.0, f32::NAN, -1.0);
        assert_eq!(d.velocity(), 0.0);
        assert_eq!(d.rest(), 5.0);
        d.tick(f32::INFINITY);
        d.tick(-1.0);
        assert_eq!(d.position(), 5.0);
        assert!(d.is_settled_within(0.5));
    }

    #[test]
    fn snap_lands_on_the_rest_point() {
        let mut d = Decay::new(0.0, 400.0, 2.0);
        d.snap();
        assert_eq!(d.position(), 200.0);
        assert_eq!(d.velocity(), 0.0);
    }
}
