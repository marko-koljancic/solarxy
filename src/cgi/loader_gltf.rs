use cgmath::{InnerSpace, Matrix as _, Matrix3, Matrix4, SquareMatrix, Vector3, Vector4};

use super::geometry::{RawImageData, RawMaterialData, RawMeshData, RawModelData};

pub fn load_gltf(file_path: &str) -> anyhow::Result<RawModelData> {
    let (document, buffers, images) = gltf::import(file_path)?;

    let mut materials = extract_materials(&document, &images, file_path);
    let (meshes, polygon_count) = extract_meshes(&document, &buffers);

    if materials.is_empty() && meshes.iter().any(|m| m.material_index.is_some()) {
        materials.push(RawMaterialData {
            name: "gltf_default".to_string(),
            diffuse_texture_path: None,
            normal_texture_path: None,
            diffuse_texture_data: None,
            normal_texture_data: None,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor: 0.5,
            metallic_factor: 0.0,
            emissive_factor: [0.0, 0.0, 0.0],
            alpha_mode: 0,
            alpha_cutoff: 0.5,
            ambient: None,
            diffuse: None,
            specular: None,
            shininess: None,
            dissolve: None,
            optical_density: None,
            ambient_texture_name: None,
            diffuse_texture_name: None,
            specular_texture_name: None,
            normal_texture_name: None,
            shininess_texture_name: None,
            dissolve_texture_name: None,
        });
    }

    Ok(RawModelData {
        meshes,
        materials,
        polygon_count,
    })
}

fn extract_materials(
    document: &gltf::Document,
    images: &[gltf::image::Data],
    file_path: &str,
) -> Vec<RawMaterialData> {
    let parent_dir = std::path::Path::new(file_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."));

    document
        .materials()
        .map(|mat| {
            let pbr = mat.pbr_metallic_roughness();

            let (diffuse_path, diffuse_data) = match pbr.base_color_texture() {
                Some(info) => resolve_texture(&info.texture(), images, parent_dir),
                None => (None, None),
            };

            let (normal_path, normal_data) = match mat.normal_texture() {
                Some(info) => resolve_texture(&info.texture(), images, parent_dir),
                None => (None, None),
            };

            let (mr_path, mr_data) = match pbr.metallic_roughness_texture() {
                Some(info) => resolve_texture(&info.texture(), images, parent_dir),
                None => (None, None),
            };

            let (occ_path, occ_data) = match mat.occlusion_texture() {
                Some(info) => resolve_texture(&info.texture(), images, parent_dir),
                None => (None, None),
            };

            let (emissive_path, emissive_data) = match mat.emissive_texture() {
                Some(info) => resolve_texture(&info.texture(), images, parent_dir),
                None => (None, None),
            };

            let emissive_factor = mat.emissive_factor();

            let alpha_mode = match mat.alpha_mode() {
                gltf::material::AlphaMode::Opaque => 0,
                gltf::material::AlphaMode::Mask => 1,
                gltf::material::AlphaMode::Blend => 2,
            };
            let alpha_cutoff = mat.alpha_cutoff().unwrap_or(0.5);

            let base_color = pbr.base_color_factor();

            RawMaterialData {
                name: mat.name().unwrap_or("gltf_material").to_string(),
                diffuse_texture_path: diffuse_path,
                normal_texture_path: normal_path,
                diffuse_texture_data: diffuse_data,
                normal_texture_data: normal_data,
                metallic_roughness_texture_path: mr_path,
                metallic_roughness_texture_data: mr_data,
                occlusion_texture_path: occ_path,
                occlusion_texture_data: occ_data,
                emissive_texture_path: emissive_path,
                emissive_texture_data: emissive_data,
                roughness_factor: pbr.roughness_factor(),
                metallic_factor: pbr.metallic_factor(),
                emissive_factor,
                alpha_mode,
                alpha_cutoff,
                ambient: None,
                diffuse: Some([base_color[0], base_color[1], base_color[2]]),
                specular: None,
                shininess: None,
                dissolve: Some(base_color[3]),
                optical_density: None,
                ambient_texture_name: None,
                diffuse_texture_name: pbr
                    .base_color_texture()
                    .map(|t| format!("texture_index:{}", t.texture().source().index())),
                specular_texture_name: None,
                normal_texture_name: mat
                    .normal_texture()
                    .map(|t| format!("texture_index:{}", t.texture().source().index())),
                shininess_texture_name: None,
                dissolve_texture_name: None,
            }
        })
        .collect()
}

fn resolve_texture(
    texture: &gltf::Texture,
    images: &[gltf::image::Data],
    parent_dir: &std::path::Path,
) -> (Option<std::path::PathBuf>, Option<RawImageData>) {
    let image = texture.source();

    match image.source() {
        gltf::image::Source::Uri { uri, .. } => {
            if uri.starts_with("data:") {
                (None, image_data_to_raw(&images[image.index()]))
            } else {
                (Some(parent_dir.join(uri)), None)
            }
        }
        gltf::image::Source::View { .. } => (None, image_data_to_raw(&images[image.index()])),
    }
}

#[allow(clippy::unnecessary_wraps)]
fn image_data_to_raw(img: &gltf::image::Data) -> Option<RawImageData> {
    let pixels = match img.format {
        gltf::image::Format::R8G8B8A8 => img.pixels.clone(),
        gltf::image::Format::R8G8B8 => img
            .pixels
            .chunks_exact(3)
            .flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 255])
            .collect(),
        gltf::image::Format::R16G16B16A16 => img
            .pixels
            .chunks_exact(8)
            .flat_map(|c| [c[0], c[2], c[4], c[6]])
            .collect(),
        gltf::image::Format::R16G16B16 => img
            .pixels
            .chunks_exact(6)
            .flat_map(|c| [c[0], c[2], c[4], 255])
            .collect(),
        gltf::image::Format::R8 => img.pixels.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        gltf::image::Format::R16 => img
            .pixels
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], 255])
            .collect(),
        gltf::image::Format::R8G8 => img
            .pixels
            .chunks_exact(2)
            .flat_map(|rg| [rg[0], rg[1], 0, 255])
            .collect(),
        gltf::image::Format::R16G16 => img
            .pixels
            .chunks_exact(4)
            .flat_map(|c| [c[0], c[2], 0, 255])
            .collect(),
        gltf::image::Format::R32G32B32A32FLOAT => img
            .pixels
            .chunks_exact(16)
            .flat_map(|c| {
                let r = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let g = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                let b = f32::from_le_bytes([c[8], c[9], c[10], c[11]]);
                let a = f32::from_le_bytes([c[12], c[13], c[14], c[15]]);
                [
                    (r.clamp(0.0, 1.0) * 255.0) as u8,
                    (g.clamp(0.0, 1.0) * 255.0) as u8,
                    (b.clamp(0.0, 1.0) * 255.0) as u8,
                    (a.clamp(0.0, 1.0) * 255.0) as u8,
                ]
            })
            .collect(),
        gltf::image::Format::R32G32B32FLOAT => img
            .pixels
            .chunks_exact(12)
            .flat_map(|c| {
                let r = f32::from_le_bytes([c[0], c[1], c[2], c[3]]);
                let g = f32::from_le_bytes([c[4], c[5], c[6], c[7]]);
                let b = f32::from_le_bytes([c[8], c[9], c[10], c[11]]);
                [
                    (r.clamp(0.0, 1.0) * 255.0) as u8,
                    (g.clamp(0.0, 1.0) * 255.0) as u8,
                    (b.clamp(0.0, 1.0) * 255.0) as u8,
                    255,
                ]
            })
            .collect(),
    };

    Some(RawImageData {
        pixels,
        width: img.width,
        height: img.height,
    })
}

fn extract_meshes(
    document: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> (Vec<RawMeshData>, usize) {
    let mut meshes = Vec::new();
    let mut total_polygons = 0usize;

    for scene in document.scenes() {
        for node in scene.nodes() {
            collect_meshes_recursive(
                &node,
                Matrix4::identity(),
                buffers,
                &mut meshes,
                &mut total_polygons,
            );
        }
    }

    (meshes, total_polygons)
}

fn collect_meshes_recursive(
    node: &gltf::Node,
    parent_transform: Matrix4<f32>,
    buffers: &[gltf::buffer::Data],
    meshes: &mut Vec<RawMeshData>,
    total_polygons: &mut usize,
) {
    let local: [[f32; 4]; 4] = node.transform().matrix();
    let local_mat = Matrix4::from(local);
    let world_transform = parent_transform * local_mat;

    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                tracing::warn!(
                    "Skipping non-triangle primitive in mesh '{}' (mode: {:?})",
                    mesh.name().unwrap_or("unnamed"),
                    primitive.mode()
                );
                continue;
            }

            let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

            let positions: Vec<[f32; 3]> = match reader.read_positions() {
                Some(iter) => iter
                    .map(|p| {
                        let v = world_transform * Vector4::new(p[0], p[1], p[2], 1.0);
                        [v.x, v.y, v.z]
                    })
                    .collect(),
                None => continue,
            };

            let indices: Vec<u32> = match reader.read_indices() {
                Some(iter) => iter.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };

            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|iter| {
                let normal_matrix = extract_normal_matrix(&world_transform);
                iter.map(|n| {
                    let v = normal_matrix * Vector3::new(n[0], n[1], n[2]);
                    let len = v.magnitude();
                    if len > 1e-10 {
                        [v.x / len, v.y / len, v.z / len]
                    } else {
                        n
                    }
                })
                .collect()
            });

            let tex_coords: Option<Vec<[f32; 2]>> = reader
                .read_tex_coords(0)
                .map(|iter| iter.into_f32().collect());

            let material_index = Some(primitive.material().index().unwrap_or(0));

            *total_polygons += indices.len() / 3;

            meshes.push(RawMeshData {
                name: mesh.name().unwrap_or("gltf_mesh").to_string(),
                positions,
                indices,
                normals,
                tex_coords,
                material_index,
            });
        }
    }

    for child in node.children() {
        collect_meshes_recursive(&child, world_transform, buffers, meshes, total_polygons);
    }
}

fn extract_normal_matrix(transform: &Matrix4<f32>) -> Matrix3<f32> {
    let upper3x3 = Matrix3::new(
        transform.x.x,
        transform.x.y,
        transform.x.z,
        transform.y.x,
        transform.y.y,
        transform.y.z,
        transform.z.x,
        transform.z.y,
        transform.z.z,
    );
    upper3x3.invert().unwrap_or(Matrix3::identity()).transpose()
}
