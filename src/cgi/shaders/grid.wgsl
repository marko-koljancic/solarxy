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
@group(0) @binding(0) var<uniform> camera: Camera;

struct GridUniform {
    cell_size: f32,
    color_r: f32,
    color_g: f32,
    color_b: f32,
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
    let alpha = 1.0 - smoothstep(0.0, 0.008, min(dx, dz));
    if alpha < 0.01 {
        discard;
    }
    let world_pos = vec3<f32>(in.world_xz.x, 0.0, in.world_xz.y);
    let dist = distance(camera.view_pos.xyz, world_pos);
    let fade = 1.0 - smoothstep(grid.cell_size * 30.0, grid.cell_size * 60.0, dist);
    let color = vec3<f32>(grid.color_r, grid.color_g, grid.color_b);
    return vec4<f32>(color, alpha * 0.75 * fade);
}
