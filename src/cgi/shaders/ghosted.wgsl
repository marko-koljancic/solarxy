struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
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

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
}

@vertex
fn vs_ghosted(model: VertexInput, instance: InstanceInput) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );
    let world_pos = model_matrix * vec4<f32>(model.position, 1.0);
    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_pos;
    return out;
}

@fragment
fn fs_ghosted_fill(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(0.65, 0.70, 0.80, 0.25);
}

@fragment
fn fs_ghosted_wire(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(0.15, 0.18, 0.30, 0.85);
}

@group(1) @binding(0)
var<uniform> wire_color: vec4<f32>;

@fragment
fn fs_wireframe(in: VertexOutput) -> @location(0) vec4<f32> {
    return wire_color;
}
