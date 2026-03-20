struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> camera: Camera;

struct GridUniform {
    cell_size: f32,
    pad0: f32,
    pad1: f32,
    pad2: f32,
}
@group(0) @binding(1) var<uniform> grid: GridUniform;

struct VertexInput {
    @location(0) position: vec3<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_xz: vec2<f32>,
}

@vertex
fn vs_grid(model: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip_position = camera.view_proj * vec4<f32>(model.position, 1.0);
    out.world_xz = model.position.xz;
    return out;
}

@fragment
fn fs_grid(in: VertexOutput) -> @location(0) vec4<f32> {
    let dx = min(fract(in.world_xz.x / grid.cell_size), 1.0 - fract(in.world_xz.x / grid.cell_size));
    let dz = min(fract(in.world_xz.y / grid.cell_size), 1.0 - fract(in.world_xz.y / grid.cell_size));
    let alpha = 1.0 - smoothstep(0.0, 0.02, min(dx, dz));
    if alpha < 0.01 {
        discard;
    }
    return vec4<f32>(0.55, 0.60, 0.65, alpha * 0.5);
}
