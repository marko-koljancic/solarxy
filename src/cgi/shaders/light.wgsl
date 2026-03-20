struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct LightEntry {
    position: vec3<f32>,  // bytes 0-11; WGSL pads to 16 before next vec3
    color: vec3<f32>,     // bytes 16-27
    intensity: f32,       // bytes 28-31
}
struct LightsUniform { lights: array<LightEntry, 3>, }
@group(1) @binding(0)
var<uniform> lights: LightsUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec3<f32>,
};

@vertex
fn vs_main(
    model: VertexInput,
    @builtin(instance_index) idx: u32,
) -> VertexOutput {
    let light = lights.lights[idx];
    let scale = 0.1;
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(model.position * scale + light.position, 1.0);
    out.color = light.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4<f32>(in.color, 1.0);
}
