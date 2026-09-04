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
    // (glTF-consistent).
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
// The rect-area light tables (see `ltc.rs`). Indexed by perceptual
// roughness and sqrt(1 - dot(N, V)).
@group(2) @binding(7) var t_ltc_transform: texture_2d<f32>;
@group(2) @binding(8) var t_ltc_magnitude: texture_2d<f32>;
@group(2) @binding(9) var s_ltc: sampler;

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
    // Offsets 64 to 159: the principled surface properties, six
    // vec4-shaped blocks so every vec3 lands on a 16-byte boundary.
    // Mirrors `material::MaterialUniform`; the sizes are cross-checked in
    // `tests/uniform_layout.rs`.
    ior: f32,
    transmission: f32,
    thickness: f32,
    attenuation_distance: f32,
    attenuation_color: vec3<f32>,
    emissive_strength: f32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    anisotropy: f32,
    anisotropy_rotation: f32,
    sheen_color: vec3<f32>,
    sheen_roughness: f32,
    specular_color: vec3<f32>,
    specular_intensity: f32,
    iridescence: f32,
    iridescence_ior: f32,
    iridescence_thickness_min: f32,
    iridescence_thickness_max: f32,
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
    // Rect-area only; zero for every other kind. Mirrors
    // `light::LightEntry`, which is size-asserted at 96 bytes: the array
    // STRIDE depends on these, so they are not a prefix-safe addition.
    half_x: vec3<f32>,
    two_sided: f32,
    half_y: vec3<f32>,
    _pad_entry: f32,
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
    ibl_intensity: f32,
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

/// The base surface's response to one direct light.
///
/// `f0` arrives computed rather than derived here, because the index of
/// refraction, the specular tint and any thin film all modify it and all
/// three are shared with the image-based and area-light paths. At the
/// material defaults `f0` is exactly `mix(vec3(0.04), albedo, metallic)`,
/// which is what this function used to compute for itself.
///
/// `tangent` and `bitangent` drive the anisotropic form. They are only
/// consulted when anisotropy is non-zero: the anisotropic distribution
/// reduces to the isotropic one mathematically at zero, but not to the
/// same floating-point result, and preserving the old arithmetic exactly
/// is what keeps the golden gate a signal rather than noise.
fn cook_torrance(
    N: vec3<f32>, V: vec3<f32>, L: vec3<f32>,
    albedo: vec3<f32>, roughness: f32, metallic: f32,
    f0: vec3<f32>, tangent: vec3<f32>, bitangent: vec3<f32>,
) -> vec3<f32> {
    let H = normalize(V + L);
    let NdotV = max(dot(N, V), 0.001);
    let NdotL = max(dot(N, L), 0.001);
    let NdotH = max(dot(N, H), 0.0);
    let HdotV = max(dot(H, V), 0.0);

    let F = F_schlick(HdotV, f0);

    var specular: vec3<f32>;
    if material.anisotropy == 0.0 {
        let D = D_GGX(NdotH, roughness);
        let G = G_smith(NdotV, NdotL, roughness);
        specular = (D * G * F) / (4.0 * NdotV * NdotL);
    } else {
        // Split the roughness along the tangent frame. The clamp keeps the
        // sharp axis from collapsing to a zero-width lobe at full
        // anisotropy, which would divide by zero in the distribution.
        let alpha = roughness * roughness;
        let at = max(alpha * (1.0 + material.anisotropy), 1e-4);
        let ab = max(alpha * (1.0 - material.anisotropy), 1e-4);
        let D = D_GGX_aniso(NdotH, dot(tangent, H), dot(bitangent, H), at, ab);
        let Vis = V_GGX_aniso(
            NdotL, NdotV,
            dot(tangent, V), dot(bitangent, V),
            dot(tangent, L), dot(bitangent, L),
            at, ab,
        );
        specular = D * Vis * F;
    }

    let kD = (1.0 - F) * (1.0 - metallic);
    let diffuse = kD * albedo / PI;

    return (diffuse + specular) * NdotL;
}

fn lambert_direct(N: vec3<f32>, L: vec3<f32>, albedo: vec3<f32>) -> vec3<f32> {
    let NdotL = max(dot(N, L), 0.0);
    return (albedo / PI) * NdotL;
}

// --- Principled surface lobes ---
//
// The parameters these read are the industry principled set; the
// evaluations below are the ones the KHR extension specifications define
// against exactly those parameters, so nothing is remapped between what a
// file carries and what the viewport shows. They are real-time
// approximations: clearcoat, sheen and iridescence in a forward raster
// shader cannot do the multiple scattering an offline integrator does, and
// the parameter help says so. Reference-grade evaluation of this same data
// is the path tracer's job.
//
// Every lobe is guarded on its own factor in `fs_main`, and every default
// is the identity of its effect, so a material that sets none of them
// takes precisely the arithmetic it took before any of this existed. That
// is deliberate: it keeps the golden gate meaningful, because a diff then
// means a real change rather than a reformulation.

fn sq(x: f32) -> f32 {
    return x * x;
}

/// Normal-incidence reflectance for the base surface.
///
/// The dielectric part comes from the index of refraction and is then
/// tinted and scaled by the specular parameters; metals take the albedo as
/// before. At the defaults (ior 1.5, specular 1.0, white tint) the branch
/// keeps the literal 0.04, rather than recomputing it from the formula and
/// landing on 0.040000001 instead.
fn base_f0(albedo: vec3<f32>, metallic: f32) -> vec3<f32> {
    var dielectric = vec3(0.04);
    if material.ior != 1.5 {
        dielectric = vec3(sq((material.ior - 1.0) / (material.ior + 1.0)));
    }
    dielectric = min(
        dielectric * material.specular_color * material.specular_intensity,
        vec3(1.0),
    );
    return mix(dielectric, albedo, metallic);
}

// Anisotropic GGX. At anisotropy 0 the two roughnesses are equal and this
// reduces to the isotropic form mathematically, but NOT bit-exactly, which
// is why `fs_main` keeps the isotropic path rather than routing everything
// through here.
fn D_GGX_aniso(NdotH: f32, TdotH: f32, BdotH: f32, at: f32, ab: f32) -> f32 {
    let a2 = at * ab;
    let f = vec3(ab * TdotH, at * BdotH, a2 * NdotH);
    let w2 = a2 / dot(f, f);
    return a2 * w2 * w2 / PI;
}

fn V_GGX_aniso(
    NdotL: f32, NdotV: f32,
    TdotV: f32, BdotV: f32, TdotL: f32, BdotL: f32,
    at: f32, ab: f32,
) -> f32 {
    let lv = NdotL * length(vec3(at * TdotV, ab * BdotV, NdotV));
    let ll = NdotV * length(vec3(at * TdotL, ab * BdotL, NdotL));
    return 0.5 / max(lv + ll, 1e-6);
}

// Charlie distribution and its matching visibility term: the sheen lobe
// that gives fabric its retroreflective rim. The rational fits are the
// ones the sheen specification gives.
fn D_Charlie(roughness: f32, NdotH: f32) -> f32 {
    let alpha = max(roughness * roughness, 1e-6);
    let inv = 1.0 / alpha;
    let sin2h = max(1.0 - NdotH * NdotH, 1e-7);
    return (2.0 + inv) * pow(sin2h, inv * 0.5) / (2.0 * PI);
}

fn lambda_sheen_helper(x: f32, alpha: f32) -> f32 {
    let one_minus_sq = sq(1.0 - alpha);
    let a = mix(21.5473, 25.3245, one_minus_sq);
    let b = mix(3.82987, 3.32435, one_minus_sq);
    let c = mix(0.19823, 0.16801, one_minus_sq);
    let d = mix(-1.97760, -1.27393, one_minus_sq);
    let e = mix(-4.32054, -4.85967, one_minus_sq);
    return a / (1.0 + b * pow(max(x, 1e-6), c)) + d * x + e;
}

fn lambda_sheen(cos_theta: f32, alpha: f32) -> f32 {
    if abs(cos_theta) < 0.5 {
        return exp(lambda_sheen_helper(cos_theta, alpha));
    }
    return exp(
        2.0 * lambda_sheen_helper(0.5, alpha)
            - lambda_sheen_helper(max(1.0 - cos_theta, 1e-6), alpha),
    );
}

fn V_Sheen(NdotL: f32, NdotV: f32, roughness: f32) -> f32 {
    let alpha = max(roughness * roughness, 1e-6);
    let denom = (1.0 + lambda_sheen(NdotV, alpha) + lambda_sheen(NdotL, alpha))
        * (4.0 * NdotV * NdotL);
    return clamp(1.0 / max(denom, 1e-6), 0.0, 1.0);
}

/// How much the base layer survives under the sheen lobe.
///
/// The specification scales the base by the sheen lobe's directional
/// albedo, which its reference implementation reads from a lookup table.
/// A table is a scene-level texture binding, and this pass is at 10 of the
/// 16 sampled textures core WebGPU guarantees with the remainder already
/// spoken for, so the scaling uses a compact analytic stand-in instead: it
/// is monotonic in roughness, strongest at grazing angles, and reaches
/// unity as the sheen colour goes to black. It is a stand-in, named as one
/// here and in the parameter help, and the honest place to replace it is
/// alongside the path tracer.
fn sheen_albedo_scaling(NdotV: f32, NdotL: f32) -> f32 {
    let strongest = max(
        material.sheen_color.r,
        max(material.sheen_color.g, material.sheen_color.b),
    );
    let alpha = max(material.sheen_roughness * material.sheen_roughness, 1e-6);
    let at_v = alpha * (1.0 - NdotV * NdotV);
    let at_l = alpha * (1.0 - NdotL * NdotL);
    return clamp((1.0 - strongest * at_v) * (1.0 - strongest * at_l), 0.0, 1.0);
}

// --- Thin-film interference (iridescence) ---
//
// Belcour and Barla's airy-summation model, the one the iridescence
// specification carries in its appendix. The spectral response is
// integrated against three Gaussian-fitted sensitivity curves in XYZ and
// converted to linear sRGB, which is why the constants below look like
// nothing else in this file: they are the fit, not tunable values.

const XYZ_TO_REC709: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(3.2404542, -0.9692660, 0.0556434),
    vec3<f32>(-1.5371385, 1.8760108, -0.2040259),
    vec3<f32>(-0.4985314, 0.0415560, 1.0572252),
);

fn ior_to_f0(transmitted: f32, incident: f32) -> f32 {
    return sq((transmitted - incident) / (transmitted + incident));
}

fn ior_to_f0_rgb(transmitted: vec3<f32>, incident: f32) -> vec3<f32> {
    let d = (transmitted - vec3(incident)) / (transmitted + vec3(incident));
    return d * d;
}

fn f0_to_ior(f0: vec3<f32>) -> vec3<f32> {
    let root = sqrt(f0);
    return (vec3(1.0) + root) / max(vec3(1.0) - root, vec3(1e-5));
}

fn eval_sensitivity(opd: f32, shift: vec3<f32>) -> vec3<f32> {
    let phase = 2.0 * PI * opd * 1.0e-9;
    let val = vec3(5.4856e-13, 4.4201e-13, 5.2481e-13);
    let pos = vec3(1.6810e+06, 1.7953e+06, 2.2084e+06);
    let var_ = vec3(4.3278e+09, 9.3046e+09, 6.6121e+09);

    var xyz = val * sqrt(2.0 * PI * var_) * cos(pos * phase + shift)
        * exp(-sq(phase) * var_);
    xyz.x += 9.7470e-14 * sqrt(2.0 * PI * 4.5282e+09)
        * cos(2.2399e+06 * phase + shift.x) * exp(-4.5282e+09 * sq(phase));
    xyz /= 1.0685e-7;
    return XYZ_TO_REC709 * xyz;
}

fn eval_iridescence(outer_ior: f32, film_ior: f32, cos_theta1: f32,
                    thickness: f32, base: vec3<f32>) -> vec3<f32> {
    // Fold the film back into the surrounding medium as thickness goes to
    // zero, so the effect fades in rather than switching on.
    let iri_ior = mix(outer_ior, film_ior, smoothstep(0.0, 0.03, thickness));
    let sin_theta2_sq = sq(outer_ior / iri_ior) * (1.0 - sq(cos_theta1));
    let cos_theta2_sq = 1.0 - sin_theta2_sq;
    if cos_theta2_sq < 0.0 {
        // Total internal reflection.
        return vec3(1.0);
    }
    let cos_theta2 = sqrt(cos_theta2_sq);

    // First interface, medium to film.
    let r12 = F_schlick(cos_theta1, vec3(ior_to_f0(iri_ior, outer_ior))).x;
    let t121 = 1.0 - r12;
    var phi12 = 0.0;
    if iri_ior < outer_ior {
        phi12 = PI;
    }
    let phi21 = PI - phi12;

    // Second interface, film to base.
    let base_ior = f0_to_ior(clamp(base, vec3(0.0), vec3(0.9999)));
    let r23 = F_schlick(cos_theta2, ior_to_f0_rgb(base_ior, iri_ior));
    var phi23 = vec3(0.0);
    if base_ior.x < iri_ior { phi23.x = PI; }
    if base_ior.y < iri_ior { phi23.y = PI; }
    if base_ior.z < iri_ior { phi23.z = PI; }

    let opd = 2.0 * iri_ior * thickness * cos_theta2;
    let phi = vec3(phi21) + phi23;

    let r123 = clamp(r12 * r23, vec3(1e-5), vec3(0.9999));
    let r123_root = sqrt(r123);
    let rs = sq(t121) * r23 / (vec3(1.0) - r123);

    // The DC term, then two pairs of diracs. Two is where the series has
    // visually converged for the thickness range this exposes.
    var result = r12 + rs;
    var cm = rs - t121;
    for (var m = 1; m <= 2; m++) {
        cm *= r123_root;
        result += cm * 2.0 * eval_sensitivity(f32(m) * opd, f32(m) * phi);
    }
    return max(result, vec3(0.0));
}

// --- Rect-area lights, via linearly transformed cosines ---
//
// Heitz, Dupuy, Hill and Neubelt (2016). A cosine integrated over a
// polygon has a closed form; a GGX lobe does not. So warp the lobe into a
// cosine with a matrix, warp the rectangle by the same matrix, and
// integrate. The matrix is what `ltc.rs` tabulates.

// The diffuse lobe IS a cosine, so its "transform" is the identity.
const IDENTITY3: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>(1.0, 0.0, 0.0),
    vec3<f32>(0.0, 1.0, 0.0),
    vec3<f32>(0.0, 0.0, 1.0),
);

const LTC_LUT_SIZE: f32 = 64.0;
const LTC_LUT_SCALE: f32 = (LTC_LUT_SIZE - 1.0) / LTC_LUT_SIZE;
const LTC_LUT_BIAS: f32 = 0.5 / LTC_LUT_SIZE;

fn ltc_uv(NdotV: f32, roughness: f32) -> vec2<f32> {
    // sqrt(1 - cos) rather than the angle: it spends texels where the lobe
    // changes fastest, which is near grazing.
    let uv = vec2<f32>(roughness, sqrt(1.0 - clamp(NdotV, 0.0, 1.0)));
    // Half a texel in from each end so linear filtering can still reach
    // the first and last rows.
    return uv * LTC_LUT_SCALE + LTC_LUT_BIAS;
}

// The integral of a cosine over a spherical polygon, given its vector form
// factor, with the below-horizon part removed. The rational fit is from
// Hill's "Real-Time Area Lighting: a Journey from Research to Production".
fn ltc_clipped_sphere_form_factor(f: vec3<f32>) -> f32 {
    let l = length(f);
    return max((l * l + f.z) / (l + 1.0), 0.0);
}

// One edge's contribution to the vector form factor. The polynomial
// approximates theta/sin(theta)/2PI, which is what makes this cheap enough
// to run per light per pixel.
fn ltc_edge_form_factor(v1: vec3<f32>, v2: vec3<f32>) -> vec3<f32> {
    let x = dot(v1, v2);
    let y = abs(x);
    let a = 0.8543985 + (0.4965155 + 0.0145206 * y) * y;
    let b = 3.4175940 + (4.1616724 + y) * y;
    let v = a / b;
    var theta_sintheta = 0.5 * inverseSqrt(max(1.0 - x * x, 1e-7)) - v;
    if x > 0.0 {
        theta_sintheta = v;
    }
    return cross(v1, v2) * theta_sintheta;
}

// How much of the rectangle `corners` reaches point `P`, through the lobe
// `m_inv` describes. Pass the identity for the diffuse term.
fn ltc_evaluate(
    N: vec3<f32>,
    V: vec3<f32>,
    P: vec3<f32>,
    m_inv: mat3x3<f32>,
    corners: array<vec3<f32>, 4>,
    two_sided: bool,
) -> f32 {
    var c = corners;

    // Behind the panel is unlit, unless it emits from both faces. The
    // winding of `corners` defines which side is the front.
    let light_normal = cross(c[1] - c[0], c[3] - c[0]);
    let facing = dot(light_normal, P - c[0]);
    if facing < 0.0 && !two_sided {
        return 0.0;
    }

    // A shading frame around N, with the view in its x-z plane, because
    // that is the frame the table was fitted in.
    //
    // t2 is NEGATED so the frame is right-handed. Without it the frame has
    // determinant -1, every cross product inside the form factor comes out
    // reversed, the accumulated vector points away from the surface, and
    // the horizon clip takes the whole thing to zero. The symptom is not a
    // subtly wrong highlight, it is an area light that emits nothing at
    // all.
    let t1 = normalize(V - N * dot(V, N));
    let t2 = -cross(N, t1);
    let to_shading = transpose(mat3x3<f32>(t1, t2, N));
    let m = m_inv * to_shading;

    // Project the rectangle onto the unit sphere around P, warped.
    var w: array<vec3<f32>, 4>;
    w[0] = normalize(m * (c[0] - P));
    w[1] = normalize(m * (c[1] - P));
    w[2] = normalize(m * (c[2] - P));
    w[3] = normalize(m * (c[3] - P));

    var form = vec3<f32>(0.0);
    form += ltc_edge_form_factor(w[0], w[1]);
    form += ltc_edge_form_factor(w[1], w[2]);
    form += ltc_edge_form_factor(w[2], w[3]);
    form += ltc_edge_form_factor(w[3], w[0]);

    return ltc_clipped_sphere_form_factor(form);
}

// The four corners, counter-clockwise seen from the emitting side.
fn ltc_corners(center: vec3<f32>, half_x: vec3<f32>, half_y: vec3<f32>) -> array<vec3<f32>, 4> {
    return array<vec3<f32>, 4>(
        center - half_x - half_y,
        center + half_x - half_y,
        center + half_x + half_y,
        center - half_x + half_y,
    );
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

    // The alpha this surface writes to the target. Only a blended material
    // carries its authored alpha out: OPAQUE ignores alpha by the glTF
    // contract, and a MASK fragment that survived the cutoff is opaque by
    // definition. The lane matters only to a transparent film back, where an
    // authored half-alpha on an opaque material would otherwise punch a hole
    // in the matte; when the composite is not carrying alpha, nothing reads
    // it.
    let surface_alpha = select(1.0, base_alpha, material.alpha_mode == 2u);

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
        return vec4(material.base_color.rgb * albedo_sample.rgb * in.vcolor.rgb, surface_alpha);
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

    // `tbn` is the WORLD-to-TANGENT matrix (the vertex stage builds it as
    // transpose(mat3x3(T, B, N))), so its transpose takes a tangent-space
    // normal-map sample back into world space. Since the 0.8.1 hoist that
    // is the only thing tangent space is still used for: shading itself
    // runs in world space.
    let tbn = mat3x3<f32>(in.tbn_col0, in.tbn_col1, in.tbn_col2);

    // Shading basis, needed by both the material branches below (which
    // override N) and the IBL block further down.
    let N_world = normalize(in.world_normal);
    let V_world = normalize(camera.view_pos.xyz - in.world_position);

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
        // These looks deliberately ignore normal maps, so the shading
        // normal is the geometric one (was tangent-space Z pre-hoist).
        N = N_world;
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
        // Tangent-space sample lifted into world space: transpose(tbn) is
        // mat3x3(T, B, N), the tangent-to-world direction.
        N = normalize(transpose(tbn) * (n_sample.xyz * 2.0 - 1.0));
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
        // These looks deliberately ignore normal maps, so the shading
        // normal is the geometric one (was tangent-space Z pre-hoist).
        N = N_world;
    }

    let NdotV_ibl = max(dot(N_world, V_world), 0.001);

    // The base reflectance every path below shares. Iridescence is a
    // modification of it rather than a lobe of its own: a thin film changes
    // what fraction reflects at each wavelength, so it belongs here where
    // the direct, image-based and area-light paths all pick it up.
    var F0 = base_f0(albedo, metallic);
    if material.iridescence > 0.0 {
        let film = eval_iridescence(
            1.0,
            material.iridescence_ior,
            NdotV_ibl,
            material.iridescence_thickness_max,
            F0,
        );
        F0 = mix(F0, film, material.iridescence);
    }

    // The frame the anisotropic lobe stretches along. `transpose(tbn)` is
    // mat3x3(T, B, N), so its first two columns are the world-space
    // tangent and bitangent; the material's rotation turns them within the
    // tangent plane. A mesh with no usable tangents yields a degenerate
    // frame, so fall back to the geometric normal's basis rather than
    // normalizing a zero vector into NaN.
    let t2w = transpose(tbn);
    var aniso_tangent = t2w[0];
    var aniso_bitangent = t2w[1];
    if dot(aniso_tangent, aniso_tangent) < 1e-8 {
        aniso_tangent = normalize(cross(N_world, vec3(0.0, 0.0, 1.0)) + vec3(1e-6, 0.0, 0.0));
        aniso_bitangent = cross(N_world, aniso_tangent);
    } else {
        aniso_tangent = normalize(aniso_tangent);
        aniso_bitangent = normalize(aniso_bitangent);
    }
    if material.anisotropy_rotation != 0.0 {
        let c = cos(material.anisotropy_rotation);
        let s = sin(material.anisotropy_rotation);
        let rotated_t = c * aniso_tangent + s * aniso_bitangent;
        aniso_bitangent = c * aniso_bitangent - s * aniso_tangent;
        aniso_tangent = rotated_t;
    }

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
    // The environment's intensity scales every image-based term, including
    // the Clay ambient (which IS the IBL, reduced to its L0 coefficient),
    // but never the hemisphere rows below: those come from ambient and
    // hemisphere light nodes, which carry their own intensity. Seeded to
    // 1.0 by every constructor, so this multiply is identity until the
    // environment sets it.
    let diffuse_ibl =
        select(diffuse_ibl_pbr, ibl_ambient * albedo, is_clay) * lights.ibl_intensity;
    let specular_ibl = select(specular_ibl_pbr, vec3<f32>(0.0), is_clay) * lights.ibl_intensity;

    // Hemisphere/ambient light-node term: blends ground-to-sky by the
    // world-space up component of the normal. All-zero (exactly no
    // contribution) when no ambient or hemisphere lights exist.
    let hemi_sky = vec3<f32>(lights.hemi_sky_r, lights.hemi_sky_g, lights.hemi_sky_b);
    let hemi_ground =
        vec3<f32>(lights.hemi_ground_r, lights.hemi_ground_g, lights.hemi_ground_b);
    let hemi_up = clamp(N_world.y * 0.5 + 0.5, 0.0, 1.0);
    let hemi = mix(hemi_ground, hemi_sky, hemi_up) * albedo / PI;

    var ambient = (diffuse_ibl + specular_ibl + hemi) * ao;

    // The principled layers over the image-based term. Each is guarded on
    // its own factor and skipped entirely for the stylized looks, which
    // define themselves by NOT being physically layered. Model 0 is the
    // PBR arm, so this one test excludes matcap, toon, unlit, both clays,
    // chrome and silhouette at once, and it is uniform across a draw
    // because it reads only the material uniform and the viewport override.
    let is_principled = camera.material_override == 0u && model_id == 0u;

    if is_principled && material.transmission > 0.0 {
        // Refract the environment. The specification's reference
        // implementation copies the framebuffer so glass shows the scene
        // behind it; this forward path has no such copy, and adding one is
        // a pass, not a shader edit. So glass reads correctly against an
        // environment and does not show objects behind it. Named in the
        // parameter help rather than left to be discovered, and removed
        // rather than approximated better by the path tracer.
        let eta = 1.0 / max(material.ior, 1.0);
        let refracted = refract(-V_world, N_world, eta);
        let refr_dir = rotate_yaw(refracted, camera.hdri_rotation);
        var through = textureSampleLevel(
            t_prefiltered, s_prefiltered, refr_dir, mip_level).rgb;

        // Beer-Lambert absorption over the volume's thickness.
        if material.attenuation_distance > 0.0 {
            let tint = clamp(material.attenuation_color, vec3(1e-4), vec3(1.0));
            let sigma = -log(tint) / material.attenuation_distance;
            through *= exp(-sigma * max(material.thickness, 0.0));
        }

        // Transmission replaces the diffuse lobe, not the specular one:
        // light either scatters back out or passes through.
        ambient = mix(ambient, through * lights.ibl_intensity + specular_ibl * ao,
                      material.transmission);
    }

    if is_principled && material.sheen_color.r + material.sheen_color.g
        + material.sheen_color.b > 0.0 {
        // Sheen's image-based term, taken from the diffuse irradiance
        // rather than a second prefiltered chain: the Charlie lobe is wide
        // and low-frequency, so the irradiance map is already close to its
        // convolution, and a second chain would cost a texture binding
        // this pass does not have.
        let scale = sheen_albedo_scaling(NdotV_ibl, NdotV_ibl);
        ambient = ambient * scale
            + irradiance * material.sheen_color * lights.ibl_intensity * ao;
    }

    if is_principled && material.clearcoat > 0.0 {
        // A second specular lobe at the coat's own roughness, over a fixed
        // index of refraction of 1.5. The coat both adds its own
        // reflection and attenuates everything under it by what it
        // reflects away.
        let coat_mip = material.clearcoat_roughness * MAX_REFLECTION_LOD;
        let coat_env = textureSampleLevel(
            t_prefiltered, s_prefiltered, ibl_r, coat_mip).rgb;
        let coat_brdf = textureSampleLevel(
            t_brdf_lut,
            s_brdf_lut,
            vec2(NdotV_ibl, material.clearcoat_roughness),
            0.0,
        ).rg;
        let coat_f = F_schlick(NdotV_ibl, vec3(0.04)).x * material.clearcoat;
        ambient = ambient * (1.0 - coat_f)
            + coat_env * (0.04 * coat_brdf.x + coat_brdf.y)
                * material.clearcoat * lights.ibl_intensity;
    }

    let proj = in.light_clip_pos.xyz / in.light_clip_pos.w;
    let uv = proj.xy * vec2(0.5, -0.5) + 0.5;
    let in_map = all(uv >= vec2(0.0)) && all(uv <= vec2(1.0));
    let shadow = select(1.0, textureSampleCompare(shadow_map, shadow_sampler, uv, proj.z + SHADOW_BIAS), in_map);

    var radiance_acc = vec3(0.0);
    let is_toon = model_id == 2u;

    // Chrome (global 3u or per-material 6u) is env-reflection-only: it
    // skips the direct-light loop entirely.
    if camera.material_override != 3u && model_id != 6u {
        // All lighting runs in WORLD space (0.8.1 hoist). Pre-hoist this
        // loop moved each light into tangent space instead; the TBN is
        // orthonormal so the two agree to within float rounding, and area
        // lights need world-space rect corners, which tangent space cannot
        // express per-light.
        for (var i = 0u; i < lights.count; i++) {
            let light = lights.lights[i];

            // Explicit vec3 copies: naga's Metal backend emits packed_float3
            // for struct members, which several vector ops reject directly.
            let light_dir = vec3<f32>(light.direction);
            let light_pos = vec3<f32>(light.position);

            // Rect area: integrate over the rectangle instead of shading
            // from a single direction, then skip the point/spot machinery.
            if light.kind == 3u {
                let half_x = vec3<f32>(light.half_x);
                let half_y = vec3<f32>(light.half_y);
                let corners = ltc_corners(light_pos, half_x, half_y);
                let two_sided = light.two_sided > 0.5;
                // No hidden multiplier: intensity is a plain linear scale.
                // There used to be a * 3.0 here and in the punctual arm
                // below, which meant no authored value could be matched
                // against a reference. Removing it is only safe alongside
                // the node defaults and the synthesized viewer rig, which
                // moved by the same factor in the same commit.
                let scale = light.intensity;

                // Diffuse is the plain cosine integral: the identity
                // transform IS the cosine lobe.
                let diffuse = ltc_evaluate(
                    N, V_world, in.world_position, IDENTITY3, corners, two_sided);

                if is_clay {
                    // Clay is directionless matte by definition, so it takes
                    // the diffuse integral and no specular, exactly as
                    // `lambert_direct` drops the specular for a point light.
                    radiance_acc += light.color * (albedo / PI) * diffuse * scale;
                } else if is_toon {
                    // Toon bands on dot(N, L), which an area light has no
                    // single L for. The direction to the panel's centre is
                    // the honest stand-in: it degrades to the point-light
                    // answer as the rectangle shrinks.
                    let to_center = normalize(light_pos - in.world_position);
                    let ndotl = max(dot(N, to_center), 0.0);
                    let banded = floor(ndotl * material.toon_steps)
                        / max(material.toon_steps - 1.0, 1.0);
                    radiance_acc += light.color * (albedo / PI)
                        * clamp(banded, 0.0, 1.0) * diffuse * scale;
                } else {
                    let uv = ltc_uv(dot(N, V_world), roughness);
                    let t1 = textureSampleLevel(t_ltc_transform, s_ltc, uv, 0.0);
                    let t2 = textureSampleLevel(t_ltc_magnitude, s_ltc, uv, 0.0);
                    // The packing contract with `gen_ltc_lut`: columns of
                    // M^-1, with the middle entry normalized to 1.
                    let m_inv = mat3x3<f32>(
                        vec3<f32>(t1.x, 0.0, t1.y),
                        vec3<f32>(0.0, 1.0, 0.0),
                        vec3<f32>(t1.z, 0.0, t1.w),
                    );
                    let specular = ltc_evaluate(
                        N, V_world, in.world_position, m_inv, corners, two_sided);

                    // Hill's LTC Fresnel: the table's magnitude and Fresnel
                    // terms rebuild what the split-sum path would have
                    // given. F0 is the shared one, so the index of
                    // refraction, the specular tint and any thin film
                    // reach area lights exactly as they reach the rest.
                    let fresnel = F0 * t2.x + (vec3(1.0) - F0) * t2.y;
                    let kD = (1.0 - metallic);
                    var lit = kD * albedo / PI * diffuse + fresnel * specular;

                    if is_principled {
                        let NdotV_a = max(dot(N, V_world), 0.001);

                        // Sheen has no published fit for the linearly
                        // transformed cosine, so it evaluates against the
                        // panel's dominant direction and is scaled by the
                        // rectangle's own form factor. It degrades to the
                        // point-light answer as the panel shrinks, which is
                        // the same stand-in the toon arm above makes for
                        // the same reason.
                        if material.sheen_color.r + material.sheen_color.g
                            + material.sheen_color.b > 0.0 {
                            let to_center = normalize(light_pos - in.world_position);
                            let Hs = normalize(V_world + to_center);
                            let NdotL_a = max(dot(N, to_center), 0.001);
                            let sheen = material.sheen_color
                                * D_Charlie(material.sheen_roughness, max(dot(N, Hs), 0.0))
                                * V_Sheen(NdotL_a, NdotV_a, material.sheen_roughness);
                            lit = lit * sheen_albedo_scaling(NdotV_a, NdotL_a)
                                + sheen * diffuse;
                        }

                        // Clearcoat DOES have a linearly transformed
                        // cosine: it is a GGX lobe, so it is the same
                        // machinery sampled a second time at the coat's
                        // roughness. This is real parity, not a stand-in.
                        if material.clearcoat > 0.0 {
                            let coat_r = clamp(material.clearcoat_roughness, 0.04, 1.0);
                            let coat_uv = ltc_uv(dot(N, V_world), coat_r);
                            let c1 = textureSampleLevel(t_ltc_transform, s_ltc, coat_uv, 0.0);
                            let c2 = textureSampleLevel(t_ltc_magnitude, s_ltc, coat_uv, 0.0);
                            let coat_inv = mat3x3<f32>(
                                vec3<f32>(c1.x, 0.0, c1.y),
                                vec3<f32>(0.0, 1.0, 0.0),
                                vec3<f32>(c1.z, 0.0, c1.w),
                            );
                            let coat_spec = ltc_evaluate(
                                N, V_world, in.world_position, coat_inv, corners, two_sided);
                            let coat_f0 = vec3(0.04);
                            let coat_fresnel = coat_f0 * c2.x + (vec3(1.0) - coat_f0) * c2.y;
                            let attenuate = F_schlick(NdotV_a, coat_f0).x * material.clearcoat;
                            lit = lit * (1.0 - attenuate)
                                + coat_fresnel * coat_spec * material.clearcoat;
                        }
                    }

                    radiance_acc += light.color * scale * lit;
                }
                continue;
            }

            var L: vec3<f32>;
            var atten = 1.0;
            if light.kind == 1u {
                // Directional: L opposes the light's travel direction.
                L = normalize(-light_dir);
            } else {
                let to_light = light_pos - in.world_position;
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
                    let dir_w = normalize(light_dir);
                    let cos_angle = dot(-L, dir_w);
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
            // See the rect-area arm above: intensity is a plain linear
            // scale, and dropping the multiplier at one site and not the
            // other would leave area lights three times brighter than
            // everything else with nothing to show for it.
            let scale = light.intensity * atten * shadow_factor;
            var brdf = select(
                cook_torrance(
                    N, V_world, L, albedo, roughness, metallic,
                    F0, aniso_tangent, aniso_bitangent,
                ),
                lambert_direct(N, L, albedo),
                is_clay,
            );

            // The principled layers, on the same light. Sheen scales what
            // is under it and adds its own lobe; the coat reflects a
            // fraction away and adds its own on top. Same order as the
            // image-based path above, so a surface reads consistently
            // whether it is lit by an environment or by a light node.
            if is_principled {
                let NdotL_p = max(dot(N, L), 0.001);
                let NdotV_p = max(dot(N, V_world), 0.001);
                let H = normalize(V_world + L);

                if material.sheen_color.r + material.sheen_color.g
                    + material.sheen_color.b > 0.0 {
                    let NdotH = max(dot(N, H), 0.0);
                    let sheen = material.sheen_color
                        * D_Charlie(material.sheen_roughness, NdotH)
                        * V_Sheen(NdotL_p, NdotV_p, material.sheen_roughness);
                    brdf = brdf * sheen_albedo_scaling(NdotV_p, NdotL_p)
                        + sheen * NdotL_p;
                }

                if material.clearcoat > 0.0 {
                    let NdotH = max(dot(N, H), 0.0);
                    let HdotV = max(dot(H, V_world), 0.0);
                    let coat_r = clamp(material.clearcoat_roughness, 0.04, 1.0);
                    let coat_f = F_schlick(HdotV, vec3(0.04)).x * material.clearcoat;
                    let coat = D_GGX(NdotH, coat_r)
                        * G_smith(NdotV_p, NdotL_p, coat_r)
                        / (4.0 * NdotV_p * NdotL_p);
                    brdf = brdf * (1.0 - coat_f) + vec3(coat * coat_f) * NdotL_p;
                }
            }

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
    let alpha = select(surface_alpha, 1.0, camera.material_override != 0u);
    return vec4(color, alpha);
}
