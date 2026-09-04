// The material response: Fresnel, GGX, sheen, iridescence and the layer
// operators the principled surface is composed from. Declares no entry point.
//
// Composed over the traversal, the atlas, the material and the sampler. Every
// direction below is in tangent space, where +z is the shading normal, which is
// the convention `bsdf_basis` establishes:
//
//   wo  outgoing, towards the camera
//   wi  incident, towards the light
//   wh  the half vector between them, the micronormal
//
// Anisotropy rides a two-component alpha, roughness along the tangent in x and
// along the bitangent in y. Every function here is a pure function of its
// arguments and none of them reads the surface record, which is what lets the
// probe drive one lobe at a time.
//
// Ported from MIT-licensed open-source path tracing work; the notice ships with
// the release. The per-function paper citations below are the real documentation
// of the math and are carried with it.
//
// Four things are deliberately NOT as the source has them.
//
// `pow` is replaced by multiplication wherever the base can be negative, because
// WGSL leaves `pow` with a negative base undefined and the source relies on
// GLSL's willingness to square one anyway. `schlick_fresnel` clamps its cosine
// rather than trusting callers, because two of the source's own call sites pass
// an unclamped dot product and a cosine above one turns `pow(1 - c, 5)` into a
// negative base. The sheen coefficient table is indexed by literals rather than
// by a loop counter, because indexing an array *value* by a runtime index is not
// something WGSL allows. And the multiple-scattering compensation term is
// absent: see `conductor_fresnel`.

// GGX's own floor on the distribution's denominator.
const BSDF_EPSILON: f32 = 1e-5;
// A perfectly smooth microfacet distribution is a delta and cannot be sampled,
// so roughness is clamped here rather than special-cased at every use.
const MIN_ROUGHNESS: f32 = 1e-3;
// Grazing angles make the Smith terms diverge. This is the floor on any cosine
// that ends up in a denominator.
const MIN_INCIDENT_COS: f32 = 1e-3;

// The smallest share of samples the reflection lobe keeps on a transmissive
// surface. Numerical rather than aesthetic; the reason is at `get_lobe_weights`.
const TRANS_SPECULAR_FLOOR: f32 = 0.01;

// A test-only skew on the reflection and transmission shares.
//
// The lobe weights are a sampling density: they appear in the selection and in
// the mixture density, and nowhere else. Any positive assignment therefore has
// to leave the estimator's expectation alone, and that invariance is the only
// available proof that they are a density rather than a response. Nothing ships
// a way to set this, exactly as the estimator override ships none.
override BSDF_TRANSMISSION_BIAS: f32 = 0.0;

// The sheen fit's two coefficient rows, at alpha = 1 and alpha = 0.
// Estevez and Kulla, section 3.
const VELVET_P0: array<f32, 5> = array<f32, 5>(25.3245, 3.32435, 0.16801, -1.27393, -4.85967);
const VELVET_P1: array<f32, 5> = array<f32, 5>(21.5473, 3.82987, 0.19823, -1.97760, -4.32054);

// Rec.709 primaries, column major, for the thin-film spectral integration.
const XYZ_TO_REC709: mat3x3f = mat3x3f(
    vec3f(3.2404542, -0.9692660, 0.0556434),
    vec3f(-1.5371385, 1.8760108, -0.2040259),
    vec3f(-0.4985314, 0.0415560, 1.0572252),
);

// An orthonormal basis whose third column is the normal, so tangent space is
// z-up and the frame's inverse is its transpose.
//
// The tangent direction is arbitrary, which is correct for every isotropic lobe
// and not correct for an anisotropic one; `surface_frame` supplies a real
// tangent when it has one and falls back to this.
fn bsdf_basis(normal: vec3f) -> mat3x3f {
    var other: vec3f;
    if abs(normal.x) > 0.5 {
        other = vec3f(0.0, 1.0, 0.0);
    } else {
        other = vec3f(1.0, 0.0, 0.0);
    }
    let ortho = normalize(cross(normal, other));
    let ortho2 = normalize(cross(normal, ortho));
    return mat3x3f(ortho2, ortho, normal);
}

// Normal-incidence reflectance of a dielectric against air.
//
// Squared by multiplication rather than by `pow`: the ratio is negative for any
// index below one, and `pow` with a negative base is undefined in WGSL where
// GLSL happened to give the right answer.
fn ior_to_f0(ior: f32) -> f32 {
    let r = (1.0 - ior) / (1.0 + ior);
    return r * r;
}

fn ior_to_f0_general(transmitted_ior: f32, incident_ior: f32) -> f32 {
    let r = (transmitted_ior - incident_ior) / (transmitted_ior + incident_ior);
    return r * r;
}

fn ior_to_f0_general_v3(transmitted_ior: vec3f, incident_ior: vec3f) -> vec3f {
    let v = (transmitted_ior - incident_ior) / (transmitted_ior + incident_ior);
    return v * v;
}

fn fresnel0_to_ior(f0: vec3f) -> vec3f {
    let sqrt_f0 = sqrt(f0);
    return (vec3f(1.0) + sqrt_f0) / (vec3f(1.0) - sqrt_f0);
}

// Schlick's approximation. The cosine is clamped rather than trusted.
fn schlick_fresnel(cosine: f32, f0: f32) -> f32 {
    let c = clamp(cosine, 0.0, 1.0);
    let m = 1.0 - c;
    let m2 = m * m;
    return f0 + (1.0 - f0) * m2 * m2 * m;
}

fn schlick_fresnel_v3(cosine: f32, f0: vec3f, f90: vec3f) -> vec3f {
    let c = clamp(cosine, 0.0, 1.0);
    let m = 1.0 - c;
    let m2 = m * m;
    return f0 + (f90 - f0) * m2 * m2 * m;
}

fn total_internal_reflection(cos_theta: f32, eta: f32) -> bool {
    let c = clamp(abs(cos_theta), 0.0, 1.0);
    let sin_theta = sqrt(max(0.0, 1.0 - c * c));
    return eta * sin_theta > 1.0;
}

// Schlick, or full reflectance past the critical angle.
fn evaluate_fresnel(cosine: f32, eta: f32, f0: vec3f, f90: vec3f) -> vec3f {
    if total_internal_reflection(cosine, eta) {
        return f90;
    }
    return schlick_fresnel_v3(cosine, f0, f90);
}

// A half vector drawn from the distribution of visible normals.
//
// Heitz, https://hal.archives-ouvertes.fr/hal-01509746/document. The shape is
// stretch the view into the isotropic configuration, sample the visible
// hemisphere there, then unstretch: sampling visible normals rather than all
// normals is what removes the samples that the masking term would have thrown
// away anyway.
fn ggx_direction(incident_dir: vec3f, alpha: vec2f, uv: vec2f) -> vec3f {
    let v = normalize(vec3f(alpha * incident_dir.xy, incident_dir.z));

    var t1: vec3f;
    if v.z < 0.9999 {
        t1 = normalize(cross(v, vec3f(0.0, 0.0, 1.0)));
    } else {
        t1 = vec3f(1.0, 0.0, 0.0);
    }
    let t2 = cross(t1, v);

    let a = 1.0 / (1.0 + v.z);
    let r = sqrt(uv.x);
    var phi: f32;
    if uv.y < a {
        phi = uv.y / a * PI;
    } else {
        phi = PI + (uv.y - a) / (1.0 - a) * PI;
    }

    let p1 = r * cos(phi);
    var p2 = r * sin(phi);
    if uv.y >= a {
        p2 *= v.z;
    }

    var n = p1 * t1 + p2 * t2 + v * sqrt(max(0.0, 1.0 - p1 * p1 - p2 * p2));
    return normalize(vec3f(alpha * n.xy, max(0.0, n.z)));
}

// Smith's masking lambda for one direction.
//
// Walter et al., equation (34), in the anisotropic form of Filament's equation
// (43).
fn ggx_lambda(v: vec3f, alpha: vec2f) -> f32 {
    let n_dot_v = max(v.z, MIN_INCIDENT_COS);
    let cos2 = n_dot_v * n_dot_v;
    let t = (alpha.x * alpha.x * v.x * v.x + alpha.y * alpha.y * v.y * v.y) / cos2;
    return (-1.0 + sqrt(1.0 + t)) / 2.0;
}

fn ggx_shadow_mask_g1(v: vec3f, alpha: vec2f) -> f32 {
    return 1.0 / (1.0 + ggx_lambda(v, alpha));
}

// The height-correlated visibility term, which is G divided by
// `4 * NdotV * NdotL`, so the specular lobe never forms that quotient itself.
fn ggx_smith_visibility(v: vec3f, l: vec3f, alpha: vec2f) -> f32 {
    let n_dot_v = max(v.z, MIN_INCIDENT_COS);
    let n_dot_l = max(l.z, MIN_INCIDENT_COS);
    let ggx_v = n_dot_l * length(vec3f(alpha.x * v.x, alpha.y * v.y, n_dot_v));
    let ggx_l = n_dot_v * length(vec3f(alpha.x * l.x, alpha.y * l.y, n_dot_l));
    return 0.5 / max(BSDF_EPSILON, ggx_v + ggx_l);
}

// Trowbridge-Reitz, anisotropic.
//
// The denominator gets a floor at the smallest positive normal float rather than
// at this file's epsilon, and the difference is not cosmetic. At the low end of the
// roughness clamp the alphas are 1e-6, so this quantity is legitimately around
// 1e-24: a floor of 1e-5 replaces a near-delta distribution with a flat one worth
// about 1e-27, the specular lobe stops having any density at all, and every
// specular sample is then charged the diffuse density instead. That reads as a
// smooth surface reflecting roughly twice the light that reached it, which is how
// the furnace test found it.
fn ggx_distribution(h: vec3f, alpha: vec2f) -> f32 {
    let a2 = alpha.x * alpha.y;
    let v = vec3f(alpha.y * h.x, alpha.x * h.y, a2 * h.z);
    // Only a genuinely zero half vector can reach this, which upstream degeneracy
    // can produce and a unit one cannot.
    let v2 = max(1e-30, dot(v, v));
    let w2 = a2 / v2;
    return a2 * w2 * w2 / PI;
}

// The density of `ggx_direction`, already divided by the Jacobian of reflection,
// so this is a density over the reflected direction rather than over the half
// vector. `HdotV` cancels because it is positive by construction.
//
// Heitz, equation (17).
fn ggx_reflection_adjusted_pdf(v: vec3f, h: vec3f, alpha: vec2f) -> f32 {
    let n_dot_v = max(v.z, MIN_INCIDENT_COS);
    return ggx_distribution(h, alpha) * ggx_shadow_mask_g1(v, alpha) / (4.0 * n_dot_v);
}

// The density of `ggx_direction` over the HALF VECTOR, before any Jacobian.
//
// Heitz, equation (3). `ggx_reflection_adjusted_pdf` above is this divided by
// the Jacobian of reflection, `4 * dot(v, h)`, which is where its cancellation
// of `HdotV` comes from. The two are written separately rather than one in
// terms of the other on purpose: the reflection form is what three passing
// histogram comparisons already pin, it differs from this one when `dot(v, h)`
// is not positive, and it stays byte for byte what it was.
//
// Refraction has a different Jacobian, so it needs the unadjusted density.
fn ggx_vndf_half_pdf(v: vec3f, h: vec3f, alpha: vec2f) -> f32 {
    let n_dot_v = max(v.z, MIN_INCIDENT_COS);
    return ggx_distribution(h, alpha) * ggx_shadow_mask_g1(v, alpha)
        * max(0.0, dot(v, h)) / n_dot_v;
}

// The Charlie velvet sheen distribution. Estevez and Kulla, equation (2).
fn velvet_d(cos_theta_h: f32, roughness: f32) -> f32 {
    var alpha = max(roughness, 0.07);
    alpha = alpha * alpha;
    let inv_alpha = 1.0 / alpha;
    let sin_theta_h = max(1.0 - cos_theta_h * cos_theta_h, 0.001);
    return (2.0 + inv_alpha) * pow(sin_theta_h, 0.5 * inv_alpha) / (2.0 * PI);
}

// The fitted shadowing curve. The table is indexed by literals because WGSL does
// not allow a runtime index into an array value, which is how the source writes
// it.
fn velvet_l(x: f32, alpha: f32) -> f32 {
    let one_minus_alpha = 1.0 - alpha;
    let q = one_minus_alpha * one_minus_alpha;

    let a = mix(VELVET_P1[0], VELVET_P0[0], q);
    let b = mix(VELVET_P1[1], VELVET_P0[1], q);
    let c = mix(VELVET_P1[2], VELVET_P0[2], q);
    let d = mix(VELVET_P1[3], VELVET_P0[3], q);
    let e = mix(VELVET_P1[4], VELVET_P0[4], q);

    return a / (1.0 + b * pow(abs(x), c)) + d * x + e;
}

// Estevez and Kulla, equation (3). The two branches meet at 0.5 by construction,
// which is what keeps the curve continuous there.
fn velvet_lambda(cos_theta: f32, alpha: f32) -> f32 {
    if abs(cos_theta) < 0.5 {
        return exp(velvet_l(cos_theta, alpha));
    }
    return exp(2.0 * velvet_l(0.5, alpha) - velvet_l(1.0 - cos_theta, alpha));
}

// Estevez and Kulla, section 3.
fn velvet_g(cos_theta_o: f32, cos_theta_i: f32, roughness: f32) -> f32 {
    var alpha = max(roughness, 0.07);
    alpha = alpha * alpha;
    return 1.0 / (1.0 + velvet_lambda(cos_theta_o, alpha) + velvet_lambda(cos_theta_i, alpha));
}

// The analytic directional-albedo fit the layering uses. Estevez and Kulla,
// section 5. This is energy compensation with no table, which is why sheen keeps
// its compensation in this release and metal does not.
fn directional_albedo_sheen(cos_theta_in: f32, alpha: f32) -> f32 {
    let cos_theta = saturate(cos_theta_in);
    let c = 1.0 - cos_theta;
    let c3 = c * c * c;
    return 0.65584461 * c3 + 1.0 / (4.16526551 + exp(-7.97291361 * sqrt(alpha) + 6.33516894));
}

// How much the layers beneath sheen are attenuated so the sheen lobe adds no
// energy on top of an already fully reflective base.
//
// A black sheen colour makes this exactly one, which is what lets this release
// carry no separate sheen weight: the authoring model stores the sheen
// reflectance directly, as glTF does, so black is off.
fn sheen_albedo_scaling(wo: vec3f, wi: vec3f, sheen_color: vec3f, sheen_roughness: f32) -> f32 {
    var alpha = max(sheen_roughness, 0.07);
    alpha = alpha * alpha;
    let max_sheen = max(max(sheen_color.r, sheen_color.g), sheen_color.b);
    let e_wo = directional_albedo_sheen(clamp(wo.z, 0.001, 1.0), alpha);
    let e_wi = directional_albedo_sheen(clamp(wi.z, 0.001, 1.0), alpha);
    return min(1.0 - max_sheen * e_wo, 1.0 - max_sheen * e_wi);
}

// The sheen lobe itself. Estevez and Kulla, equation (1).
//
// Named for the lobe rather than for the colour it returns, which the source
// does not: there it collides with both a surface field and a local of the same
// name, and a reader has to disambiguate three things by context.
fn sheen_lobe(wo: vec3f, wi: vec3f, wh: vec3f, sheen_color: vec3f, sheen_roughness: f32) -> vec3f {
    let cos_theta_o = clamp(wo.z, 0.001, 1.0);
    let cos_theta_i = clamp(wi.z, 0.001, 1.0);
    let d = velvet_d(wh.z, sheen_roughness);
    let g = velvet_g(cos_theta_o, cos_theta_i, sheen_roughness);
    return sheen_color * d * g / (4.0 * abs(cos_theta_o * cos_theta_i));
}

// The spectral sensitivity of the thin-film integration, evaluated analytically
// rather than by sampling wavelengths.
//
// Belcour and Barla, 2017, section 4.
fn eval_sensitivity(opd: f32, shift: vec3f) -> vec3f {
    let phase = 2.0 * PI * opd * 1.0e-9;
    let val = vec3f(5.4856e-13, 4.4201e-13, 5.2481e-13);
    let pos = vec3f(1.6810e+06, 1.7953e+06, 2.2084e+06);
    let variance = vec3f(4.3278e+09, 9.3046e+09, 6.6121e+09);

    var xyz = val * sqrt(2.0 * PI * variance) * cos(pos * phase + shift)
        * exp(-phase * phase * variance);
    xyz.x += 9.7470e-14 * sqrt(2.0 * PI * 4.5282e+09) * cos(2.2399e+06 * phase + shift.x)
        * exp(-4.5282e+09 * phase * phase);
    xyz /= 1.0685e-7;

    return XYZ_TO_REC709 * xyz;
}

// Reflectance of a thin film over a base of reflectance `base_f0`.
//
// Belcour and Barla, 2017. A simplified model: it ignores polarization and uses
// the Fresnel approximation. The dirac pairs are summed to two orders, which is
// where the source stops and where the visible interference already is.
fn iridescent_fresnel(
    cos_theta1: f32,
    base_f0: vec3f,
    iridescence_ior: f32,
    outside_ior: f32,
    thickness: f32,
) -> vec3f {
    let ratio = outside_ior / iridescence_ior;
    let c1 = clamp(cos_theta1, 0.0, 1.0);
    let sin_theta2_sq = ratio * ratio * (1.0 - c1 * c1);
    let cos_theta2_sq = 1.0 - sin_theta2_sq;

    // Past the critical angle the film reflects everything.
    if cos_theta2_sq < 0.0 {
        return vec3f(1.0);
    }
    let cos_theta2 = sqrt(cos_theta2_sq);

    // First interface: outside into the film.
    let r0 = ior_to_f0_general(iridescence_ior, outside_ior);
    let r12 = schlick_fresnel(c1, r0);
    let t121 = 1.0 - r12;
    let phi12 = select(0.0, PI, iridescence_ior < outside_ior);
    let phi21 = PI - phi12;

    // Second interface: the film into the base. The nudge on `base_f0` guards
    // the singularity at a reflectance of one.
    let base_ior = fresnel0_to_ior(base_f0 + 0.0001);
    let r1 = ior_to_f0_general_v3(base_ior, vec3f(iridescence_ior));
    let r23 = schlick_fresnel_v3(cos_theta2, r1, vec3f(1.0));
    let phi23 = select(vec3f(0.0), vec3f(PI), base_ior < vec3f(iridescence_ior));

    // Optical path difference, and the phase it implies.
    let opd = 2.0 * iridescence_ior * thickness * cos_theta2;
    let phi = vec3f(phi21) + phi23;

    let r123 = clamp(r12 * r23, vec3f(1e-5), vec3f(0.9999));
    let r123_sqrt = sqrt(r123);
    let rs = t121 * t121 * r23 / (vec3f(1.0) - r123);

    // The constant term, then two orders of dirac pairs.
    var i = r12 + rs;
    var cm = rs - t121;
    for (var m = 1; m <= 2; m += 1) {
        cm *= r123_sqrt;
        let sm = 2.0 * eval_sensitivity(f32(m) * opd, f32(m) * phi);
        i += cm * sm;
    }

    return max(i, vec3f(0.0));
}

// Blends by the largest channel of a per-channel weight, so a coloured Fresnel
// does not brighten the base where it is dark in one channel.
fn rgb_mix(base: vec3f, specular: vec3f, rgb_alpha: vec3f) -> vec3f {
    let alpha_max = max(max(rgb_alpha.x, rgb_alpha.y), rgb_alpha.z);
    return (1.0 - alpha_max) * base + rgb_alpha * specular;
}

fn iridescent_dielectric_layer(
    dielectric_base: vec3f,
    base: vec3f,
    specular: vec3f,
    h_dot_l: f32,
    outside_ior: f32,
    base_ior: f32,
    iridescence_ior: f32,
    thickness: f32,
    strength: f32,
) -> vec3f {
    let base_f0 = vec3f(ior_to_f0(base_ior));
    let f = iridescent_fresnel(h_dot_l, base_f0, iridescence_ior, outside_ior, thickness);
    return mix(dielectric_base, rgb_mix(base, specular, f), strength);
}

fn iridescent_conductor_layer(
    metal_base: vec3f,
    specular: vec3f,
    base_f0: vec3f,
    h_dot_l: f32,
    outside_ior: f32,
    iridescence_ior: f32,
    thickness: f32,
    strength: f32,
) -> vec3f {
    let f = iridescent_fresnel(h_dot_l, base_f0, iridescence_ior, outside_ior, thickness);
    return mix(metal_base, specular * f, strength);
}

// The Disney diffuse term with its retroreflective lobe, without the subsurface
// approximation. Burley, equation (4).
fn diffuse_brdf(n_dot_v: f32, n_dot_l: f32, v_dot_h: f32, color: vec3f, alpha: f32) -> vec3f {
    let fl = schlick_fresnel(n_dot_l, 0.0);
    let fv = schlick_fresnel(n_dot_v, 0.0);

    let bias = mix(0.0, 0.5, alpha) - 1.0;
    let energy_factor = mix(1.0, 1.0 / 1.51, alpha);

    let rr = 2.0 * alpha * v_dot_h * v_dot_h;
    let retro = rr * (fl + fv + fl * fv * (rr + 2.0 * bias));
    let fresnel = (1.0 + bias * fl) * (1.0 + bias * fv);

    return energy_factor * (color / PI) * (retro + fresnel);
}

// The microfacet specular term, without its Fresnel: the layer operators above
// apply that, because which Fresnel is correct depends on whether the layer is a
// dielectric or a conductor.
fn specular_brdf(v: vec3f, l: vec3f, h: vec3f, alpha: vec2f) -> vec3f {
    let vis = ggx_smith_visibility(v, l, alpha);
    let d = ggx_distribution(h, alpha);
    return vec3f(d * vis);
}

// The dielectric layer operator, with a tinted normal-incidence reflectance and
// a weight. glTF's `KHR_materials_specular`.
fn fresnel_mix(
    v_dot_h: f32,
    f0_color: vec3f,
    ior: f32,
    eta: f32,
    weight: f32,
    base: vec3f,
    layer: vec3f,
) -> vec3f {
    var f0 = ior_to_f0(ior) * f0_color;
    f0 = min(f0, vec3f(1.0));
    // `evaluate_fresnel` rather than Schlick alone, because Schlick does not
    // know about the critical angle and this one does.
    //
    // The Fresnel that picks the lobe and the Fresnel that values it have to be
    // the same function. `get_lobe_weights` has always used this one, so past
    // the critical angle on the inside of a solid it correctly routed every
    // sample to reflection, and this then reflected the four percent Schlick
    // gives at normal incidence and dropped the rest. Total internal reflection
    // lost most of its energy, once per interior bounce, which is a first-order
    // reason glass read as grey rather than as glass.
    //
    // Provably inert everywhere else: total internal reflection needs an index
    // ratio above one, which `surface_from` grants only on a back face that
    // transmits. Every opaque material and every front face is bit-identical.
    let fr = evaluate_fresnel(abs(v_dot_h), eta, f0, vec3f(1.0));
    let max_fr = max(max(fr.r, fr.g), fr.b);
    return (1.0 - weight * max_fr) * base + weight * fr * layer;
}

// The conductor layer operator.
//
// **The multiple-scattering compensation term is absent, deliberately.** The
// source multiplies by `1 + f0 * (1 - E) / E`, where `E` is a directional albedo
// read from a 32 by 32 table, which recovers the energy a single-scattering GGX
// loses between microfacets. That table needs a sampled binding, and every free
// number in the sampled group is reserved for the environment, so this release
// ships the single-scattering term and measures the deficit rather than
// renegotiating a binding budget the design says later stages do not renegotiate.
//
// The consequence is visible in exactly one place and is worth knowing before
// reading it as a bug: a rough metal is darker than it should be, and the
// furnace grid shows it as darkening along its high-metalness rows. Restoring it
// is one binding and one init-time dispatch, not new math: the source can
// generate the table on device with one thread per texel.
//
// `NdotV` and the roughness are not parameters here for the same reason. They
// existed only to index the table.
fn conductor_fresnel(v_dot_h: f32, f0: vec3f, lobe: vec3f) -> vec3f {
    return lobe * schlick_fresnel_v3(abs(v_dot_h), f0, vec3f(1.0));
}

// The clearcoat layer operator: a smooth dielectric coat over everything else.
fn fresnel_coat(v_dot_nc: f32, ior: f32, base: vec3f, layer: vec3f, weight: f32) -> vec3f {
    let f0 = ior_to_f0(ior);
    let f = schlick_fresnel(abs(v_dot_nc), f0);
    return mix(base, layer, weight * f);
}

// The clearcoat's index of refraction is fixed rather than authored, which is
// glTF's choice: `KHR_materials_clearcoat` specifies a polyurethane coat and
// carries no index parameter.
const CLEARCOAT_IOR: f32 = 1.5;

// Everything a lobe reads about one point on one surface.
//
// World-space directions never reach a lobe: they are rotated into the frame at
// the two entry points and back once on the way out, which is what keeps every
// function above written in one convention. Both frames are carried because the
// clearcoat may sit on a different normal than the base; nothing authors a
// separate clearcoat normal in this release, so they are equal today and the
// layer operators do not have to change when one does.
//
// Neither inverse is computed as an inverse. The frames are orthonormal by
// construction, so the transpose is exact and costs nothing, where the source
// spends an adjugate and a determinant per hit and does it twice.
struct Surface {
    // Column 2 is the shading normal.
    frame: mat3x3f,
    inv_frame: mat3x3f,
    clearcoat_frame: mat3x3f,
    inv_clearcoat_frame: mat3x3f,

    // False only when a transmissive surface is hit from inside. An opaque
    // surface is always front-facing, because nothing downstream of it reads the
    // distinction and the ratio of indices would otherwise invert on a back face
    // that cannot transmit.
    front_face: bool,
    // Ratio of indices across the interface, in the direction the ray travels.
    eta: f32,
    // Normal-incidence reflectance implied by `eta`.
    f0: f32,

    // Squared roughness along the tangent in x and the bitangent in y. Squared
    // exactly once, here, so no lobe has to know whether it received a
    // perceptual or a slope value.
    alpha: vec2f,
    clearcoat_alpha: f32,

    color: vec3f,
    emission: vec3f,
    metalness: f32,

    ior: f32,
    transmission: f32,
    // A surface with no interior to attenuate through.
    thin_film: bool,
    attenuation_color: vec3f,
    attenuation_distance: f32,

    clearcoat: f32,

    // The sheen reflectance itself, with no separate weight: the authoring model
    // stores it the way glTF does, so black is off and every place the source
    // multiplies by a sheen weight is multiplying by one here.
    sheen_color: vec3f,
    sheen_roughness: f32,

    iridescence: f32,
    iridescence_ior: f32,
    iridescence_thickness: f32,

    specular_color: vec3f,
    specular_intensity: f32,
}

// Resolves a record and a texture-resolved sample into a shadeable surface.
//
// `normal_ws` is the interpolated shading normal in world space before sidedness
// is applied. `tangent_ws` is `shading_tangent` carried into world space, whose
// `w` is handedness and whose zero `w` means the mesh gave us nothing usable.
// `side` is +1 on a front face and -1 on a back one.
fn surface_from(
    m: TracedMaterial,
    s: MaterialSample,
    normal_ws: vec3f,
    tangent_ws: vec4f,
    side: f32,
) -> Surface {
    var surf: Surface;

    let has_tangent = tangent_ws.w != 0.0 && dot(tangent_ws.xyz, tangent_ws.xyz) > BSDF_EPSILON;

    // The normal map is applied before sidedness, in the frame the tangent
    // defines, because the map is authored against the front face.
    var normal = normalize(normal_ws);
    if s.has_normal_map && has_tangent {
        let t = normalize(tangent_ws.xyz);
        let b = normalize(cross(normal, t) * tangent_ws.w);
        let tbn = mat3x3f(t, b, normal);
        normal = normalize(tbn * s.normal_ts);
    }
    normal = normal * side;

    // The anisotropy direction lives in the frame rather than in the alpha: a
    // rotated highlight is a rotated basis, and folding the rotation into a
    // two-component roughness instead would only be able to express the two axes.
    let strength = saturate(abs(m.anisotropy));
    let aniso_dir = vec2f(cos(m.anisotropy_rotation), sin(m.anisotropy_rotation));

    if strength > 0.0 && has_tangent {
        // Re-orthogonalize against the shading normal first. The vertex tangent
        // was perpendicular to the geometric normal, and a normal map has just
        // moved the normal, so the pair is no longer orthonormal.
        var t = normalize(tangent_ws.xyz);
        t = normalize(t - normal * dot(normal, t));
        let b = normalize(cross(normal, t) * tangent_ws.w);
        surf.frame = mat3x3f(
            t * aniso_dir.x + b * aniso_dir.y,
            b * aniso_dir.x - t * aniso_dir.y,
            normal,
        );
    } else {
        surf.frame = bsdf_basis(normal);
    }
    surf.inv_frame = transpose(surf.frame);

    // No clearcoat normal is authored in this release, so the coat sits on the
    // base normal. Kept as its own pair so a later slot changes this function
    // and nothing else.
    surf.clearcoat_frame = surf.frame;
    surf.inv_clearcoat_frame = surf.inv_frame;

    let roughness = clamp(s.roughness, MIN_ROUGHNESS, 1.0);
    let alpha_b = roughness * roughness;
    // Stretching only along the tangent, which is the anisotropy the strength
    // parameter means: at full strength the tangent axis is fully rough while the
    // bitangent axis keeps the authored value.
    surf.alpha = vec2f(mix(alpha_b, 1.0, strength * strength), alpha_b);

    let clearcoat_roughness = clamp(m.clearcoat_roughness, MIN_ROUGHNESS, 1.0);
    surf.clearcoat_alpha = clearcoat_roughness * clearcoat_roughness;

    surf.color = s.base_color.rgb;
    surf.emission = s.emissive;
    surf.metalness = saturate(s.metallic);

    surf.ior = m.ior;
    surf.transmission = saturate(m.transmission);
    surf.thin_film = m.thickness == 0.0;
    surf.attenuation_color = m.attenuation_color;
    surf.attenuation_distance = m.attenuation_distance;

    surf.clearcoat = saturate(m.clearcoat);

    surf.sheen_color = m.sheen_color;
    surf.sheen_roughness = clamp(m.sheen_roughness, MIN_ROUGHNESS, 1.0);

    surf.iridescence = saturate(m.iridescence);
    surf.iridescence_ior = m.iridescence_ior;
    // No thickness map exists in the five-slot model, so the range collapses to
    // its upper end, which is what the source uses when a map is absent.
    surf.iridescence_thickness = m.iridescence_thickness_max;

    surf.specular_color = m.specular_color;
    surf.specular_intensity = m.specular_intensity;

    // A back face only matters where light can pass through. Opaque geometry is
    // reported front-facing so the index ratio below does not invert on a
    // surface that cannot transmit.
    surf.front_face = side == 1.0 || surf.transmission == 0.0;
    if surf.thin_film || surf.front_face {
        surf.eta = 1.0 / surf.ior;
    } else {
        surf.eta = surf.ior;
    }
    surf.f0 = ior_to_f0(surf.eta);

    return surf;
}

// Which lobe a sample came from. Diagnostic for the caller, and the axis the
// histogram test bins on: a per-lobe comparison needs to know which lobe it is
// looking at, and inferring it from the direction afterwards cannot distinguish
// specular from clearcoat.
const LOBE_NONE: u32 = 0u;
const LOBE_DIFFUSE: u32 = 1u;
const LOBE_SPECULAR: u32 = 2u;
const LOBE_TRANSMISSION: u32 = 3u;
const LOBE_CLEARCOAT: u32 = 4u;

// How likely each lobe is to be chosen. Sums to one whenever anything can be
// chosen at all.
struct LobeWeights {
    diffuse: f32,
    specular: f32,
    transmission: f32,
    clearcoat: f32,
}

// One direction pair resolved into both frames, so `bsdf_evaluate` reads and
// never recomputes.
struct BxdfContext {
    // Base frame: outgoing, incident, half.
    v: vec3f,
    l: vec3f,
    h: vec3f,
    // Clearcoat frame, same three.
    vc: vec3f,
    lc: vec3f,
    hc: vec3f,
    v_dot_h: f32,
}

struct BsdfEval {
    color: vec3f,
    pdf: f32,
}

struct ScatterRecord {
    // The response, with the cosine already folded in, so a caller divides by the
    // density and multiplies nothing else. This is the source's convention and
    // worth keeping: the alternative puts a `max(0, wi.z)` at every call site.
    color: vec3f,
    // The one-sample mixture density: the sum over every lobe that could have
    // produced this direction, weighted by how likely that lobe was to be chosen.
    // NOT the density of the lobe that actually produced it.
    pdf: f32,
    // World space.
    direction: vec3f,
    lobe: u32,
    // Whether the path crossed to the other side of the surface, so the caller
    // can charge the transmissive budget rather than infer it from a dot product.
    transmissive: bool,
}

// A non-positive density is the termination signal, as in the source. It covers
// a genuinely zero-probability direction and the degenerate-geometry early-out
// alike, which is what lets the bounce loop have one exit test.
fn is_terminating_scatter(rec: ScatterRecord) -> bool {
    return rec.pdf <= 0.0;
}

// Rec.709 luminance, for the roulette's throughput comparison.
fn luminance(rgb: vec3f) -> f32 {
    return dot(rgb, vec3f(0.2126, 0.7152, 0.0722));
}

// WGSL has neither `isnan` nor `isinf`. A value is its own equal unless it is
// NaN, and a finite magnitude is bounded.
fn is_finite_v3(v: vec3f) -> bool {
    return all(v == v) && all(abs(v) < vec3f(1e30));
}

// The half vector, aware that a transmitted direction needs the index ratio.
//
// Distinct from `half_vector` below, and the distinction is load-bearing: the
// source overloads one name for both, the reflection path wants the plain form,
// the transmission path wants this one, and mixing them up produces glass that
// looks nearly right. WGSL forbidding overloading is doing us a favour here.
//
// Burley, section 2.2 on the transmission half vector.
fn half_vector_eta(wi: vec3f, wo: vec3f, eta: f32) -> vec3f {
    var h: vec3f;
    if wi.z > 0.0 {
        h = normalize(wi + wo);
    } else {
        h = normalize(wi + wo * eta);
    }
    // Face the macronormal. `sign` of exactly zero is zero and would annihilate
    // the vector, so the sided form is written with a select.
    return h * select(-1.0, 1.0, h.z >= 0.0);
}

fn half_vector(a: vec3f, b: vec3f) -> vec3f {
    return normalize(a + b);
}

// A cosine-weighted direction in the upper hemisphere, whose density is
// `wi.z / PI`.
//
// The sphere-offset construction rather than a polar one: sample the sphere,
// translate along the normal, renormalize. It is exactly cosine-weighted and
// costs no trigonometry beyond what the sphere sampler already spent.
//
// The guard is not decoration. At `uv.x == 0` the sphere sampler returns the
// south pole exactly, the translation lands on the origin, and normalizing it
// would produce NaN. White noise reaches that point with probability zero; a
// stratified sampler starts its first stratum there.
fn diffuse_direction(uv: vec2f) -> vec3f {
    var d = sample_sphere(uv);
    d.z += 1.0;
    if dot(d, d) < BSDF_EPSILON {
        return vec3f(0.0, 0.0, 1.0);
    }
    return normalize(d);
}

// Whether this surface transmits as a thin wall rather than as a solid.
//
// `thickness` is the authored field, and an index of one is folded in here
// because it is optically the same thing and because it is the one true
// singularity in the refraction density below: the denominator
// `dot(wi, h) + eta * dot(wo, h)` vanishes exactly when eta is one. It also
// removes a latent NaN that predates this lobe, since a thin surface at an
// index of one transmits along exactly `-wo`, and `half_vector_eta` would then
// normalize the zero vector.
fn transmission_is_thin(surf: Surface) -> bool {
    return surf.thin_film || abs(surf.eta - 1.0) < 1e-3;
}

// One transmitted sample: the direction, the micronormal it went through, and
// whether it exists at all.
struct TransmissionSample {
    wi: vec3f,
    h: vec3f,
    valid: bool,
}

// The transmitted direction, drawn from the visible-normal distribution.
//
// One micronormal draw feeds two maps. A solid refracts through it. A thin wall
// does not refract twice through parallel interfaces, which is what this used to
// do; it takes the reflection about the micronormal, mirrored to the far side.
// Both agree with the old behaviour in the flat limit, where each returns `-wo`,
// the straight-through direction a parallel slab produces.
//
// The thin form is chosen because it has a density and the double refraction did
// not. Mirroring is a measure-preserving involution of the sphere, so the density
// of the mirrored draw is the reflection density at the mirrored direction, which
// `ggx_reflection_adjusted_pdf` already computes. The old form refracted through a
// rough interface and back through a flat one, which is not a slab either.
//
// The doc this replaces said the visible-normal formulation had been reverted
// upstream for producing black pixels. That warning was misaimed and is corrected
// here rather than carried: the note it came from sits on a different reverted
// block, and the formulation it names is separately wrong three ways, each of
// which darkens. It pairs a real density with a constant value, it drops the
// numerator of the refraction Jacobian, and it puts eta on the wrong term. The
// first of those alone is black glass, and it is what
// `smooth_glass_transmits_rather_than_swallowing` exists to catch.
fn transmission_direction(wo: vec3f, surf: Surface, uv: vec2f) -> TransmissionSample {
    var out: TransmissionSample;
    out.h = ggx_direction(wo, surf.alpha, uv);
    out.valid = false;
    out.wi = vec3f(0.0);

    if transmission_is_thin(surf) {
        // Reflect about the micronormal, then mirror through the surface. At a
        // flat micronormal this is exactly `-wo`.
        let wr = -normalize(reflect(wo, out.h));
        if wr.z <= 0.0 {
            // A micronormal steep enough to send the reflection below the
            // horizon has no mirrored partner above it.
            //
            // This is the ordinary microfacet horizon loss rather than anything
            // this lobe introduces, and it was measured rather than assumed:
            // against the specular lobe at matched roughness it rejects 2.5
            // against 2.5 percent at 0.4, 11.1 against 11.5 at 0.6, 28.0
            // against 29.1 at 0.8 and 48.2 against 50.0 at 1.0. It tracks the
            // reflection lobe within two points everywhere and is always
            // slightly the lower of the two, because the mirror maps a
            // marginally different set of micronormals below the horizon. A
            // better thin sampler would have to beat the specular lobe, not
            // this one.
            return out;
        }
        out.wi = vec3f(wr.xy, -wr.z);
        out.valid = true;
        return out;
    }

    let wi = refract(normalize(-wo), out.h, surf.eta);
    if dot(wi, wi) < BSDF_EPSILON {
        // Total internal reflection at the micro scale. Rejected rather than
        // reflected: a reflected sample would land on the far side of the
        // surface from this lobe, and the mixture density would then owe a term
        // for it whose support is the set of micronormals that turn back for
        // this direction pair, which is not cheaply writable. Rejecting loses
        // energy rather than creating it, so the furnace ceiling still holds and
        // the loss prints as a deficit. It is unreachable on a front face, where
        // the index ratio is below one and the discriminant cannot go negative.
        return out;
    }
    out.wi = normalize(wi);
    out.valid = true;
    return out;
}

// The micronormal a transmitted pair went through, recovered from the pair.
//
// Both entry points reach the transmission value and the transmission density
// through this, so a sampled density and an evaluated one agree by construction
// rather than by inspection. That is the same guarantee the half-vector-free
// lobe weights give on the selection side.
//
// Solid: from `refract`, `wi = eta * (-wo) + (eta * dot(wo, h) - cos_t) * h`, so
// `wi + eta * wo` is parallel to `h` and `half_vector_eta` recovers it exactly.
// Thin: the mirrored direction is the reflection about `h`, so the ordinary half
// vector of the outgoing and mirrored directions is `h` by definition.
fn transmission_half(wo: vec3f, wi: vec3f, surf: Surface) -> vec3f {
    if transmission_is_thin(surf) {
        return half_vector(wo, vec3f(wi.xy, -wi.z));
    }
    return half_vector_eta(wi, wo, surf.eta);
}

// The transmission lobe's response: a real BTDF rather than a constant.
//
// Walter et al., equation (21), with the radiance-compression factor for
// camera-side transport. The value and the density below are one change and
// cannot be separated: a real density against a constant value collapses as
// roughness falls, which is the black glass this lobe was reverted for once
// already.
//
// The `eta * eta` is radiance compression across a refracting interface, and it
// is worth stating because its number looks alarming. Entering a denser medium
// it is about 0.44 and leaving it is about 2.25, so a round trip through a solid
// is exactly one. A furnace sweep measures a single crossing, so its transmissive
// planes read around 0.44 at normal incidence, and that is the physics rather
// than an energy deficit.
//
// `wi` is z-mirrored into `ggx_smith_visibility` because that helper floors the
// incident cosine at `MIN_INCIDENT_COS`, so a genuinely negative `wi.z` would
// silently return the wrong masking term. The factor of four recovers
// `G2 / (|wo.z| |wi.z|)` from a helper that carries `G2 / (4 |wo.z| |wi.z|)`.
fn transmission_btdf(wo: vec3f, wi: vec3f, h: vec3f, surf: Surface) -> vec3f {
    // Metalness gates transmission, because a conductor does not transmit.
    //
    // This is the third place that rule is written and the last to learn it.
    // `get_lobe_weights` has always folded metalness into the reflection share,
    // and the shadow walk computes its survival as one minus metalness times
    // transmission. The value did not, which was invisible while it was a
    // constant paired with a weight rather than a density, and became energy
    // creation the moment it became a real response: at a metalness of 0.2 the
    // transmitted branch returned its full 0.96 while the reflected branch
    // returned 0.232, and the furnace read 1.19. Gated, the two sum to one.
    let tint = surf.transmission * (1.0 - surf.metalness) * surf.color;
    let fresnel = evaluate_fresnel(abs(dot(wo, h)), surf.eta, vec3f(surf.f0), vec3f(1.0));
    let survives = vec3f(1.0) - fresnel;

    if transmission_is_thin(surf) {
        // The thin case is the specular lobe evaluated at the mirrored
        // direction, which is why it is admissible to the histogram comparison
        // by construction rather than by argument.
        return tint * survives * specular_brdf(wo, vec3f(wi.xy, -wi.z), h, surf.alpha);
    }

    let l_dot_h = dot(wi, h);
    let v_dot_h = dot(wo, h);
    let denom = l_dot_h + surf.eta * v_dot_h;
    let jacobian = abs(l_dot_h * v_dot_h) / max(BSDF_EPSILON, denom * denom);
    let d = ggx_distribution(h, surf.alpha);
    let g_over_cos = 4.0 * ggx_smith_visibility(wo, vec3f(wi.xy, abs(wi.z)), surf.alpha);
    return tint * survives * d * g_over_cos * jacobian * surf.eta * surf.eta;
}

// The density of `transmission_direction`, over the transmitted direction.
//
// A real probability density, which is what this was not. It used to return the
// reciprocal of what survives Fresnel reflection, a weight above one standing
// where a density belongs, and the disagreement between that and the sampler is
// why this lobe carried a histogram exemption.
//
// Solid: the visible-normal density over half vectors times the Jacobian of
// refraction, Walter et al. equation (17), in this file's convention where
// `surf.eta` is the incident index over the transmitted one.
// Thin: the reflection density, exactly, because mirroring preserves solid angle.
fn transmission_pdf(wo: vec3f, wi: vec3f, h: vec3f, surf: Surface) -> f32 {
    if transmission_is_thin(surf) {
        return ggx_reflection_adjusted_pdf(wo, h, surf.alpha);
    }
    let l_dot_h = dot(wi, h);
    let v_dot_h = dot(wo, h);
    let denom = l_dot_h + surf.eta * v_dot_h;
    let jacobian = abs(l_dot_h) / max(BSDF_EPSILON, denom * denom);
    return ggx_vndf_half_pdf(wo, h, surf.alpha) * jacobian;
}

// Beer-Lambert attenuation through a volume, glTF's `KHR_materials_volume`.
//
// A distance of zero is the specification's infinite default and means no
// attenuation, which is the convention the material record documents. The clamp
// on the colour guards the logarithm: a perfectly black attenuation colour is
// infinite absorption, and infinity times zero distance is NaN rather than one.
fn transmission_attenuation(dist: f32, att_color: vec3f, att_dist: f32) -> vec3f {
    if att_dist <= 0.0 {
        return vec3f(1.0);
    }
    let ot = -log(max(att_color, vec3f(1e-6))) / att_dist;
    return exp(-ot * dist);
}

// How likely each lobe is to be sampled.
//
// The Fresnel estimate steers the split: a grazing dielectric is mostly specular,
// a metal is entirely so. The specular share is deliberately floored at a quarter
// so a face-on dielectric still sends enough samples into its highlight to
// resolve it.
//
// Three departures from the source. Two are because transmission is live here
// and is not there: its transmission slot is computed and then discarded, since
// the selection distribution zeroes that entry, and here it is kept; and the
// normalization is extended to cover it, so transmission shares what the
// clearcoat leaves rather than sitting outside the distribution unnormalized,
// which is what the source's own commented-out line was reaching for.
//
// The third matters more. **There is no half-vector parameter.** The source takes
// one, and then its two entry points pass different things: the sampler passes the
// macronormal, because no half vector exists until a direction has been drawn,
// while the evaluator passes the real one. The result is that the same surface and
// the same direction pair get two different selection distributions depending on
// which way you came in, so a sampled density and an evaluated density disagree by
// construction. Removing the parameter fixes it in the direction that was already
// correct: these weights describe how a lobe came to be *chosen*, and that
// happened before there was a half vector to consult. The cosine is `wo.z`, which
// is what `dot(macronormal, wo)` was computing.
fn get_lobe_weights(wo: vec3f, wo_clearcoat: vec3f, surf: Surface) -> LobeWeights {
    let f_estimate = evaluate_fresnel(wo.z, surf.eta, vec3f(surf.f0), vec3f(1.0)).x;

    // Fresnel decides the split, with a floor that exists for one numerical
    // reason and no aesthetic one. At an index of exactly one the Fresnel term
    // is exactly zero, a lobe with no selection probability contributes no term
    // to the mixture density, and `direct_light` then rejects the connection
    // outright and the surface goes black to every light. One percent is small
    // enough that the split still follows Fresnel and large enough that the
    // density is positive wherever the response is.
    //
    // This used to floor at 0.25, where a dielectric at normal incidence wants
    // about 0.04, so the reflection lobe was oversampled sixfold and the
    // transmission lobe starved by the same factor. The estimator was unbiased
    // either way, because the mixture density agreed with the weights; what it
    // cost was variance, all of it on the transmitted result.
    //
    // The upper end already handles total internal reflection and must keep
    // doing so: past the critical angle `evaluate_fresnel` returns one, so this
    // is one, the transmission weight is exactly zero, and the density's own
    // guard skips its term.
    let trans_specular_prob = clamp(
        mix(max(TRANS_SPECULAR_FLOOR, f_estimate), 1.0, surf.metalness) + BSDF_TRANSMISSION_BIAS,
        TRANS_SPECULAR_FLOOR,
        1.0,
    );
    let diff_specular_prob = 0.5 + 0.5 * surf.metalness;

    var w: LobeWeights;
    w.diffuse = (1.0 - surf.transmission) * (1.0 - diff_specular_prob);
    w.specular = surf.transmission * trans_specular_prob
        + (1.0 - surf.transmission) * diff_specular_prob;
    w.transmission = surf.transmission * (1.0 - trans_specular_prob);

    let clearcoat_f0 = ior_to_f0(CLEARCOAT_IOR);
    w.clearcoat = surf.clearcoat * schlick_fresnel(saturate(wo_clearcoat.z), clearcoat_f0);

    let body = w.diffuse + w.specular + w.transmission;
    if body > 0.0 {
        let scale = (1.0 - w.clearcoat) / body;
        w.diffuse *= scale;
        w.specular *= scale;
        w.transmission *= scale;
    }

    // The source has no guard here and relies on the diffuse share never being
    // able to reach zero, which stops being true the moment the split constants
    // are touched. A distribution with no mass is a division by zero at the
    // selection step, so it collapses to diffuse instead.
    if w.diffuse + w.specular + w.transmission + w.clearcoat <= 0.0 {
        w.diffuse = 1.0;
    }
    return w;
}

// The layered response for one direction pair, and its mixture density.
//
// The composition order is glTF's operator chain and is reproduced exactly,
// because the order IS the result: diffuse and specular combined by the
// dielectric Fresnel, the iridescent film over that, the conductor path in
// parallel, the two mixed by metalness, sheen scaling what is beneath it and
// adding its own lobe, and the clearcoat's Fresnel over all of it.
fn bsdf_evaluate(ctx: BxdfContext, surf: Surface, w: LobeWeights) -> BsdfEval {
    var out: BsdfEval;
    out.color = vec3f(0.0);
    out.pdf = 0.0;

    let n_dot_v = ctx.v.z;
    let n_dot_l = ctx.l.z;
    let n_dot_vc = ctx.vc.z;

    // The transmitted micronormal, recovered once here and shared by the value
    // and the density, so the two cannot describe different geometry. `ctx.h`
    // keeps its meaning as the reflection half vector and the transmission
    // branch does not read it.
    //
    // The reconstruction normalizes a sum that can be near zero in degenerate
    // configurations, so it is guarded rather than allowed to produce a NaN that
    // would propagate into the image.
    var wh_t = vec3f(0.0, 0.0, 1.0);
    var transmits = n_dot_l < 0.0 && surf.transmission > 0.0;
    if transmits {
        let recovered = transmission_half(ctx.v, ctx.l, surf);
        if dot(recovered, recovered) > 0.5 {
            wh_t = recovered;
        } else {
            transmits = false;
        }
    }

    if n_dot_l > 0.0 {
        // Reflection.
        let specular = specular_brdf(ctx.v, ctx.l, ctx.h, surf.alpha);

        // The diffuse term loses what transmission takes and the specular term
        // does not, which is the source's behaviour and physically the right
        // one: a glass surface still reflects its highlight at full strength.
        // Applied to the diffuse input rather than to the combined dielectric,
        // because scaling the latter would dim the highlight with it.
        let diffuse = diffuse_brdf(n_dot_v, n_dot_l, ctx.v_dot_h, surf.color, surf.alpha.y)
            * (1.0 - surf.transmission);

        let dielectric_base = fresnel_mix(
            ctx.v_dot_h,
            surf.specular_color,
            surf.ior,
            surf.eta,
            surf.specular_intensity,
            diffuse,
            specular,
        );
        let dielectric = iridescent_dielectric_layer(
            dielectric_base,
            diffuse,
            specular,
            ctx.v_dot_h,
            1.0,
            surf.ior,
            surf.iridescence_ior,
            surf.iridescence_thickness,
            surf.iridescence,
        );

        let metallic_base = conductor_fresnel(ctx.v_dot_h, surf.color, specular);
        let metallic = iridescent_conductor_layer(
            metallic_base,
            specular,
            surf.color,
            ctx.v_dot_h,
            1.0,
            surf.iridescence_ior,
            surf.iridescence_thickness,
            surf.iridescence,
        );

        var material = mix(dielectric, metallic, surf.metalness);

        // Sheen carries no separate weight in this authoring model, so the
        // source's `mix(1, scaling, sheen)` is the scaling itself and a black
        // sheen colour makes it exactly one.
        material *= sheen_albedo_scaling(ctx.v, ctx.l, surf.sheen_color, surf.sheen_roughness);
        material += sheen_lobe(ctx.v, ctx.l, ctx.h, surf.sheen_color, surf.sheen_roughness);

        out.color = material;
    } else if surf.transmission > 0.0 && transmits {
        // Transmission. It replaces rather than adds, which is correct because
        // the two branches are mutually exclusive on the sign of `wi.z`.
        out.color = transmission_btdf(ctx.v, ctx.l, wh_t, surf);
    }

    // The clearcoat sits over whichever of the two happened, and reads the colour
    // beneath it, so it is a blend rather than an addition.
    if w.clearcoat > 0.0 && ctx.lc.z >= 0.0 {
        let clearcoat = specular_brdf(ctx.vc, ctx.lc, ctx.hc, vec2f(surf.clearcoat_alpha));
        out.color = fresnel_coat(
            max(n_dot_vc, MIN_INCIDENT_COS),
            CLEARCOAT_IOR,
            out.color,
            clearcoat,
            surf.clearcoat,
        );
    }

    // The mixture density: every lobe that could have produced this direction,
    // weighted by how likely it was to be chosen. This is what makes one sample
    // from a mixture unbiased, and it is why the density is not the sampled
    // lobe's own.
    if w.diffuse > 0.0 && n_dot_l > 0.0 {
        out.pdf += w.diffuse * n_dot_l / PI;
    }
    if w.specular > 0.0 && n_dot_l > 0.0 {
        out.pdf += w.specular * ggx_reflection_adjusted_pdf(ctx.v, ctx.h, surf.alpha);
    }
    if w.clearcoat > 0.0 && ctx.lc.z > 0.0 {
        out.pdf += w.clearcoat
            * ggx_reflection_adjusted_pdf(ctx.vc, ctx.hc, vec2f(surf.clearcoat_alpha));
    }
    if w.transmission > 0.0 && transmits {
        out.pdf += w.transmission * transmission_pdf(ctx.v, ctx.l, wh_t, surf);
    }

    // The cosine, folded exactly once, here rather than at either exit.
    //
    // Both exits used to do this themselves and agreed only by inspection. One
    // fold after the clearcoat blend is numerically identical to folding each
    // branch before it, because `fresnel_coat` is linear in both of its colour
    // arguments, and it cannot drift.
    //
    // It is correct for transmission as well as for reflection, which the issue
    // that raised this doubted for a good reason: while the transmitted value
    // was a constant rather than a distribution function it was not. Now that
    // the value carries `1 / (|wo.z| |wi.z|)` the way the reflection branch
    // always has, both want the same single cosine.
    out.color *= abs(ctx.l.z);

    return out;
}

// Draws one direction and evaluates the whole surface for it.
//
// Combined rather than split into a sampler and an evaluator, which is the
// source's shape and the right one for a mixture: the density that has to come
// back is the mixture's, so every lobe has to be evaluated anyway and splitting
// would evaluate them twice.
fn bsdf_sample(
    world_wo: vec3f,
    surf: Surface,
    rng: ptr<function, RngState>,
) -> ScatterRecord {
    var rec: ScatterRecord;
    rec.color = vec3f(0.0);
    rec.pdf = 0.0;
    rec.direction = vec3f(0.0, 0.0, 1.0);
    rec.lobe = LOBE_NONE;
    rec.transmissive = false;

    let wo = normalize(surf.inv_frame * world_wo);
    let wo_c = normalize(surf.inv_clearcoat_frame * world_wo);

    // Inherited from the source, including its consequence. A shading normal bent
    // by interpolation or by a normal map can face away from a ray that the
    // geometry faced, and there is no meaningful response to evaluate when it
    // does. The source notes this shows up on a smooth sphere and that
    // terminating here costs fireflies on a clearcoated surface; the alternative
    // it suggests is offsetting ray origins, which belongs with the accumulation
    // work rather than here.
    if wo.z < 0.0 || wo_c.z < 0.0 {
        return rec;
    }

    let w = get_lobe_weights(wo, wo_c, surf);

    var cdf: vec4f;
    cdf.x = w.diffuse;
    cdf.y = cdf.x + w.specular;
    cdf.z = cdf.y + w.transmission;
    cdf.w = cdf.z + w.clearcoat;
    if cdf.w <= 0.0 {
        return rec;
    }

    let r = rand1_strat(rng, RNG_DIM_LOBE) * cdf.w;
    let uv = rand2_strat(rng, RNG_DIM_DIRECTION);

    var wi: vec3f;
    var wh: vec3f;

    if r <= cdf.x {
        rec.lobe = LOBE_DIFFUSE;
        wi = diffuse_direction(uv);
        wh = half_vector(wi, wo);
    } else if r <= cdf.y {
        rec.lobe = LOBE_SPECULAR;
        wh = ggx_direction(wo, surf.alpha, uv);
        wi = -normalize(reflect(wo, wh));
    } else if r <= cdf.z {
        rec.lobe = LOBE_TRANSMISSION;
        // A micronormal that could not refract, or whose mirrored partner lies
        // below the horizon, yields no sample. That rejection is now part of the
        // sampler's distribution rather than a disagreement with the density:
        // the histogram comparison normalizes over every draw rather than over
        // the survivors, so an atom of rejection is a legal outcome.
        let t = transmission_direction(wo, surf, uv);
        if !t.valid {
            rec.lobe = LOBE_NONE;
            return rec;
        }
        wi = t.wi;
        // The micronormal it actually went through, rather than one recovered
        // from the pair. `bsdf_evaluate` recovers the same vector from the same
        // pair, which is what keeps the sampled and evaluated densities equal.
        wh = t.h;
    } else {
        // The clearcoat, and also the terminal branch the source does not have.
        // Without it a rounding error above `cdf.w` would leave the direction
        // uninitialized, which WGSL rejects outright and which would be undefined
        // behaviour if it did not.
        rec.lobe = LOBE_CLEARCOAT;
        let wh_c = ggx_direction(wo_c, vec2f(surf.clearcoat_alpha), uv);
        let wi_c = -normalize(reflect(wo_c, wh_c));
        wi = normalize(surf.inv_frame * (surf.clearcoat_frame * wi_c));
        wh = normalize(surf.inv_frame * (surf.clearcoat_frame * wh_c));
    }

    // Whether the sample crossed the interface, decided by where it went rather
    // than by which lobe sent it. The transmission branch used to claim this for
    // itself, but the specular lobe can also put a direction below the horizon on
    // a transmissive surface, and such a sample crossed without charging the
    // transmissive budget or entering the medium.
    rec.transmissive = wi.z < 0.0;

    var ctx: BxdfContext;
    ctx.v = wo;
    ctx.l = wi;
    ctx.h = wh;
    ctx.v_dot_h = saturate(dot(wo, wh));
    ctx.vc = wo_c;
    // Both frames are equal in this release, so these two products are the
    // identity; written out because the day a clearcoat normal is authored they
    // stop being, and a lobe should not have to be revisited then.
    ctx.lc = normalize(surf.inv_clearcoat_frame * (surf.frame * wi));
    ctx.hc = normalize(surf.inv_clearcoat_frame * (surf.frame * wh));

    // The cosine is already folded, inside `bsdf_evaluate`, so a caller only
    // ever divides by the density. It used to be folded here and again at the
    // other exit, agreeing only by inspection.
    let eval = bsdf_evaluate(ctx, surf, w);
    rec.pdf = eval.pdf;
    rec.color = eval.color;
    rec.direction = normalize(surf.frame * wi);
    return rec;
}

// The evaluate entry point: the response and the mixture density for a direction
// pair the caller already has, rather than one this drew.
//
// This is what light sampling will call, and it is the other half of what the
// histogram comparison needs: the density asserted at a direction has to come from
// somewhere other than the sampler that produced the direction, or the test is
// comparing the sampler to itself.
//
// It reconstructs the half vector the way the sampler's lobes would have, which is
// the plain form above the surface and the index-scaled one below it. Together with
// `get_lobe_weights` having no half-vector parameter, that is what makes this agree
// with `bsdf_sample` exactly rather than approximately.
fn bsdf_result(world_wo: vec3f, world_wi: vec3f, surf: Surface) -> BsdfEval {
    var out: BsdfEval;
    out.color = vec3f(0.0);
    out.pdf = 0.0;

    let wo = normalize(surf.inv_frame * world_wo);
    let wi = normalize(surf.inv_frame * world_wi);
    let wo_c = normalize(surf.inv_clearcoat_frame * world_wo);
    let wi_c = normalize(surf.inv_clearcoat_frame * world_wi);

    // Same degenerate-geometry rule as the sampler, so the two agree about which
    // configurations have no response at all.
    if wo.z < 0.0 || wo_c.z < 0.0 {
        return out;
    }

    let wh = half_vector_eta(wi, wo, surf.eta);
    let w = get_lobe_weights(wo, wo_c, surf);

    var ctx: BxdfContext;
    ctx.v = wo;
    ctx.l = wi;
    ctx.h = wh;
    ctx.v_dot_h = saturate(dot(wo, wh));
    ctx.vc = wo_c;
    ctx.lc = wi_c;
    ctx.hc = half_vector(wo_c, wi_c);

    let e = bsdf_evaluate(ctx, surf, w);
    out.color = e.color;
    out.pdf = e.pdf;
    return out;
}

// Path continuation probability, and the factor the throughput pays for it.
//
// Ported from the source's bounce loop rather than from its BSDF, because the
// WGSL implementation has no roulette at all: it terminates on the bounce budget
// and on a non-positive density and nothing else.
//
// Returns the multiplier to apply to throughput, or zero to terminate. The
// multiplier is capped, and the cap is a **bias**: it trades a small amount of
// correctness for suppressing the single bright sample that a long path
// occasionally produces. Stated rather than left to be discovered, because a
// biased estimator that is not labelled is how a renderer acquires a reputation
// for being subtly wrong.
const RR_MIN_BOUNCES: u32 = 3u;
const RR_THROUGHPUT_CAP: f32 = 20.0;

fn russian_roulette(
    throughput: vec3f,
    scatter_color: vec3f,
    pdf: f32,
    depth: u32,
    rng: ptr<function, RngState>,
) -> f32 {
    let current = luminance(throughput);
    if current <= 0.0 || pdf <= 0.0 {
        return 0.0;
    }
    // The square root softens the termination: a path whose contribution has
    // halved is not cut with probability one half.
    var p = sqrt(luminance(throughput * scatter_color / pdf) / current);
    // Short paths always survive, so the roulette adds variance only where the
    // path was going to contribute little anyway.
    p = max(p, select(0.0, 1.0, depth < RR_MIN_BOUNCES));
    p = min(p, 1.0);
    if rand1_strat(rng, RNG_DIM_ROULETTE) > p {
        return 0.0;
    }
    return min(1.0 / p, RR_THROUGHPUT_CAP);
}
