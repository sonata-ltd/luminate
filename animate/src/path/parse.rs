//! SVG path data (the `d` attribute) into a [`PathData`].

use iced_core::Point;
use svgtypes::{SimplePathSegment, SimplifyingPathParser};

use super::{PathData, PathError};

impl PathData {
    /// Parses SVG path data, the `d` attribute of a `<path>`.
    ///
    /// Absolute and relative commands, `H`/`V`, the `S`/`T` shorthands and
    /// arcs are all accepted; arcs are turned into cubics on the way in.
    /// Coordinates are read as `f64` and stored as `f32`.
    ///
    /// # Errors
    ///
    /// [`PathError::Syntax`] for data that does not parse,
    /// [`PathError::NonFinite`] for a coordinate that overflows `f32`, and
    /// [`PathError::Empty`] for data that draws nothing.
    pub fn parse(d: &str) -> Result<Self, PathError> {
        let mut builder = Self::builder();

        for segment in SimplifyingPathParser::from(d) {
            builder = match segment.map_err(|_| PathError::Syntax)? {
                SimplePathSegment::MoveTo { x, y } => builder.move_to(point(x, y)),
                SimplePathSegment::LineTo { x, y } => builder.line_to(point(x, y)),
                SimplePathSegment::Quadratic { x1, y1, x, y } => {
                    builder.quad_to(point(x1, y1), point(x, y))
                }
                SimplePathSegment::CurveTo {
                    x1,
                    y1,
                    x2,
                    y2,
                    x,
                    y,
                } => builder.cubic_to(point(x1, y1), point(x2, y2), point(x, y)),
                SimplePathSegment::ClosePath => builder.close(),
            };
        }

        builder.build()
    }
}

fn point(x: f64, y: f64) -> Point {
    Point::new(x as f32, y as f32)
}

#[cfg(test)]
mod tests {
    use crate::path::{ArcLength, PathData, PathError};

    #[test]
    fn relative_and_absolute_commands_build_the_same_path() {
        let relative = PathData::parse("M10 10 l10 0 l0 10 z").unwrap();
        let absolute = PathData::parse("M10 10 L20 10 L20 20 Z").unwrap();
        assert_eq!(relative, absolute);
    }

    #[test]
    fn shorthands_and_arcs_are_expanded() {
        assert_eq!(
            PathData::parse("M0 0 H10 V10").unwrap(),
            PathData::parse("M0 0 L10 0 L10 10").unwrap()
        );
        assert_eq!(
            PathData::parse("M0 0 Q5 10 10 0 T20 0").unwrap(),
            PathData::parse("M0 0 Q5 10 10 0 Q15 -10 20 0").unwrap()
        );

        let circle = PathData::parse("M10 0 A10 10 0 0 1 -10 0 A10 10 0 0 1 10 0 Z").unwrap();
        let length = ArcLength::new(&circle).length();
        assert!(
            (length - std::f32::consts::TAU * 10.0).abs() < 0.3,
            "{length}"
        );
    }

    #[test]
    fn bad_data_is_an_error_not_a_panic() {
        assert_eq!(PathData::parse("hello"), Err(PathError::Syntax));
        assert_eq!(PathData::parse(""), Err(PathError::Empty));
        assert_eq!(PathData::parse("M0 0 M5 5"), Err(PathError::Empty));
        assert_eq!(PathData::parse("M0 0 L1e39 0"), Err(PathError::NonFinite));
        assert!(PathData::parse("M0 0 L NaN 0").is_err());
    }
}
