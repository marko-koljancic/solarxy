// Selection outline: the jump-flood stages. The pipeline is three stages:
//
// 1. Mask: the selected objects' silhouettes render into an R8 target
//    (that pass reuses validation.wgsl's transform-only vertex shader
//    with a white color uniform; nothing of it lives here).
// 2. Jump flood (fs_jfa_init + fs_jfa_step): nearest-seed pixel
//    coordinates propagate through a ping-pong pair of Rg32Float targets
//    in halving steps (16, 8, 4, 2, 1), so any outline width up to 16 px
//    resolves in five fullscreen passes. Rg32Float keeps integer pixel
//    coordinates exact at any target size (f16 loses integers past 2048).
// 3. Blit (fs_outline): a constant-width, constant-color rim OUTSIDE the
//    silhouette draws onto the composited swapchain view, after tone
//    mapping, so the outline never blooms and never darkens under AO.
//
// (-1, -1) is the no-seed sentinel throughout.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// The composite pass's fullscreen triangle, verbatim: with a pane
// viewport set, uv spans 0..1 across the viewport, which is exactly how
// the pane maps onto the shared offscreen targets.
@vertex
fn vs_fullscreen(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

// The source texture: the mask for init, the previous ping-pong for the
// steps and the blit. Read with textureLoad only (Rg32Float is not
// filterable).
@group(0) @binding(0) var t_src: texture_2d<f32>;

struct OutlineParams {
    // rgb premultiplied by nothing; a scales the rim.
    color: vec4<f32>,
    // Rim width in pixels (resolved distance from the silhouette edge).
    width: f32,
    // The current jump-flood step size (only the step passes read it).
    step: i32,
    _pad0: f32,
    _pad1: f32,
}
@group(1) @binding(0) var<uniform> params: OutlineParams;

@fragment
fn fs_jfa_init(in: VertexOutput) -> @location(0) vec2<f32> {
    let p = vec2<i32>(in.position.xy);
    let m = textureLoad(t_src, p, 0).r;
    if m > 0.5 {
        // Seed: this pixel's own center coordinate.
        return in.position.xy;
    }
    return vec2<f32>(-1.0, -1.0);
}

@fragment
fn fs_jfa_step(in: VertexOutput) -> @location(0) vec2<f32> {
    let dims = vec2<i32>(textureDimensions(t_src));
    let p = vec2<i32>(in.position.xy);
    var best = vec2<f32>(-1.0, -1.0);
    var best_d = 1e20;
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = p + vec2<i32>(dx, dy) * params.step;
            if q.x < 0 || q.y < 0 || q.x >= dims.x || q.y >= dims.y {
                continue;
            }
            let seed = textureLoad(t_src, q, 0).rg;
            if seed.x < 0.0 {
                continue;
            }
            let d = distance(seed, in.position.xy);
            if d < best_d {
                best_d = d;
                best = seed;
            }
        }
    }
    return best;
}

@fragment
fn fs_outline(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(t_src));
    let p = vec2<i32>(in.uv * dims);
    let seed = textureLoad(t_src, p, 0).rg;
    if seed.x < 0.0 {
        discard;
    }
    let d = distance(seed, vec2<f32>(p) + vec2<f32>(0.5, 0.5));
    // Outside the silhouette only (a seed's own distance is ~0), out to
    // the preferred width, with a one-pixel soft edge.
    if d <= 0.5 || d > params.width {
        discard;
    }
    let a = params.color.a * clamp(params.width - d + 1.0, 0.0, 1.0);
    return vec4<f32>(params.color.rgb, a);
}
