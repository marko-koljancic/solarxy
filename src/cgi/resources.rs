use std::path::Path;

use wgpu::util::DeviceExt;

use super::geometry::{self, RawModelData};
use super::{loader_gltf, loader_obj, loader_ply, loader_stl, material, model, texture};

pub const SUPPORTED_EXTENSIONS: &[&str] = &["obj", "stl", "ply", "gltf", "glb"];

pub fn is_supported_model_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| SUPPORTED_EXTENSIONS.iter().any(|s| ext.eq_ignore_ascii_case(s)))
        .unwrap_or(false)
}

pub fn load_model_any(
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
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

    upload_model(raw, file_path, device, queue, layout)
}

pub struct ModelStats {
    pub polys: usize,
    pub tris: usize,
    pub verts: usize,
}

fn upload_model(
    raw: RawModelData,
    file_path: &str,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
) -> anyhow::Result<(model::Model, model::NormalsGeometry, ModelStats)> {
    let (mesh_vertices, mesh_indices, bounds, normals_geo) = geometry::process_raw_model(&raw);
    let mut gpu_materials = Vec::new();
    for mat in &raw.materials {
        let diffuse_texture = if let Some(ref data) = mat.diffuse_texture_data {
            texture::Texture::from_raw_rgba(
                device,
                queue,
                &data.pixels,
                data.width,
                data.height,
                Some(&mat.name),
                false,
            )
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load embedded diffuse texture: {}", e);
                create_default_texture(device, queue, false)
            })
        } else {
            match &mat.diffuse_texture_path {
                Some(path) if !path.is_empty() => load_texture(path, false, device, queue).unwrap_or_else(|e| {
                    eprintln!("Warning: Failed to load diffuse texture '{}': {}", path, e);
                    create_default_texture(device, queue, false)
                }),
                _ => create_default_texture(device, queue, false),
            }
        };

        let normal_texture = if let Some(ref data) = mat.normal_texture_data {
            texture::Texture::from_raw_rgba(
                device,
                queue,
                &data.pixels,
                data.width,
                data.height,
                Some(&mat.name),
                true,
            )
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load embedded normal texture: {}", e);
                create_default_texture(device, queue, true)
            })
        } else {
            match &mat.normal_texture_path {
                Some(path) if !path.is_empty() => load_texture(path, true, device, queue)
                    .unwrap_or_else(|_| create_default_texture(device, queue, true)),
                _ => create_default_texture(device, queue, true),
            }
        };

        gpu_materials.push(material::Material::new(
            device,
            &mat.name,
            diffuse_texture,
            normal_texture,
            layout,
        ));
    }

    if gpu_materials.is_empty() {
        let diffuse = create_default_texture_colored(device, queue, [226, 213, 195, 255]);
        let normal = create_default_texture(device, queue, true);
        gpu_materials.push(material::Material::new(device, "clay_default", diffuse, normal, layout));
    }

    let mut gpu_meshes = Vec::new();
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

        let material_index = raw.meshes[i].material_index.unwrap_or(0);
        gpu_meshes.push(model::Mesh {
            name: raw.meshes[i].name.clone(),
            vertex_buffer,
            index_buffer,
            num_elements: indices.len() as u32,
            material: material_index,
        });
    }

    let total_tris: usize = mesh_indices.iter().map(|idx| idx.len() / 3).sum();
    let total_verts: usize = mesh_vertices.iter().map(|v| v.len()).sum();
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
    }
}

pub fn create_grid_quad(device: &wgpu::Device, bounds: &model::AABB) -> (model::Mesh, f32) {
    let y = bounds.min.y - 0.001;
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
        },
        cell_size,
    )
}

fn create_default_texture_colored(device: &wgpu::Device, queue: &wgpu::Queue, rgba: [u8; 4]) -> texture::Texture {
    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, image::Rgba(rgba)));
    texture::Texture::from_image(device, queue, &img, Some("default_texture"), false)
        .expect("Failed to create default texture")
}

fn create_default_texture(device: &wgpu::Device, queue: &wgpu::Queue, is_normal_map: bool) -> texture::Texture {
    let color = if is_normal_map {
        image::Rgba([128u8, 128, 255, 255])
    } else {
        image::Rgba([255u8, 255, 255, 255])
    };

    let img = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1, 1, color));

    texture::Texture::from_image(device, queue, &img, Some("default_texture"), is_normal_map)
        .expect("Failed to create default texture")
}
