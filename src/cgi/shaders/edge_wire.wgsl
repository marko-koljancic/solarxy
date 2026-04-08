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
    depth_near: f32,
    depth_far: f32,
}
@group(0) @binding(0)
var<uniform> camera: Camera;

struct WireParams {
    color: vec4<f32>,
    width_px: f32,
    viewport_w: f32,
    viewport_h: f32,
    _pad: f32,
}
@group(1) @binding(0)
var<uniform> params: WireParams;

@group(2) @binding(0)
var<storage, read> positions: array<vec4<f32>>;
@group(2) @binding(1)
var<storage, read> edge_indices: array<u32>;

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
fn vs_edge_quad(@builtin(vertex_index) vid: u32, instance: InstanceInput) -> VertexOutput {
    let edge_id = vid / 6u;
    let corner_id = vid % 6u;
    let i0 = edge_indices[edge_id * 2u];
    let i1 = edge_indices[edge_id * 2u + 1u];

    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let p0_clip = camera.view_proj * model_matrix * vec4(positions[i0].xyz, 1.0);
    let p1_clip = camera.view_proj * model_matrix * vec4(positions[i1].xyz, 1.0);

    let p0_ndc = p0_clip.xy / p0_clip.w;
    let p1_ndc = p1_clip.xy / p1_clip.w;

    let screen_dir = p1_ndc - p0_ndc;
    let screen_len = length(screen_dir * vec2(params.viewport_w, params.viewport_h));

    var dir: vec2<f32>;
    if screen_len < 0.001 {
        dir = vec2(1.0, 0.0);
    } else {
        dir = normalize(screen_dir);
    }
    let perp = vec2(-dir.y, dir.x);
    let half_w = vec2(params.width_px / params.viewport_w, params.width_px / params.viewport_h);

    let idx_map = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let ci = idx_map[corner_id];
    let is_p1 = f32(ci >= 2u);
    let side = select(-1.0, 1.0, ci % 2u == 1u);

    let base_ndc = mix(p0_ndc, p1_ndc, is_p1);
    let ndc = base_ndc + perp * half_w * side;
    let w = mix(p0_clip.w, p1_clip.w, is_p1);
    let z_ndc = mix(p0_clip.z / p0_clip.w, p1_clip.z / p1_clip.w, is_p1);

    var out: VertexOutput;
    out.clip_position = vec4(ndc * w, z_ndc * w, w);
    return out;
}

@fragment
fn fs_edge_wire(in: VertexOutput) -> @location(0) vec4<f32> {
    return params.color;
}

@fragment
fn fs_edge_wire_ghosted(in: VertexOutput) -> @location(0) vec4<f32> {
    return vec4(params.color.rgb, params.color.a * 0.5);
}

@vertex
fn vs_uv_edge_quad(@builtin(vertex_index) vid: u32) -> VertexOutput {
    let edge_id = vid / 6u;
    let corner_id = vid % 6u;
    let i0 = edge_indices[edge_id * 2u];
    let i1 = edge_indices[edge_id * 2u + 1u];

    let p0_clip = camera.view_proj * vec4(positions[i0].xy, 0.0, 1.0);
    let p1_clip = camera.view_proj * vec4(positions[i1].xy, 0.0, 1.0);

    let p0_ndc = p0_clip.xy / p0_clip.w;
    let p1_ndc = p1_clip.xy / p1_clip.w;

    let screen_dir = p1_ndc - p0_ndc;
    let screen_len = length(screen_dir * vec2(params.viewport_w, params.viewport_h));

    var dir: vec2<f32>;
    if screen_len < 0.001 {
        dir = vec2(1.0, 0.0);
    } else {
        dir = normalize(screen_dir);
    }
    let perp = vec2(-dir.y, dir.x);
    let half_w = vec2(params.width_px / params.viewport_w, params.width_px / params.viewport_h);

    let idx_map = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    let ci = idx_map[corner_id];
    let is_p1 = f32(ci >= 2u);
    let side = select(-1.0, 1.0, ci % 2u == 1u);

    let base_ndc = mix(p0_ndc, p1_ndc, is_p1);
    let ndc = base_ndc + perp * half_w * side;
    let w = mix(p0_clip.w, p1_clip.w, is_p1);
    let z_ndc = mix(p0_clip.z / p0_clip.w, p1_clip.z / p1_clip.w, is_p1);

    var out: VertexOutput;
    out.clip_position = vec4(ndc * w, z_ndc * w, w);
    return out;
}
