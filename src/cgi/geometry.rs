use cgmath::InnerSpace;

use super::model::{self, ModelVertex, NormalsGeometry, AABB};

pub struct RawMeshData {
    pub name: String,
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    pub normals: Option<Vec<[f32; 3]>>,
    pub tex_coords: Option<Vec<[f32; 2]>>,
    pub material_index: Option<usize>,
}

pub struct RawImageData {
    pub pixels: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

pub struct RawMaterialData {
    pub name: String,
    pub diffuse_texture_path: Option<String>,
    pub normal_texture_path: Option<String>,
    pub diffuse_texture_data: Option<RawImageData>,
    pub normal_texture_data: Option<RawImageData>,
}

pub struct RawModelData {
    pub meshes: Vec<RawMeshData>,
    pub materials: Vec<RawMaterialData>,
    pub polygon_count: usize,
}

pub fn compute_normals(positions: &[[f32; 3]], indices: &[u32]) -> Vec<[f32; 3]> {
    let mut normals = vec![[0.0f32; 3]; positions.len()];

    for c in indices.chunks(3) {
        let p0: cgmath::Vector3<f32> = positions[c[0] as usize].into();
        let p1: cgmath::Vector3<f32> = positions[c[1] as usize].into();
        let p2: cgmath::Vector3<f32> = positions[c[2] as usize].into();
        let face_normal = (p1 - p0).cross(p2 - p0);
        for &vi in c {
            let n = cgmath::Vector3::from(normals[vi as usize]) + face_normal;
            normals[vi as usize] = n.into();
        }
    }
    for n in &mut normals {
        let v = cgmath::Vector3::from(*n);
        if v.magnitude() > 0.0 {
            *n = v.normalize().into();
        }
    }
    normals
}

pub fn compute_tangent_basis(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    tex_coords: &[[f32; 2]],
    indices: &[u32],
) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let n = positions.len();
    let mut tangents = vec![[0.0f32; 3]; n];
    let mut bitangents = vec![[0.0f32; 3]; n];
    let mut triangles_included = vec![0u32; n];

    for c in indices.chunks(3) {
        let pos0: cgmath::Vector3<f32> = positions[c[0] as usize].into();
        let pos1: cgmath::Vector3<f32> = positions[c[1] as usize].into();
        let pos2: cgmath::Vector3<f32> = positions[c[2] as usize].into();

        let uv0: cgmath::Vector2<f32> = tex_coords[c[0] as usize].into();
        let uv1: cgmath::Vector2<f32> = tex_coords[c[1] as usize].into();
        let uv2: cgmath::Vector2<f32> = tex_coords[c[2] as usize].into();

        let delta_pos1 = pos1 - pos0;
        let delta_pos2 = pos2 - pos0;
        let delta_uv1 = uv1 - uv0;
        let delta_uv2 = uv2 - uv0;

        let r = 1.0 / (delta_uv1.x * delta_uv2.y - delta_uv1.y * delta_uv2.x);
        let tangent = (delta_pos1 * delta_uv2.y - delta_pos2 * delta_uv1.y) * r;
        let bitangent = (delta_pos2 * delta_uv1.x - delta_pos1 * delta_uv2.x) * -r;

        for &vi in c {
            let i = vi as usize;
            tangents[i] = (tangent + cgmath::Vector3::from(tangents[i])).into();
            bitangents[i] = (bitangent + cgmath::Vector3::from(bitangents[i])).into();
            triangles_included[i] += 1;
        }
    }

    for (i, count) in triangles_included.into_iter().enumerate() {
        if count > 0 {
            let denom = 1.0 / count as f32;
            tangents[i] = (cgmath::Vector3::from(tangents[i]) * denom).into();
            bitangents[i] = (cgmath::Vector3::from(bitangents[i]) * denom).into();
        }
    }

    let _ = normals;
    (tangents, bitangents)
}

pub fn compute_bounds(positions: &[[f32; 3]]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];

    for p in positions {
        for i in 0..3 {
            min[i] = min[i].min(p[i]);
            max[i] = max[i].max(p[i]);
        }
    }

    for i in 0..3 {
        if min[i].is_infinite() {
            min[i] = -1.0;
            max[i] = 1.0;
        }
    }

    AABB {
        min: cgmath::Point3::new(min[0], min[1], min[2]),
        max: cgmath::Point3::new(max[0], max[1], max[2]),
    }
}

pub fn build_normals_geometry(
    positions: &[[f32; 3]],
    normals: &[[f32; 3]],
    indices: &[u32],
    bounds: &AABB,
) -> NormalsGeometry {
    let mesh_diagonal = bounds.diagonal();
    let scale = if mesh_diagonal > 1e-10 {
        mesh_diagonal * 0.05
    } else {
        0.1
    };

    let mut vertex_lines: Vec<[f32; 3]> = Vec::with_capacity(positions.len() * 2);
    for (pos, normal) in positions.iter().zip(normals.iter()) {
        vertex_lines.push(*pos);
        vertex_lines.push([
            pos[0] + normal[0] * scale,
            pos[1] + normal[1] * scale,
            pos[2] + normal[2] * scale,
        ]);
    }

    let mut face_lines: Vec<[f32; 3]> = Vec::new();
    for c in indices.chunks(3) {
        let p0: cgmath::Vector3<f32> = positions[c[0] as usize].into();
        let p1: cgmath::Vector3<f32> = positions[c[1] as usize].into();
        let p2: cgmath::Vector3<f32> = positions[c[2] as usize].into();
        let edge1 = p1 - p0;
        let edge2 = p2 - p0;
        let face_normal = edge1.cross(edge2);
        if face_normal.magnitude() > 1e-10 {
            let fn_norm = face_normal.normalize();
            let center = (p0 + p1 + p2) / 3.0;
            face_lines.push([center.x, center.y, center.z]);
            face_lines.push([
                center.x + fn_norm.x * scale,
                center.y + fn_norm.y * scale,
                center.z + fn_norm.z * scale,
            ]);
        }
    }

    NormalsGeometry {
        vertex_lines,
        face_lines,
    }
}

type ProcessedModel = (Vec<Vec<ModelVertex>>, Vec<Vec<u32>>, AABB, Vec<AABB>, NormalsGeometry);

pub fn process_raw_model(raw: &RawModelData) -> ProcessedModel {
    let mut all_positions: Vec<[f32; 3]> = Vec::new();
    let mut all_normals: Vec<[f32; 3]> = Vec::new();
    let mut global_min = [f32::INFINITY; 3];
    let mut global_max = [f32::NEG_INFINITY; 3];

    let mut mesh_vertex_data: Vec<Vec<ModelVertex>> = Vec::new();
    let mut mesh_index_data: Vec<Vec<u32>> = Vec::new();
    let mut mesh_bounds: Vec<AABB> = Vec::new();
    let mut all_vertex_lines: Vec<[f32; 3]> = Vec::new();
    let mut all_face_lines: Vec<[f32; 3]> = Vec::new();

    for mesh in &raw.meshes {
        if mesh.positions.is_empty() || mesh.indices.is_empty() {
            mesh_vertex_data.push(Vec::new());
            mesh_index_data.push(Vec::new());
            mesh_bounds.push(AABB {
                min: cgmath::Point3::new(0.0, 0.0, 0.0),
                max: cgmath::Point3::new(0.0, 0.0, 0.0),
            });
            continue;
        }

        let normals = match &mesh.normals {
            Some(n) => n.clone(),
            None => compute_normals(&mesh.positions, &mesh.indices),
        };

        let tex_coords: Vec<[f32; 2]> = match &mesh.tex_coords {
            Some(tc) => tc.clone(),
            None => vec![[0.0, 0.0]; mesh.positions.len()],
        };

        let has_uvs = mesh.tex_coords.is_some();
        let (tangents, bitangents) = if has_uvs {
            compute_tangent_basis(&mesh.positions, &normals, &tex_coords, &mesh.indices)
        } else {
            (
                vec![[0.0; 3]; mesh.positions.len()],
                vec![[0.0; 3]; mesh.positions.len()],
            )
        };

        let vertices: Vec<ModelVertex> = mesh
            .positions
            .iter()
            .enumerate()
            .map(|(i, pos)| ModelVertex {
                position: *pos,
                tex_coords: tex_coords[i],
                normal: normals[i],
                tangent: tangents[i],
                bitangent: bitangents[i],
            })
            .collect();

        for p in &mesh.positions {
            for j in 0..3 {
                global_min[j] = global_min[j].min(p[j]);
                global_max[j] = global_max[j].max(p[j]);
            }
        }

        all_positions.extend_from_slice(&mesh.positions);
        all_normals.extend_from_slice(&normals);

        let bounds = compute_bounds(&mesh.positions);
        mesh_bounds.push(bounds);
        let bounds = mesh_bounds.last().unwrap();
        let normals_geo = build_normals_geometry(&mesh.positions, &normals, &mesh.indices, bounds);
        all_vertex_lines.extend(normals_geo.vertex_lines);
        all_face_lines.extend(normals_geo.face_lines);

        mesh_vertex_data.push(vertices);
        mesh_index_data.push(mesh.indices.clone());
    }

    for i in 0..3 {
        if global_min[i].is_infinite() {
            global_min[i] = -1.0;
            global_max[i] = 1.0;
        }
    }

    let bounds = model::AABB {
        min: cgmath::Point3::new(global_min[0], global_min[1], global_min[2]),
        max: cgmath::Point3::new(global_max[0], global_max[1], global_max[2]),
    };

    let normals_geo = NormalsGeometry {
        vertex_lines: all_vertex_lines,
        face_lines: all_face_lines,
    };

    (mesh_vertex_data, mesh_index_data, bounds, mesh_bounds, normals_geo)
}
