struct ShadowUniform {
    light_vp: mat4x4<f32>,
}
@group(0) @binding(0) var<uniform> shadow: ShadowUniform;

@group(1) @binding(0) var t_diffuse: texture_2d<f32>;
@group(1) @binding(1) var s_diffuse: sampler;

struct MaterialUniform {
    roughness_factor: f32,
    metallic_factor: f32,
    ao_strength: f32,
    alpha_cutoff: f32,
    emissive: vec3<f32>,
    alpha_mode: u32,
}
@group(1) @binding(8) var<uniform> material: MaterialUniform;

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

struct ShadowVaryings {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
}

@vertex
fn vs_shadow(v: VertexInput, inst: InstanceInput) -> ShadowVaryings {
    let model_matrix = mat4x4<f32>(
        inst.model_matrix_0,
        inst.model_matrix_1,
        inst.model_matrix_2,
        inst.model_matrix_3,
    );
    var out: ShadowVaryings;
    out.clip_position = shadow.light_vp * model_matrix * vec4<f32>(v.position, 1.0);
    out.tex_coords = v.tex_coords;
    return out;
}

@fragment
fn fs_shadow(in: ShadowVaryings) {
    if material.alpha_mode == 1u {
        let alpha = textureSample(t_diffuse, s_diffuse, in.tex_coords).a;
        if alpha < material.alpha_cutoff {
            discard;
        }
    }
}
