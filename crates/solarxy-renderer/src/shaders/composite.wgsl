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
    ssao_enabled: u32,
    ssao_strength: f32,
    tone_mode: u32,
    exposure: f32,
}
@group(1) @binding(0) var<uniform> composite: CompositeParams;

@group(2) @binding(0) var ssao_texture: texture_2d<f32>;
@group(2) @binding(1) var ssao_sampler: sampler;

fn tone_none(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3(0.0), vec3(1.0));
}

fn tone_linear(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3(0.0), vec3(1.0));
}

fn tone_reinhard(c: vec3<f32>) -> vec3<f32> {
    return c / (c + vec3(1.0));
}

fn tone_aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3(0.0), vec3(1.0));
}

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(scene_texture, tex_sampler, in.uv).rgb;
    if composite.bloom_enabled != 0u {
        let bloom_color = textureSample(bloom_texture, tex_sampler, in.uv).rgb;
        color = color + bloom_color * composite.bloom_strength;
    }
    if composite.ssao_enabled != 0u {
        let ao = textureSample(ssao_texture, ssao_sampler, in.uv).r;
        color = color * mix(1.0, ao, composite.ssao_strength);
    }
    color = color * composite.exposure;
    var mapped: vec3<f32>;
    switch composite.tone_mode {
        case 1u: { mapped = tone_linear(color); }
        case 2u: { mapped = tone_reinhard(color); }
        case 3u: { mapped = tone_aces(color); }
        default: { mapped = tone_none(color); }
    }
    return vec4<f32>(mapped, 1.0);
}
