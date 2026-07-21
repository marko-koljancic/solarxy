//! Loads CPU-side `solarxy_core::RawModelData` into GPU-side meshes,
//! materials, and textures.
//!
//! The single CPU↔GPU boundary for `AlphaMode`: the CPU enum
//! (`solarxy_core::AlphaMode`) is converted to `u32` here via
//! `From<AlphaMode> for u32` before being copied into [`crate::material::MaterialUniform`].

use std::path::Path;

use wgpu::util::DeviceExt;

use super::geometry::{self, RawImageData, RawMaterialData, RawModelData};
use super::{material, model, texture};
use crate::error::RendererError;
use crate::validation::ViewerValidation;

pub fn is_supported_model_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            solarxy_core::SUPPORTED_EXTENSIONS
                .iter()
                .any(|s| ext.eq_ignore_ascii_case(s))
        })
}

#[cfg(test)]
mod extension_tests {
    use super::is_supported_model_extension;
    use std::path::Path;

    #[test]
    fn known_extensions_accepted_case_insensitively() {
        for ext in ["obj", "stl", "ply", "gltf", "glb", "OBJ", "PLY"] {
            let name = format!("model.{ext}");
            assert!(
                is_supported_model_extension(Path::new(&name)),
                "{ext} should be accepted"
            );
        }
    }

    #[test]
    fn unknown_or_missing_extensions_rejected() {
        for ext in ["txt", "png", "rs", "json", "fbx"] {
            let name = format!("model.{ext}");
            assert!(
                !is_supported_model_extension(Path::new(&name)),
                "{ext} should be rejected"
            );
        }
        assert!(!is_supported_model_extension(Path::new("no_extension")));
    }
}

#[cfg(feature = "std-fs")]
pub fn load_model_any(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    edge_geometry_layout: &wgpu::BindGroupLayout,
) -> Result<
    (
        model::Model,
        model::NormalsGeometry,
        ModelStats,
        ViewerValidation,
    ),
    RendererError,
> {
    let raw = solarxy_formats::load_model(file_path)?;

    upload_model(raw, file_path, device, queue, layout, edge_geometry_layout)
}

#[derive(Clone, Copy)]
pub struct ModelStats {
    pub polys: usize,
    pub tris: usize,
    pub verts: usize,
}

/// Build a [`model::TextureThumbnail`] from one role's source slots and
/// move the bytes out so the raw material drops cheaply. Returns `None`
/// when the role has no source texture; the path is cloned so the
/// caller's GPU upload retains its own copy. Used by `upload_model`.
fn take_thumbnail(
    data: &mut Option<std::sync::Arc<solarxy_core::RawImageData>>,
    path: Option<&std::path::PathBuf>,
) -> Option<model::TextureThumbnail> {
    let image = data.take()?;
    Some(model::TextureThumbnail {
        image,
        source_path: path.cloned(),
    })
}

/// A content-addressed GPU texture cache keyed by `(RawImageData.hash,
/// linear)`. Owned by `SceneObjects` for the engine-driven path so a
/// material-node factor drag (which rebuilds the material but reuses the
/// same decoded image `Arc`s) re-uploads zero texture bytes, and identical
/// images across materials share one GPU texture. Entries are dropped by
/// [`TextureCache::sweep`] once no live material holds them.
#[derive(Default)]
pub struct TextureCache {
    entries: std::collections::HashMap<(u64, bool), std::sync::Arc<texture::Texture>>,
}

impl TextureCache {
    fn get_or_try_insert(
        &mut self,
        key: (u64, bool),
        build: impl FnOnce() -> Result<texture::Texture, RendererError>,
    ) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
        if let Some(hit) = self.entries.get(&key) {
            return Ok(std::sync::Arc::clone(hit));
        }
        let built = std::sync::Arc::new(build()?);
        self.entries.insert(key, std::sync::Arc::clone(&built));
        Ok(built)
    }

    /// Drops entries no longer referenced by any material (the cache holds
    /// the only remaining `Arc`). Called on object removal and clear.
    pub fn sweep(&mut self) {
        self.entries
            .retain(|_, t| std::sync::Arc::strong_count(t) > 1);
    }
}

/// Upload a CPU-side [`RawModelData`] into GPU meshes, materials, and
/// textures. The byte-level seam a node graph or web shell feeds directly;
/// `load_model_any` is the filesystem wrapper over it.
#[allow(clippy::unnecessary_wraps)]
pub fn upload_model(
    mut raw: RawModelData,
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    edge_geometry_layout: &wgpu::BindGroupLayout,
) -> Result<
    (
        model::Model,
        model::NormalsGeometry,
        ModelStats,
        ViewerValidation,
    ),
    RendererError,
> {
    let has_uvs = raw.meshes.iter().any(|m| m.tex_coords.is_some());

    let file_ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");
    let viewer_validation = crate::validation::validate_raw_model(&raw, file_ext);

    let (mesh_vertices, mesh_indices, bounds, per_mesh_bounds, normals_geo) =
        geometry::process_raw_model(&raw);
    let mut gpu_materials = Vec::new();
    let mut material_thumbnails: Vec<model::MaterialThumbnails> = Vec::new();
    for (mat_idx, mat) in raw.materials.iter_mut().enumerate() {
        let diffuse_texture = load_or_fallback_texture(
            device,
            queue,
            mat.diffuse_texture_data.as_deref(),
            mat.diffuse_texture_path.as_ref(),
            false,
            &mat.name,
            "diffuse",
            None,
        )?;
        let normal_texture = load_or_fallback_texture(
            device,
            queue,
            mat.normal_texture_data.as_deref(),
            mat.normal_texture_path.as_ref(),
            true,
            &mat.name,
            "normal",
            None,
        )?;
        let orm_texture = load_or_create_orm(device, queue, mat, None)?;
        let emissive_texture = load_or_fallback_texture(
            device,
            queue,
            mat.emissive_texture_data.as_deref(),
            mat.emissive_texture_path.as_ref(),
            false,
            &mat.name,
            "emissive",
            None,
        )?;

        let uniform = material::MaterialUniform {
            roughness_factor: mat.roughness_factor,
            metallic_factor: mat.metallic_factor,
            ao_strength: mat.occlusion_strength,
            alpha_cutoff: mat.alpha_cutoff,
            emissive: mat.emissive_factor,
            alpha_mode: mat.alpha_mode.into(),
            material_index: mat_idx as u32,
            shading_model: mat.shading_model.into(),
            toon_steps: mat.toon_steps,
            _pad: 0.0,
            base_color: mat.base_color_factor,
        };

        gpu_materials.push(material::Material::new(
            device,
            &mat.name,
            diffuse_texture,
            normal_texture,
            orm_texture,
            emissive_texture,
            uniform,
            layout,
        ));

        // CPU-side thumbnail cache for the Material Inspector. `take` moves
        // the decoded bytes out of the raw material (which is about to
        // drop anyway) into an Arc so the inspector can hold a cheap
        // reference; paths are cloned because both the GPU upload and the
        // cache need them.
        material_thumbnails.push(model::MaterialThumbnails {
            albedo: take_thumbnail(
                &mut mat.diffuse_texture_data,
                mat.diffuse_texture_path.as_ref(),
            ),
            normal: take_thumbnail(
                &mut mat.normal_texture_data,
                mat.normal_texture_path.as_ref(),
            ),
            metallic_roughness: take_thumbnail(
                &mut mat.metallic_roughness_texture_data,
                mat.metallic_roughness_texture_path.as_ref(),
            ),
            occlusion: take_thumbnail(
                &mut mat.occlusion_texture_data,
                mat.occlusion_texture_path.as_ref(),
            ),
            emissive: take_thumbnail(
                &mut mat.emissive_texture_data,
                mat.emissive_texture_path.as_ref(),
            ),
            base_color: [
                mat.base_color_factor[0],
                mat.base_color_factor[1],
                mat.base_color_factor[2],
            ],
        });
    }

    if gpu_materials.is_empty() {
        let diffuse = create_default_texture_colored(device, queue, [204, 204, 204, 255])?;
        let normal = create_default_texture(device, queue, true)?;
        let orm = create_default_orm_texture(device, queue)?;
        let emissive = create_default_emissive_texture(device, queue)?;
        gpu_materials.push(material::Material::new(
            device,
            "clay_default",
            diffuse,
            normal,
            orm,
            emissive,
            material::MaterialUniform::default(),
            layout,
        ));
        material_thumbnails.push(model::MaterialThumbnails {
            albedo: None,
            normal: None,
            metallic_roughness: None,
            occlusion: None,
            emissive: None,
            base_color: [0.8, 0.8, 0.8],
        });
    }

    let mut gpu_meshes = Vec::new();
    let mut gpu_mesh_bounds = Vec::new();
    let mut cpu_meshes: Vec<model::CpuMesh> = Vec::new();
    let mut raw_to_gpu: Vec<Option<usize>> = vec![None; raw.meshes.len()];
    for (i, (vertices, indices)) in mesh_vertices.iter().zip(mesh_indices.iter()).enumerate() {
        if vertices.is_empty() {
            continue;
        }
        raw_to_gpu[i] = Some(gpu_meshes.len());

        let cpu_positions: Vec<[f32; 3]> = vertices.iter().map(|v| v.position).collect();
        cpu_meshes.push(model::CpuMesh {
            positions: cpu_positions,
            indices: indices.clone(),
        });

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{:?} Vertex Buffer {}", file_path, i)),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{:?} Index Buffer {}", file_path, i)),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let edge_indices_data = geometry::extract_edges(indices);
        let num_edges = (edge_indices_data.len() / 2) as u32;

        let positions_padded: Vec<[f32; 4]> = vertices
            .iter()
            .map(|v| [v.position[0], v.position[1], v.position[2], 0.0])
            .collect();
        let edge_positions_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{:?} Edge Positions {}", file_path, i)),
            contents: bytemuck::cast_slice(&positions_padded),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let edge_index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{:?} Edge Indices {}", file_path, i)),
            contents: bytemuck::cast_slice(&edge_indices_data),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let edge_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(&format!("{:?} Edge Bind Group {}", file_path, i)),
            layout: edge_geometry_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: edge_positions_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: edge_index_buffer.as_entire_binding(),
                },
            ],
        });

        let uv_edge_data = if raw.meshes[i].tex_coords.is_some() {
            let uv_padded: Vec<[f32; 4]> = vertices
                .iter()
                .map(|v| [v.tex_coords[0], 1.0 - v.tex_coords[1], 0.0, 0.0])
                .collect();
            let uv_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} UV Positions {}", file_path, i)),
                contents: bytemuck::cast_slice(&uv_padded),
                usage: wgpu::BufferUsages::STORAGE,
            });
            let uv_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&format!("{:?} UV Edge Bind Group {}", file_path, i)),
                layout: edge_geometry_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uv_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: edge_index_buffer.as_entire_binding(),
                    },
                ],
            });
            Some(model::UvEdgeData {
                uv_buffer,
                bind_group: uv_bind_group,
            })
        } else {
            None
        };

        let degen_faces = &viewer_validation.degenerate_faces[i];
        let (degen_index_buffer, degen_num_elements) = if degen_faces.is_empty() {
            (None, 0)
        } else {
            let degen_indices: Vec<u32> = degen_faces
                .iter()
                .flat_map(|&fi| {
                    let base = fi as usize * 3;
                    [indices[base], indices[base + 1], indices[base + 2]]
                })
                .collect();
            let num = degen_indices.len() as u32;
            let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&format!("{:?} Degen Index Buffer {}", file_path, i)),
                contents: bytemuck::cast_slice(&degen_indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            (Some(buf), num)
        };

        let material_index = raw.meshes[i].material_index.unwrap_or(0);
        gpu_meshes.push(model::Mesh {
            name: raw.meshes[i].name.clone(),
            vertex_buffer,
            index_buffer,
            num_elements: indices.len() as u32,
            material: material_index,
            visible: true,
            edge_data: Some(model::EdgeData {
                positions_buffer: edge_positions_buffer,
                index_buffer: edge_index_buffer,
                num_edges,
                bind_group: edge_bind_group,
            }),
            uv_edge_data,
            degen_index_buffer,
            degen_num_elements,
        });
        gpu_mesh_bounds.push(per_mesh_bounds[i]);
    }

    let total_tris: usize = mesh_indices.iter().map(|idx| idx.len() / 3).sum();
    let total_verts: usize = mesh_vertices.iter().map(std::vec::Vec::len).sum();
    let stats = ModelStats {
        polys: raw.polygon_count,
        tris: total_tris,
        verts: total_verts,
    };

    let mut viewer_validation = viewer_validation;
    viewer_validation.raw_to_gpu = raw_to_gpu;

    Ok((
        model::Model {
            meshes: gpu_meshes,
            materials: gpu_materials,
            bounds,
            mesh_bounds: gpu_mesh_bounds,
            cpu_meshes,
            material_thumbnails,
            has_uvs,
        },
        normals_geo,
        stats,
        viewer_validation,
    ))
}

/// Upload GPU materials for cooked geometry (`solarxy_core::scene`).
/// Mirrors `upload_model`'s material loop minus the Material-Inspector
/// thumbnail capture (cooked materials arrive `Arc`-shared, so the
/// thumbnail bytes cannot be moved out; the inspector pipeline for
/// engine-driven objects comes with the asset milestone). Falls back to
/// the clay default when `materials` is empty, exactly like model loads.
pub(crate) fn upload_cooked_materials(
    materials: &[std::sync::Arc<RawMaterialData>],
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    cache: &mut TextureCache,
) -> Result<Vec<material::Material>, RendererError> {
    let mut gpu_materials = Vec::with_capacity(materials.len().max(1));
    for (mat_idx, mat) in materials.iter().enumerate() {
        let diffuse_texture = load_or_fallback_texture(
            device,
            queue,
            mat.diffuse_texture_data.as_deref(),
            mat.diffuse_texture_path.as_ref(),
            false,
            &mat.name,
            "diffuse",
            Some(cache),
        )?;
        let normal_texture = load_or_fallback_texture(
            device,
            queue,
            mat.normal_texture_data.as_deref(),
            mat.normal_texture_path.as_ref(),
            true,
            &mat.name,
            "normal",
            Some(cache),
        )?;
        let orm_texture = load_or_create_orm(device, queue, mat, Some(cache))?;
        let emissive_texture = load_or_fallback_texture(
            device,
            queue,
            mat.emissive_texture_data.as_deref(),
            mat.emissive_texture_path.as_ref(),
            false,
            &mat.name,
            "emissive",
            Some(cache),
        )?;

        let uniform = material::MaterialUniform {
            roughness_factor: mat.roughness_factor,
            metallic_factor: mat.metallic_factor,
            ao_strength: mat.occlusion_strength,
            alpha_cutoff: mat.alpha_cutoff,
            emissive: mat.emissive_factor,
            alpha_mode: mat.alpha_mode.into(),
            material_index: mat_idx as u32,
            shading_model: mat.shading_model.into(),
            toon_steps: mat.toon_steps,
            _pad: 0.0,
            base_color: mat.base_color_factor,
        };

        gpu_materials.push(material::Material::new(
            device,
            &mat.name,
            diffuse_texture,
            normal_texture,
            orm_texture,
            emissive_texture,
            uniform,
            layout,
        ));
    }

    if gpu_materials.is_empty() {
        let diffuse = create_default_texture_colored(device, queue, [204, 204, 204, 255])?;
        let normal = create_default_texture(device, queue, true)?;
        let orm = create_default_orm_texture(device, queue)?;
        let emissive = create_default_emissive_texture(device, queue)?;
        gpu_materials.push(material::Material::new(
            device,
            "clay_default",
            diffuse,
            normal,
            orm,
            emissive,
            material::MaterialUniform::default(),
            layout,
        ));
    }

    Ok(gpu_materials)
}

#[cfg(feature = "std-fs")]
pub fn load_binary(file_path: &str) -> Result<Vec<u8>, RendererError> {
    std::fs::read(file_path).map_err(|source| RendererError::Io {
        path: file_path.to_string(),
        source,
    })
}

#[cfg(feature = "std-fs")]
pub fn load_texture(
    file_path: &str,
    is_normal_map: bool,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<texture::Texture, RendererError> {
    let data = load_binary(file_path)?;
    texture::Texture::from_bytes(
        device,
        queue,
        &data,
        file_path,
        texture::TextureOpts::material(is_normal_map),
    )
}

pub fn create_floor_quad(device: &wgpu::Device, bounds: &model::AABB) -> model::Mesh {
    let y = bounds.min.y - 0.001;
    let he = bounds.diagonal() * 1.5;

    let vertices = [
        model::ModelVertex {
            position: [-he, y, -he],
            tex_coords: [0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
        model::ModelVertex {
            position: [he, y, -he],
            tex_coords: [1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
        model::ModelVertex {
            position: [he, y, he],
            tex_coords: [1.0, 1.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
        model::ModelVertex {
            position: [-he, y, he],
            tex_coords: [0.0, 1.0],
            normal: [0.0, 1.0, 0.0],
            tangent: [1.0, 0.0, 0.0],
            bitangent: [0.0, 0.0, 1.0],
        },
    ];
    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Floor Vertex Buffer"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Floor Index Buffer"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    model::Mesh {
        name: "floor".to_string(),
        vertex_buffer,
        index_buffer,
        num_elements: indices.len() as u32,
        material: 0,
        visible: true,
        edge_data: None,
        uv_edge_data: None,
        degen_index_buffer: None,
        degen_num_elements: 0,
    }
}

pub fn create_grid_quad(device: &wgpu::Device, bounds: &model::AABB) -> (model::Mesh, f32) {
    let y = -0.001_f32;
    let he = bounds.diagonal() * 8.0;
    let cell_size = bounds.diagonal() * 0.15;

    let vertices: [[f32; 3]; 4] = [[-he, y, -he], [he, y, -he], [he, y, he], [-he, y, he]];
    let indices: [u32; 6] = [0, 2, 1, 0, 3, 2];

    let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Grid Vertex Buffer"),
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("Grid Index Buffer"),
        contents: bytemuck::cast_slice(&indices),
        usage: wgpu::BufferUsages::INDEX,
    });

    (
        model::Mesh {
            name: "grid".to_string(),
            vertex_buffer,
            index_buffer,
            num_elements: indices.len() as u32,
            material: 0,
            visible: true,
            edge_data: None,
            uv_edge_data: None,
            degen_index_buffer: None,
            degen_num_elements: 0,
        },
        cell_size,
    )
}

fn create_default_texture_colored(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: [u8; 4],
) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
    let img =
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba)));
    texture::Texture::from_image(
        device,
        queue,
        &img,
        Some("default_texture"),
        texture::TextureOpts::flat(false),
    )
    .map(std::sync::Arc::new)
}

fn create_default_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    is_normal_map: bool,
) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
    let color = if is_normal_map {
        image::Rgba([128u8, 128, 255, 255])
    } else {
        image::Rgba([255u8, 255, 255, 255])
    };

    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, color));

    texture::Texture::from_image(
        device,
        queue,
        &img,
        Some("default_texture"),
        texture::TextureOpts::flat(is_normal_map),
    )
    .map(std::sync::Arc::new)
}

fn create_default_orm_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
    texture::Texture::from_raw_rgba(
        device,
        queue,
        &[255, 255, 255, 255],
        1,
        1,
        Some("default_orm"),
        texture::TextureOpts::flat(true),
    )
    .map(std::sync::Arc::new)
}

fn create_default_emissive_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
    texture::Texture::from_raw_rgba(
        device,
        queue,
        &[255, 255, 255, 255],
        1,
        1,
        Some("default_emissive"),
        texture::TextureOpts::flat(false),
    )
    .map(std::sync::Arc::new)
}

#[allow(clippy::too_many_arguments)]
fn load_or_fallback_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    embedded: Option<&RawImageData>,
    path: Option<&std::path::PathBuf>,
    is_linear: bool,
    mat_name: &str,
    kind: &str,
    cache: Option<&mut TextureCache>,
) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
    if let Some(data) = embedded {
        let build = || {
            texture::Texture::from_raw_rgba(
                device,
                queue,
                &data.pixels,
                data.width,
                data.height,
                Some(mat_name),
                texture::TextureOpts::material(is_linear),
            )
        };
        let built = match cache {
            Some(cache) => cache.get_or_try_insert((data.hash, is_linear), build),
            None => build().map(std::sync::Arc::new),
        };
        built.or_else(|e| {
            tracing::warn!("Failed to load embedded {kind} texture: {e}");
            create_default_texture(device, queue, is_linear)
        })
    } else {
        match path {
            #[cfg(feature = "std-fs")]
            Some(p) => {
                let p_str = p.to_string_lossy();
                load_texture(&p_str, is_linear, device, queue)
                    .map(std::sync::Arc::new)
                    .or_else(|e| {
                        tracing::warn!("Failed to load {kind} texture '{}': {e}", p.display());
                        create_default_texture(device, queue, is_linear)
                    })
            }
            // Without std-fs there is no filesystem to chase texture paths
            // into; a path-only reference degrades to the default texture
            // (web asset delivery replaces this in the import pipeline).
            #[cfg(not(feature = "std-fs"))]
            Some(p) => {
                tracing::warn!(
                    "No filesystem to load {kind} texture '{}'; using default",
                    p.display()
                );
                create_default_texture(device, queue, is_linear)
            }
            _ => {
                if kind == "emissive" {
                    create_default_emissive_texture(device, queue)
                } else if kind == "orm" {
                    create_default_orm_texture(device, queue)
                } else {
                    create_default_texture(device, queue, is_linear)
                }
            }
        }
    }
}

/// Loads the material's occlusion/roughness/metallic (ORM) texture,
/// compositing a separate occlusion map into the R channel.
///
/// Occlusion survives every source shape: a separate occlusion map with or
/// without an MR map, occlusion supplied as decoded bytes or (under
/// `std-fs`) a file path, and an occlusion map whose resolution differs from
/// the MR map (nearest-resampled onto the ORM grid). Scalar roughness /
/// metallic ride the shader's `roughness_factor` / `metallic_factor` and
/// occlusion strength rides `ao_strength`, so this only packs channels and
/// never applies factors.
fn load_or_create_orm(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mat: &RawMaterialData,
    mut cache: Option<&mut TextureCache>,
) -> Result<std::sync::Arc<texture::Texture>, RendererError> {
    let has_mr = mat.metallic_roughness_texture_data.is_some()
        || mat.metallic_roughness_texture_path.is_some();
    let has_occ = mat.occlusion_texture_data.is_some() || mat.occlusion_texture_path.is_some();

    // The MR map (or a white default) as the ORM: the historic behavior when
    // there is nothing to composite.
    let load_mr_or_default = |cache: Option<&mut TextureCache>| {
        if has_mr {
            load_or_fallback_texture(
                device,
                queue,
                mat.metallic_roughness_texture_data.as_deref(),
                mat.metallic_roughness_texture_path.as_ref(),
                true,
                &mat.name,
                "orm",
                cache,
            )
        } else {
            create_default_orm_texture(device, queue)
        }
    };

    // No occlusion map: nothing to composite.
    if !has_occ {
        return load_mr_or_default(cache.as_deref_mut());
    }

    // MR and occlusion are the same file: it is already an ORM pack.
    if let (Some(a), Some(b)) = (
        &mat.metallic_roughness_texture_path,
        &mat.occlusion_texture_path,
    ) && a == b
    {
        return load_mr_or_default(cache.as_deref_mut());
    }

    // MR and occlusion are the same decoded image (the common glTF ORM pack
    // referenced from both slots): compositing occ.R into mr.R is a no-op, so
    // load the MR texture directly instead of re-compositing and re-uploading.
    if let (Some(mr), Some(occ)) = (
        &mat.metallic_roughness_texture_data,
        &mat.occlusion_texture_data,
    ) && (std::sync::Arc::ptr_eq(mr, occ) || mr.hash == occ.hash)
    {
        return load_mr_or_default(cache.as_deref_mut());
    }

    // Decode both sources to CPU pixels (decoded bytes as-is, or a file path
    // under std-fs). Owned locals hold any path-decoded image so the refs
    // outlive the match arms.
    let mr_owned;
    let mr_ref: Option<&RawImageData> =
        if let Some(d) = mat.metallic_roughness_texture_data.as_deref() {
            Some(d)
        } else {
            mr_owned = decode_texture_source(mat.metallic_roughness_texture_path.as_ref());
            mr_owned.as_ref()
        };
    let occ_owned;
    let occ_ref: Option<&RawImageData> = if let Some(d) = mat.occlusion_texture_data.as_deref() {
        Some(d)
    } else {
        occ_owned = decode_texture_source(mat.occlusion_texture_path.as_ref());
        occ_owned.as_ref()
    };

    // Occlusion could not be decoded (a path-only reference with no
    // filesystem): fall back to the MR map rather than dropping to black.
    let Some(occ) = occ_ref else {
        return load_mr_or_default(cache.as_deref_mut());
    };

    let composited = composite_orm_pixels(mr_ref, occ);
    let build = || {
        texture::Texture::from_raw_rgba(
            device,
            queue,
            &composited.pixels,
            composited.width,
            composited.height,
            Some(&mat.name),
            texture::TextureOpts::material(true),
        )
    };
    match cache {
        Some(cache) => cache.get_or_try_insert((composited.hash, true), build),
        None => build().map(std::sync::Arc::new),
    }
}

/// Decodes a texture source for CPU compositing: decoded bytes are returned
/// as-is; a file path is read and decoded under `std-fs`. Returns `None`
/// when only a path is available on a build without filesystem access (the
/// web import pipeline delivers textures as decoded bytes instead).
fn decode_texture_source(path: Option<&std::path::PathBuf>) -> Option<RawImageData> {
    #[cfg(feature = "std-fs")]
    if let Some(p) = path {
        let p_str = p.to_string_lossy();
        match load_binary(&p_str) {
            Ok(bytes) => match image::load_from_memory(&bytes) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (w, h) = rgba.dimensions();
                    return Some(RawImageData::new(rgba.into_raw(), w, h));
                }
                Err(e) => tracing::warn!("Failed to decode ORM source '{}': {e}", p.display()),
            },
            Err(e) => tracing::warn!("Failed to read ORM source '{}': {e}", p.display()),
        }
    }
    #[cfg(not(feature = "std-fs"))]
    let _ = path;
    None
}

/// Nearest-neighbour sample of one channel of `img` at `(x, y)` on a
/// `dst_w x dst_h` grid. Identity when `img` already matches the
/// destination size; otherwise it resamples.
fn sample_nearest(
    img: &RawImageData,
    x: u32,
    y: u32,
    dst_w: u32,
    dst_h: u32,
    channel: usize,
) -> u8 {
    let sx = if dst_w == 0 {
        0
    } else {
        ((u64::from(x) * u64::from(img.width) / u64::from(dst_w)) as u32)
            .min(img.width.saturating_sub(1))
    };
    let sy = if dst_h == 0 {
        0
    } else {
        ((u64::from(y) * u64::from(img.height) / u64::from(dst_h)) as u32)
            .min(img.height.saturating_sub(1))
    };
    let idx = (sy as usize * img.width as usize + sx as usize) * 4 + channel;
    img.pixels.get(idx).copied().unwrap_or(255)
}

/// Packs an ORM texture: R = occlusion, G / B = the MR map's roughness /
/// metallic (or white when there is no MR map, so the shader's scalar
/// factors alone drive those channels), A = 255. The output takes the MR
/// map's size when present, else the occlusion map's; occlusion is
/// nearest-resampled onto that grid so a mismatched resolution still
/// composites instead of being dropped.
fn composite_orm_pixels(mr: Option<&RawImageData>, occ: &RawImageData) -> RawImageData {
    let (w, h) = match mr {
        Some(m) => (m.width, m.height),
        None => (occ.width, occ.height),
    };
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y as usize * w as usize + x as usize) * 4;
            pixels[i] = sample_nearest(occ, x, y, w, h, 0);
            let (g, b) = match mr {
                Some(m) => (
                    sample_nearest(m, x, y, w, h, 1),
                    sample_nearest(m, x, y, w, h, 2),
                ),
                None => (255, 255),
            };
            pixels[i + 1] = g;
            pixels[i + 2] = b;
            pixels[i + 3] = 255;
        }
    }
    RawImageData::new(pixels, w, h)
}

#[cfg(test)]
mod orm_tests {
    use super::{composite_orm_pixels, sample_nearest};
    use solarxy_core::RawImageData;

    /// A `w x h` image whose every texel is `rgba`.
    fn solid(w: u32, h: u32, rgba: [u8; 4]) -> RawImageData {
        let pixels = (0..w * h).flat_map(|_| rgba).collect();
        RawImageData::new(pixels, w, h)
    }

    #[test]
    fn matched_dims_take_occlusion_r_and_keep_mr_gb() {
        let mr = solid(2, 2, [99, 10, 20, 255]);
        let occ = solid(2, 2, [200, 0, 0, 255]);
        let out = composite_orm_pixels(Some(&mr), &occ);
        assert_eq!((out.width, out.height), (2, 2));
        for px in out.pixels.chunks(4) {
            assert_eq!(px, [200, 10, 20, 255], "R from occ, G/B from mr, A opaque");
        }
    }

    #[test]
    fn compositing_an_image_with_itself_is_identity() {
        // Justifies the same-image short-circuit in `load_or_create_orm`: when
        // MR and occlusion are the same decoded image, compositing reproduces
        // it exactly, so loading the MR texture directly is equivalent.
        let img = RawImageData::new(
            vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 100, 110, 120, 255,
            ],
            2,
            2,
        );
        let out = composite_orm_pixels(Some(&img), &img);
        assert_eq!((out.width, out.height), (2, 2));
        assert_eq!(out.pixels, img.pixels);
    }

    #[test]
    fn mismatched_dims_upsample_occlusion_onto_the_mr_grid() {
        let mr = solid(2, 2, [0, 30, 40, 255]);
        let occ = solid(1, 1, [123, 0, 0, 255]);
        let out = composite_orm_pixels(Some(&mr), &occ);
        assert_eq!((out.width, out.height), (2, 2), "output takes the mr size");
        for px in out.pixels.chunks(4) {
            assert_eq!(px, [123, 30, 40, 255]);
        }
    }

    #[test]
    fn mismatched_dims_downsample_occlusion_onto_the_mr_grid() {
        let mr = solid(1, 1, [0, 50, 60, 255]);
        let occ = RawImageData::new(
            vec![10, 0, 0, 255, 20, 0, 0, 255, 30, 0, 0, 255, 40, 0, 0, 255],
            2,
            2,
        );
        let out = composite_orm_pixels(Some(&mr), &occ);
        assert_eq!((out.width, out.height), (1, 1));
        // Nearest sample of the 2x2 occ at the single texel is its (0,0).
        assert_eq!(out.pixels, vec![10, 50, 60, 255]);
    }

    #[test]
    fn absent_mr_builds_white_gb_with_occlusion_r() {
        // AO-only material: no MR map, so G/B stay white (the shader's
        // scalar factors drive roughness/metallic) and only R carries AO.
        let occ = solid(2, 2, [77, 5, 5, 255]);
        let out = composite_orm_pixels(None, &occ);
        assert_eq!((out.width, out.height), (2, 2), "output takes the occ size");
        for px in out.pixels.chunks(4) {
            assert_eq!(px, [77, 255, 255, 255]);
        }
    }

    #[test]
    fn sample_nearest_is_identity_at_matching_size_and_clamps() {
        let img = RawImageData::new(
            vec![1, 2, 3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255],
            2,
            2,
        );
        // Identity at matching size: channel G of texel (1,1) is 11.
        assert_eq!(sample_nearest(&img, 1, 1, 2, 2, 1), 11);
        // An out-of-range destination coordinate clamps to the last texel.
        assert_eq!(sample_nearest(&img, 9, 9, 2, 2, 0), 10);
    }
}
