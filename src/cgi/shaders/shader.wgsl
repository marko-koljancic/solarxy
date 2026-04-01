struct Camera {
    view_pos: vec4<f32>,
    view_proj: mat4x4<f32>,
}
@group(1) @binding(0)
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
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
    @location(1) tangent_position: vec3<f32>,
    @location(2) tangent_view_position: vec3<f32>,
    @location(3) tbn_col0: vec3<f32>,
    @location(4) tbn_col1: vec3<f32>,
    @location(5) tbn_col2: vec3<f32>,
    @location(6) light_clip_pos: vec4<f32>,
    @location(7) world_normal: vec3<f32>,
};

struct ShadowUniform {
    light_vp: mat4x4<f32>}
@group(3) @binding(0) var<uniform> shadow_uni: ShadowUniform;
@group(3) @binding(1) var shadow_map: texture_depth_2d;
@group(3) @binding(2) var shadow_sampler: sampler_comparison;

@vertex
fn vs_main(
    model: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    let model_matrix = mat4x4<f32>(
        instance.model_matrix_0,
        instance.model_matrix_1,
        instance.model_matrix_2,
        instance.model_matrix_3,
    );

    let normal_matrix = mat3x3<f32>(
        instance.normal_matrix_0,
        instance.normal_matrix_1,
        instance.normal_matrix_2,
    );

    let world_normal = normalize(normal_matrix * model.normal);
    let world_tangent = normalize(normal_matrix * model.tangent);
    let world_bitangent = normalize(normal_matrix * model.bitangent);
    let tangent_matrix = transpose(mat3x3<f32>(
        world_tangent,
        world_bitangent,
        world_normal,
    ));

    let world_position = model_matrix * vec4<f32>(model.position, 1.0);

    var out: VertexOutput;
    out.clip_position = camera.view_proj * world_position;
    out.tex_coords = model.tex_coords;
    out.tangent_position = tangent_matrix * world_position.xyz;
    out.tangent_view_position = tangent_matrix * camera.view_pos.xyz;
    out.tbn_col0 = tangent_matrix[0];
    out.tbn_col1 = tangent_matrix[1];
    out.tbn_col2 = tangent_matrix[2];
    out.light_clip_pos = shadow_uni.light_vp * world_position;
    out.world_normal = world_normal;
    return out;
}

@group(0) @binding(0) var t_diffuse: texture_2d<f32>;
@group(0) @binding(1) var s_diffuse: sampler;
@group(0) @binding(2) var t_normal: texture_2d<f32>;
@group(0) @binding(3) var s_normal: sampler;
@group(0) @binding(4) var t_orm: texture_2d<f32>;
@group(0) @binding(5) var s_orm: sampler;
@group(0) @binding(6) var t_emissive: texture_2d<f32>;
@group(0) @binding(7) var s_emissive: sampler;

struct MaterialUniform {
    roughness_factor: f32,
    metallic_factor: f32,
    ao_strength: f32,
    alpha_cutoff: f32,
    emissive: vec3<f32>,
    alpha_mode: u32,
}
@group(0) @binding(8) var<uniform> material: MaterialUniform;

struct LightEntry {
    position: vec3<f32>,
    color: vec3<f32>,
    intensity: f32,
}
struct LightsUniform {
    lights: array<LightEntry, 3>,
}
@group(2) @binding(0)
var<uniform> lights: LightsUniform;

const PI: f32 = 3.14159265358979;

fn D_GGX(NdotH: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let d = NdotH * NdotH * (a2 - 1.0) + 1.0;
    return a2 / (PI * d * d);
}

fn G_schlick(NdotV: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    return NdotV / (NdotV * (1.0 - k) + k);
}

fn G_smith(NdotV: f32, NdotL: f32, roughness: f32) -> f32 {
    return G_schlick(NdotV, roughness) * G_schlick(NdotL, roughness);
}

fn F_schlick(cosTheta: f32, F0: vec3<f32>) -> vec3<f32> {
    return F0 + (1.0 - F0) * pow(clamp(1.0 - cosTheta, 0.0, 1.0), 5.0);
}

fn cook_torrance(N: vec3<f32>, V: vec3<f32>, L: vec3<f32>, albedo: vec3<f32>, roughness: f32, metallic: f32) -> vec3<f32> {
    let H = normalize(V + L);
    let NdotV = max(dot(N, V), 0.001);
    let NdotL = max(dot(N, L), 0.001);
    let NdotH = max(dot(N, H), 0.0);
    let HdotV = max(dot(H, V), 0.0);

    let F0 = mix(vec3(0.04), albedo, metallic);
    let F = F_schlick(HdotV, F0);
    let D = D_GGX(NdotH, roughness);
    let G = G_smith(NdotV, NdotL, roughness);

    let specular = (D * G * F) / (4.0 * NdotV * NdotL);
    let kD = (1.0 - F) * (1.0 - metallic);
    let diffuse = kD * albedo / PI;

    return (diffuse + specular) * NdotL;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo_sample = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let n_sample = textureSample(t_normal, s_normal, in.tex_coords);
    let orm_sample = textureSample(t_orm, s_orm, in.tex_coords);
    let emissive_sample = textureSample(t_emissive, s_emissive, in.tex_coords);

    let albedo = albedo_sample.xyz;

    let ao = mix(1.0, orm_sample.r, material.ao_strength);
    let roughness = material.roughness_factor * orm_sample.g;
    let metallic = material.metallic_factor * orm_sample.b;

    let tbn = mat3x3<f32>(in.tbn_col0, in.tbn_col1, in.tbn_col2);
    let N = normalize(n_sample.xyz * 2.0 - 1.0);
    let V = normalize(in.tangent_view_position - in.tangent_position);

    let N_world = normalize(in.world_normal);
    let sky = vec3(0.45, 0.48, 0.55);
    let ground = vec3(0.25, 0.22, 0.18);
    let ambient = mix(ground, sky, N_world.y * 0.5 + 0.5) * albedo * ao;

    let proj = in.light_clip_pos.xyz / in.light_clip_pos.w;
    let uv = proj.xy * vec2(0.5, -0.5) + 0.5;
    let in_map = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let shadow = select(1.0, textureSampleCompare(shadow_map, shadow_sampler, uv, proj.z - 0.002), in_map);

    var radiance_acc = vec3(0.0);

    {
        let L = normalize(tbn * lights.lights[0].position - in.tangent_position);
        let scale = lights.lights[0].intensity * 3.0 * shadow;
        radiance_acc += lights.lights[0].color * cook_torrance(N, V, L, albedo, roughness, metallic) * scale;
    }

    for (var i = 1u; i < 3u; i++) {
        let L = normalize(tbn * lights.lights[i].position - in.tangent_position);
        let scale = lights.lights[i].intensity * 3.0;
        radiance_acc += lights.lights[i].color * cook_torrance(N, V, L, albedo, roughness, metallic) * scale;
    }

    let emissive = material.emissive * emissive_sample.rgb;
    let color = ambient + radiance_acc + emissive;
    return vec4(color, albedo_sample.a);
}
