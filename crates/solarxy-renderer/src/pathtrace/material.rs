//! The traced material record: one material as the kernel reads it.
//!
//! One authoring model, two consumers. [`TracedMaterial`] is built from the
//! same [`RawMaterialData`] that produces the raster path's
//! [`MaterialUniform`](crate::material::MaterialUniform), so look development
//! transfers between the viewport and the render instead of restarting. A
//! tracer-only material model was rejected for exactly that reason: the two
//! would disagree about look indefinitely and nothing would notice.
//!
//! **There is no `wgpu` in this file**, for the same reason there is none in
//! [`super::scene`]: the record is built where the scene is packed, and that
//! pack has to be able to move into the import worker, which hosts a headless
//! wasm instance with no device at all.
//!
//! # Five texture slots, not seventeen
//!
//! [`RawMaterialData`] carries seventeen texture slots. This record carries the
//! five [`TextureRole`](super::scene::TextureRole) names, which are the five the
//! atlas packs. The other twelve modulate scalars that are all present here per
//! texel, so no lobe is unimplementable without them, and they are reachable
//! only from a glTF import: no node can author one and the raster path samples
//! none of them.
//!
//! Five is also what holds the record at 256 bytes. Seventeen would be roughly
//! 512 and would grow the role enum, the atlas arrangement, and the packer's
//! tests. Growing later is cheap and deliberately so: this record is rebuilt
//! from [`RawMaterialData`] on every pack and is persisted nowhere, so there is
//! no stored layout to stay compatible with. Widening it is two array lengths
//! and twelve enum arms.
//!
//! # Why the offsets are what they are
//!
//! Rust aligns `[f32; 4]` to 4 and WGSL aligns `vec4<f32>` to 16, so the two
//! agree only if every vector lands on a 16-byte boundary by construction. That
//! is what the nine leading blocks are: each is sixteen bytes, each colour is a
//! `vec3` with a scalar filling the fourth slot behind it, and the two texture
//! arrays follow at 144 and 224. Get it wrong and the Rust size assert still
//! passes, the shader still compiles, and the image is quietly of the wrong
//! numbers, which is why `tests/uniform_layout.rs` measures the WGSL side and
//! `record_offsets_are_the_documented_ones` measures this one.

use bytemuck::{Pod, Zeroable};
use solarxy_core::geometry::RawMaterialData;

use super::atlas::TEXTURE_UNUSED;
use super::scene::MaterialTextures;

/// How many texture slots a record carries. See the module documentation for
/// why it is not seventeen.
pub const TEXTURE_SLOTS: usize = 5;

/// Bits 0 and 1 of [`TracedMaterial::flags`]: `solarxy_core::AlphaMode` as u32.
pub const FLAG_ALPHA_MODE_MASK: u32 = 0x3;

/// Bits 2 to 5 of [`TracedMaterial::flags`]: `solarxy_core::ShadingModel` as
/// u32, which spans 0 to 7 and so needs three of the four bits reserved for it.
///
/// The tracer shades every material as `Pbr` in this release. The field rides
/// along because the stylized models are a divergence to state rather than one
/// to discover: Matcap and Toon are view-dependent stylizations a path tracer
/// cannot represent, and Clay, `ClayDark`, Chrome and Silhouette are viewport
/// looks rather than surfaces.
pub const FLAG_SHADING_MODEL_SHIFT: u32 = 2;

/// The mask applied after [`FLAG_SHADING_MODEL_SHIFT`].
pub const FLAG_SHADING_MODEL_MASK: u32 = 0xF;

/// The albedo [`TracedMaterial::fallback`] carries, linear.
///
/// The raster path's synthesized default is a white base-colour factor times a
/// one-texel sRGB grey of 204 (`resources.rs`, `clay_default`). The tracer has
/// no such texture to bind, so the grey folds into the factor and the sRGB
/// decode is applied here rather than by a texture format:
/// `((204/255 + 0.055) / 1.055) ^ 2.4`. The two consumers therefore agree on
/// the effective albedo, which is the property that matters;
/// `the_fallback_albedo_is_the_raster_clay_decoded` recomputes it rather than
/// trusting this literal.
pub const FALLBACK_ALBEDO: f32 = 0.603_827_4;

/// One material, as the kernel's `materials` storage array holds it.
///
/// Indexed by `Instance::material_base`, in the global slot numbering
/// [`TraceSceneCache::material_slots`](super::scene::TraceSceneCache::material_slots)
/// defines.
///
/// The layout is fixed and appending is the only safe change; see the module
/// documentation for why. Offsets are stated per field because a transposition
/// of two same-sized blocks passes every automatic check there is.
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, Pod, Zeroable)]
pub struct TracedMaterial {
    /// 0: base colour and opacity, glTF `baseColorFactor` semantics. Multiplied
    /// by the base-colour tap when there is one.
    pub base_color: [f32; 4],

    /// 16: emitted radiance before [`Self::emissive_strength`].
    pub emissive: [f32; 3],
    /// 28: multiplies [`Self::emissive`], letting emission exceed unit range.
    pub emissive_strength: f32,

    /// 32: what transmitted light becomes over
    /// [`Self::attenuation_distance`]. White is no tint.
    pub attenuation_color: [f32; 3],
    /// 44: distance at which transmitted light reaches
    /// [`Self::attenuation_color`]. **Zero means no attenuation**, standing in
    /// for the specification's infinite default, exactly as
    /// [`RawMaterialData::attenuation_distance`] documents.
    pub attenuation_distance: f32,

    /// 48: colour of the retroreflective sheen lobe. Black is no sheen.
    pub sheen_color: [f32; 3],
    /// 60: roughness of the sheen lobe.
    pub sheen_roughness: f32,

    /// 64: tints the dielectric reflectance at normal incidence.
    pub specular_color: [f32; 3],
    /// 76: scales the dielectric reflectance derived from [`Self::ior`].
    pub specular_intensity: f32,

    /// 80
    pub metallic: f32,
    /// 84
    pub roughness: f32,
    /// 88: index of refraction of the base dielectric.
    pub ior: f32,
    /// 92: how much light passes through rather than reflecting diffusely.
    pub transmission: f32,

    /// 96: distance through the volume in world units. Zero is thin-walled.
    pub thickness: f32,
    /// 100
    pub clearcoat: f32,
    /// 104
    pub clearcoat_roughness: f32,
    /// 108: how far the specular highlight stretches along the tangent.
    pub anisotropy: f32,

    /// 112: rotation of the anisotropy direction in the tangent plane, radians.
    pub anisotropy_rotation: f32,
    /// 116: strength of the thin-film interference effect.
    pub iridescence: f32,
    /// 120: index of refraction of the thin film.
    pub iridescence_ior: f32,
    /// 124: film thickness in nanometres at the low end of the range.
    pub iridescence_thickness_min: f32,

    /// 128: film thickness in nanometres at the high end, and the thickness
    /// used when no thickness map drives it.
    pub iridescence_thickness_max: f32,
    /// 132: how far the occlusion tap pulls ambient down.
    pub occlusion_strength: f32,
    /// 136: alpha-test cutoff, read when the alpha mode is `Mask`.
    pub alpha_cutoff: f32,
    /// 140: [`FLAG_ALPHA_MODE_MASK`] and [`FLAG_SHADING_MODEL_SHIFT`]. Bits 6
    /// and above are reserved.
    pub flags: u32,

    /// 144: per slot, `(u_scale, v_scale, u_offset, v_offset)` page-normalized,
    /// as `AtlasPlan::rect` produced it. In
    /// [`TextureRole::ALL`](super::scene::TextureRole::ALL) order.
    pub tex_rect: [[f32; 4]; TEXTURE_SLOTS],
    /// 224: per slot, the packed descriptor or [`TEXTURE_UNUSED`]. Never left
    /// zeroed: a zero descriptor is a **legal** one naming atlas layer zero,
    /// which is the trap
    /// [`TextureSlot::default`](super::scene::TextureSlot::default) documents.
    pub tex_desc: [u32; TEXTURE_SLOTS],

    /// 244: reserved. Not free space for a later stage to raid: the record is
    /// at its 256-byte target and the next field grows it to 272 rather than
    /// spending these.
    pub _pad: [u32; 3],
}

const _: () = assert!(std::mem::size_of::<TracedMaterial>() == 256);

impl TracedMaterial {
    /// Builds the record from the material the viewport shades and the texture
    /// slots the packer arranged.
    ///
    /// The texture half is copied rather than recomputed: the packer owns the
    /// layer number and the rectangle because it owns the arrangement, so
    /// deriving them twice would be two chances to disagree.
    #[must_use]
    pub fn from_raw(mat: &RawMaterialData, textures: &MaterialTextures) -> Self {
        let mut record = Self {
            base_color: mat.base_color_factor,
            emissive: mat.emissive_factor,
            emissive_strength: mat.emissive_strength,
            attenuation_color: mat.attenuation_color,
            attenuation_distance: mat.attenuation_distance,
            sheen_color: mat.sheen_color,
            sheen_roughness: mat.sheen_roughness,
            specular_color: mat.specular_color,
            specular_intensity: mat.specular_intensity,
            metallic: mat.metallic_factor,
            roughness: mat.roughness_factor,
            ior: mat.ior,
            transmission: mat.transmission,
            thickness: mat.thickness,
            clearcoat: mat.clearcoat,
            clearcoat_roughness: mat.clearcoat_roughness,
            anisotropy: mat.anisotropy,
            anisotropy_rotation: mat.anisotropy_rotation,
            iridescence: mat.iridescence,
            iridescence_ior: mat.iridescence_ior,
            iridescence_thickness_min: mat.iridescence_thickness_min,
            iridescence_thickness_max: mat.iridescence_thickness_max,
            occlusion_strength: mat.occlusion_strength,
            alpha_cutoff: mat.alpha_cutoff,
            flags: pack_flags(mat),
            tex_rect: [[0.0; 4]; TEXTURE_SLOTS],
            tex_desc: [TEXTURE_UNUSED; TEXTURE_SLOTS],
            _pad: [0; 3],
        };
        record.write_textures(textures);
        record
    }

    /// The record a slot no material covers gets, and the one an empty scene
    /// uploads.
    ///
    /// It is the raster path's `clay_default` in traced form: see
    /// [`FALLBACK_ALBEDO`] for why the albedo is not one. This exists because
    /// the alternative is a zeroed record, and a zeroed record is not empty: it
    /// is black, mirror-smooth, and pointing all five texture slots at atlas
    /// layer zero.
    #[must_use]
    pub fn fallback() -> Self {
        Self {
            base_color: [FALLBACK_ALBEDO, FALLBACK_ALBEDO, FALLBACK_ALBEDO, 1.0],
            emissive: [0.0; 3],
            emissive_strength: 1.0,
            attenuation_color: [1.0; 3],
            attenuation_distance: 0.0,
            sheen_color: [0.0; 3],
            sheen_roughness: 0.0,
            specular_color: [1.0; 3],
            specular_intensity: 1.0,
            metallic: 0.0,
            roughness: 0.7,
            ior: 1.5,
            transmission: 0.0,
            thickness: 0.0,
            clearcoat: 0.0,
            clearcoat_roughness: 0.0,
            anisotropy: 0.0,
            anisotropy_rotation: 0.0,
            iridescence: 0.0,
            iridescence_ior: 1.3,
            iridescence_thickness_min: 100.0,
            iridescence_thickness_max: 400.0,
            occlusion_strength: 1.0,
            alpha_cutoff: 0.5,
            flags: 0,
            tex_rect: [[0.0; 4]; TEXTURE_SLOTS],
            tex_desc: [TEXTURE_UNUSED; TEXTURE_SLOTS],
            _pad: [0; 3],
        }
    }

    /// Overwrites the texture half from a packer arrangement.
    fn write_textures(&mut self, textures: &MaterialTextures) {
        for (i, slot) in textures.slots.iter().enumerate() {
            self.tex_rect[i] = slot.rect;
            self.tex_desc[i] = slot.desc;
        }
    }
}

/// Packs the two enums the kernel needs to branch on into one word.
fn pack_flags(mat: &RawMaterialData) -> u32 {
    let alpha = u32::from(mat.alpha_mode) & FLAG_ALPHA_MODE_MASK;
    let shading =
        (u32::from(mat.shading_model) & FLAG_SHADING_MODEL_MASK) << FLAG_SHADING_MODEL_SHIFT;
    alpha | shading
}

#[cfg(test)]
// Exact float equality throughout, and deliberately: every comparison below
// checks a copy against its source, so a value that is merely close is the
// failure being looked for rather than rounding to tolerate.
#[allow(clippy::float_cmp)]
mod tests {
    use std::mem::offset_of;

    use solarxy_core::geometry::{AlphaMode, ShadingModel};

    use super::super::scene::TextureSlot;
    use super::*;

    /// The layout the module documentation states, field by field.
    ///
    /// The size assert alone does not buy this: nine sixteen-byte blocks are
    /// nine chances to transpose two of them, and a transposition changes no
    /// size anywhere. `tests/uniform_layout.rs` pins the WGSL side to the same
    /// 256 and `tests/pathtrace_material.rs` reads a record back through the
    /// real binding, which is what makes the pair conclusive.
    #[test]
    fn record_offsets_are_the_documented_ones() {
        assert_eq!(offset_of!(TracedMaterial, base_color), 0);
        assert_eq!(offset_of!(TracedMaterial, emissive), 16);
        assert_eq!(offset_of!(TracedMaterial, emissive_strength), 28);
        assert_eq!(offset_of!(TracedMaterial, attenuation_color), 32);
        assert_eq!(offset_of!(TracedMaterial, attenuation_distance), 44);
        assert_eq!(offset_of!(TracedMaterial, sheen_color), 48);
        assert_eq!(offset_of!(TracedMaterial, sheen_roughness), 60);
        assert_eq!(offset_of!(TracedMaterial, specular_color), 64);
        assert_eq!(offset_of!(TracedMaterial, specular_intensity), 76);
        assert_eq!(offset_of!(TracedMaterial, metallic), 80);
        assert_eq!(offset_of!(TracedMaterial, roughness), 84);
        assert_eq!(offset_of!(TracedMaterial, ior), 88);
        assert_eq!(offset_of!(TracedMaterial, transmission), 92);
        assert_eq!(offset_of!(TracedMaterial, thickness), 96);
        assert_eq!(offset_of!(TracedMaterial, clearcoat), 100);
        assert_eq!(offset_of!(TracedMaterial, clearcoat_roughness), 104);
        assert_eq!(offset_of!(TracedMaterial, anisotropy), 108);
        assert_eq!(offset_of!(TracedMaterial, anisotropy_rotation), 112);
        assert_eq!(offset_of!(TracedMaterial, iridescence), 116);
        assert_eq!(offset_of!(TracedMaterial, iridescence_ior), 120);
        assert_eq!(offset_of!(TracedMaterial, iridescence_thickness_min), 124);
        assert_eq!(offset_of!(TracedMaterial, iridescence_thickness_max), 128);
        assert_eq!(offset_of!(TracedMaterial, occlusion_strength), 132);
        assert_eq!(offset_of!(TracedMaterial, alpha_cutoff), 136);
        assert_eq!(offset_of!(TracedMaterial, flags), 140);
        assert_eq!(offset_of!(TracedMaterial, tex_rect), 144);
        assert_eq!(offset_of!(TracedMaterial, tex_desc), 224);
        assert_eq!(offset_of!(TracedMaterial, _pad), 244);
    }

    /// Every vector sits where WGSL will expect one.
    ///
    /// The failure this catches is the one the module documentation names: Rust
    /// aligns `[f32; 4]` to 4 and would happily place a colour at offset 12.
    #[test]
    fn every_vector_lands_on_a_sixteen_byte_boundary() {
        for offset in [
            offset_of!(TracedMaterial, base_color),
            offset_of!(TracedMaterial, emissive),
            offset_of!(TracedMaterial, attenuation_color),
            offset_of!(TracedMaterial, sheen_color),
            offset_of!(TracedMaterial, specular_color),
            offset_of!(TracedMaterial, tex_rect),
        ] {
            assert_eq!(offset % 16, 0, "offset {offset} is not vec4-aligned");
        }
    }

    /// Every scalar the raster consumer reads is carried here too.
    ///
    /// Not a layout test: a guard on the one-model claim. A field added to
    /// `RawMaterialData` and wired into `MaterialUniform::from_material` but
    /// not into `from_raw` is a viewport that shades it and a render that does
    /// not, which reads as a tracer bug.
    #[test]
    fn every_principled_scalar_survives_the_build() {
        let mat = populated();
        let record = TracedMaterial::from_raw(&mat, &MaterialTextures::default());

        assert_eq!(record.base_color, mat.base_color_factor);
        assert_eq!(record.emissive, mat.emissive_factor);
        assert_eq!(record.emissive_strength, mat.emissive_strength);
        assert_eq!(record.attenuation_color, mat.attenuation_color);
        assert_eq!(record.attenuation_distance, mat.attenuation_distance);
        assert_eq!(record.sheen_color, mat.sheen_color);
        assert_eq!(record.sheen_roughness, mat.sheen_roughness);
        assert_eq!(record.specular_color, mat.specular_color);
        assert_eq!(record.specular_intensity, mat.specular_intensity);
        assert_eq!(record.metallic, mat.metallic_factor);
        assert_eq!(record.roughness, mat.roughness_factor);
        assert_eq!(record.ior, mat.ior);
        assert_eq!(record.transmission, mat.transmission);
        assert_eq!(record.thickness, mat.thickness);
        assert_eq!(record.clearcoat, mat.clearcoat);
        assert_eq!(record.clearcoat_roughness, mat.clearcoat_roughness);
        assert_eq!(record.anisotropy, mat.anisotropy);
        assert_eq!(record.anisotropy_rotation, mat.anisotropy_rotation);
        assert_eq!(record.iridescence, mat.iridescence);
        assert_eq!(record.iridescence_ior, mat.iridescence_ior);
        assert_eq!(
            record.iridescence_thickness_min,
            mat.iridescence_thickness_min
        );
        assert_eq!(
            record.iridescence_thickness_max,
            mat.iridescence_thickness_max
        );
        assert_eq!(record.occlusion_strength, mat.occlusion_strength);
        assert_eq!(record.alpha_cutoff, mat.alpha_cutoff);
    }

    #[test]
    fn a_material_with_no_textures_writes_five_unused_descriptors() {
        let record = TracedMaterial::from_raw(&populated(), &MaterialTextures::default());
        assert_eq!(record.tex_desc, [TEXTURE_UNUSED; TEXTURE_SLOTS]);
        assert_eq!(record.tex_rect, [[0.0; 4]; TEXTURE_SLOTS]);
    }

    #[test]
    fn the_texture_half_is_copied_slot_for_slot() {
        let mut textures = MaterialTextures::default();
        for (i, slot) in textures.slots.iter_mut().enumerate() {
            let n = i as f32;
            *slot = TextureSlot {
                desc: 0x40_00 | i as u32,
                rect: [0.5, 0.25, n, n + 1.0],
            };
        }
        let record = TracedMaterial::from_raw(&populated(), &textures);

        for i in 0..TEXTURE_SLOTS {
            assert_eq!(record.tex_desc[i], textures.slots[i].desc);
            assert_eq!(record.tex_rect[i], textures.slots[i].rect);
        }
    }

    #[test]
    fn the_flags_word_carries_both_enums() {
        let mut mat = populated();
        mat.alpha_mode = AlphaMode::Mask;
        mat.shading_model = ShadingModel::Silhouette;
        let flags = TracedMaterial::from_raw(&mat, &MaterialTextures::default()).flags;

        assert_eq!(flags & FLAG_ALPHA_MODE_MASK, 1);
        assert_eq!(
            (flags >> FLAG_SHADING_MODEL_SHIFT) & FLAG_SHADING_MODEL_MASK,
            7
        );
        assert_eq!(flags >> 6, 0, "bits 6 and above are reserved");
    }

    /// The widest enum pair still fits the bits allotted.
    #[test]
    fn no_enum_value_overflows_its_field() {
        for model in [ShadingModel::Pbr, ShadingModel::Silhouette] {
            for mode in [AlphaMode::Opaque, AlphaMode::Blend] {
                let mut mat = populated();
                mat.alpha_mode = mode;
                mat.shading_model = model;
                let flags = TracedMaterial::from_raw(&mat, &MaterialTextures::default()).flags;
                assert_eq!(flags & FLAG_ALPHA_MODE_MASK, u32::from(mode));
                assert_eq!(
                    (flags >> FLAG_SHADING_MODEL_SHIFT) & FLAG_SHADING_MODEL_MASK,
                    u32::from(model)
                );
            }
        }
    }

    #[test]
    fn the_fallback_writes_unused_descriptors_and_a_usable_surface() {
        let record = TracedMaterial::fallback();
        assert_eq!(record.tex_desc, [TEXTURE_UNUSED; TEXTURE_SLOTS]);
        assert!(record.roughness > 0.0, "a mirror is not a sane default");
        assert!(record.base_color[3] > 0.0, "an invisible default is worse");
        assert_eq!(record.ior, 1.5);
        assert_eq!(record.occlusion_strength, 1.0);
    }

    /// The fallback albedo is the raster clay, decoded.
    ///
    /// Recomputed from the byte rather than compared against a second literal,
    /// so the constant cannot drift away from what it claims to be.
    #[test]
    fn the_fallback_albedo_is_the_raster_clay_decoded() {
        let encoded = 204.0_f32 / 255.0;
        let linear = ((encoded + 0.055) / 1.055).powf(2.4);
        assert!(
            (FALLBACK_ALBEDO - linear).abs() < 1e-6,
            "FALLBACK_ALBEDO is {FALLBACK_ALBEDO}, the decode is {linear}"
        );
    }

    /// A zeroed record is legal bytes and a wrong material, which is the whole
    /// reason [`TracedMaterial::fallback`] exists.
    #[test]
    fn a_zeroed_record_points_every_slot_at_layer_zero() {
        let zeroed: TracedMaterial = bytemuck::Zeroable::zeroed();
        for desc in zeroed.tex_desc {
            assert_eq!(desc & TEXTURE_UNUSED, 0, "zero reads as a present texture");
        }
    }

    /// A material with every principled property set away from its default, so
    /// a field that fails to survive the build shows as a mismatch rather than
    /// as two zeroes agreeing.
    fn populated() -> RawMaterialData {
        RawMaterialData {
            base_color_factor: [0.1, 0.2, 0.3, 0.4],
            emissive_factor: [0.5, 0.6, 0.7],
            emissive_strength: 2.5,
            attenuation_color: [0.8, 0.9, 0.11],
            attenuation_distance: 3.5,
            sheen_color: [0.12, 0.13, 0.14],
            sheen_roughness: 0.15,
            specular_color: [0.16, 0.17, 0.18],
            specular_intensity: 0.19,
            metallic_factor: 0.21,
            roughness_factor: 0.22,
            ior: 1.61,
            transmission: 0.23,
            thickness: 0.24,
            clearcoat: 0.25,
            clearcoat_roughness: 0.26,
            anisotropy: 0.27,
            anisotropy_rotation: 0.28,
            iridescence: 0.29,
            iridescence_ior: 1.31,
            iridescence_thickness_min: 120.0,
            iridescence_thickness_max: 430.0,
            occlusion_strength: 0.32,
            alpha_cutoff: 0.33,
            ..RawMaterialData::default()
        }
    }
}
