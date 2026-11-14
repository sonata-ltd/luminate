struct Params {
    size_scale_opacity: vec4<f32>,
    center_radii: vec4<f32>,
    corners: vec4<f32>,
    offsets: array<vec4<f32>, 8>,
    colors: array<vec4<f32>, 8>,
}
@group(0) @binding(0) var<uniform> params: Params;
struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}
@vertex fn vs_main(@builtin(vertex_index) index: u32) -> Vertex {
    let positions = array<vec2<f32>, 3>(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    let position = positions[index];
    var out: Vertex;
    out.position = vec4(position, 0.0, 1.0);
    out.uv = vec2((position.x + 1.0) * 0.5, (1.0 - position.y) * 0.5);
    return out;
}
fn coverage(uv: vec2<f32>) -> f32 {
    let size = params.size_scale_opacity.xy;
    let p = uv * size;
    var alpha = 1.0;
    for (var i = 0u; i < 4u; i += 1u) {
        let r = params.corners[i];
        let corner = vec2(select(p.x, size.x - p.x, i == 1u || i == 2u), select(p.y, size.y - p.y, i >= 2u));
        if r > 0.0 && corner.x < r && corner.y < r {
            let distance = length(corner - vec2(r)) - r;
            alpha = min(alpha, clamp(0.5 - distance * params.size_scale_opacity.z, 0.0, 1.0));
        }
    }
    return alpha;
}
@fragment fn fs_main(in: Vertex) -> @location(0) vec4<f32> {
    let t = clamp(length((in.uv - params.center_radii.xy) / params.center_radii.zw), 0.0, 1.0);
    var color = params.colors[0];
    for (var i = 1u; i < 8u; i += 1u) {
        let start = params.offsets[i - 1u].x;
        let end = params.offsets[i].x;
        if t < end {
            var weight = 0.0;
            if end > start { weight = clamp((t - start) / (end - start), 0.0, 1.0); }
            color = mix(params.colors[i - 1u], params.colors[i], weight);
            break;
        }
        color = params.colors[i];
    }
    return color * (coverage(in.uv) * params.size_scale_opacity.w);
}
