//! Footprints: the one shape every icon is built from.

use super::project::Vec3;

/// Segments each rounded corner is sampled into.
pub(crate) const ARC_STEPS: usize = 8;

/// Samples the closed outline of a rounded rectangle lying flat in the plane
/// `z = centre.z`, centred on `centre`.
///
/// `extents` is the full width along `x` and along `y`. `radius` is clamped
/// to half the shorter side, so any radius at or above that yields a stadium
/// and `f32::MAX` is a safe way to ask for one.
///
/// The result is convex, and an affine projection keeps it convex.
pub(crate) fn footprint(centre: Vec3, extents: (f32, f32), radius: f32) -> Vec<Vec3> {
    let half_x = extents.0 / 2.0;
    let half_y = extents.1 / 2.0;
    let radius = radius.clamp(0.0, half_x.min(half_y));

    if radius <= 0.0 {
        return vec![
            Vec3::new(centre.x + half_x, centre.y + half_y, centre.z),
            Vec3::new(centre.x - half_x, centre.y + half_y, centre.z),
            Vec3::new(centre.x - half_x, centre.y - half_y, centre.z),
            Vec3::new(centre.x + half_x, centre.y - half_y, centre.z),
        ];
    }

    let quarter = std::f32::consts::FRAC_PI_2;
    let inset_x = half_x - radius;
    let inset_y = half_y - radius;

    // Counter-clockwise from the corner at (+x, +y). Each arc is sampled
    // inclusive of both ends, and the straight edges are the chords between
    // consecutive arcs.
    let corners = [
        (inset_x, inset_y, 0.0),
        (-inset_x, inset_y, quarter),
        (-inset_x, -inset_y, quarter * 2.0),
        (inset_x, -inset_y, quarter * 3.0),
    ];

    let mut points = Vec::with_capacity(4 * (ARC_STEPS + 1));

    for (corner_x, corner_y, start) in corners {
        for step in 0..=ARC_STEPS {
            let angle = start + quarter * (step as f32 / ARC_STEPS as f32);

            points.push(Vec3::new(
                centre.x + corner_x + radius * angle.cos(),
                centre.y + corner_y + radius * angle.sin(),
                centre.z,
            ));
        }
    }

    points
}

#[cfg(test)]
mod tests {
    use iced_luminate::iced::Point;

    use super::super::project::project;
    use super::*;

    /// True when a closed polygon never turns both ways. Collinear triples,
    /// which a zero-radius corner produces, do not count as a turn.
    fn is_convex(points: &[Point]) -> bool {
        let mut sign = 0.0_f32;

        for (index, a) in points.iter().enumerate() {
            let a = *a;
            let b = points[(index + 1) % points.len()];
            let c = points[(index + 2) % points.len()];

            let cross = (b.x - a.x) * (c.y - b.y) - (b.y - a.y) * (c.x - b.x);

            if cross.abs() < 1e-4 {
                continue;
            }

            if sign == 0.0 {
                sign = cross.signum();
            } else if cross.signum() != sign {
                return false;
            }
        }

        true
    }

    #[test]
    fn a_rounded_footprint_samples_every_corner() {
        let outline = footprint(Vec3::new(0.0, 0.0, 0.0), (20.0, 10.0), 3.0);

        assert_eq!(outline.len(), 4 * (ARC_STEPS + 1));
    }

    #[test]
    fn a_square_footprint_is_four_points() {
        let outline = footprint(Vec3::new(0.0, 0.0, 0.0), (20.0, 10.0), 0.0);

        assert_eq!(outline.len(), 4);
    }

    #[test]
    fn a_footprint_sits_at_the_height_of_its_centre() {
        let outline = footprint(Vec3::new(0.0, 0.0, 7.0), (20.0, 10.0), 3.0);

        assert!(outline.iter().all(|point| point.z == 7.0));
    }

    #[test]
    fn the_radius_clamps_to_half_the_shorter_side() {
        let saturated = footprint(Vec3::new(0.0, 0.0, 0.0), (20.0, 10.0), 5.0);
        let over = footprint(Vec3::new(0.0, 0.0, 0.0), (20.0, 10.0), f32::MAX);

        assert_eq!(saturated, over);
    }

    #[test]
    fn a_projected_footprint_stays_convex() {
        let outline = footprint(Vec3::new(4.0, -2.0, 1.0), (24.0, 12.0), 4.0);
        let projected: Vec<Point> = outline.iter().copied().map(project).collect();

        assert!(is_convex(&projected));
    }

    #[test]
    fn a_footprint_is_centred_on_its_centre() {
        let outline = footprint(Vec3::new(5.0, 6.0, 0.0), (20.0, 10.0), 2.0);

        let min_x = outline.iter().fold(f32::MAX, |acc, p| acc.min(p.x));
        let max_x = outline.iter().fold(f32::MIN, |acc, p| acc.max(p.x));

        assert!((f32::midpoint(min_x, max_x) - 5.0).abs() < 1e-4);
    }
}
