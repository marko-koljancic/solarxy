//! GPU-side PBR material: [`MaterialUniform`] (`#[repr(C)]`, 160 bytes) and
//! the bundle of textures + bind group consumed by the main shader.
//!
//! `MaterialUniform.alpha_mode` and `.shading_model` are `u32` for shader
//! binding; the CPU-side enums live in `solarxy_core::geometry`
//! (`AlphaMode`, `ShadingModel`), with the conversions at
//! `crate::resources`.

use wgpu::util::DeviceExt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlphaMode {
    #[default]
    Opaque = 0,
    Mask = 1,
    Blend = 2,
}

#[repr(C)]
#[derive(Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MaterialUniform {
    pub roughness_factor: f32,
    pub metallic_factor: f32,
    pub ao_strength: f32,
    pub alpha_cutoff: f32,
    pub emissive: [f32; 3],
    pub alpha_mode: u32,
    pub material_index: u32,
    /// `solarxy_core::geometry::ShadingModel` as u32: 0 Pbr,
    /// 1 Matcap, 2 Toon, 3 Unlit, 4 Clay, 5 `ClayDark`, 6 Chrome,
    /// 7 Silhouette. Occupies a former pad slot, so it cost no growth.
    ///
    /// On the prefix rule: a shader may declare a leading run of this
    /// struct and omit the rest, because wgpu checks size at the binding
    /// rather than shape. `shadow.wgsl` does exactly that, declaring
    /// through `alpha_mode` and no further, which naga lays out at 32
    /// bytes. Appending is therefore always safe and reordering never is.
    pub shading_model: u32,
    /// Toon band count (read only when `shading_model == 2`).
    pub toon_steps: f32,
    pub _pad: f32,
    /// Factor multiplied into the base-color sample (glTF semantics);
    /// appended at offset 48, after the prefix shadow.wgsl declares.
    pub base_color: [f32; 4],

    // ---- Principled surface properties, offsets 64 to 159 ----
    //
    // Six vec4-shaped blocks appended in one go. Grouping them four floats
    // at a time is not cosmetic: WGSL aligns a vec3 to 16 bytes in the
    // uniform address space, so each `[f32; 3]` here sits at a multiple of
    // 16 with a scalar filling the fourth slot behind it. Get that wrong
    // and the mismatch renders as a black viewport rather than an error,
    // which is what `tests/uniform_layout.rs` now checks this struct for.
    //
    // Every default is the identity of its effect, and `fs_main` branches
    // around each lobe on its own factor, so a material that sets none of
    // them takes exactly the arithmetic it took before they existed.
    /// Index of refraction of the base dielectric.
    pub ior: f32,
    /// How much light passes through rather than reflecting diffusely.
    pub transmission: f32,
    /// Distance through the volume, in world units. Zero is thin-walled.
    pub thickness: f32,
    /// Distance at which transmitted light reaches `attenuation_color`.
    /// Zero means no attenuation (see `RawMaterialData` for why not
    /// infinity).
    pub attenuation_distance: f32,
    /// The colour transmitted light becomes over `attenuation_distance`.
    pub attenuation_color: [f32; 3],
    /// Multiplies `emissive`, letting emission exceed the unit range.
    pub emissive_strength: f32,
    /// Strength of the clear-coat layer.
    pub clearcoat: f32,
    /// Roughness of the clear-coat layer, independent of the base.
    pub clearcoat_roughness: f32,
    /// How far the specular highlight stretches along the tangent.
    pub anisotropy: f32,
    /// Rotation of the anisotropy direction in the tangent plane, radians.
    pub anisotropy_rotation: f32,
    /// Colour of the retroreflective sheen lobe. Black is no sheen.
    pub sheen_color: [f32; 3],
    /// Roughness of the sheen lobe.
    pub sheen_roughness: f32,
    /// Tints the dielectric reflectance at normal incidence.
    pub specular_color: [f32; 3],
    /// Scales the dielectric reflectance derived from `ior`.
    pub specular_intensity: f32,
    /// Strength of the thin-film interference effect.
    pub iridescence: f32,
    /// Index of refraction of the thin film.
    pub iridescence_ior: f32,
    /// Film thickness in nanometres at the low end of the range.
    pub iridescence_thickness_min: f32,
    /// Film thickness in nanometres at the high end, and the thickness
    /// used when no thickness map drives it.
    pub iridescence_thickness_max: f32,
}

const _: () = assert!(std::mem::size_of::<MaterialUniform>() == 160);

impl MaterialUniform {
    /// Build the GPU record from a CPU material.
    ///
    /// Both upload paths route through here: the file importer's
    /// `upload_model` and the cooked-graph `upload_cooked_materials`. They
    /// previously carried identical struct literals that nothing kept in
    /// step, so a field added to one and forgotten in the other would have
    /// surfaced only as the same material shading differently depending on
    /// whether it arrived from a file or from the node graph.
    #[must_use]
    pub fn from_material(mat: &solarxy_core::RawMaterialData, material_index: u32) -> Self {
        Self {
            roughness_factor: mat.roughness_factor,
            metallic_factor: mat.metallic_factor,
            ao_strength: mat.occlusion_strength,
            alpha_cutoff: mat.alpha_cutoff,
            emissive: mat.emissive_factor,
            alpha_mode: mat.alpha_mode.into(),
            material_index,
            shading_model: mat.shading_model.into(),
            toon_steps: mat.toon_steps,
            _pad: 0.0,
            base_color: mat.base_color_factor,
            ior: mat.ior,
            transmission: mat.transmission,
            thickness: mat.thickness,
            attenuation_distance: mat.attenuation_distance,
            attenuation_color: mat.attenuation_color,
            emissive_strength: mat.emissive_strength,
            clearcoat: mat.clearcoat,
            clearcoat_roughness: mat.clearcoat_roughness,
            anisotropy: mat.anisotropy,
            anisotropy_rotation: mat.anisotropy_rotation,
            sheen_color: mat.sheen_color,
            sheen_roughness: mat.sheen_roughness,
            specular_color: mat.specular_color,
            specular_intensity: mat.specular_intensity,
            iridescence: mat.iridescence,
            iridescence_ior: mat.iridescence_ior,
            iridescence_thickness_min: mat.iridescence_thickness_min,
            iridescence_thickness_max: mat.iridescence_thickness_max,
        }
    }
}

impl Default for MaterialUniform {
    fn default() -> Self {
        Self {
            roughness_factor: 0.7,
            metallic_factor: 0.0,
            ao_strength: 1.0,
            alpha_cutoff: 0.5,
            emissive: [0.0, 0.0, 0.0],
            alpha_mode: 0,
            material_index: 0,
            shading_model: 0,
            toon_steps: 3.0,
            _pad: 0.0,
            base_color: [1.0, 1.0, 1.0, 1.0],
            ior: 1.5,
            transmission: 0.0,
            thickness: 0.0,
            attenuation_distance: 0.0,
            attenuation_color: [1.0, 1.0, 1.0],
            emissive_strength: 1.0,
            clearcoat: 0.0,
            clearcoat_roughness: 0.0,
            anisotropy: 0.0,
            anisotropy_rotation: 0.0,
            sheen_color: [0.0, 0.0, 0.0],
            sheen_roughness: 0.0,
            specular_color: [1.0, 1.0, 1.0],
            specular_intensity: 1.0,
            iridescence: 0.0,
            iridescence_ior: 1.3,
            iridescence_thickness_min: 100.0,
            iridescence_thickness_max: 400.0,
        }
    }
}

/// Textures are `Arc`-shared so the content-hash
/// [`TextureCache`](crate::resources::TextureCache) can hand the same GPU
/// texture to every material that references the same image.
#[allow(dead_code)]
pub struct Material {
    pub name: String,
    pub diffuse_texture: std::sync::Arc<super::texture::Texture>,
    pub normal_texture: std::sync::Arc<super::texture::Texture>,
    pub orm_texture: std::sync::Arc<super::texture::Texture>,
    pub emissive_texture: std::sync::Arc<super::texture::Texture>,
    pub uniform: MaterialUniform,
    pub uniform_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

impl Material {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        name: &str,
        diffuse_texture: std::sync::Arc<super::texture::Texture>,
        normal_texture: std::sync::Arc<super::texture::Texture>,
        orm_texture: std::sync::Arc<super::texture::Texture>,
        emissive_texture: std::sync::Arc<super::texture::Texture>,
        uniform: MaterialUniform,
        layout: &wgpu::BindGroupLayout,
    ) -> Self {
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{name}_material_uniform")),
            contents: bytemuck::cast_slice(&[uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&diffuse_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&diffuse_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&normal_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&normal_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&orm_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&orm_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&emissive_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&emissive_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
            label: Some(name),
        });

        Self {
            name: name.to_string(),
            diffuse_texture,
            normal_texture,
            orm_texture,
            emissive_texture,
            uniform,
            uniform_buffer,
            bind_group,
        }
    }
}
