//! [`SizeLayout::Composited`](super::SizeLayout::Composited): a transition
//! drawn from two recorded textures instead of a line reshaped every frame.
//!
//! When a size or a weight starts to move, the line is shaped twice — once
//! as it was, once as it will be — and each is recorded into a texture. Every
//! frame of the transition then costs no shaping and no rasterising: the
//! line box is interpolated between the two measured boxes, and the two
//! textures are scaled to the size of the moment, stretched to the width of
//! the box and cross-faded.
//!
//! The two ends are what make it seamless. Where a texture is drawn within
//! [`IDENTITY_TOLERANCE`] of its recorded size it is drawn 1:1, on whole
//! device pixels, holding its glyphs at the same sub-pixel phase the live
//! text would have: the first frame of a transition is then the text as it
//! stood, and the last is the text as it will stand, pixel for pixel. A
//! texture composited at a fractional offset, or a shade off 1:1, would
//! instead go from soft to sharp in the frame the transition hands back to
//! the live text: the snap a browser shows at the end of a `transform`
//! animation.

use std::sync::Arc;

use iced::advanced::graphics::text as raw_text;
use iced::advanced::text::{Alignment, Wrapping};
use iced::{Color, Point, Rectangle, Size, Transformation, Vector};
use iced_texture_cache::{Backend, Composite, Record, TextureCache, TextureRenderer};

use super::shaped::Shaped;
use super::step::{MAX_WEIGHT, MIN_SIZE, MIN_WEIGHT};
use crate::theme::typography::TextStyle;

/// Device pixels of transparent margin recorded around the line, so the
/// filter keeps the anti-aliasing at its edge and a glyph that overhangs its
/// advance is not cut. Whole pixels, so the texel grid stays on the device
/// grid.
const BLEED: f32 = 2.0;

/// How far, in device pixels, a texture may be drawn from the size it was
/// recorded at and still be drawn 1:1.
///
/// An eighth of a pixel is below anything the eye can tell apart from 1:1,
/// and snapping to it is what lets a transition hand over to the live text
/// without the frame in which a resampled texture turns sharp.
pub(super) const IDENTITY_TOLERANCE: f32 = 0.125;

/// How far, in device pixels, the phase a texture was recorded at may be
/// from the one its line is drawn at before it is recorded again.
///
/// As good as none, in every frame. The phase decides which pixel each glyph
/// lands on: horizontally through cosmic-text's quarter-pixel bins, and
/// vertically through the truncation of the line's position plus the
/// glyph's offset. A texture recorded at a stale phase therefore sits up to
/// a whole pixel from where the live text of the same moment sits, and the
/// gap closes with a jump the frame the live text takes over. A line that
/// moves — its layout reshuffled by an animation around it — is recorded
/// again every frame it moves, from glyphs already rasterised at its size.
const PHASE_TOLERANCE: f32 = 1e-3;

/// The `wght` value to shape at for an animated `weight`: whole units, on
/// the axis.
pub(super) fn whole_weight(weight: f32) -> u16 {
    if weight.is_finite() {
        // In range after the clamp, so the cast is exact.
        weight.round().clamp(MIN_WEIGHT, MAX_WEIGHT) as u16
    } else {
        400
    }
}

/// How far `value` has come from `from` towards `to`, within `0..=1`.
///
/// No distance to cover counts as arrived.
pub(super) fn progress(value: f32, from: f32, to: f32) -> f32 {
    let span = to - from;

    if span.abs() <= f32::EPSILON || !value.is_finite() {
        return 1.0;
    }

    ((value - from) / span).clamp(0.0, 1.0)
}

/// The opacities of the ending and the arriving line at `progress`.
///
/// Two layers at `1 - p` and `p` cover only three quarters of what either
/// covers alone halfway through, and the line would pale in the middle of
/// its own transition. The ending line fades on `1 - p²` instead: where the
/// two overlap, which is most of every glyph, coverage stays near full, and
/// it is still gone exactly when the arriving line is whole.
pub(super) fn crossfade(progress: f32) -> (f32, f32) {
    let p = progress.clamp(0.0, 1.0);

    (1.0 - p * p, p)
}

/// The circular distance between two sub-pixel phases in `0..1`.
fn phase_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).abs().fract();

    d.min(1.0 - d)
}

/// The sub-pixel phase of `point` on the device grid, as far as it moves a
/// glyph inside a texture: horizontally only.
///
/// cosmic-text places a glyph horizontally in quarter-pixel bins, which the
/// phase selects. Vertically it truncates the line's position and rounds
/// the baseline inside the line apart ([`LayoutGlyph::physical`], "hinting
/// in Y axis"), so a texture's glyphs land on whole texels whatever the
/// phase, and where the live text's baseline lands is worked out in
/// [`quad_top`] instead.
///
/// [`LayoutGlyph::physical`]: iced::advanced::graphics::text::cosmic_text::LayoutGlyph::physical
fn phase_of(point: Point, scale_factor: f32) -> Vector {
    Vector::new((point.x * scale_factor).rem_euclid(1.0), 0.0)
}

/// The top of the quad a texture is composited into, such that the baseline
/// recorded in it lands on the device row the live text of this moment
/// would put its first baseline on.
///
/// The live text puts it at `trunc(top) + round(baseline)`, in device
/// pixels: the line box's top truncated, the baseline inside the box rounded
/// on its own. The baseline of the moment is where cosmic-text would centre
/// a line of `size` in the line height of the moment — the end's baseline,
/// its distance from the middle of the line scaled with the size. Placed
/// anywhere else, a composited frame would sit up to a pixel from the live
/// text of the same moment while the layout around it moves, and jump the
/// frame the live text takes over.
///
/// `lines` is how many lines the end has, `end_line` and `end_baseline` its
/// line height and first baseline, `k` the scale from its size to the size
/// of the moment, and `sy` the vertical scale the texture is drawn at.
#[allow(clippy::too_many_arguments)]
fn quad_top(
    block: Rectangle,
    lines: f32,
    end_line: f32,
    end_baseline: f32,
    k: f32,
    sy: f32,
    scale_factor: f32,
) -> f32 {
    let line_at = block.height / lines;
    let baseline = (end_baseline - end_line / 2.0).mul_add(k, line_at / 2.0);

    let row = (block.y * scale_factor).trunc() + (baseline * scale_factor).round();
    let recorded = BLEED + (end_baseline * scale_factor).round();

    recorded.mul_add(-sy, row) / scale_factor
}

/// What a texture was recorded with; a change in any of it is a re-record.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Recorded {
    /// The sub-pixel phase the glyphs sit at inside the texture, in device
    /// pixels.
    phase: Vector,
    color: Color,
    scale_factor: f32,
}

/// The texture a line of `min_bounds` is recorded into at `phase`: its size
/// in device pixels, and where the line starts in it, in logical pixels.
fn texture(min_bounds: Size, phase: Vector, scale_factor: f32) -> (Size<u32>, Point) {
    let side = |length: f32, phase: f32| {
        // Finite and non-negative by construction; a line longer than
        // `u32::MAX` device pixels is refused by `record` before this matters.
        ((length * scale_factor + phase).ceil().max(0.0) + 2.0 * BLEED) as u32
    };

    (
        Size::new(
            side(min_bounds.width, phase.x),
            side(min_bounds.height, phase.y),
        ),
        Point::new(
            (BLEED + phase.x) / scale_factor,
            (BLEED + phase.y) / scale_factor,
        ),
    )
}

/// The rectangle a texture is composited into, for its line to land on
/// `line`.
///
/// The line may be stretched: `line` is the size the line is drawn at, and
/// the texture around it — margin and phase included — scales with it. The
/// vertical position is replaced by [`quad_top`] where the texture is drawn.
fn quad(
    line: Rectangle,
    min_bounds: Size,
    physical: Size<u32>,
    origin: Point,
    scale_factor: f32,
) -> Rectangle {
    let stretch = |drawn: f32, recorded: f32| {
        if recorded > 0.0 {
            drawn / recorded
        } else {
            1.0
        }
    };
    let (sx, sy) = (
        stretch(line.width, min_bounds.width),
        stretch(line.height, min_bounds.height),
    );

    Rectangle {
        x: line.x - origin.x * sx,
        y: line.y - origin.y * sy,
        width: physical.width as f32 / scale_factor * sx,
        height: physical.height as f32 / scale_factor * sy,
    }
}

/// How far past a whole device pixel a 1:1 texture is placed.
///
/// The software backend places an image by *truncating* its device-pixel
/// origin (`iced_tiny_skia`'s raster pipeline: `(bounds.x / width_scale) as
/// i32`), and an origin that should be exactly 13 comes out of
/// `13.0 / 1.5 * 1.5` as 12.999 999 and lands a whole pixel to the left. A
/// thousandth of a pixel past the grid, away from zero, truncates to the
/// right pixel there, on either side of zero, and filters to nothing anyone
/// can see on wgpu.
const GRID_NUDGE: f32 = 1e-3;

/// `quad`, whose origin is meant to be on the device grid, put there
/// exactly: rounded to the nearest device pixel, and nudged past it by
/// [`GRID_NUDGE`] so a backend that truncates keeps it. Truncation runs
/// towards zero, so a negative origin — a line at the window's edge, whose
/// texture starts in its margin — is nudged the other way.
fn on_device_grid(quad: Rectangle, scale_factor: f32) -> Rectangle {
    let snap = |logical: f32| {
        let pixel = (logical * scale_factor).round();

        (pixel + GRID_NUDGE.copysign(pixel)) / scale_factor
    };

    Rectangle {
        x: snap(quad.x),
        y: snap(quad.y),
        ..quad
    }
}

/// `quad` moved to the nearest whole texel of its own grid.
///
/// For the software backend, which places an image by truncating its origin
/// to a whole texel (`iced_tiny_skia`: `(bounds.y / height_scale) as i32`):
/// truncated, a scaled texture lands up to a texel short of where it should
/// be, a whole pixel above the live text at a scale near 1:1; rounded here
/// first, it lands within half a texel, and the nudge keeps the truncation
/// from undoing it.
fn on_texel_grid(quad: Rectangle, physical: Size<u32>) -> Rectangle {
    let snap = |origin: f32, extent: f32, texels: u32| {
        let per = extent / texels as f32;

        if per > 0.0 && per.is_finite() {
            let texel = (origin / per).round();

            (texel + GRID_NUDGE.copysign(texel)) * per
        } else {
            origin
        }
    };

    Rectangle {
        x: snap(quad.x, quad.width, physical.width),
        y: snap(quad.y, quad.height, physical.height),
        ..quad
    }
}

/// Where a line recorded at `end_size` with `min_bounds` is drawn at `size`
/// inside `block`, the line box of the moment, and whether that is 1:1.
///
/// The line is scaled uniformly to the size of the moment, then stretched to
/// the width of the box: the two ends of a weight transition differ in width
/// by the weight alone, and stretching both to the box lays their letters
/// over each other instead of side by side. It is centred vertically, as
/// cosmic-text centres a line in its line height.
pub(super) fn placement(
    block: Rectangle,
    min_bounds: Size,
    end_size: f32,
    size: f32,
    scale_factor: f32,
) -> (Rectangle, bool) {
    let k = if end_size > 0.0 { size / end_size } else { 1.0 };
    let height = min_bounds.height * k;

    let identity = (block.width - min_bounds.width).abs() * scale_factor <= IDENTITY_TOLERANCE
        && (height - min_bounds.height).abs() * scale_factor <= IDENTITY_TOLERANCE;

    let (width, height) = if identity {
        (min_bounds.width, min_bounds.height)
    } else {
        (block.width, height)
    };

    (
        Rectangle {
            x: block.x,
            y: block.y + (block.height - height) / 2.0,
            width,
            height,
        },
        identity,
    )
}

/// One end of a transition: the line shaped and recorded at one size and
/// weight.
#[derive(Debug)]
pub(super) struct End {
    size: f32,
    weight: u16,
    shaped: Shaped,
    min_bounds: Size,
    /// The line height the kit's scale gives `size`, unrounded; the line box
    /// of the moment grows from it by the same amount per line.
    line_exact: f32,
    /// The line height the end is shaped at, and its first baseline from the
    /// top of the buffer.
    line_height: f32,
    baseline: f32,
    /// The [`Shaped::generation`] the measurements above and the texture were
    /// taken from; `None` before the first shape.
    shaped_generation: Option<u64>,
    cache: TextureCache,
    recorded: Option<Recorded>,
}

impl End {
    fn new(size: f32, weight: u16) -> Self {
        Self {
            size: size.max(MIN_SIZE),
            weight,
            shaped: Shaped::new(),
            min_bounds: Size::ZERO,
            line_exact: 0.0,
            line_height: 0.0,
            baseline: 0.0,
            shaped_generation: None,
            cache: TextureCache::new(),
            recorded: None,
        }
    }

    /// Shapes the line at this end, unless it already stands at exactly
    /// these inputs.
    fn shape(
        &mut self,
        content: &str,
        style: TextStyle,
        bounds: Size,
        align_x: Alignment,
        wrapping: Wrapping,
    ) {
        let resized = style.resized(self.size);
        self.line_exact = style.resized_exact(self.size).line_height;

        let min_bounds =
            self.shaped
                .update(content, resized, self.weight, bounds, align_x, wrapping);

        // Unchanged, as it is on almost every frame: the measurements and
        // the texture still hold.
        let generation = self.shaped.generation();
        if self.shaped_generation == Some(generation) {
            return;
        }
        self.shaped_generation = Some(generation);

        self.min_bounds = min_bounds;
        self.line_height = resized.line_height;
        self.baseline = self
            .shaped
            .buffer()
            .layout_runs()
            .next()
            .map_or(resized.line_height, |run| run.line_y);

        // Any reshape is a new line, whether or not it measures the same:
        // "12/40" and "13/40" in tabular figures, or fonts loaded meanwhile.
        // Nothing in the record's own inputs would notice.
        self.cache.invalidate();
    }

    /// How many lines this end's line breaks into.
    fn lines(&self) -> f32 {
        if self.line_exact > 0.0 {
            (self.min_bounds.height / self.line_exact).round().max(1.0)
        } else {
            1.0
        }
    }

    /// The box this end's line takes at `size`, whose line height is
    /// `line_at`: its measured box, as wide as the size makes it, with every
    /// line grown by the difference in line height.
    fn line_box(&self, size: f32, line_at: f32) -> Size {
        let k = size / self.size;
        let lines = self.lines();

        Size::new(
            self.min_bounds.width * k,
            (self.min_bounds.height + lines * (line_at - self.line_exact)).max(0.0),
        )
    }

    /// Records this end if the texture does not hold it, then composites it
    /// at `opacity` for `size` inside `block`.
    #[allow(clippy::too_many_arguments)]
    fn draw<Renderer>(
        &mut self,
        renderer: &mut Renderer,
        block: Rectangle,
        size: f32,
        opacity: f32,
        color: Color,
        viewport: &Rectangle,
    ) where
        Renderer: raw_text::Renderer + TextureRenderer,
    {
        if opacity.is_nan() || opacity <= 0.0 {
            return;
        }

        let scale_factor = renderer.scale_factor();
        let (line, identity) = placement(block, self.min_bounds, self.size, size, scale_factor);
        let phase = phase_of(line.position(), scale_factor);

        // The phase decides which pixel each glyph truncates to, in every
        // frame: scaled or not, a texture at a stale phase puts the line up
        // to a pixel from where the live text of the same moment is.
        let stale = self.recorded.is_none_or(|recorded| {
            recorded.color != color
                || recorded.scale_factor != scale_factor
                || phase_distance(recorded.phase.x, phase.x) > PHASE_TOLERANCE
                || phase_distance(recorded.phase.y, phase.y) > PHASE_TOLERANCE
        });

        let recorded = if stale {
            self.cache.invalidate();
            Recorded {
                phase,
                color,
                scale_factor,
            }
        } else {
            self.recorded.unwrap_or(Recorded {
                phase,
                color,
                scale_factor,
            })
        };
        self.recorded = Some(recorded);

        let (physical, origin) = texture(self.min_bounds, recorded.phase, scale_factor);
        let buffer = Arc::downgrade(self.shaped.buffer());
        let extent = Rectangle::with_size(Size::new(
            physical.width as f32 / scale_factor,
            physical.height as f32 / scale_factor,
        ));

        let record = renderer.record(&self.cache, physical, scale_factor, |renderer| {
            renderer.fill_raw(raw_text::Raw {
                buffer: buffer.clone(),
                position: origin,
                color,
                clip_bounds: extent,
            });
        });

        match record {
            Record::Fresh | Record::Reused => {
                let filter = renderer.filter_quality();
                let mut quad = quad(line, self.min_bounds, physical, origin, scale_factor);
                quad.y = quad_top(
                    block,
                    self.lines(),
                    self.line_height,
                    self.baseline,
                    size / self.size,
                    quad.height * scale_factor / physical.height as f32,
                    scale_factor,
                );
                let quad = if identity {
                    on_device_grid(quad, scale_factor)
                } else if renderer.backend() == Backend::TinySkia {
                    on_texel_grid(quad, physical)
                } else {
                    quad
                };

                renderer.draw_cached(
                    &self.cache,
                    quad,
                    line,
                    *viewport,
                    Transformation::IDENTITY,
                    Composite {
                        opacity,
                        ..Composite::plain(filter)
                    },
                );
            }
            Record::Uncacheable => {
                // Too large for a texture: the line in place, unscaled, is
                // the least wrong thing left to draw.
                if let Some(clip_bounds) = line.intersection(viewport) {
                    renderer.fill_raw(raw_text::Raw {
                        buffer,
                        position: line.position(),
                        color: Color {
                            a: color.a * opacity.min(1.0),
                            ..color
                        },
                        clip_bounds,
                    });
                }
            }
        }
    }
}

/// A transition between two recorded lines.
#[derive(Debug)]
pub(super) struct Transition {
    from: End,
    to: End,
}

impl Transition {
    /// A transition from `from` to `to`, each `(size, weight)`; nothing is
    /// shaped or recorded yet.
    pub(super) fn new(from: (f32, u16), to: (f32, u16)) -> Self {
        Self {
            from: End::new(from.0, from.1),
            to: End::new(to.0, to.1),
        }
    }

    /// Whether this transition is heading for `size` at `weight`.
    #[allow(clippy::float_cmp)]
    pub(super) fn heads_for(&self, size: f32, weight: u16) -> bool {
        // Both are targets handed over as they are, not results of
        // arithmetic: a retarget is a different number.
        self.to.size == size.max(MIN_SIZE) && self.to.weight == weight
    }

    /// The height of the line box once the transition has arrived.
    pub(super) fn resting_height(&self) -> f32 {
        self.to.min_bounds.height
    }

    /// Shapes both ends in `style`, unless they already stand at exactly
    /// these inputs.
    pub(super) fn shape(
        &mut self,
        content: &str,
        style: TextStyle,
        bounds: Size,
        align_x: Alignment,
        wrapping: Wrapping,
    ) {
        self.from.shape(content, style, bounds, align_x, wrapping);
        self.to.shape(content, style, bounds, align_x, wrapping);
    }

    /// How far along the transition is, given the size and the weight of
    /// the moment.
    ///
    /// A weight that changes decides it: the width of the box and the
    /// crossfade both follow the weight's mix of the two ends. Otherwise the
    /// size does.
    pub(super) fn progress(&self, size: f32, weight: f32) -> f32 {
        if self.from.weight == self.to.weight {
            progress(size, self.from.size, self.to.size)
        } else {
            progress(
                weight,
                f32::from(self.from.weight),
                f32::from(self.to.weight),
            )
        }
    }

    /// The line box at `size` and `progress` in `style`: each end's box at
    /// that size, mixed by the progress.
    pub(super) fn line_box(&self, style: TextStyle, size: f32, progress: f32) -> Size {
        let line_at = style.resized_exact(size).line_height;
        let (from, to) = (
            self.from.line_box(size, line_at),
            self.to.line_box(size, line_at),
        );
        let mix = |a: f32, b: f32| (b - a).mul_add(progress, a);

        Size::new(mix(from.width, to.width), mix(from.height, to.height))
    }

    /// Composites both ends for `size` and `progress` inside `block`, the
    /// ending one under the arriving one.
    ///
    /// `opacity` fades the whole transition; it scales how opaque the
    /// textures are drawn, never the colour they are recorded in, so a fade
    /// records nothing again.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn draw<Renderer>(
        &mut self,
        renderer: &mut Renderer,
        block: Rectangle,
        size: f32,
        progress: f32,
        color: Color,
        opacity: f32,
        viewport: &Rectangle,
    ) where
        Renderer: raw_text::Renderer + TextureRenderer,
    {
        let (from, to) = crossfade(progress);

        self.from
            .draw(renderer, block, size, from * opacity, color, viewport);
        self.to
            .draw(renderer, block, size, to * opacity, color, viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A reshape is a new line even when it measures the same: "12" and "21"
    /// take the same width, and a texture kept because the size held would
    /// go on showing the old one for the rest of the transition.
    #[test]
    fn a_reshape_that_keeps_the_size_records_again() {
        use crate::theme::typography::TextSize;

        crate::Luminate::load_fonts();

        let style = TextStyle::text(TextSize::Md, iced::font::Weight::Normal);
        let bounds = Size::new(600.0, 100.0);
        let mut end = End::new(16.0, 400);

        end.shape("12", style, bounds, Alignment::Default, Wrapping::None);
        let before = end.min_bounds;
        let _ = end.cache.take_invalidated();

        end.shape("12", style, bounds, Alignment::Default, Wrapping::None);
        assert!(
            !end.cache.is_invalidated(),
            "nothing changed, and the texture was thrown away anyway"
        );

        end.shape("21", style, bounds, Alignment::Default, Wrapping::None);
        assert_eq!(
            end.min_bounds, before,
            "the test needs two lines of one size"
        );
        assert!(
            end.cache.is_invalidated(),
            "the line changed and its texture was kept"
        );
    }

    #[test]
    fn progress_runs_from_the_start_to_the_end_and_no_further() {
        assert_eq!(progress(14.0, 14.0, 18.0), 0.0);
        assert_eq!(progress(16.0, 14.0, 18.0), 0.5);
        assert_eq!(progress(18.0, 14.0, 18.0), 1.0);
        assert_eq!(progress(18.5, 14.0, 18.0), 1.0, "an overshoot is arrived");
        assert_eq!(progress(500.0, 600.0, 400.0), 0.5, "a way down counts too");
        assert_eq!(progress(3.0, 7.0, 7.0), 1.0, "no distance is arrived");
        assert_eq!(progress(f32::NAN, 1.0, 2.0), 1.0);
    }

    #[test]
    fn the_crossfade_starts_and_ends_on_a_single_line() {
        assert_eq!(crossfade(0.0), (1.0, 0.0));
        assert_eq!(crossfade(1.0), (0.0, 1.0));

        let (from, to) = crossfade(0.5);
        let coverage = to + from * (1.0 - to);
        assert!(
            coverage > 0.85,
            "the line pales halfway through: {coverage} of full coverage"
        );
    }

    #[test]
    fn a_line_near_its_recorded_size_is_placed_1_to_1() {
        let block = Rectangle::new(Point::new(10.3, 20.6), Size::new(100.02, 24.0));
        let min_bounds = Size::new(100.0, 24.0);

        let (line, identity) = placement(block, min_bounds, 16.0, 16.0, 2.0);
        assert!(identity, "0.04 device px off is 1:1");
        assert_eq!(line.size(), min_bounds);
        assert_eq!(line.position(), block.position());

        let (line, identity) = placement(block, min_bounds, 16.0, 17.0, 2.0);
        assert!(!identity);
        assert_eq!(line.width, block.width, "stretched to the box");
        assert_eq!(line.height, 24.0 * 17.0 / 16.0, "scaled by the size");
    }

    /// The seam: drawn 1:1, a texture lands on whole device pixels and its
    /// line exactly where the live text would be, sub-pixel phase included.
    #[test]
    fn a_1_to_1_texture_lands_its_line_on_the_live_texts_pixels() {
        for scale_factor in [1.0, 1.25, 2.0] {
            let line = Rectangle::new(Point::new(10.3, 20.6), Size::new(100.0, 24.0));
            let phase = phase_of(line.position(), scale_factor);
            let (physical, origin) = texture(line.size(), phase, scale_factor);
            let quad = quad(line, line.size(), physical, origin, scale_factor);

            let device = |logical: f32| logical * scale_factor;
            assert!(
                (device(quad.x) - device(quad.x).round()).abs() < 1e-3,
                "the texel grid is off the device grid at {scale_factor}x: {quad:?}"
            );
            assert!(
                (quad.x + origin.x - line.x).abs() < 1e-4,
                "the line does not land where the live text is horizontally"
            );
            assert!(
                (quad.width - physical.width as f32 / scale_factor).abs() < 1e-4,
                "1:1 is one texel per device pixel"
            );
        }
    }

    /// Vertically a texture lands its baseline where the live text would: the
    /// line box's top truncated, the baseline inside it rounded, in device
    /// pixels. At 1:1 that is a whole device pixel for the quad itself.
    #[test]
    fn a_texture_lands_its_baseline_on_the_live_texts_row() {
        for scale_factor in [1.0_f32, 1.25, 1.5, 2.0] {
            let block = Rectangle::new(Point::new(10.3, 20.6), Size::new(100.0, 24.0));
            let (end_line, end_baseline) = (24.0, 17.4);
            let live = (block.y * scale_factor).trunc() + (end_baseline * scale_factor).round();

            let top = quad_top(block, 1.0, end_line, end_baseline, 1.0, 1.0, scale_factor);
            let recorded = BLEED + (end_baseline * scale_factor).round();

            assert!(
                (top * scale_factor - (top * scale_factor).round()).abs() < 1e-3,
                "{scale_factor}x: a 1:1 quad is off the device grid"
            );
            assert!(
                (top * scale_factor + recorded - live).abs() < 1e-3,
                "{scale_factor}x: the baseline is not on the live text's row"
            );

            // Scaled, the baseline of the moment is re-centred in the line
            // box of the moment, and still lands on a whole device row.
            let taller = Rectangle {
                height: 30.0,
                ..block
            };
            let top = quad_top(
                taller,
                1.0,
                end_line,
                end_baseline,
                1.25,
                1.25,
                scale_factor,
            );
            let landed = top * scale_factor + recorded * 1.25;
            assert!((landed - landed.round()).abs() < 1e-3);
        }
    }

    /// A scaled origin must survive a backend that truncates it to a texel:
    /// it lands on the nearest one, not on the one below.
    #[test]
    fn a_scaled_quad_truncates_to_its_nearest_texel() {
        let physical = Size::new(100, 30);
        for (origin, per) in [(21.968_f32, 1.0007_f32), (-3.4, 0.8), (7.51, 1.25)] {
            let quad = Rectangle::new(
                Point::new(origin, origin),
                Size::new(per * 100.0, per * 30.0),
            );
            let snapped = on_texel_grid(quad, physical);

            let meant = (origin / per).round() as i32;
            assert_eq!(
                (snapped.x / per) as i32,
                meant,
                "{origin} at {per} per texel"
            );
            assert_eq!((snapped.y / per) as i32, meant);
        }
    }

    /// A 1:1 origin must survive a backend that truncates it to a pixel.
    #[test]
    fn a_1_to_1_quad_truncates_to_the_pixel_it_is_meant_for() {
        for scale_factor in [1.0_f32, 1.25, 1.5, 1.75, 2.0, 3.0] {
            for pixel in -50..200 {
                let meant = pixel as f32;
                // The float error of a round trip through logical pixels.
                let quad = Rectangle::new(
                    Point::new(meant / scale_factor, meant / scale_factor),
                    Size::new(10.0, 10.0),
                );
                let quad = on_device_grid(quad, scale_factor);

                assert_eq!(
                    (quad.x * scale_factor) as i32,
                    pixel,
                    "{scale_factor}x lands pixel {pixel} elsewhere"
                );
                assert!((quad.y * scale_factor - meant).abs() < 0.01);
            }
        }
    }

    #[test]
    fn a_texture_leaves_room_for_the_line_its_phase_and_the_margin() {
        let (physical, origin) = texture(Size::new(100.0, 24.0), Vector::new(0.5, 0.0), 2.0);

        assert_eq!(physical, Size::new(201 + 4, 48 + 4));
        assert_eq!(origin, Point::new(1.25, 1.0));
    }

    #[test]
    fn phases_are_compared_around_the_pixel() {
        assert!(phase_distance(0.98, 0.02) < 0.05);
        assert!((phase_distance(0.25, 0.75) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_weight_is_shaped_in_whole_units_on_the_axis() {
        assert_eq!(whole_weight(436.6), 437);
        assert_eq!(whole_weight(1_200.0), 900);
        assert_eq!(whole_weight(f32::NAN), 400);
    }

    #[test]
    fn an_end_grows_its_line_box_with_the_size() {
        let mut end = End::new(16.0, 400);
        end.min_bounds = Size::new(100.0, 24.0);
        end.line_exact = 24.0;

        assert_eq!(end.line_box(16.0, 24.0), Size::new(100.0, 24.0));
        assert_eq!(end.line_box(20.0, 30.0), Size::new(125.0, 30.0));

        end.min_bounds = Size::new(100.0, 48.0);
        assert_eq!(
            end.line_box(20.0, 30.0).height,
            60.0,
            "two lines grow twice"
        );
    }

    #[test]
    fn the_line_box_mixes_the_two_ends() {
        let style = TextStyle::text(
            crate::theme::typography::TextSize::Md,
            iced::font::Weight::Normal,
        );
        let mut transition = Transition::new((16.0, 400), (16.0, 500));
        transition.from.min_bounds = Size::new(100.0, 24.0);
        transition.from.line_exact = 24.0;
        transition.to.min_bounds = Size::new(104.0, 24.0);
        transition.to.line_exact = 24.0;

        assert_eq!(transition.progress(16.0, 450.0), 0.5, "the weight decides");
        assert_eq!(
            transition.line_box(style, 16.0, 0.5),
            Size::new(102.0, 24.0)
        );
        assert!(transition.heads_for(16.0, 500));
        assert!(!transition.heads_for(18.0, 500));
    }

    /// Between the ends the box takes the line height of the moment,
    /// unrounded, so it and whatever follows it grow a little every frame.
    #[test]
    fn the_line_box_between_two_sizes_is_unrounded() {
        use crate::theme::typography::TextSize;

        let style = TextStyle::text(TextSize::Sm, iced::font::Weight::Medium);
        let mut transition = Transition::new((14.0, 500), (18.0, 500));
        transition.from.min_bounds = Size::new(140.0, 20.0);
        transition.from.line_exact = 20.0;
        transition.to.min_bounds = Size::new(180.0, 28.0);
        transition.to.line_exact = 28.0;

        // 14/20, 16/24, 18/28: 16.25 px is 24.5 px of line box.
        let size = 16.25;
        let progress = transition.progress(size, 500.0);
        let line = transition.line_box(style, size, progress);

        assert_eq!(line.height, 24.5);
        assert!((line.width - 162.5).abs() < 1e-3, "{}", line.width);
    }
}
