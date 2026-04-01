use std::io::{BufReader, Cursor};
use tobj::LoadError;

use super::geometry::{RawMaterialData, RawMeshData, RawModelData};

pub fn load_obj(file_path: &str) -> anyhow::Result<RawModelData> {
    let obj_text = std::fs::read_to_string(file_path)?;
    let obj_cursor = Cursor::new(obj_text);
    let mut obj_reader = BufReader::new(obj_cursor);

    let obj_dir = std::path::Path::new(file_path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|| ".".to_string());

    let (models, obj_materials) = tobj::load_obj_buf(
        &mut obj_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |p| {
            let mat_path = std::path::Path::new(&obj_dir).join(p);
            let mat_text = std::fs::read_to_string(&mat_path).map_err(|_| LoadError::ReadError)?;
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_text)))
        },
    )?;

    let mut materials = Vec::new();
    for m in obj_materials.unwrap_or_default() {
        let diffuse_path = m.diffuse_texture.as_deref().map(|p| {
            std::path::Path::new(&obj_dir)
                .join(p)
                .to_string_lossy()
                .to_string()
        });

        let normal_path = m.normal_texture.as_deref().map(|p| {
            let cleaned = p
                .split_whitespace()
                .filter(|s| !s.starts_with('-'))
                .collect::<Vec<_>>()
                .join(" ");
            std::path::Path::new(&obj_dir)
                .join(cleaned)
                .to_string_lossy()
                .to_string()
        });

        let roughness_factor = m
            .unknown_param
            .get("Pr")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.5);
        let metallic_factor = m
            .unknown_param
            .get("Pm")
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0);

        let (alpha_mode, alpha_cutoff) = match m.dissolve {
            Some(d) if d < 1.0 => (1u32, 0.5f32),
            _ => (0, 0.5),
        };

        materials.push(RawMaterialData {
            name: m.name.clone(),
            diffuse_texture_path: diffuse_path,
            normal_texture_path: normal_path,
            diffuse_texture_data: None,
            normal_texture_data: None,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor,
            metallic_factor,
            emissive_factor: [0.0, 0.0, 0.0],
            alpha_mode,
            alpha_cutoff,
        });
    }

    let polygon_count: usize = models
        .iter()
        .map(|m| {
            if m.mesh.face_arities.is_empty() {
                m.mesh.indices.len() / 3
            } else {
                m.mesh.face_arities.len()
            }
        })
        .sum();

    let mut meshes = Vec::new();
    for m in models {
        if m.mesh.positions.is_empty() || m.mesh.indices.is_empty() {
            continue;
        }

        let num_verts = m.mesh.positions.len() / 3;
        let positions: Vec<[f32; 3]> = (0..num_verts)
            .map(|i| {
                [
                    m.mesh.positions[i * 3],
                    m.mesh.positions[i * 3 + 1],
                    m.mesh.positions[i * 3 + 2],
                ]
            })
            .collect();

        let normals = if m.mesh.normals.is_empty() {
            None
        } else {
            Some(
                (0..num_verts)
                    .map(|i| {
                        [
                            m.mesh.normals[i * 3],
                            m.mesh.normals[i * 3 + 1],
                            m.mesh.normals[i * 3 + 2],
                        ]
                    })
                    .collect(),
            )
        };

        let tex_coords = if m.mesh.texcoords.is_empty() {
            None
        } else {
            Some(
                (0..num_verts)
                    .map(|i| [m.mesh.texcoords[i * 2], 1.0 - m.mesh.texcoords[i * 2 + 1]])
                    .collect(),
            )
        };

        meshes.push(RawMeshData {
            name: m.name,
            positions,
            indices: m.mesh.indices,
            normals,
            tex_coords,
            material_index: m.mesh.material_id,
        });
    }

    Ok(RawModelData {
        meshes,
        materials,
        polygon_count,
    })
}
