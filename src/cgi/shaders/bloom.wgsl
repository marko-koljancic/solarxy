// Bloom extraction and blur passes.
// Operates pre-tone-map on linear HDR values.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    // Flip V so uv (0,0) maps to texture top-left
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@group(0) @binding(0) var source_texture: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;

struct BloomParams {
    threshold: f32,
    strength: f32,
    texel_size: vec2<f32>,
}
@group(1) @binding(0) var<uniform> params: BloomParams;

fn luminance(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

@fragment
fn fs_brightness_extract(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(source_texture, source_sampler, in.uv).rgb;
    let lum = luminance(color);
    let contribution = max(lum - params.threshold, 0.0) / max(lum, 0.001);
    return vec4<f32>(color * contribution, 1.0);
}

// 9-tap separable Gaussian, sigma ~= 2.0
const OFFSETS: array<f32, 4> = array<f32, 4>(1.0, 2.0, 3.0, 4.0);
const WEIGHTS: array<f32, 5> = array<f32, 5>(
    0.2270270270,
    0.1945945946,
    0.1216216216,
    0.0540540541,
    0.0162162162,
);

@fragment
fn fs_blur_horizontal(in: VertexOutput) -> @location(0) vec4<f32> {
    var result = textureSample(source_texture, source_sampler, in.uv).rgb * WEIGHTS[0];
    for (var i = 0u; i < 4u; i++) {
        let offset = vec2<f32>(OFFSETS[i] * params.texel_size.x, 0.0);
        result += textureSample(source_texture, source_sampler, in.uv + offset).rgb * WEIGHTS[i + 1u];
        result += textureSample(source_texture, source_sampler, in.uv - offset).rgb * WEIGHTS[i + 1u];
    }
    return vec4<f32>(result, 1.0);
}

@fragment
fn fs_blur_vertical(in: VertexOutput) -> @location(0) vec4<f32> {
    var result = textureSample(source_texture, source_sampler, in.uv).rgb * WEIGHTS[0];
    for (var i = 0u; i < 4u; i++) {
        let offset = vec2<f32>(0.0, OFFSETS[i] * params.texel_size.y);
        result += textureSample(source_texture, source_sampler, in.uv + offset).rgb * WEIGHTS[i + 1u];
        result += textureSample(source_texture, source_sampler, in.uv - offset).rgb * WEIGHTS[i + 1u];
    }
    return vec4<f32>(result, 1.0);
}
