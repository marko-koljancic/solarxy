pub use solarxy_core::geometry::{
    compute_bounds, compute_normals, compute_tangent_basis, compute_tangent_from_normal,
    extract_edges, RawImageData, RawMaterialData, RawMeshData, RawModelData,
};
pub use solarxy_core::AABB;

use cgmath::InnerSpace;
use super::model::{self, ModelVertex, NormalsGeometry};

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

type ProcessedModel = (
    Vec<Vec<ModelVertex>>,
    Vec<Vec<u32>>,
    AABB,
    Vec<AABB>,
    NormalsGeometry,
);

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
            compute_tangent_from_normal(&normals)
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
        let normals_geo =
            build_normals_geometry(&mesh.positions, &normals, &mesh.indices, &bounds);
        mesh_bounds.push(bounds);
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

    (
        mesh_vertex_data,
        mesh_index_data,
        bounds,
        mesh_bounds,
        normals_geo,
    )
}
