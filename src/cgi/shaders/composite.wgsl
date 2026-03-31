// Final composite pass: additive bloom + Reinhard tone mapping.
// Bloom operates pre-tone-map: the scene texture contains linear HDR values.
// Tone mapping (Reinhard) is applied here after bloom compositing, ensuring
// bloom is driven by true HDR luminance, not compressed LDR values.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var bloom_texture: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;

struct CompositeParams {
    bloom_strength: f32,
    bloom_enabled: u32,
    _pad: vec2<f32>,
}
@group(1) @binding(0) var<uniform> composite: CompositeParams;

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(scene_texture, tex_sampler, in.uv).rgb;
    if (composite.bloom_enabled != 0u) {
        let bloom_color = textureSample(bloom_texture, tex_sampler, in.uv).rgb;
        color = color + bloom_color * composite.bloom_strength;
    }
    // Reinhard tone mapping
    let mapped = color / (color + vec3<f32>(1.0));
    return vec4<f32>(mapped, 1.0);
}
