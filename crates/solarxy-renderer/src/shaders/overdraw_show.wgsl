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

@group(0) @binding(0) var count_texture: texture_2d<f32>;
@group(0) @binding(1) var count_sampler: sampler;

fn ramp(count: f32) -> vec3<f32> {
    if count <= 0.0 { return vec3<f32>(0.0, 0.0, 0.0); }
    if count <= 1.0 { return vec3<f32>(0.118, 0.227, 0.541); }
    if count <= 3.0 { return vec3<f32>(0.055, 0.647, 0.914); }
    if count <= 6.0 { return vec3<f32>(0.988, 0.827, 0.302); }
    if count <= 10.0 { return vec3<f32>(0.976, 0.451, 0.086); }
    return vec3<f32>(0.863, 0.149, 0.149);
}

@fragment
fn fs_show(in: VertexOutput) -> @location(0) vec4<f32> {
    let dims = vec2<f32>(textureDimensions(count_texture));
    let screen_uv = in.position.xy / dims;
    let count = textureSample(count_texture, count_sampler, screen_uv).r;
    return vec4<f32>(ramp(count), 1.0);
}
