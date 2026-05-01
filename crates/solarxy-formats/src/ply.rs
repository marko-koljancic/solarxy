use std::io::BufReader;
use ply_rs_bw::ply::Property;

use solarxy_core::{AlphaMode, RawMaterialData, RawMeshData, RawModelData};

fn ply_prop_to_f32(prop: &Property) -> f32 {
    match *prop {
        Property::Float(v) => v,
        Property::Double(v) => v as f32,
        Property::Int(v) => v as f32,
        Property::UInt(v) => v as f32,
        Property::Short(v) => f32::from(v),
        Property::UShort(v) => f32::from(v),
        Property::Char(v) => f32::from(v),
        Property::UChar(v) => f32::from(v),
        _ => 0.0,
    }
}

fn ply_prop_to_indices(prop: &Property) -> Vec<u32> {
    match prop {
        Property::ListInt(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUInt(v) => v.clone(),
        Property::ListShort(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUShort(v) => v.iter().map(|&i| u32::from(i)).collect(),
        Property::ListUChar(v) => v.iter().map(|&i| u32::from(i)).collect(),
        Property::ListChar(v) => v.iter().map(|&i| i as u32).collect(),
        _ => Vec::new(),
    }
}

pub fn find_companion_texture(ply_path: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(ply_path);
    let parent = path.parent()?;
    let stem = path.file_stem()?.to_str()?;
    let suffixes = ["_0", "", "_diffuse"];
    let extensions = ["jpg", "jpeg", "png", "bmp", "tga"];
    for suffix in &suffixes {
        for ext in &extensions {
            let candidate = parent.join(format!("{}{}.{}", stem, suffix, ext));
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn load_ply(file_path: &str) -> anyhow::Result<RawModelData> {
    let file = std::fs::File::open(file_path)?;
    let mut reader = BufReader::new(file);
    let parser = ply_rs_bw::parser::Parser::<ply_rs_bw::ply::DefaultElement>::new();
    let ply = parser.read_ply(&mut reader)?;

    let ply_vertices = ply
        .payload
        .get("vertex")
        .ok_or_else(|| anyhow::anyhow!("PLY file has no 'vertex' element"))?;

    let ply_faces = ply
        .payload
        .get("face")
        .ok_or_else(|| anyhow::anyhow!("PLY file has no 'face' element"))?;

    if ply_vertices.is_empty() || ply_faces.is_empty() {
        anyhow::bail!("PLY file contains no geometry");
    }

    let (has_normals, has_uvs, uv_keys) = if let Some(first) = ply_vertices.first() {
        let has_normals = first.get("nx").is_some();
        let uv_keys: Option<(&str, &str)> = if first.get("s").is_some() && first.get("t").is_some()
        {
            Some(("s", "t"))
        } else if first.get("u").is_some() && first.get("v").is_some() {
            Some(("u", "v"))
        } else if first.get("texture_u").is_some() && first.get("texture_v").is_some() {
            Some(("texture_u", "texture_v"))
        } else {
            None
        };
        (has_normals, uv_keys.is_some(), uv_keys)
    } else {
        (false, false, None)
    };

    let multi_tex_verts = ply.payload.get("multi_texture_vertex");
    let multi_tex_faces = ply.payload.get("multi_texture_face");
    let has_multi_tex = multi_tex_verts.is_some_and(|v| !v.is_empty())
        && multi_tex_faces.is_some_and(|f| !f.is_empty());

    let multi_tex_uvs: Vec<[f32; 2]> = match multi_tex_verts.filter(|v| !v.is_empty()) {
        Some(verts) if has_multi_tex => verts
            .iter()
            .map(|elem| {
                [
                    elem.get("u").map_or(0.0, ply_prop_to_f32),
                    elem.get("v").map_or(0.0, ply_prop_to_f32),
                ]
            })
            .collect(),
        _ => Vec::new(),
    };

    let has_uvs = has_uvs || has_multi_tex;

    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(ply_vertices.len());
    let mut normals_vec: Vec<[f32; 3]> = Vec::with_capacity(ply_vertices.len());
    let mut tex_coords_vec: Vec<[f32; 2]> = Vec::with_capacity(ply_vertices.len());

    for elem in ply_vertices {
        let x = elem.get("x").map_or(0.0, ply_prop_to_f32);
        let y = elem.get("y").map_or(0.0, ply_prop_to_f32);
        let z = elem.get("z").map_or(0.0, ply_prop_to_f32);
        positions.push([x, y, z]);

        if has_normals {
            normals_vec.push([
                elem.get("nx").map_or(0.0, ply_prop_to_f32),
                elem.get("ny").map_or(0.0, ply_prop_to_f32),
                elem.get("nz").map_or(0.0, ply_prop_to_f32),
            ]);
        }

        if let Some((u_key, v_key)) = uv_keys {
            tex_coords_vec.push([
                elem.get(u_key).map_or(0.0, ply_prop_to_f32),
                elem.get(v_key).map_or(0.0, ply_prop_to_f32),
            ]);
        }
    }

    let polygon_count = ply_faces.len();

    let mut indices: Vec<u32> = Vec::new();
    if let Some(mt_faces) = multi_tex_faces.filter(|_| has_multi_tex) {
        let geo_faces: Vec<Vec<u32>> = ply_faces
            .iter()
            .map(|face| {
                face.get("vertex_indices")
                    .or_else(|| face.get("vertex_index"))
                    .map(ply_prop_to_indices)
                    .unwrap_or_default()
            })
            .collect();

        let mut vert_map: std::collections::HashMap<(u32, u32), u32> =
            std::collections::HashMap::new();
        let mut final_positions: Vec<[f32; 3]> = Vec::new();
        let mut final_normals: Vec<[f32; 3]> = Vec::new();
        let mut final_uvs: Vec<[f32; 2]> = Vec::new();

        for (fi, mt_face) in mt_faces.iter().enumerate() {
            let vis = if fi < geo_faces.len() {
                &geo_faces[fi]
            } else {
                continue;
            };
            let tex_vis = mt_face
                .get("texture_vertex_indices")
                .or_else(|| mt_face.get("texture_vertex_index"))
                .map(ply_prop_to_indices)
                .unwrap_or_default();

            let mut resolved = Vec::with_capacity(vis.len());
            for (vi_idx, &pos_idx) in vis.iter().enumerate() {
                let uv_idx = tex_vis.get(vi_idx).copied().unwrap_or(0);
                let key = (pos_idx, uv_idx);
                let final_idx = *vert_map.entry(key).or_insert_with(|| {
                    let idx = final_positions.len() as u32;
                    final_positions.push(positions[pos_idx as usize]);
                    if has_normals {
                        final_normals.push(normals_vec[pos_idx as usize]);
                    }
                    if let Some(uv) = multi_tex_uvs.get(uv_idx as usize) {
                        final_uvs.push(*uv);
                    } else {
                        final_uvs.push([0.0, 0.0]);
                    }
                    idx
                });
                resolved.push(final_idx);
            }

            for i in 1..resolved.len().saturating_sub(1) {
                indices.push(resolved[0]);
                indices.push(resolved[i]);
                indices.push(resolved[i + 1]);
            }
        }

        positions = final_positions;
        normals_vec = final_normals;
        tex_coords_vec = final_uvs;
    } else {
        for face in ply_faces {
            let vis = face
                .get("vertex_indices")
                .or_else(|| face.get("vertex_index"))
                .map(ply_prop_to_indices)
                .unwrap_or_default();
            for i in 1..vis.len().saturating_sub(1) {
                indices.push(vis[0]);
                indices.push(vis[i]);
                indices.push(vis[i + 1]);
            }
        }
    }

    let normals = if has_normals && !normals_vec.is_empty() {
        Some(normals_vec)
    } else {
        None
    };

    let tex_coords = if has_uvs && !tex_coords_vec.is_empty() {
        Some(tex_coords_vec)
    } else {
        None
    };

    let mut materials = Vec::new();
    let companion_tex = if has_uvs {
        find_companion_texture(file_path)
    } else {
        None
    };
    let mat_name = if companion_tex.is_some() {
        "ply_textured"
    } else {
        "clay_default"
    };
    materials.push(RawMaterialData {
        name: mat_name.to_string(),
        diffuse_texture_path: companion_tex,
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
        alpha_mode: AlphaMode::Opaque,
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

    Ok(RawModelData {
        meshes: vec![RawMeshData {
            name: file_path.to_string(),
            positions,
            indices,
            normals,
            tex_coords,
            material_index: Some(0),
        }],
        materials,
        polygon_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ply_prop_to_f32_float() {
        assert!((ply_prop_to_f32(&Property::Float(1.5)) - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn ply_prop_to_f32_double() {
        assert!((ply_prop_to_f32(&Property::Double(2.5)) - 2.5).abs() < f32::EPSILON);
    }

    #[test]
    fn ply_prop_to_f32_int() {
        assert!((ply_prop_to_f32(&Property::Int(42)) - 42.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ply_prop_to_f32_uchar() {
        assert!((ply_prop_to_f32(&Property::UChar(255)) - 255.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ply_prop_to_f32_list_returns_zero() {
        let prop = Property::ListInt(vec![1, 2, 3]);
        assert!((ply_prop_to_f32(&prop)).abs() < f32::EPSILON);
    }

    #[test]
    fn ply_prop_to_indices_list_int() {
        let prop = Property::ListInt(vec![0, 1, 2]);
        assert_eq!(ply_prop_to_indices(&prop), vec![0u32, 1, 2]);
    }

    #[test]
    fn ply_prop_to_indices_list_uint() {
        let prop = Property::ListUInt(vec![3, 4, 5]);
        assert_eq!(ply_prop_to_indices(&prop), vec![3u32, 4, 5]);
    }

    #[test]
    fn ply_prop_to_indices_list_uchar() {
        let prop = Property::ListUChar(vec![0, 1]);
        assert_eq!(ply_prop_to_indices(&prop), vec![0u32, 1]);
    }

    #[test]
    fn ply_prop_to_indices_scalar_empty() {
        let prop = Property::Float(1.0);
        assert!(ply_prop_to_indices(&prop).is_empty());
    }

    #[test]
    fn ply_prop_to_indices_negative_int_wraps() {
        let prop = Property::ListInt(vec![-1]);
        let result = ply_prop_to_indices(&prop);
        assert_eq!(result, vec![u32::MAX]);
    }

    #[test]
    fn ply_prop_to_f32_short_and_ushort() {
        assert!((ply_prop_to_f32(&Property::Short(-100)) - (-100.0)).abs() < f32::EPSILON);
        assert!((ply_prop_to_f32(&Property::UShort(60000)) - 60000.0).abs() < f32::EPSILON);
    }
}
