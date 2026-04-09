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

@group(0) @binding(0) var depth_texture: texture_depth_2d;
@group(0) @binding(1) var normal_texture: texture_2d<f32>;
@group(0) @binding(2) var noise_texture: texture_2d<f32>;
@group(0) @binding(3) var tex_sampler: sampler;

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
@group(1) @binding(0) var<uniform> camera: Camera;

struct SsaoKernel {
    samples: array<vec4<f32>, 64>,
}
@group(0) @binding(4) var<uniform> kernel: SsaoKernel;

const RADIUS_FACTOR: f32 = 0.04;
const BIAS_FACTOR: f32 = 0.1;
const KERNEL_SIZE: u32 = 64u;

fn reconstruct_view_pos(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, depth, 1.0);
    let view_pos = camera.inv_proj * ndc;
    return view_pos.xyz / view_pos.w;
}

@fragment
fn fs_ssao(in: VertexOutput) -> @location(0) vec4<f32> {
    let normal_tex_size = vec2<f32>(textureDimensions(normal_texture));
    let depth_tex_size = vec2<f32>(textureDimensions(depth_texture));
    let noise_size_i = vec2<i32>(textureDimensions(noise_texture));

    let depth_coords = vec2<i32>(in.uv * depth_tex_size);
    let depth = textureLoad(depth_texture, depth_coords, 0);
    if depth >= 1.0 {
        return vec4<f32>(1.0);
    }

    let frag_pos = reconstruct_view_pos(in.uv, depth);

    let world_normal = textureSample(normal_texture, tex_sampler, in.uv).xyz * 2.0 - 1.0;
    let view_normal = normalize((camera.view * vec4<f32>(world_normal, 0.0)).xyz);

    let pixel_coord = vec2<i32>(in.uv * normal_tex_size);
    let noise_coord = vec2<i32>(
        pixel_coord.x - (pixel_coord.x / noise_size_i.x) * noise_size_i.x,
        pixel_coord.y - (pixel_coord.y / noise_size_i.y) * noise_size_i.y,
    );
    let random_vec = textureLoad(noise_texture, noise_coord, 0).xy;

    let ref_up = select(
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(1.0, 0.0, 0.0),
        abs(view_normal.y) > 0.95,
    );
    let t0 = normalize(cross(ref_up, view_normal));
    let b0 = cross(view_normal, t0);
    let rc = random_vec.x;
    let rs = random_vec.y;
    let tangent = t0 * rc + b0 * rs;
    let bitangent = -t0 * rs + b0 * rc;
    let tbn = mat3x3<f32>(tangent, bitangent, view_normal);

    let radius = RADIUS_FACTOR * abs(frag_pos.z);
    let bias = BIAS_FACTOR * radius;

    let depth_max = vec2<i32>(depth_tex_size) - 1;
    var occlusion = 0.0;
    for (var i = 0u; i < KERNEL_SIZE; i++) {
        let sample_dir = tbn * kernel.samples[i].xyz;
        let sample_pos = frag_pos + sample_dir * radius;

        let proj = camera.proj * vec4<f32>(sample_pos, 1.0);
        var sample_uv = proj.xy / proj.w;
        sample_uv = sample_uv * 0.5 + 0.5;
        sample_uv.y = 1.0 - sample_uv.y;

        let sample_coords = clamp(vec2<i32>(sample_uv * depth_tex_size), vec2<i32>(0), depth_max);
        let sample_depth = textureLoad(depth_texture, sample_coords, 0);
        let sample_view_pos = reconstruct_view_pos(sample_uv, sample_depth);

        let range_check = smoothstep(0.0, 1.0, radius / abs(frag_pos.z - sample_view_pos.z));
        if sample_view_pos.z >= sample_pos.z + bias {
            occlusion += range_check;
        }
    }

    let ao = pow(1.0 - occlusion / f32(KERNEL_SIZE), 2.0);
    return vec4<f32>(ao, ao, ao, 1.0);
}
