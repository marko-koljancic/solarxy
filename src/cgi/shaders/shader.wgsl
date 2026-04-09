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
    @location(8) world_position: vec3<f32>,
};

struct ShadowUniform {
    light_vp: mat4x4<f32>}
@group(3) @binding(0) var<uniform> shadow_uni: ShadowUniform;
@group(3) @binding(1) var shadow_map: texture_depth_2d;
@group(3) @binding(2) var shadow_sampler: sampler_comparison;

@group(2) @binding(1) var t_ibl: texture_cube<f32>;
@group(2) @binding(2) var s_ibl: sampler;
@group(2) @binding(3) var t_prefiltered: texture_cube<f32>;
@group(2) @binding(4) var s_prefiltered: sampler;
@group(2) @binding(5) var t_brdf_lut: texture_2d<f32>;
@group(2) @binding(6) var s_brdf_lut: sampler;

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
    var world_tangent = normal_matrix * model.tangent;
    var world_bitangent = normal_matrix * model.bitangent;
    if length(world_tangent) < 1e-6 {
        let up = select(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), abs(world_normal.y) < 0.999);
        world_tangent = normalize(cross(up, world_normal));
        world_bitangent = cross(world_normal, world_tangent);
    } else {
        world_tangent = normalize(world_tangent);
        world_bitangent = normalize(world_bitangent);
    }
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
    out.world_position = world_position.xyz;
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
    material_index: u32,
}
@group(0) @binding(8) var<uniform> material: MaterialUniform;

struct LightEntry {
    position: vec3<f32>,
    color: vec3<f32>,
    intensity: f32,
}
struct LightsUniform {
    lights: array<LightEntry, 3>,
    sphere_scale: f32,
    ibl_avg_r: f32,
    ibl_avg_g: f32,
    ibl_avg_b: f32,
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

fn lambert_direct(N: vec3<f32>, L: vec3<f32>, albedo: vec3<f32>) -> vec3<f32> {
    let NdotL = max(dot(N, L), 0.0);
    return (albedo / PI) * NdotL;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo_sample = textureSample(t_diffuse, s_diffuse, in.tex_coords);

    if material.alpha_mode == 1u && albedo_sample.a < material.alpha_cutoff {
        discard;
    }

    if camera.inspection_mode == 1u {
        let id = f32(material.material_index) + 1.0;
        let r = fract(sin(id * 43758.5453) * 1.0);
        let g = fract(sin(id * 22578.1459) * 1.0);
        let b = fract(sin(id * 19642.3721) * 1.0);
        return vec4(r, g, b, 1.0);
    }

    if camera.inspection_mode == 2u {
        let ddx = dpdx(in.tex_coords);
        let ddy = dpdy(in.tex_coords);
        let density = length(ddx) * length(ddy);

        if density == 0.0 {
            return vec4(0.5, 0.5, 0.5, 1.0);
        }

        let td_target = max(camera.texel_density_target, 0.001);
        let t = clamp(log2(density / td_target) / 2.0, -1.0, 1.0);

        var color: vec3<f32>;
        if t < 0.0 {
            color = mix(vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0), -t);
        } else {
            color = mix(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), t);
        }
        return vec4(color, 1.0);
    }

    if camera.inspection_mode == 3u {
        let z = in.clip_position.z;
        var linear_z: f32;
        if camera.proj[3][3] == 0.0 {
            linear_z = camera.near * camera.far
                / (camera.far - z * (camera.far - camera.near));
        } else {
            linear_z = camera.near + z * (camera.far - camera.near);
        }
        let normalized = 1.0
            - saturate((linear_z - camera.depth_near) / (camera.depth_far - camera.depth_near));
        return vec4(vec3(normalized), 1.0);
    }

    if camera.material_override == 4u {
        return vec4(0.0, 0.0, 0.0, 1.0);
    }

    var albedo: vec3<f32>;
    var roughness: f32;
    var metallic: f32;
    var ao: f32;
    var emissive_color: vec3<f32>;
    var N: vec3<f32>;

    let tbn = mat3x3<f32>(in.tbn_col0, in.tbn_col1, in.tbn_col2);

    if camera.material_override == 0u {
        let n_sample = textureSample(t_normal, s_normal, in.tex_coords);
        let orm_sample = textureSample(t_orm, s_orm, in.tex_coords);
        let emissive_sample = textureSample(t_emissive, s_emissive, in.tex_coords);

        albedo = albedo_sample.xyz;
        ao = mix(1.0, orm_sample.r, material.ao_strength);
        roughness = clamp(
            material.roughness_factor * orm_sample.g * camera.roughness_scale,
            0.04,
            1.0,
        );
        metallic = clamp(
            material.metallic_factor * orm_sample.b * camera.metallic_scale,
            0.0,
            1.0,
        );
        N = normalize(n_sample.xyz * 2.0 - 1.0);
        emissive_color = material.emissive * emissive_sample.rgb;
    } else {
        switch camera.material_override {
            case 1u: { albedo = vec3(0.8); roughness = 0.7; metallic = 0.0; }
            case 2u: { albedo = vec3(0.025); roughness = 1.0; metallic = 0.0; }
            case 3u: { albedo = vec3(0.05); roughness = 0.03; metallic = 1.0; }
            default: { albedo = vec3(0.8); roughness = 0.7; metallic = 0.0; }
        }
        ao = 1.0;
        emissive_color = vec3(0.0);
        N = vec3(0.0, 0.0, 1.0);
    }

    let V = normalize(in.tangent_view_position - in.tangent_position);

    let N_world = normalize(in.world_normal);
    let V_world = normalize(camera.view_pos.xyz - in.world_position);
    let F0 = mix(vec3(0.04), albedo, metallic);
    let NdotV_ibl = max(dot(N_world, V_world), 0.001);
    let F_ibl = F_schlick(NdotV_ibl, F0);
    let kD_ibl = (1.0 - F_ibl) * (1.0 - metallic);
    let irradiance = textureSampleLevel(t_ibl, s_ibl, N_world, 0.0).rgb;
    let diffuse_ibl_pbr = irradiance * albedo * kD_ibl;

    let R = reflect(-V_world, N_world);
    let MAX_REFLECTION_LOD = 5.0;
    let mip_level = roughness * MAX_REFLECTION_LOD;
    let prefiltered_color = textureSampleLevel(t_prefiltered, s_prefiltered, R, mip_level).rgb;
    let brdf_uv = vec2(max(dot(N_world, V_world), 0.0), roughness);
    let brdf = textureSample(t_brdf_lut, s_brdf_lut, brdf_uv).rg;
    let specular_ibl_pbr = prefiltered_color * (F0 * brdf.x + brdf.y);

    let is_clay = camera.material_override == 1u || camera.material_override == 2u;
    let ibl_ambient = vec3<f32>(lights.ibl_avg_r, lights.ibl_avg_g, lights.ibl_avg_b);
    let diffuse_ibl = select(diffuse_ibl_pbr, ibl_ambient * albedo, is_clay);
    let specular_ibl = select(specular_ibl_pbr, vec3<f32>(0.0), is_clay);

    let ambient = (diffuse_ibl + specular_ibl) * ao;

    let proj = in.light_clip_pos.xyz / in.light_clip_pos.w;
    let uv = proj.xy * vec2(0.5, -0.5) + 0.5;
    let in_map = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let shadow = select(1.0, textureSampleCompare(shadow_map, shadow_sampler, uv, proj.z - 0.002), in_map);

    var radiance_acc = vec3(0.0);

    if camera.material_override != 3u {
        {
            let L = normalize(tbn * lights.lights[0].position - in.tangent_position);
            let scale = lights.lights[0].intensity * 3.0 * shadow;
            let brdf = select(
                cook_torrance(N, V, L, albedo, roughness, metallic),
                lambert_direct(N, L, albedo),
                is_clay,
            );
            radiance_acc += lights.lights[0].color * brdf * scale;
        }

        for (var i = 1u; i < 3u; i++) {
            let L = normalize(tbn * lights.lights[i].position - in.tangent_position);
            let scale = lights.lights[i].intensity * 3.0;
            let brdf = select(
                cook_torrance(N, V, L, albedo, roughness, metallic),
                lambert_direct(N, L, albedo),
                is_clay,
            );
            radiance_acc += lights.lights[i].color * brdf * scale;
        }
    }

    let color = ambient + radiance_acc + emissive_color;
    let alpha = select(albedo_sample.a, 1.0, camera.material_override != 0u);
    return vec4(color, alpha);
}
