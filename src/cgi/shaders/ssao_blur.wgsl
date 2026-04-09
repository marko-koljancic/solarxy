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

@group(0) @binding(0) var ao_texture: texture_2d<f32>;
@group(0) @binding(1) var depth_texture: texture_depth_2d;
@group(0) @binding(2) var tex_sampler: sampler;

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

const BLUR_RADIUS: i32 = 6;
const DEPTH_THRESHOLD: f32 = 0.1;

fn linearize_depth(d: f32) -> f32 {
    return camera.near * camera.far / (camera.far - d * (camera.far - camera.near));
}

fn bilateral_blur(uv: vec2<f32>, direction: vec2<f32>) -> f32 {
    let tex_size = vec2<f32>(textureDimensions(ao_texture));
    let depth_size = vec2<f32>(textureDimensions(depth_texture));
    let texel = 1.0 / tex_size;

    let center_ao = textureSample(ao_texture, tex_sampler, uv).r;
    let center_depth = linearize_depth(textureLoad(depth_texture, vec2<i32>(uv * depth_size), 0));

    let depth_max = vec2<i32>(depth_size) - 1;
    var result = center_ao;
    var total_weight = 1.0;

    for (var i = 1; i <= BLUR_RADIUS; i++) {
        let offset = direction * texel * f32(i);
        let gauss_weight = exp(-0.5 * f32(i * i) / 4.0);

        for (var sign = -1; sign <= 1; sign += 2) {
            let sample_uv = uv + offset * f32(sign);
            let sample_ao = textureSample(ao_texture, tex_sampler, sample_uv).r;
            let sample_coords = clamp(vec2<i32>(sample_uv * depth_size), vec2<i32>(0), depth_max);
            let sample_depth = linearize_depth(textureLoad(depth_texture, sample_coords, 0));

            let depth_diff = abs(center_depth - sample_depth);
            let depth_weight = exp(-depth_diff * depth_diff / (2.0 * DEPTH_THRESHOLD * DEPTH_THRESHOLD));

            let weight = gauss_weight * depth_weight;
            result += sample_ao * weight;
            total_weight += weight;
        }
    }

    return result / total_weight;
}

@fragment
fn fs_blur_h(in: VertexOutput) -> @location(0) vec4<f32> {
    let ao = bilateral_blur(in.uv, vec2<f32>(1.0, 0.0));
    return vec4<f32>(ao, ao, ao, 1.0);
}

@fragment
fn fs_blur_v(in: VertexOutput) -> @location(0) vec4<f32> {
    let ao = bilateral_blur(in.uv, vec2<f32>(0.0, 1.0));
    return vec4<f32>(ao, ao, ao, 1.0);
}
