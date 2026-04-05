use std::path::Path;

use wgpu::util::DeviceExt;

use super::geometry::{self, RawImageData, RawMaterialData, RawModelData};
use super::{loader_gltf, loader_obj, loader_ply, loader_stl, material, model, texture};

pub fn is_supported_model_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            crate::SUPPORTED_EXTENSIONS
                .iter()
                .any(|s| ext.eq_ignore_ascii_case(s))
        })
}

pub fn load_model_any(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    edge_geometry_layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry, ModelStats)> {
    let ext = std::path::Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let raw = match ext.as_str() {
        "stl" => loader_stl::load_stl(file_path)?,
        "ply" => loader_ply::load_ply(file_path)?,
        "gltf" | "glb" => loader_gltf::load_gltf(file_path)?,
        _ => loader_obj::load_obj(file_path)?,
    };

    upload_model(raw, file_path, device, queue, layout, edge_geometry_layout)
}

pub struct ModelStats {
    pub polys: usize,
    pub tris: usize,
    pub verts: usize,
}

#[allow(clippy::unnecessary_wraps)]
fn upload_model(
    raw: RawModelData,
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    edge_geometry_layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry, ModelStats)> {
    let has_uvs = raw.meshes.iter().any(|m| m.tex_coords.is_some());
    let (mesh_vertices, mesh_indices, bounds, per_mesh_bounds, normals_geo) =
        geometry::process_raw_model(&raw);
    let mut gpu_materials = Vec::new();
    for mat in &raw.materials {
        let diffuse_texture = load_or_fallback_texture(
            device,
            queue,
            mat.diffuse_texture_data.as_ref(),
            mat.diffuse_texture_path.as_ref(),
            false,
            &mat.name,
            "diffuse",
        );
        let normal_texture = load_or_fallback_texture(
            device,
            queue,
            mat.normal_texture_data.as_ref(),
            mat.normal_texture_path.as_ref(),
            true,
            &mat.name,
            "normal",
        );
        let orm_texture = load_or_create_orm(device, queue, mat);
        let emissive_texture = load_or_fallback_texture(
            device,
            queue,
            mat.emissive_texture_data.as_ref(),
            mat.emissive_texture_path.as_ref(),
            false,
            &mat.name,
            "emissive",
        );

        let uniform = material::MaterialUniform {
            roughness_factor: mat.roughness_factor,
            metallic_factor: mat.metallic_factor,
            ao_strength: 1.0,
            alpha_cutoff: mat.alpha_cutoff,
            emissive: mat.emissive_factor,
            alpha_mode: mat.alpha_mode,
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
        let diffuse = create_default_texture_colored(device, queue, [204, 204, 204, 255]);
        let normal = create_default_texture(device, queue, true);
        let orm = create_default_orm_texture(device, queue);
        let emissive = create_default_emissive_texture(device, queue);
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

    let mut gpu_meshes = Vec::new();
    let mut gpu_mesh_bounds = Vec::new();
    for (i, (vertices, indices)) in mesh_vertices.iter().zip(mesh_indices.iter()).enumerate() {
        if vertices.is_empty() {
            continue;
        }

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

        let material_index = raw.meshes[i].material_index.unwrap_or(0);
        gpu_meshes.push(model::Mesh {
            name: raw.meshes[i].name.clone(),
            vertex_buffer,
            index_buffer,
            num_elements: indices.len() as u32,
            material: material_index,
            edge_data: Some(model::EdgeData {
                positions_buffer: edge_positions_buffer,
                index_buffer: edge_index_buffer,
                num_edges,
                bind_group: edge_bind_group,
            }),
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

    Ok((
        model::Model {
            meshes: gpu_meshes,
            materials: gpu_materials,
            bounds,
            mesh_bounds: gpu_mesh_bounds,
            has_uvs,
        },
        normals_geo,
        stats,
    ))
}

pub fn load_binary(file_path: &str) -> anyhow::Result<Vec<u8>> {
    let path = std::path::Path::new(file_path);
    Ok(std::fs::read(path)?)
}

pub fn load_texture(
    file_path: &str,
    is_normal_map: bool,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> anyhow::Result<texture::Texture> {
    let data = load_binary(file_path)?;
    texture::Texture::from_bytes(device, queue, &data, file_path, is_normal_map)
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
        edge_data: None,
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
            edge_data: None,
        },
        cell_size,
    )
}

fn create_default_texture_colored(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: [u8; 4],
) -> texture::Texture {
    let img =
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba)));
    texture::Texture::from_image(device, queue, &img, Some("default_texture"), false)
        .expect("Failed to create default texture")
}

fn create_default_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    is_normal_map: bool,
) -> texture::Texture {
    let color = if is_normal_map {
        image::Rgba([128u8, 128, 255, 255])
    } else {
        image::Rgba([255u8, 255, 255, 255])
    };

    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, color));

    texture::Texture::from_image(device, queue, &img, Some("default_texture"), is_normal_map)
        .expect("Failed to create default texture")
}

fn create_default_orm_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> texture::Texture {
    texture::Texture::from_raw_rgba(
        device,
        queue,
        &[255, 255, 255, 255],
        1,
        1,
        Some("default_orm"),
        true,
    )
    .expect("Failed to create default ORM texture")
}

fn create_default_emissive_texture(device: &wgpu::Device, queue: &wgpu::Queue) -> texture::Texture {
    texture::Texture::from_raw_rgba(
        device,
        queue,
        &[255, 255, 255, 255],
        1,
        1,
        Some("default_emissive"),
        false,
    )
    .expect("Failed to create default emissive texture")
}

fn load_or_fallback_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    embedded: Option<&RawImageData>,
    path: Option<&std::path::PathBuf>,
    is_linear: bool,
    mat_name: &str,
    kind: &str,
) -> texture::Texture {
    if let Some(data) = embedded {
        texture::Texture::from_raw_rgba(
            device,
            queue,
            &data.pixels,
            data.width,
            data.height,
            Some(mat_name),
            is_linear,
        )
        .unwrap_or_else(|e| {
            tracing::warn!("Failed to load embedded {kind} texture: {e}");
            create_default_texture(device, queue, is_linear)
        })
    } else {
        match path {
            Some(p) => {
                let p_str = p.to_string_lossy();
                load_texture(&p_str, is_linear, device, queue).unwrap_or_else(|e| {
                    tracing::warn!("Failed to load {kind} texture '{}': {e}", p.display());
                    create_default_texture(device, queue, is_linear)
                })
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

fn load_or_create_orm(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mat: &RawMaterialData,
) -> texture::Texture {
    let mr_tex = if mat.metallic_roughness_texture_data.is_some()
        || mat.metallic_roughness_texture_path.is_some()
    {
        load_or_fallback_texture(
            device,
            queue,
            mat.metallic_roughness_texture_data.as_ref(),
            mat.metallic_roughness_texture_path.as_ref(),
            true,
            &mat.name,
            "orm",
        )
    } else {
        return create_default_orm_texture(device, queue);
    };

    if mat.occlusion_texture_data.is_some() || mat.occlusion_texture_path.is_some() {
        let same_image = match (
            &mat.metallic_roughness_texture_path,
            &mat.occlusion_texture_path,
        ) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        };
        if same_image {
            return mr_tex;
        }

        if let Some(ref occ_data) = mat.occlusion_texture_data {
            if let Some(composited) =
                composite_orm_pixels(mat.metallic_roughness_texture_data.as_ref(), occ_data)
            {
                return texture::Texture::from_raw_rgba(
                    device,
                    queue,
                    &composited.pixels,
                    composited.width,
                    composited.height,
                    Some(&mat.name),
                    true,
                )
                .unwrap_or(mr_tex);
            }
        }
    }

    mr_tex
}

fn composite_orm_pixels(
    mr_data: Option<&RawImageData>,
    occ_data: &RawImageData,
) -> Option<RawImageData> {
    let mr = mr_data.as_ref()?;
    if mr.width != occ_data.width || mr.height != occ_data.height {
        return None;
    }
    let mut pixels = mr.pixels.clone();
    for i in (0..pixels.len()).step_by(4) {
        if i < occ_data.pixels.len() {
            pixels[i] = occ_data.pixels[i];
        }
    }
    Some(RawImageData {
        pixels,
        width: mr.width,
        height: mr.height,
    })
}
