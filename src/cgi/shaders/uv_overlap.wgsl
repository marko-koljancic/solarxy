struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
    view: mat4x4<f32>,
    proj: mat4x4<f32>,
    inv_proj: mat4x4<f32>,
    near: f32,
    far: f32,
    _pad: vec2<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) tex_coords: vec2<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) tangent: vec3<f32>,
    @location(4) bitangent: vec3<f32>,
}

struct InstanceInput {
    @location(5) model_matrix_0: vec4<f32>,
    @location(6) model_matrix_1: vec4<f32>,
    @location(7) model_matrix_2: vec4<f32>,
    @location(8) model_matrix_3: vec4<f32>,
    @location(9) normal_matrix_0: vec3<f32>,
    @location(10) normal_matrix_1: vec3<f32>,
    @location(11) normal_matrix_2: vec3<f32>,
}

@vertex
fn vs_uv_count(model: VertexInput, instance: InstanceInput) -> @builtin(position) vec4<f32> {
    let uv_pos = vec4(model.tex_coords.x, 1.0 - model.tex_coords.y, 0.0, 1.0);
    return camera.view_proj * uv_pos;
}

@fragment
fn fs_uv_count() -> @location(0) vec4<f32> {
    return vec4(1.0 / 255.0, 0.0, 0.0, 0.0);
}

struct OverlayVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_overlap_fullscreen(@builtin(vertex_index) id: u32) -> OverlayVertex {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: OverlayVertex;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(uv.x, 1.0 - uv.y);
    return out;
}

@group(0) @binding(0) var overlap_texture: texture_2d<f32>;
@group(0) @binding(1) var overlap_sampler: sampler;

@fragment
fn fs_uv_overlap(in: OverlayVertex) -> @location(0) vec4<f32> {
    let count_val = textureSample(overlap_texture, overlap_sampler, in.uv).r;
    if count_val > 1.5 / 255.0 {
        return vec4(1.0, 0.0, 0.0, 0.35);
    }
    return vec4(0.0);
}
