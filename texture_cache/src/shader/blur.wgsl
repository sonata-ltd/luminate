@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

// `taps` is fixed at eight entries: `blur::gaussian::plan` adds passes
// rather than taps so this bound always holds, and a `vec4` per tap keeps
// the array's stride at the 16 bytes every backend accepts for a uniform.
// x is the offset in source texels, y the weight, z and w are padding.
struct Params {
    // Size of one source texel in normalised coordinates.
    texel: vec2<f32>,
    // (1, 0) for the horizontal pass, (0, 1) for the vertical one.
    direction: vec2<f32>,
    tap_count: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
    taps: array<vec4<f32>, 8>,
}
@group(0) @binding(2) var<uniform> params: Params;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    // The same oversized triangle the composite pass uses.
    let x = f32(i32(index & 1u) * 4 - 1);
    let y = f32(i32(index >> 1u) * 4 - 1);
    var out: VertexOutput;
    out.position = vec4<f32>(x, y, 0.0, 1.0);
    out.uv = vec2<f32>((x + 1.0) * 0.5, (1.0 - y) * 0.5);
    return out;
}

// The resolution reduction, not the blur: one bilinear fetch per
// destination texel.
//
// When the source level's dimension is even, the destination centre lands
// exactly on the corner between four source texels and the fetch is an
// exact 2x2 box average. When it is odd it is not: halving 5 to 3 puts the
// destination centres at source coordinates 0.833, 2.5 and 4.167, so the
// middle one is a pure copy of a single texel and its neighbours are
// 2/3-1/3 mixes. That error is spread across the whole row rather than
// confined to the far edge, and one wrong texel becomes `downscale` wrong
// device pixels once the derivative is drawn back at screen size.
//
// It is accepted rather than fixed: the residual Gaussian that follows
// hides most of it, and the level sizes still agree with
// `Blur::target_size` either way. It does mean this is NOT the same filter
// `blur::cpu::downsample` applies, which averages whole and partial blocks
// of the source explicitly.
@fragment
fn fs_halve(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSampleLevel(source_texture, source_sampler, in.uv, 0.0);
}

// One separable Gaussian pass. The first tap is the centre and stands
// alone; every other is mirrored, so a kernel of `2n - 1` texels costs `n`
// fetches.
//
// Clamp-to-edge is the sampler's, not ours: the address mode duplicates the
// edge row, which is what the CSS backdrop-filter specification prescribes
// and what keeps an opaque edge opaque instead of fading it out.
//
// The texture is premultiplied and no channel is ever un- or
// re-premultiplied here — the sum is a plain weighted sum of what the
// sampler returns.
//
// Which space that sum happens in depends on the build. Every target
// carries the recording format: sRGB-typed when
// `iced_graphics::color::GAMMA_CORRECTION` is on, in which case the
// hardware linearises on read and re-encodes on write and the sum is in
// linear light; plain `Rgba8Unorm` under the crate's default `web-colors`,
// in which case it sums the stored bytes exactly as the software chain
// does. See the "Colour space" section of `blur/gpu.rs` for what that
// costs, and for why premultiplied channels make the sRGB case a third
// space rather than true linear light.
@fragment
fn fs_blur(in: VertexOutput) -> @location(0) vec4<f32> {
    let step = params.direction * params.texel;
    var color = textureSampleLevel(source_texture, source_sampler, in.uv, 0.0)
        * params.taps[0].y;

    for (var i = 1u; i < params.tap_count; i = i + 1u) {
        let tap = params.taps[i];
        let offset = step * tap.x;
        color = color
            + textureSampleLevel(source_texture, source_sampler, in.uv + offset, 0.0) * tap.y
            + textureSampleLevel(source_texture, source_sampler, in.uv - offset, 0.0) * tap.y;
    }

    return color;
}
