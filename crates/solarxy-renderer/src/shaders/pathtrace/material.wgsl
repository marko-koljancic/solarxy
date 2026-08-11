// The material fragment: a record and a texture coordinate in, the shading
// inputs a lobe reads out. Declares no entry point.
//
// Composed over the traversal, which declares `TracedMaterial` and the scene
// group, and over the atlas, which declares `sample_atlas`. This is that
// function's first real caller.
//
// Two contracts from below are honoured here rather than by the caller. An
// absent texture makes `sample_atlas` return transparent black, which is not a
// usable value for any channel, so every tap below is guarded by its
// descriptor's presence bit and falls back to the factor alone. And a texture's
// uv set is named by its own descriptor rather than assumed, which is what lets
// one material read a second uv channel without a second code path; the
// arranger writes channel zero today because that is the only set the arena
// packs.
//
// The record is passed by value. WGSL can take a pointer into the storage
// address space, but the value form is what every front end has always
// accepted, and the alternative to measure against is not a pointer but hoisting
// the whole fetch, which is a decision for the kernel rather than for this file.

// Slot order, matching `TextureRole::ALL` on the Rust side. Named rather than
// numbered at the call site, because the failure mode of a wrong index is a
// normal map sampled as a colour, which reads as a shading bug.
const MAT_SLOT_BASE_COLOR: u32 = 0u;
const MAT_SLOT_NORMAL: u32 = 1u;
const MAT_SLOT_METALLIC_ROUGHNESS: u32 = 2u;
const MAT_SLOT_OCCLUSION: u32 = 3u;
const MAT_SLOT_EMISSIVE: u32 = 4u;

// The alpha modes of `TracedMaterial.flags`, mirroring `solarxy_core::AlphaMode`.
const MAT_ALPHA_OPAQUE: u32 = 0u;
const MAT_ALPHA_MASK: u32 = 1u;
const MAT_ALPHA_BLEND: u32 = 2u;

// The surface a lobe shades: every factor with its texture already folded in.
struct MaterialSample {
    // Linear base colour in `rgb`, opacity in `a`.
    base_color: vec4f,
    // Emitted radiance, `emissive_strength` already applied.
    emissive: vec3f,
    metallic: f32,
    roughness: f32,
    // 1.0 is unoccluded. A tracer computes its own occlusion, so this is here
    // for parity with the viewport rather than for the light transport.
    occlusion: f32,
    // Tangent-space normal from the normal map, decoded to `-1..1`. Meaningless
    // unless `has_normal_map` is set, and applying it needs a tangent basis the
    // arena does not carry yet: `VertexAttr.tangent` is written zero.
    normal_ts: vec3f,
    has_normal_map: bool,
}

// Which of the two uv sets a descriptor reads. `uv_set` is `VertexAttr.uv`:
// uv0 in `xy`, uv1 in `zw`.
fn material_uv(uv_set: vec4f, desc: u32) -> vec2f {
    if tex_uv_channel(desc) == 0u {
        return uv_set.xy;
    }
    return uv_set.zw;
}

fn material_slot_present(m: TracedMaterial, slot: u32) -> bool {
    return tex_present(m.tex_desc[slot]);
}

// One slot's texel, or transparent black when the slot is empty.
fn material_tap(m: TracedMaterial, slot: u32, uv_set: vec4f) -> vec4f {
    let desc = m.tex_desc[slot];
    return sample_atlas(desc, m.tex_rect[slot], material_uv(uv_set, desc));
}

fn material_alpha_mode(m: TracedMaterial) -> u32 {
    return m.flags & MAT_ALPHA_MODE_MASK;
}

// The stylized shading models are carried and not honoured: this release traces
// every material as the principled surface. Read the field rather than assuming
// zero, so a later stage that does honour one has nothing to unpick.
fn material_shading_model(m: TracedMaterial) -> u32 {
    return (m.flags >> MAT_SHADING_MODEL_SHIFT) & MAT_SHADING_MODEL_MASK;
}

// Resolves every factor against its texture.
//
// The channel assignments are glTF's, which is what the importers write and what
// the raster uber-shader reads: base colour multiplies all four channels,
// metallic is the metallic-roughness map's blue and roughness its green,
// occlusion is the occlusion map's red scaled by its strength, and emissive
// multiplies the factor before the strength.
fn material_sample(m: TracedMaterial, uv_set: vec4f) -> MaterialSample {
    var s: MaterialSample;

    s.base_color = m.base_color;
    if material_slot_present(m, MAT_SLOT_BASE_COLOR) {
        s.base_color = s.base_color * material_tap(m, MAT_SLOT_BASE_COLOR, uv_set);
    }

    s.metallic = m.metallic;
    s.roughness = m.roughness;
    if material_slot_present(m, MAT_SLOT_METALLIC_ROUGHNESS) {
        let mr = material_tap(m, MAT_SLOT_METALLIC_ROUGHNESS, uv_set);
        s.metallic = s.metallic * mr.b;
        s.roughness = s.roughness * mr.g;
    }

    s.occlusion = 1.0;
    if material_slot_present(m, MAT_SLOT_OCCLUSION) {
        let ao = material_tap(m, MAT_SLOT_OCCLUSION, uv_set).r;
        s.occlusion = 1.0 + m.occlusion_strength * (ao - 1.0);
    }

    s.emissive = m.emissive;
    if material_slot_present(m, MAT_SLOT_EMISSIVE) {
        s.emissive = s.emissive * material_tap(m, MAT_SLOT_EMISSIVE, uv_set).rgb;
    }
    s.emissive = s.emissive * m.emissive_strength;

    s.normal_ts = vec3f(0.0, 0.0, 1.0);
    s.has_normal_map = material_slot_present(m, MAT_SLOT_NORMAL);
    if s.has_normal_map {
        let n = material_tap(m, MAT_SLOT_NORMAL, uv_set).rgb;
        s.normal_ts = normalize(n * 2.0 - vec3f(1.0));
    }

    return s;
}

// Whether the surface exists at this point at all.
//
// Blend passes here unconditionally: a tracer resolves partial coverage by
// letting a ray through with a probability rather than by blending, and that
// decision lives with each walk -- the integrator draws it from the alpha-test
// dimension, and the shadow walk charges the authored opacity
// deterministically. What is here is the test both can apply without a random
// number.
fn material_alpha_passes(m: TracedMaterial, s: MaterialSample) -> bool {
    if material_alpha_mode(m) == MAT_ALPHA_MASK {
        return s.base_color.a >= m.alpha_cutoff;
    }
    return true;
}
