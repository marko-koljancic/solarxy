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
    // Per-vertex linear color; white from vs_main, the mesh's color
    // attribute from vs_main_colored. Multiplied into the base color
    // (glTF-consistent, decision M-8).
    @location(9) vcolor: vec4<f32>,
};

struct ShadowUniform {
    light_vp: mat4x4<f32>,
}
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
    return shade_vertex(model, instance, vec4(1.0));
}

// The colored-mesh variant: identical, plus the color vertex buffer at
// location 12 (only bound by the *_colored pipelines).
@vertex
fn vs_main_colored(
    model: VertexInput,
    instance: InstanceInput,
    @location(12) color: vec4<f32>,
) -> VertexOutput {
    return shade_vertex(model, instance, color);
}

fn shade_vertex(
    model: VertexInput,
    instance: InstanceInput,
    vcolor: vec4<f32>,
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
    out.vcolor = vcolor;
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
    // Per-material shading model (0 Pbr, 1 Matcap, 2 Toon, 3 Unlit,
    // 4 Clay, 5 ClayDark, 6 Chrome, 7 Silhouette) and the toon band count,
    // at offsets 32/36 (the former pad slots).
    shading_model: u32,
    toon_steps: f32,
    // Offset 48 (vec4 alignment); factor x map, glTF style.
    base_color: vec4<f32>,
}
@group(0) @binding(8) var<uniform> material: MaterialUniform;

// Matches the Rust `LightEntry` / `LightsUniform` layout (light.rs; size
// asserts there). kind: 0 = point, 1 = directional, 2 = spot. range = 0
// and decay = 0 disable their attenuation terms, which is how the
// synthesized viewer rig keeps pre-generalization output exactly.
struct LightEntry {
    position: vec3<f32>,
    kind: u32,
    direction: vec3<f32>,
    intensity: f32,
    color: vec3<f32>,
    range: f32,
    decay: f32,
    cos_inner: f32,
    cos_outer: f32,
    shadowed: f32,
}
struct LightsUniform {
    lights: array<LightEntry, 8>,
    count: u32,
    sphere_scale: f32,
    ibl_avg_r: f32,
    ibl_avg_g: f32,
    ibl_avg_b: f32,
    hemi_sky_r: f32,
    hemi_sky_g: f32,
    hemi_sky_b: f32,
    hemi_ground_r: f32,
    hemi_ground_g: f32,
    hemi_ground_b: f32,
    _pad_tail: f32,
}
@group(2) @binding(0)
var<uniform> lights: LightsUniform;

const PI: f32 = 3.14159265358979;
const SHADOW_BIAS: f32 = -0.002;

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

// Yaw a direction around +Y — rotates IBL cubemap lookups in lockstep
// with the visible HDRI sky (skybox.wgsl). A no-op for gradient/fallback
// IBL, whose lookups depend only on the rotation-invariant Y axis.
fn rotate_yaw(d: vec3<f32>, yaw: f32) -> vec3<f32> {
    let c = cos(yaw);
    let s = sin(yaw);
    return vec3<f32>(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let albedo_sample = textureSample(t_diffuse, s_diffuse, in.tex_coords);
    let base_alpha = albedo_sample.a * material.base_color.a * in.vcolor.a;

    if material.alpha_mode == 1u && base_alpha < material.alpha_cutoff {
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

        // Single unconditional return: WebGPU's uniformity analysis
        // (Tint/Dawn) rejects the later implicit-derivative texture samples
        // if a return in this branch is conditional on the non-uniform
        // density. Native Naga accepted the early return; the web does not.
        var color = vec3(0.5, 0.5, 0.5);
        if density != 0.0 {
            let td_target = max(camera.texel_density_target, 0.001);
            let t = clamp(log2(density / td_target) / 2.0, -1.0, 1.0);
            if t < 0.0 {
                color = mix(vec3(0.0, 1.0, 0.0), vec3(0.0, 0.0, 1.0), -t);
            } else {
                color = mix(vec3(0.0, 1.0, 0.0), vec3(1.0, 0.0, 0.0), t);
            }
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

    // Per-material shading model. A global viewport override
    // (camera.material_override != 0u) wins over the per-material model,
    // so inspection and override workflows see every object uniformly.
    let model_id = select(0u, material.shading_model, camera.material_override == 0u);

    if model_id == 7u {
        // Silhouette: solid black, no lighting.
        return vec4(0.0, 0.0, 0.0, 1.0);
    }
    if model_id == 3u {
        // Unlit: flat factor x map (glTF KHR_materials_unlit).
        return vec4(material.base_color.rgb * albedo_sample.rgb * in.vcolor.rgb, base_alpha);
    }
    if model_id == 1u {
        // Matcap: the base-color texture IS the matcap, sampled by the
        // view-space normal (explicit level: sampling stays valid under
        // WebGPU's uniformity analysis, and matcaps need no mips).
        let n_view =
            normalize((camera.view * vec4(normalize(in.world_normal), 0.0)).xyz);
        let uv_m = n_view.xy * vec2(0.5, -0.5) + 0.5;
        let matcap = textureSampleLevel(t_diffuse, s_diffuse, uv_m, 0.0).rgb;
        return vec4(material.base_color.rgb * matcap, 1.0);
    }

    var albedo: vec3<f32>;
    var roughness: f32;
    var metallic: f32;
    var ao: f32;
    var emissive_color: vec3<f32>;
    var N: vec3<f32>;

    let tbn = mat3x3<f32>(in.tbn_col0, in.tbn_col1, in.tbn_col2);

    if camera.material_override == 0u && model_id >= 4u && model_id <= 6u {
        // The promoted per-material Clay / ClayDark / Chrome looks: the
        // same constants as the matching global overrides.
        switch model_id {
            case 4u: { albedo = vec3(0.8); roughness = 0.7; metallic = 0.0; }
            case 5u: { albedo = vec3(0.025); roughness = 1.0; metallic = 0.0; }
            default: { albedo = vec3(0.05); roughness = 0.03; metallic = 1.0; }
        }
        ao = 1.0;
        emissive_color = vec3(0.0);
        N = vec3(0.0, 0.0, 1.0);
    } else if camera.material_override == 0u {
        let n_sample = textureSample(t_normal, s_normal, in.tex_coords);
        let orm_sample = textureSample(t_orm, s_orm, in.tex_coords);
        let emissive_sample = textureSample(t_emissive, s_emissive, in.tex_coords);

        albedo = material.base_color.rgb * albedo_sample.xyz * in.vcolor.rgb;
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
    let ibl_n = rotate_yaw(N_world, camera.hdri_rotation);
    let irradiance = textureSampleLevel(t_ibl, s_ibl, ibl_n, 0.0).rgb;
    let diffuse_ibl_pbr = irradiance * albedo * kD_ibl;

    let R = reflect(-V_world, N_world);
    let MAX_REFLECTION_LOD = 5.0;
    let mip_level = roughness * MAX_REFLECTION_LOD;
    let ibl_r = rotate_yaw(R, camera.hdri_rotation);
    let prefiltered_color = textureSampleLevel(t_prefiltered, s_prefiltered, ibl_r, mip_level).rgb;
    let brdf_uv = vec2(max(dot(N_world, V_world), 0.0), roughness);
    // Explicit level: the BRDF LUT is single-mip, and level sampling stays
    // valid in non-uniform control flow under WebGPU's uniformity analysis.
    let brdf = textureSampleLevel(t_brdf_lut, s_brdf_lut, brdf_uv, 0.0).rg;
    let specular_ibl_pbr = prefiltered_color * (F0 * brdf.x + brdf.y);

    let is_clay = camera.material_override == 1u || camera.material_override == 2u
        || model_id == 4u || model_id == 5u;
    let ibl_ambient = vec3<f32>(lights.ibl_avg_r, lights.ibl_avg_g, lights.ibl_avg_b);
    let diffuse_ibl = select(diffuse_ibl_pbr, ibl_ambient * albedo, is_clay);
    let specular_ibl = select(specular_ibl_pbr, vec3<f32>(0.0), is_clay);

    // Hemisphere/ambient light-node term: blends ground-to-sky by the
    // world-space up component of the normal. All-zero (exactly no
    // contribution) when no ambient or hemisphere lights exist.
    let hemi_sky = vec3<f32>(lights.hemi_sky_r, lights.hemi_sky_g, lights.hemi_sky_b);
    let hemi_ground =
        vec3<f32>(lights.hemi_ground_r, lights.hemi_ground_g, lights.hemi_ground_b);
    let hemi_up = clamp(N_world.y * 0.5 + 0.5, 0.0, 1.0);
    let hemi = mix(hemi_ground, hemi_sky, hemi_up) * albedo / PI;

    let ambient = (diffuse_ibl + specular_ibl + hemi) * ao;

    let proj = in.light_clip_pos.xyz / in.light_clip_pos.w;
    let uv = proj.xy * vec2(0.5, -0.5) + 0.5;
    let in_map = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let shadow = select(1.0, textureSampleCompare(shadow_map, shadow_sampler, uv, proj.z + SHADOW_BIAS), in_map);

    var radiance_acc = vec3(0.0);
    let is_toon = model_id == 2u;

    // Chrome (global 3u or per-material 6u) is env-reflection-only: it
    // skips the direct-light loop entirely.
    if camera.material_override != 3u && model_id != 6u {
        // All lighting runs in tangent space; the TBN is orthonormal, so
        // distances and angles match their world-space values.
        for (var i = 0u; i < lights.count; i++) {
            let light = lights.lights[i];

            // Explicit vec3 copies: naga's Metal backend emits packed_float3
            // for struct members, which cannot multiply a float3x3 directly.
            let light_dir = vec3<f32>(light.direction);
            let light_pos = vec3<f32>(light.position);

            var L: vec3<f32>;
            var atten = 1.0;
            if light.kind == 1u {
                // Directional: L opposes the light's travel direction.
                L = normalize(tbn * (-light_dir));
            } else {
                let light_pos_t = tbn * light_pos;
                let to_light = light_pos_t - in.tangent_position;
                let dist = length(to_light);
                // normalize() (not to_light / dist): bit-parity with the
                // pre-generalization shader for the golden comparison.
                L = normalize(to_light);

                // range = 0 and decay = 0 both multiply by exactly 1.0 —
                // the synthesized viewer rig's parity path.
                if light.range > 0.0 {
                    let w = clamp(1.0 - pow(dist / light.range, 4.0), 0.0, 1.0);
                    atten *= w * w;
                }
                if light.decay > 0.0 {
                    atten *= 1.0 / pow(max(dist, 0.01), light.decay);
                }

                if light.kind == 2u {
                    // Spot cone: smooth falloff between the cone cosines.
                    let dir_t = normalize(tbn * light_dir);
                    let cos_angle = dot(-L, dir_t);
                    let cone = clamp(
                        (cos_angle - light.cos_outer)
                            / max(light.cos_inner - light.cos_outer, 1e-4),
                        0.0,
                        1.0,
                    );
                    atten *= cone;
                }
            }

            let shadow_factor = select(1.0, shadow, light.shadowed > 0.5);
            let scale = light.intensity * 3.0 * atten * shadow_factor;
            var brdf = select(
                cook_torrance(N, V, L, albedo, roughness, metallic),
                lambert_direct(N, L, albedo),
                is_clay,
            );
            if is_toon {
                // Cel shading: quantize the diffuse term into
                // material.toon_steps bands (a stepped lambert).
                let ndotl = max(dot(N, L), 0.0);
                let banded = floor(ndotl * material.toon_steps)
                    / max(material.toon_steps - 1.0, 1.0);
                brdf = albedo / PI * clamp(banded, 0.0, 1.0);
            }
            radiance_acc += light.color * brdf * scale;
        }
    }

    let color = ambient + radiance_acc + emissive_color;
    let alpha = select(base_alpha, 1.0, camera.material_override != 0u);
    return vec4(color, alpha);
}
