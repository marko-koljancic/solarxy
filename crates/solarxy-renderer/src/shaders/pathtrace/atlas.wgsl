// The texture atlas, as the kernel reads it.
//
// One paged array texture holds every texture in the scene. A material slot
// names its own sub-rectangle through two values: a bit-packed descriptor and
// a rectangle in page-normalized coordinates. Both are produced by the packer
// in `pathtrace/atlas.rs`, which owns this encoding; the two must move
// together, and `uniform_layout.rs` is not what holds them, because there is no
// struct here to measure. `pathtrace_shader_source.rs` pins the binding numbers.
//
// Three things are done here rather than by the sampler, and each is why the
// atlas can exist at all:
//
//   - Wrapping. Hardware address modes act on the whole texture, and every
//     sub-rectangle here is a fraction of one, so the coordinate is wrapped
//     into the unit square first and mapped into the rectangle after.
//   - Filtering. A descriptor carries a filter bit, and WGSL cannot index a
//     sampler, so both are bound and the branch picks one. Legal under
//     `textureSampleLevel`, which takes no derivative.
//   - The transfer function. The atlas is `rgba8unorm` throughout so one page
//     can hold a base-colour map beside a normal map; sRGB is decoded here from
//     a per-texture flag instead of being baked into a format.

// Descriptor bits. Mirrors `TextureDescriptor::pack`.
const TEX_LAYER_MASK: u32 = 0xFFu;
const TEX_UV_SHIFT: u32 = 8u;
const TEX_UV_MASK: u32 = 0x7u;
const TEX_WRAP_S_SHIFT: u32 = 11u;
const TEX_WRAP_T_SHIFT: u32 = 13u;
const TEX_WRAP_MASK: u32 = 0x3u;
const TEX_FILTER_BIT: u32 = 1u << 15u;
const TEX_SRGB_BIT: u32 = 1u << 16u;
/// The sign bit: this slot carries no texture. Note that zero is a legal
/// descriptor naming layer zero, so a slot is always written explicitly.
const TEX_UNUSED_BIT: u32 = 1u << 31u;

// Wrap codes, matching `AtlasWrap::bits`.
const WRAP_REPEAT: u32 = 0u;
const WRAP_CLAMP: u32 = 1u;
const WRAP_MIRROR: u32 = 2u;

@group(2) @binding(0) var atlas: texture_2d_array<f32>;
@group(2) @binding(1) var atlas_nearest: sampler;
@group(2) @binding(2) var atlas_linear: sampler;

/// Whether a descriptor names a texture at all.
fn tex_present(desc: u32) -> bool {
    return (desc & TEX_UNUSED_BIT) == 0u;
}

/// Which vertex uv set a descriptor reads.
fn tex_uv_channel(desc: u32) -> u32 {
    return (desc >> TEX_UV_SHIFT) & TEX_UV_MASK;
}

/// One coordinate brought into the unit square under a wrap code.
///
/// The mirror arm folds a period of two rather than calling `fract` twice: for
/// `m` in `[0, 2)`, values past one reflect, which is exactly the mirrored
/// repeat the hardware mode describes.
fn tex_wrap(x: f32, mode: u32) -> f32 {
    if mode == WRAP_CLAMP {
        return clamp(x, 0.0, 1.0);
    }
    if mode == WRAP_MIRROR {
        let m = x - 2.0 * floor(x * 0.5);
        return select(2.0 - m, m, m < 1.0);
    }
    return fract(x);
}

/// sRGB to linear, the exact piecewise transfer function rather than a 2.2
/// power, because the raster path decodes with the format's own conversion and
/// the two images have to agree.
fn tex_srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + vec3<f32>(0.055)) / 1.055, vec3<f32>(2.4));
    return select(hi, lo, c <= vec3<f32>(0.04045));
}

/// Samples one texture slot.
///
/// `rect` is `(u_scale, v_scale, u_offset, v_offset)` in page-normalized units,
/// which lands strictly inside the slot's guard ring, so a bilinear tap at
/// either extreme of the wrapped coordinate reads the border rather than the
/// neighbour packed beside it.
///
/// An absent texture returns transparent black. The caller decides what that
/// means for its channel, because the right fallback for a base-colour map (the
/// factor alone) is not the right fallback for a normal map (a flat normal).
fn sample_atlas(desc: u32, rect: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    if !tex_present(desc) {
        return vec4<f32>(0.0);
    }
    let wrapped = vec2<f32>(
        tex_wrap(uv.x, (desc >> TEX_WRAP_S_SHIFT) & TEX_WRAP_MASK),
        tex_wrap(uv.y, (desc >> TEX_WRAP_T_SHIFT) & TEX_WRAP_MASK),
    );
    let coord = wrapped * rect.xy + rect.zw;
    let layer = i32(desc & TEX_LAYER_MASK);

    var texel: vec4<f32>;
    if (desc & TEX_FILTER_BIT) != 0u {
        texel = textureSampleLevel(atlas, atlas_linear, coord, layer, 0.0);
    } else {
        texel = textureSampleLevel(atlas, atlas_nearest, coord, layer, 0.0);
    }
    if (desc & TEX_SRGB_BIT) != 0u {
        return vec4<f32>(tex_srgb_to_linear(texel.rgb), texel.a);
    }
    return texel;
}
