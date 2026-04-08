struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    near: f32,
    far: f32,
    inspection_mode: u32,
    texel_density_target: f32,
    material_override: u32,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct ShadowUniform {
    light_vp: mat4x4<f32>}
@group(1) @binding(0) var<uniform> shadow_uni: ShadowUniform;
@group(1) @binding(1) var shadow_map: texture_depth_2d;
@group(1) @binding(2) var shadow_sampler: sampler_comparison;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) light_clip_pos: vec4<f32>,
};

@vertex
fn vs_floor(
    @location(0) position: vec3<f32>,
) -> VertexOutput {
    let world = vec4<f32>(position, 1.0);
    var out: VertexOutput;
    out.clip_position = camera.view_proj * world;
    out.light_clip_pos = shadow_uni.light_vp * world;
    return out;
}

@fragment
fn fs_floor(in: VertexOutput) -> @location(0) vec4<f32> {
    let proj = in.light_clip_pos.xyz / in.light_clip_pos.w;
    let uv = proj.xy * vec2(0.5, -0.5) + 0.5;
    let in_map = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let lit = select(1.0, textureSampleCompare(shadow_map, shadow_sampler, uv, proj.z - 0.002), in_map);
    return vec4(0.0, 0.0, 0.0, (1.0 - lit) * 0.6);
}
