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
    _pad: vec2<f32>,
}
@group(0) @binding(4) var<uniform> camera: Camera;

struct SsaoKernel {
    samples: array<vec4<f32>, 64>,
}
@group(0) @binding(5) var<uniform> kernel: SsaoKernel;

const RADIUS: f32 = 0.5;
const BIAS: f32 = 0.025;
const KERNEL_SIZE: u32 = 64u;

fn reconstruct_view_pos(uv: vec2<f32>, depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv * 2.0 - 1.0, depth, 1.0);
    let view_pos = camera.inv_proj * ndc;
    return view_pos.xyz / view_pos.w;
}

@fragment
fn fs_ssao(in: VertexOutput) -> @location(0) vec4<f32> {
    let tex_size = vec2<f32>(textureDimensions(normal_texture));
    let noise_size = vec2<f32>(textureDimensions(noise_texture));

    let depth = textureSample(depth_texture, tex_sampler, in.uv);
    if depth >= 1.0 {
        return vec4<f32>(1.0);
    }

    let frag_pos = reconstruct_view_pos(in.uv, depth);

    let world_normal = textureSample(normal_texture, tex_sampler, in.uv).xyz * 2.0 - 1.0;
    let view_normal = normalize((camera.view * vec4<f32>(world_normal, 0.0)).xyz);

    let noise_uv = in.uv * tex_size / noise_size;
    let random_vec = textureSample(noise_texture, tex_sampler, noise_uv).xy;
    let random_vec3 = vec3<f32>(random_vec, 0.0);

    let tangent = normalize(random_vec3 - view_normal * dot(random_vec3, view_normal));
    let bitangent = cross(view_normal, tangent);
    let tbn = mat3x3<f32>(tangent, bitangent, view_normal);

    var occlusion = 0.0;
    for (var i = 0u; i < KERNEL_SIZE; i++) {
        let sample_dir = tbn * kernel.samples[i].xyz;
        let sample_pos = frag_pos + sample_dir * RADIUS;

        let proj = camera.proj * vec4<f32>(sample_pos, 1.0);
        var sample_uv = proj.xy / proj.w;
        sample_uv = sample_uv * 0.5 + 0.5;
        sample_uv.y = 1.0 - sample_uv.y;

        let sample_depth = textureSample(depth_texture, tex_sampler, sample_uv);
        let sample_view_pos = reconstruct_view_pos(sample_uv, sample_depth);

        let range_check = smoothstep(0.0, 1.0, RADIUS / abs(frag_pos.z - sample_view_pos.z));
        if sample_view_pos.z >= sample_pos.z + BIAS {
            occlusion += range_check;
        }
    }

    let ao = 1.0 - occlusion / f32(KERNEL_SIZE);
    return vec4<f32>(ao, ao, ao, 1.0);
}
