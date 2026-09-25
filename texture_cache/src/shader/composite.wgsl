@group(0) @binding(0) var cache_texture: texture_2d<f32>;
@group(0) @binding(1) var cache_sampler: sampler;

// Offsets, which must match `composite.rs::Params` field for field — the
// naga test parses this file but cannot compare the two layouts, so a
// mismatch would only ever show as garbage on a GPU:
//   opacity 0, mode 4, scale 8, warp_stretch 12, radius 16, size 32,
//   origin 40, drawn 48, warp_squash 56, warp_target_width 60, uv 64,
//   warp_radius 80, then twelve scalars from 96 to 144.
// Scalars packed around the vectors rather than vec4s (whose lanes would
// have to be unpacked) keep every vector naturally aligned without `@align`.
struct Params {
    opacity: f32,
    // Reconstruction kernel: 0 = Catmull-Rom, 1 = a single bilinear tap.
    // `FilterQuality::Snap` shares the single-tap value; its crispness comes
    // from the snapped geometry, not from here.
    mode: f32,
    // Device pixels per logical pixel. The corner masks ramp across one
    // device pixel, so an arc is as crisp on a 2x display as on a 1x one.
    scale: f32,
    // How far the genie's stretch has run. 0.0, with `warp_squash` at 0.0,
    // means no warp and takes the fast path below.
    warp_stretch: f32,
    // Corners of the composited rectangle in logical pixels, in iced's
    // order: top-left, top-right, bottom-right, bottom-left. All zero is a
    // rectangle and skips the mask below. Already clamped so no two on one
    // side overlap.
    radius: vec4<f32>,
    // The size of the shape being cut, in logical pixels: the space
    // `radius` is measured in.
    size: vec2<f32>,
    // Where this quad starts inside that shape, and how big it is. Usually
    // (0, 0) and `size` — a pane of frosted glass overhanging its source is
    // the exception, drawing only the part over the source while its corners
    // belong to the whole pane.
    origin: vec2<f32>,
    drawn: vec2<f32>,
    // How far the genie's squash has run: the travel along the axis.
    warp_squash: f32,
    // The width of the band the rows converge on, as a fraction of the
    // content's own width.
    warp_target_width: f32,
    // The window on the texture this instance samples: x, y, width, height
    // in normalised coordinates. A plain composite passes 0, 0, 1, 1. See
    // `composite.rs::Params::uv` for what moves this rectangle and why
    // nothing else needs to change when it does.
    uv: vec4<f32>,
    // The content's corner radii in device pixels, reordered into anchor
    // space (top-left there is the anchor corner) so the genie's mask does
    // not have to undo the flips. Real pixels, not `uv`: a radius in `uv`
    // would squeeze with the shape, which is the whole thing the genie's
    // mask exists to avoid.
    warp_radius: vec4<f32>,
    // How sharply a row's travel lags with its distance from the anchor.
    warp_stretch_power: f32,
    // The side curve's control values, at the wide end and the neck end.
    warp_curve_in: f32,
    warp_curve_out: f32,
    // Axis mirrors putting the anchor corner at the origin, 0.0 or 1.0.
    warp_flip_x: f32,
    warp_flip_y: f32,
    // The content rectangle's size in device pixels, the units of
    // `warp_radius`.
    warp_rect_width: f32,
    warp_rect_height: f32,
    // How far from the anchor the last row is drawn, as a fraction of the
    // content's height. Solved once on the CPU (`warp::Genie::far_edge`):
    // it is the same for every column.
    warp_far_edge: f32,
    // Where the content sits inside the texture, in `uv`: its origin and
    // its size. The texture carries bleed padding around the content and
    // the warp is defined on the content, so the map runs in the content's
    // own frame and steps back out to sample.
    warp_inset_x: f32,
    warp_inset_y: f32,
    warp_span_x: f32,
    warp_span_y: f32,
}
@group(0) @binding(2) var<uniform> params: Params;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // One oversized triangle, (-1,-1) (3,-1) (-1,3), covers the whole
    // viewport after clipping; the visible square maps to uv 0..1.
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    var out: VertexOutput;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// Samples a texel centre. `textureSampleLevel` (explicit LOD) is mandatory:
// it is the only sampling form allowed inside the non-uniform control flow
// below. LOD 0 is the only level there is — cache textures have no mip chain.
fn samp(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(cache_texture, cache_sampler, uv, 0.0);
}

// The texture holds premultiplied colour, so scaling every channel of the
// reconstructed sample by the group opacity keeps it premultiplied.
//
// `in.uv` covers 0..1 over this primitive's own quad. The warp, if any, maps
// it to the point of the texture it shows; mapping that into `params.uv`'s
// window before handing it to `reconstruct` is the entire backdrop-sampling
// feature. `reconstruct` itself is unchanged: its clamp to
// texel centres already keeps a window that overhangs the texture edge from
// reading past it, which is the clamp-to-edge this needs.
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let source = warp_source(in.uv);
    // `z` is coverage: zero where the collapsed shape does not reach, and
    // fractional inside a rounded corner.
    if (source.z <= 0.0) {
        return vec4<f32>(0.0);
    }
    let uv = params.uv.xy + source.xy * params.uv.zw;

    // A warped composite has already been rounded by the genie, against its
    // own curved edges; the rectangle's mask below would cut the collapsing
    // shape along the quad it no longer fills.
    var alpha = source.z;
    if (!warping()) {
        alpha = coverage(params.origin + in.uv * params.drawn);
    }

    // Premultiplied throughout, so scaling every channel by the coverage
    // keeps it premultiplied — the same reason the opacity can be a plain
    // multiply.
    return reconstruct(uv) * params.opacity * alpha;
}

// Whether a genie is running: fully open is exactly identity.
fn warping() -> bool {
    return params.warp_stretch > 0.0 || params.warp_squash > 0.0;
}

// Coverage of the rounded rectangle at `point`, in logical pixels: 1 well
// inside, 0 well outside, a ramp one device pixel wide across the edge.
// Mirrors `geometry::rounded_coverage`; a hard test would stair-step every
// arc, and a ramp a logical pixel wide would blur it on a dense display.
fn coverage(point: vec2<f32>) -> f32 {
    if (all(params.radius == vec4<f32>(0.0))) {
        return 1.0;
    }

    let half_size = params.size * 0.5;
    let centred = point - half_size;

    // The four corners are the four sign combinations, in iced's order.
    var radius = params.radius.x;
    if (centred.x > 0.0 && centred.y <= 0.0) {
        radius = params.radius.y;
    } else if (centred.x > 0.0 && centred.y > 0.0) {
        radius = params.radius.z;
    } else if (centred.x <= 0.0 && centred.y > 0.0) {
        radius = params.radius.w;
    }

    // Distance to the box inset by the radius, minus the radius, is the
    // distance to the rounded shape. Negative inside.
    let q = abs(centred) - half_size + vec2<f32>(radius);
    let distance = min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0))) - radius;

    return clamp(0.5 - distance * params.scale, 0.0, 1.0);
}

// How much of this pixel survives the corner mask: `1` away from a corner,
// the circle's coverage inside one. `warp::corner_alpha` is the same
// arithmetic; keep the two in step.
//
// The distances are to the shape's four edges rather than to a rectangle's
// corners, which is what lets it follow a right edge that curves: every row
// is rounded against its own width.
fn corner_alpha(dl: f32, dr: f32, dt: f32, db: f32, r: f32) -> f32 {
    if (r <= 0.0) {
        return 1.0;
    }

    let dx = min(dl, dr);
    let dy = min(dt, db);
    if (dx >= r || dy >= r) {
        return 1.0;
    }

    let reach = length(vec2<f32>(r - dx, r - dy));

    return clamp(r - reach + 0.5, 0.0, 1.0);
}

// The radius of the corner nearest a pixel, in anchor space: `left` and
// `top` say which edges it is closer to. `warp_radius` is in iced's order
// there, top-left first; `warp::Genie::anchor_corners` put it there.
fn warp_corner(left: bool, top: bool) -> f32 {
    if (top) {
        return select(params.warp_radius.y, params.warp_radius.x, left);
    }
    return select(params.warp_radius.z, params.warp_radius.w, left);
}

// The inverse genie: which source texel this destination pixel shows, with
// `z` set when there is one. The Rust side of the same arithmetic is
// `warp::Genie::source`, which is where it is tested.
//
// The row is found first because the row's width depends on it, which is
// what keeps this closed-form: one cube and two divides, no iteration.
fn warp_source(uv: vec2<f32>) -> vec3<f32> {
    let k = params.warp_stretch;
    let s = params.warp_squash;

    // Fully open is the overwhelmingly common case and is exactly identity.
    if (k <= 0.0 && s <= 0.0) {
        return vec3<f32>(uv, 1.0);
    }
    // Fully collapsed, everything has been drawn through the anchor. The
    // map still admits the one row *at* the anchor, a line of no height
    // that a pixel centre in the padding can land on exactly.
    if (s >= 1.0) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }


    // Into the content's frame, then into anchor space, where the corner
    // it collapses into is the origin. The frame is the content's own
    // rectangle, not the padded texture: that is the corner the API names.
    let inset = vec2<f32>(params.warp_inset_x, params.warp_inset_y);
    let span = vec2<f32>(params.warp_span_x, params.warp_span_y);
    var p = (uv - inset) / span;
    if (params.warp_flip_x > 0.5) { p.x = 1.0 - p.x; }
    if (params.warp_flip_y > 0.5) { p.y = 1.0 - p.y; }

    // The row's width comes straight from the destination row. `1 - y` is
    // how far along the travel path it sits, which is what makes the neck
    // sweep. The cubic is the side curve — the Bézier through
    // `(0, curve_in, curve_out, 1)`, which at its defaults is the smoothstep
    // and so the reference's own curve. This is `warp::bend`; keep the two
    // in step. Rows converge on `warp_target_width`, not on a point.
    let along = clamp(1.0 - p.y, 0.0, 1.0);
    let u_along = 1.0 - along;
    let bend = 3.0 * u_along * u_along * along * params.warp_curve_in
             + 3.0 * u_along * along * along * params.warp_curve_out
             + along * along * along;
    let w = 1.0 - k * bend * (1.0 - params.warp_target_width);
    if (w <= 1e-4) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }

    let u = p.x / w;

    // Undo the travel to find which row this is showing. The exponent is
    // the per-row lag: 1.0 at the anchor, rising with distance from it, so
    // a far row's shift is a high power of a number below one and is
    // therefore small. That is what keeps the wide end of the shape on
    // screen. `warp::STRETCH_POWER` is the same 2.0; keep the two in step.
    let v = p.y + pow(s, 1.0 + params.warp_stretch_power * k * p.y);
    // The padding before the anchor is where the rows go once they have
    // travelled through it. A row drawn there that still shows content has
    // been consumed, not displaced; only the content's own bleed, `v < 0`,
    // may show past the anchor, and only while nothing has travelled.
    if (p.y < 0.0 && v >= 0.0) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }

    // Round the shape's own corners at a radius that does not squeeze with
    // it. The row spans `0..w` across and the shape reaches from the anchor
    // edge to the far edge, both measured here in destination pixels: the
    // squash makes a source row longer than the destination row showing
    // it, so a distance taken in source rows (`1 - v`) would round the far
    // corners to a radius that shrinks as the collapse runs. The radius is
    // clamped to half the visible shape so a narrow end becomes a stadium
    // rather than growing corners larger than itself.
    let far = params.warp_far_edge;
    let width_px = w * params.warp_rect_width;
    let dl = p.x * params.warp_rect_width;
    let dr = (w - p.x) * params.warp_rect_width;
    let dt = p.y * params.warp_rect_height;
    let db = (far - p.y) * params.warp_rect_height;
    let radius = min(
        warp_corner(dl <= dr, dt <= db),
        0.5 * min(width_px, far * params.warp_rect_height)
    );
    let alpha = corner_alpha(dl, dr, dt, db, radius);
    if (alpha <= 0.0) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }

    // Back out of anchor space and the content's frame. The bounds test is
    // taken on the texture rather than on the content, so the bleed around
    // the content — its own anti-aliased edge — is shown exactly as it is
    // at rest; `warp::Genie::source` tests the content, as a pointer must.
    // Past the texture there is nothing left to show.
    var src = vec2<f32>(u, v);
    if (params.warp_flip_x > 0.5) { src.x = 1.0 - src.x; }
    if (params.warp_flip_y > 0.5) { src.y = 1.0 - src.y; }
    src = src * span + inset;
    if (any(src < vec2<f32>(0.0)) || any(src > vec2<f32>(1.0))) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }
    return vec3<f32>(src, alpha);}

// Catmull-Rom reconstruction (B = 0, C = 1/2): an *interpolating* kernel that
// passes through the source texels (exact at integer phase) with a mild
// high-frequency boost, keeping moving edges and text sharp during a sub-pixel
// translate — sharper than hardware bilinear and far sharper than a smoothing
// B-spline.
//
// A naive Catmull-Rom is 4x4 = 16 point samples per pixel. Instead we exploit
// the hardware *linear* sampler (the pipeline's is Linear/Linear): the central
// positive weight pair (w1, w2) of each axis is folded into a single bilinear
// fetch at a fractional offset, so the full 2D kernel costs only 3x3 = 9
// fetches — bit-for-bit the same result. The kernel is separable, so when one
// axis is at integer phase (the vertical axis during a horizontal slide, say)
// that axis collapses to its centre row/column and the cost drops to 3 fetches.
// For a settled or snapped position both axes collapse and a single fetch is
// exact.
fn reconstruct(uv: vec2<f32>) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(cache_texture));

    // The cheap tiers: one bilinear tap, clamped to the texel centres so the
    // tap never reaches past the texture's edge.
    if (params.mode > 0.5) {
        let clamped = clamp(uv * dims, vec2<f32>(0.5), dims - vec2<f32>(0.5));
        return samp(clamped / dims);
    }

    // Continuous texel coordinate; texel centres sit at integers.
    let coord = uv * dims - vec2<f32>(0.5);
    let base = floor(coord);
    let f = coord - base;

    // Per-axis integer-phase test (texel grid aligned to device pixels).
    let near_x = f.x < 0.01 || f.x > 0.99;
    let near_y = f.y < 0.01 || f.y > 0.99;

    // Fully snapped / at rest: one texel maps to one device pixel — a single
    // tap is exact and skips all reconstruction work.
    if (near_x && near_y) {
        let nearest = round(coord);
        return samp((nearest + vec2<f32>(0.5)) / dims);
    }

    // Catmull-Rom weights per axis. The four taps sum to 1 on each axis
    // (partition of unity), so no normalization is needed afterwards.
    let w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    let w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    let w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    let w3 = f * f * (-0.5 + 0.5 * f);

    // Fold the central positive pair into one hardware-bilinear fetch.
    let w12 = w1 + w2;
    let offset12 = w2 / w12;

    // Texel-centre coordinates (in texels) for the three columns/rows, clamped
    // so the outer taps never read past the texture's edge. The recorded
    // content is inset by `BLEED` transparent texels, so those taps fade into
    // transparency instead of smearing the border.
    let lo = vec2<f32>(0.5);
    let hi = dims - vec2<f32>(0.5);

    let c0 = clamp(base - 0.5, lo, hi);              // texel base-1 centre
    let c12 = clamp(base + offset12 + 0.5, lo, hi);  // bilinear-blended pair
    let c3 = clamp(base + 2.5, lo, hi);              // texel base+2 centre

    // Separable collapse: skip an axis that is at integer phase.
    if (near_y) {
        // Horizontal slide: only the central row contributes (w0.y, w3.y ~ 0).
        let y = c12.y / dims.y;
        return samp(vec2<f32>(c0.x / dims.x, y)) * w0.x
             + samp(vec2<f32>(c12.x / dims.x, y)) * w12.x
             + samp(vec2<f32>(c3.x / dims.x, y)) * w3.x;
    }
    if (near_x) {
        // Vertical slide: only the central column contributes.
        let x = c12.x / dims.x;
        return samp(vec2<f32>(x, c0.y / dims.y)) * w0.y
             + samp(vec2<f32>(x, c12.y / dims.y)) * w12.y
             + samp(vec2<f32>(x, c3.y / dims.y)) * w3.y;
    }

    // General 2D (diagonal or scaled) motion: the full 9-tap Catmull-Rom.
    let ux0 = c0.x / dims.x;
    let ux1 = c12.x / dims.x;
    let ux2 = c3.x / dims.x;
    let uy0 = c0.y / dims.y;
    let uy1 = c12.y / dims.y;
    let uy2 = c3.y / dims.y;

    var color = samp(vec2<f32>(ux0, uy0)) * (w0.x * w0.y);
    color = color + samp(vec2<f32>(ux1, uy0)) * (w12.x * w0.y);
    color = color + samp(vec2<f32>(ux2, uy0)) * (w3.x * w0.y);
    color = color + samp(vec2<f32>(ux0, uy1)) * (w0.x * w12.y);
    color = color + samp(vec2<f32>(ux1, uy1)) * (w12.x * w12.y);
    color = color + samp(vec2<f32>(ux2, uy1)) * (w3.x * w12.y);
    color = color + samp(vec2<f32>(ux0, uy2)) * (w0.x * w3.y);
    color = color + samp(vec2<f32>(ux1, uy2)) * (w12.x * w3.y);
    color = color + samp(vec2<f32>(ux2, uy2)) * (w3.x * w3.y);
    return color;
}
