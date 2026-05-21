// HDRI skybox: a fullscreen triangle that reconstructs the world-space
// view ray per pixel and samples the equirectangular HDRI. The yaw in
// `camera.hdri_rotation` is shared with the IBL cubemap lookups in
// shader.wgsl so the visible sky and the lighting it derives stay in sync.

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
    roughness_scale: f32,
    metallic_scale: f32,
    hdri_rotation: f32,
}
@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var equirect_tex: texture_2d<f32>;
@group(1) @binding(1) var equirect_sampler: sampler;

const PI: f32 = 3.14159265358979;
const INV_TAU: f32 = 0.15915494309189535;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
}

@vertex
fn vs_skybox(@builtin(vertex_index) id: u32) -> VsOut {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    let p = uv * 2.0 - vec2<f32>(1.0, 1.0);
    var out: VsOut;
    out.clip = vec4<f32>(p, 1.0, 1.0);
    out.ndc = p;
    return out;
}

// Yaw a direction around +Y. Matches `rotate_yaw` in shader.wgsl.
fn rotate_yaw(d: vec3<f32>, yaw: f32) -> vec3<f32> {
    let c = cos(yaw);
    let s = sin(yaw);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

@fragment
fn fs_skybox(in: VsOut) -> @location(0) vec4<f32> {
    // Two-point ray reconstruction — correct for perspective and ortho.
    let near_h = camera.inv_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far_h = camera.inv_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let view_dir = normalize(far_h.xyz / far_h.w - near_h.xyz / near_h.w);
    // View space -> world space: inverse of the (orthonormal) view rotation.
    let rot = mat3x3<f32>(camera.view[0].xyz, camera.view[1].xyz, camera.view[2].xyz);
    let world_dir = normalize(transpose(rot) * view_dir);
    let dir = rotate_yaw(world_dir, camera.hdri_rotation);
    // Equirect lookup — matches `sample_equirect` in ibl.rs.
    let u = atan2(dir.z, dir.x) * INV_TAU + 0.5;
    let v = acos(clamp(dir.y, -1.0, 1.0)) / PI;
    let color = textureSampleLevel(equirect_tex, equirect_sampler, vec2<f32>(u, v), 0.0).rgb;
    return vec4<f32>(color, 1.0);
}
