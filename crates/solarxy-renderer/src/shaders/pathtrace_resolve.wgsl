// The traced image's handoff into the shared post chain.
//
// One fullscreen triangle copying the accumulator's running mean into the
// linear high-dynamic-range target the composite reads. That is the whole pass,
// and its smallness is the point: everything a look is made of -- exposure, both
// grading tables, the tone map, the grade, bloom, the selection rim -- already
// runs downstream of that target for the rasterizer, so a traced image inherits
// all of it by landing in the same place rather than by growing its own copy.
//
// It lives beside the other fullscreen passes rather than under `pathtrace/`
// because it is an ordinary render pass: it declares a vertex and a fragment
// stage, it is never composed into a compute kernel, and the rules that
// directory enforces about derivatives and barriers have nothing to say about
// it.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    return out;
}

// The accumulation target, read rather than sampled.
//
// `rgba32float` is unfilterable without an optional feature, and filtering is
// not wanted here anyway: source and destination are the same size, so every
// destination pixel has exactly one source texel and any interpolation would be
// a blur applied to an image that has not been resampled. `textureLoad` at the
// fragment's own integer position says that, and needs no sampler at all.
@group(0) @binding(0) var accumulated: texture_2d<f32>;

@fragment
fn fs_resolve(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = vec2<i32>(in.position.xy);
    // Alpha is one rather than the accumulator's: the mean carries a coverage
    // lane nothing downstream reads, and handing the composite a partially
    // transparent scene would darken it against whatever the target held.
    return vec4<f32>(textureLoad(accumulated, texel, 0).rgb, 1.0);
}
