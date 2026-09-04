// The atlas probe: a list of taps in, the colour each one sampled out.
//
// Composed after `atlas.wgsl`, and the only thing that can answer whether the
// packing arithmetic and the shader's reading of it agree. The property that
// matters most is invisible from Rust: whether a bilinear tap at the extreme
// edge of a sub-rectangle stays inside its guard ring, or reaches the texture
// packed next to it. That is a question about hardware interpolation, so it
// takes hardware to answer.
//
// It reads no camera and traverses nothing, so a wrong answer here is a
// packing or sampling disagreement rather than something the scene could
// explain.

struct AtlasTap {
    // The sub-rectangle, page-normalized: `(u_scale, v_scale, u_offset, v_offset)`.
    rect: vec4f,
    // The texture coordinate, before wrapping.
    uv: vec2f,
    // The packed descriptor.
    desc: u32,
    pad: u32,
}

// The probe's own group. Group 2 is the atlas, which `atlas.wgsl` declares;
// group 1 is left empty so the sampled group keeps its number, which is the
// whole reason the numbering was reserved.
@group(0) @binding(0) var<storage, read> atlas_taps: array<AtlasTap>;
@group(0) @binding(1) var<storage, read_write> atlas_results: array<vec4f>;

// Taps per row of the dispatch grid; the host owns the value so the two cannot
// drift.
override TAP_WIDTH: u32 = 64u;

@compute @workgroup_size(8, 8, 1)
fn atlas_probe(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.y * TAP_WIDTH + gid.x;
    if index >= arrayLength(&atlas_taps) {
        return;
    }
    let tap = atlas_taps[index];
    atlas_results[index] = sample_atlas(tap.desc, tap.rect, tap.uv);
}
