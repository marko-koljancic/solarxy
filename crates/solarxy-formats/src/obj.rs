//! OBJ/MTL loading. `load_obj_bytes` parses from memory with MTL libraries
//! supplied by an [`AssetResolver`]; `load_obj` (std-fs) wraps it with a
//! directory-rooted resolver and directory-joined texture paths.

use std::io::{BufReader, Cursor};
use std::path::{Path, PathBuf};

use tobj::LoadError;

use crate::{AssetResolver, FormatsError};
use solarxy_core::geometry::RawImageData;
use solarxy_core::{AlphaMode, RawMaterialData, RawMeshData, RawModelData};

/// Parse OBJ bytes; `.mtl` libraries resolve through `resolver`. Texture
/// references are recorded as the (cleaned) relative names from the MTL —
/// byte-mode callers own path semantics.
pub fn load_obj_bytes(
    bytes: &[u8],
    resolver: &mut dyn AssetResolver,
) -> Result<RawModelData, FormatsError> {
    parse_obj(bytes, resolver, None)
}

/// Load an OBJ from disk. MTL libraries and texture paths resolve relative
/// to the OBJ's directory, exactly as before the byte-first refactor.
#[cfg(feature = "std-fs")]
pub fn load_obj(file_path: &str) -> Result<RawModelData, FormatsError> {
    let bytes = crate::read_file(file_path)?;
    let obj_dir = Path::new(file_path)
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    let mut resolver = crate::DirResolver::new(&obj_dir);
    parse_obj(&bytes, &mut resolver, Some(&obj_dir))
}

/// Shared parse core. `texture_base` joins MTL texture references onto a
/// directory (path mode); `None` records them as-is (byte mode).
fn parse_obj(
    bytes: &[u8],
    resolver: &mut dyn AssetResolver,
    texture_base: Option<&Path>,
) -> Result<RawModelData, FormatsError> {
    let mut obj_reader = BufReader::new(Cursor::new(bytes));

    // tobj's material loader is `Fn`; a RefCell bridges the `&mut` resolver.
    let resolver = std::cell::RefCell::new(resolver);
    let (models, obj_materials) = tobj::load_obj_buf(
        &mut obj_reader,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
        |p| {
            let rel = p.to_string_lossy();
            let mat_bytes = resolver
                .borrow_mut()
                .read(&rel)
                .ok_or(LoadError::ReadError)?;
            tobj::load_mtl_buf(&mut BufReader::new(Cursor::new(mat_bytes)))
        },
    )?;

    let tex_path = |rel: &str| -> PathBuf {
        texture_base.map_or_else(|| PathBuf::from(rel), |base| base.join(rel))
    };

    // Decode a referenced texture through the resolver at parse time, so
    // the byte-first path (web worker) carries pixels rather than a path
    // no filesystem can serve. The GPU upload prefers `*_texture_data`
    // and falls back to the path, so the desktop path mode also benefits.
    let tex_data = |rel: &str| -> Option<std::sync::Arc<RawImageData>> {
        let bytes = resolver.borrow_mut().read(rel)?;
        crate::decode_image_bytes(&bytes)
            .ok()
            .map(std::sync::Arc::new)
    };

    let mut materials = Vec::new();
    for m in obj_materials.unwrap_or_default() {
        let diffuse_path = m.diffuse_texture.as_deref().map(&tex_path);
        let diffuse_data = m.diffuse_texture.as_deref().and_then(&tex_data);

        let normal_rel = m.normal_texture.as_deref().map(|p| {
            p.split_whitespace()
                .filter(|s| !s.starts_with('-'))
                .collect::<Vec<_>>()
                .join(" ")
        });
        let normal_path = normal_rel.as_deref().map(&tex_path);
        let normal_data = normal_rel.as_deref().and_then(&tex_data);

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
            Some(d) if d < 1.0 => (AlphaMode::Mask, 0.5f32),
            _ => (AlphaMode::Opaque, 0.5),
        };

        // MTL Kd + dissolve become the base-color factor (white when the
        // MTL declares none), so untextured colored materials render their
        // color instead of white.
        let kd = m.diffuse.unwrap_or([1.0, 1.0, 1.0]);
        let base_alpha = m.dissolve.unwrap_or(1.0);

        materials.push(RawMaterialData {
            name: m.name.clone(),
            diffuse_texture_path: diffuse_path,
            normal_texture_path: normal_path,
            diffuse_texture_data: diffuse_data,
            normal_texture_data: normal_data,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor,
            metallic_factor,
            occlusion_strength: 1.0,
            emissive_factor: [0.0, 0.0, 0.0],
            base_color_factor: [kd[0], kd[1], kd[2], base_alpha],
            alpha_mode,
            alpha_cutoff,
            shading_model: solarxy_core::geometry::ShadingModel::default(),
            toon_steps: 3.0,
            ambient: m.ambient,
            diffuse: m.diffuse,
            specular: m.specular,
            shininess: m.shininess,
            dissolve: m.dissolve,
            optical_density: m.optical_density,
            ambient_texture_name: m.ambient_texture.clone(),
            diffuse_texture_name: m.diffuse_texture.clone(),
            specular_texture_name: m.specular_texture.clone(),
            normal_texture_name: m.normal_texture.clone(),
            shininess_texture_name: m.shininess_texture.clone(),
            dissolve_texture_name: m.dissolve_texture.clone(),
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
