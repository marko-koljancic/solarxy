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

@group(0) @binding(0) var scene_texture: texture_2d<f32>;
@group(0) @binding(1) var bloom_texture: texture_2d<f32>;
@group(0) @binding(2) var tex_sampler: sampler;
// The two colour-grading slots. Always bound: an empty slot binds an
// identity table rather than taking a second pipeline, so there are zero
// permutations. A disabled slot is skipped on the uniform flag below,
// which is uniform control flow and therefore legal around textureSample.
@group(0) @binding(3) var lut_a_texture: texture_3d<f32>;
@group(0) @binding(4) var lut_b_texture: texture_3d<f32>;
@group(0) @binding(5) var lut_sampler: sampler;

// Declared whole, field for field with the Rust struct in composite.rs.
// Every vec3 sits on a 16-byte boundary with a scalar of padding behind
// it, because the uniform address space aligns vec3 to 16 and Rust aligns
// [f32; 3] to 4. tests/uniform_layout.rs compares the two sides.
struct CompositeParams {
    bloom_strength: f32,
    bloom_enabled: u32,
    ssao_enabled: u32,
    ssao_strength: f32,

    tone_mode: u32,
    exposure: f32,
    inspection_mode: u32,
    _pad: u32,

    lut_a_enabled: u32,
    lut_a_strength: f32,
    lut_b_enabled: u32,
    lut_b_strength: f32,

    log_lo: f32,
    log_hi: f32,
    grade_enabled: u32,
    _pad_grade: f32,

    lut_a_scale: vec3<f32>,
    lut_a_bias: vec3<f32>,
    lut_b_scale: vec3<f32>,
    lut_b_bias: vec3<f32>,

    lift: vec3<f32>,
    gamma: vec3<f32>,
    gain: vec3<f32>,
}
@group(1) @binding(0) var<uniform> composite: CompositeParams;

@group(2) @binding(0) var ssao_texture: texture_2d<f32>;
@group(2) @binding(1) var ssao_sampler: sampler;

fn tone_none(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3(0.0), vec3(1.0));
}

fn tone_linear(c: vec3<f32>) -> vec3<f32> {
    return clamp(c, vec3(0.0), vec3(1.0));
}

fn tone_reinhard(c: vec3<f32>) -> vec3<f32> {
    return c / (c + vec3(1.0));
}

fn tone_aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3(0.0), vec3(1.0));
}

// Linear scene light into the 0..1 a lookup table is sampled on, through
// a log2 window. Slot A works in this space; slot B does not, because by
// then the value is already display-referred.
fn to_log(c: vec3<f32>) -> vec3<f32> {
    let floor_v = exp2(vec3(composite.log_lo));
    let l = log2(max(c, floor_v));
    return (l - vec3(composite.log_lo)) / (composite.log_hi - composite.log_lo);
}

fn sample_lut_a(c: vec3<f32>) -> vec3<f32> {
    let uvw = clamp(c * composite.lut_a_scale + composite.lut_a_bias, vec3(0.0), vec3(1.0));
    return textureSample(lut_a_texture, lut_sampler, uvw).rgb;
}

fn sample_lut_b(c: vec3<f32>) -> vec3<f32> {
    let uvw = clamp(c * composite.lut_b_scale + composite.lut_b_bias, vec3(0.0), vec3(1.0));
    return textureSample(lut_b_texture, lut_sampler, uvw).rgb;
}

// Lift, gamma, gain, in that reading order but this evaluation order:
// gain scales, lift raises the floor, gamma bends what is left.
//
// Guarded on a flag rather than relying on neutral values to cancel out,
// and that is not defensive coding. pow(x, 1.0) is not bit-identical to
// x: it compiles to exp2(1.0 * log2(x)) and comes back a unit or two of
// last place away. Leaving this unguarded would move every golden capture
// in the suite by a pixel value or two, for a feature that is supposed to
// be inert until someone turns it on.
fn grade(c: vec3<f32>) -> vec3<f32> {
    let scaled = max(c * composite.gain + composite.lift, vec3(0.0));
    // A gamma of zero would be a divide by zero; the parameter's own range
    // prevents it, and this makes the shader safe on its own terms.
    return pow(scaled, vec3(1.0) / max(composite.gamma, vec3(1e-4)));
}

@fragment
fn fs_composite(in: VertexOutput) -> @location(0) vec4<f32> {
    if composite.inspection_mode == 4u {
        let color = textureSample(scene_texture, tex_sampler, in.uv).rgb;
        return vec4<f32>(color, 1.0);
    }

    if composite.inspection_mode == 5u {
        if composite.ssao_enabled != 0u {
            let ao = textureSample(ssao_texture, ssao_sampler, in.uv).r;
            return vec4<f32>(ao, ao, ao, 1.0);
        }
        return vec4<f32>(1.0, 1.0, 1.0, 1.0);
    }

    var color = textureSample(scene_texture, tex_sampler, in.uv).rgb;
    if composite.bloom_enabled != 0u {
        let bloom_color = textureSample(bloom_texture, tex_sampler, in.uv).rgb;
        color = color + bloom_color * composite.bloom_strength;
    }
    if composite.ssao_enabled != 0u {
        let ao = textureSample(ssao_texture, ssao_sampler, in.uv).r;
        color = color * mix(1.0, ao, composite.ssao_strength);
    }
    color = color * composite.exposure;

    // Slot A: the tone-curve slot. Sampled here, on log-encoded linear
    // light, because a table that IS the tone curve has to see the scene
    // before the tone mapper does. Pair it with tone mode None, whose
    // clamp passes a display-referred result through untouched.
    if composite.lut_a_enabled != 0u {
        let graded_a = sample_lut_a(to_log(color));
        color = mix(color, graded_a, composite.lut_a_strength);
    }

    var mapped: vec3<f32>;
    switch composite.tone_mode {
        case 1u: { mapped = tone_linear(color); }
        case 2u: { mapped = tone_reinhard(color); }
        case 3u: { mapped = tone_aces(color); }
        default: { mapped = tone_none(color); }
    }

    // Slot B: the look slot. Sampled on the display-referred result, with
    // no shaper, which is what a LUT exported from a grading suite
    // expects to be fed.
    if composite.lut_b_enabled != 0u {
        let graded_b = sample_lut_b(mapped);
        mapped = mix(mapped, graded_b, composite.lut_b_strength);
    }

    if composite.grade_enabled != 0u {
        mapped = grade(mapped);
    }

    return vec4<f32>(mapped, 1.0);
}
