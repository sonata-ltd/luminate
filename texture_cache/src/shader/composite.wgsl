@group(0) @binding(0) var cache_texture: texture_2d<f32>;
@group(0) @binding(1) var cache_sampler: sampler;

// Eight scalars, exactly 32 bytes, matching the Rust side.
// Twelve scalars, exactly 48 bytes, matching the Rust side.
struct Params {
    opacity: f32,
    // Reconstruction kernel: 0 = Catmull-Rom, 1 = a single bilinear tap.
    // `FilterQuality::Snap` shares the single-tap value; its crispness comes
    // from the snapped geometry, not from here.
    mode: f32,
    // How far the genie's stretch has run. 0.0 means no warp and takes
    // the fast path below.
    warp_stretch: f32,
    // How far its squash has run: the travel along the axis.
    warp_squash: f32,
    // The width of the band the rows converge on, as a fraction of the
    // content's own width.
    warp_target_width: f32,
    // How sharply a row's travel lags with its distance from the anchor.
    warp_stretch_power: f32,
    // The side curve's control values, at the wide end and the neck end.
    warp_curve_in: f32,
    warp_curve_out: f32,
    // Axis mirrors putting the anchor corner at the origin, 0.0 or 1.0.
    warp_flip_x: f32,
    warp_flip_y: f32,
    pad0: f32,
    pad1: f32,
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
@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let source = warp_source(in.uv);
    // `z` is the hit flag: the collapsed shape does not cover the whole
    // destination rectangle, and the rest of it shows nothing.
    if (source.z < 0.5) {
        return vec4<f32>(0.0);
    }
    return reconstruct(source.xy) * params.opacity;
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

    // Into anchor space, where the corner it collapses into is the origin.
    var p = uv;
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
    if (u < 0.0 || u > 1.0) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }

    // Undo the travel to find which row this is showing. The exponent is
    // the per-row lag: 1.0 at the anchor, rising with distance from it, so
    // a far row's shift is a high power of a number below one and is
    // therefore small. That is what keeps the wide end of the shape on
    // screen. `warp::STRETCH_POWER` is the same 2.0; keep the two in step.
    // Past the far edge there is nothing left to show.
    let v = p.y + pow(s, 1.0 + params.warp_stretch_power * k * p.y);
    if (v < 0.0 || v > 1.0) {
        return vec3<f32>(0.0, 0.0, 0.0);
    }

    var src = vec2<f32>(u, v);
    if (params.warp_flip_x > 0.5) { src.x = 1.0 - src.x; }
    if (params.warp_flip_y > 0.5) { src.y = 1.0 - src.y; }
    return vec3<f32>(src, 1.0);
}

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
