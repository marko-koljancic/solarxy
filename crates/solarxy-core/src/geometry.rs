use std::collections::HashSet;
use std::path::PathBuf;

use cgmath::InnerSpace;

use crate::aabb::AABB;

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
    pub diffuse_texture_path: Option<PathBuf>,
    pub normal_texture_path: Option<PathBuf>,
    pub diffuse_texture_data: Option<RawImageData>,
    pub normal_texture_data: Option<RawImageData>,
    pub metallic_roughness_texture_path: Option<PathBuf>,
    pub metallic_roughness_texture_data: Option<RawImageData>,
    pub occlusion_texture_path: Option<PathBuf>,
    pub occlusion_texture_data: Option<RawImageData>,
    pub emissive_texture_path: Option<PathBuf>,
    pub emissive_texture_data: Option<RawImageData>,
    pub roughness_factor: f32,
    pub metallic_factor: f32,
    pub emissive_factor: [f32; 3],
    pub alpha_mode: u32,
    pub alpha_cutoff: f32,
    pub ambient: Option<[f32; 3]>,
    pub diffuse: Option<[f32; 3]>,
    pub specular: Option<[f32; 3]>,
    pub shininess: Option<f32>,
    pub dissolve: Option<f32>,
    pub optical_density: Option<f32>,
    pub ambient_texture_name: Option<String>,
    pub diffuse_texture_name: Option<String>,
    pub specular_texture_name: Option<String>,
    pub normal_texture_name: Option<String>,
    pub shininess_texture_name: Option<String>,
    pub dissolve_texture_name: Option<String>,
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

pub fn compute_tangent_from_normal(normals: &[[f32; 3]]) -> (Vec<[f32; 3]>, Vec<[f32; 3]>) {
    let mut tangents = Vec::with_capacity(normals.len());
    let mut bitangents = Vec::with_capacity(normals.len());
    for n in normals {
        let normal = cgmath::Vector3::from(*n);
        let up = if normal.y.abs() < 0.999 {
            cgmath::Vector3::new(0.0, 1.0, 0.0)
        } else {
            cgmath::Vector3::new(1.0, 0.0, 0.0)
        };
        let tangent = up.cross(normal).normalize();
        let bitangent = normal.cross(tangent);
        tangents.push(tangent.into());
        bitangents.push(bitangent.into());
    }
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

pub fn extract_edges(indices: &[u32]) -> Vec<u32> {
    let mut edge_set = HashSet::with_capacity(indices.len());
    for tri in indices.chunks(3) {
        if tri.len() < 3 {
            continue;
        }
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        edge_set.insert((a.min(b), a.max(b)));
        edge_set.insert((b.min(c), b.max(c)));
        edge_set.insert((a.min(c), a.max(c)));
    }
    let mut result = Vec::with_capacity(edge_set.len() * 2);
    for (i0, i1) in edge_set {
        result.push(i0);
        result.push(i1);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_vec3_approx(a: [f32; 3], b: [f32; 3], eps: f32) {
        assert!(
            (a[0] - b[0]).abs() < eps && (a[1] - b[1]).abs() < eps && (a[2] - b[2]).abs() < eps,
            "expected {:?} ≈ {:?}",
            a,
            b
        );
    }

    #[test]
    fn compute_normals_single_triangle() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let indices = [0u32, 1, 2];
        let normals = compute_normals(&positions, &indices);
        assert_eq!(normals.len(), 3);
        for n in &normals {
            assert_vec3_approx(*n, [0.0, 0.0, 1.0], 1e-6);
        }
    }

    #[test]
    fn compute_normals_degenerate() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let indices = [0u32, 1, 2];
        let normals = compute_normals(&positions, &indices);
        assert_eq!(normals.len(), 3);
        for n in &normals {
            let mag = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            assert!(
                mag < 1e-6,
                "degenerate triangle normal should be near zero, got magnitude {}",
                mag
            );
        }
    }

    #[test]
    fn compute_bounds_single_point() {
        let positions = [[3.0, -1.0, 2.5]];
        let bounds = compute_bounds(&positions);
        assert!((bounds.min.x - 3.0).abs() < 1e-6);
        assert!((bounds.min.y - (-1.0)).abs() < 1e-6);
        assert!((bounds.min.z - 2.5).abs() < 1e-6);
        assert!((bounds.max.x - 3.0).abs() < 1e-6);
        assert!((bounds.max.y - (-1.0)).abs() < 1e-6);
        assert!((bounds.max.z - 2.5).abs() < 1e-6);
    }

    #[test]
    fn compute_bounds_cube() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [0.0, 1.0, 1.0],
            [1.0, 1.0, 1.0],
        ];
        let bounds = compute_bounds(&positions);
        assert!((bounds.min.x - 0.0).abs() < 1e-6);
        assert!((bounds.min.y - 0.0).abs() < 1e-6);
        assert!((bounds.min.z - 0.0).abs() < 1e-6);
        assert!((bounds.max.x - 1.0).abs() < 1e-6);
        assert!((bounds.max.y - 1.0).abs() < 1e-6);
        assert!((bounds.max.z - 1.0).abs() < 1e-6);
    }

    #[test]
    fn compute_bounds_negative() {
        let positions = [[-5.0, -3.0, -1.0], [-2.0, -4.0, -6.0]];
        let bounds = compute_bounds(&positions);
        assert!((bounds.min.x - (-5.0)).abs() < 1e-6);
        assert!((bounds.min.y - (-4.0)).abs() < 1e-6);
        assert!((bounds.min.z - (-6.0)).abs() < 1e-6);
        assert!((bounds.max.x - (-2.0)).abs() < 1e-6);
        assert!((bounds.max.y - (-3.0)).abs() < 1e-6);
        assert!((bounds.max.z - (-1.0)).abs() < 1e-6);
    }

    #[test]
    fn compute_tangent_basis_unit_triangle() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals = [[0.0, 0.0, 1.0]; 3];
        let tex_coords = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let indices = [0u32, 1, 2];
        let (tangents, _bitangents) =
            compute_tangent_basis(&positions, &normals, &tex_coords, &indices);
        assert_eq!(tangents.len(), 3);
        for t in &tangents {
            assert!(
                (t[0] - 1.0).abs() < 1e-5,
                "tangent X should be ~1.0, got {}",
                t[0]
            );
            assert!(t[1].abs() < 1e-5, "tangent Y should be ~0.0, got {}", t[1]);
            assert!(t[2].abs() < 1e-5, "tangent Z should be ~0.0, got {}", t[2]);
        }
    }

    #[test]
    fn compute_tangent_basis_perpendicular() {
        let positions = [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
        let normals_data = [[0.0, 0.0, 1.0]; 3];
        let tex_coords = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let indices = [0u32, 1, 2];
        let (tangents, _) = compute_tangent_basis(&positions, &normals_data, &tex_coords, &indices);
        for (t, n) in tangents.iter().zip(normals_data.iter()) {
            let dot = t[0] * n[0] + t[1] * n[1] + t[2] * n[2];
            assert!(
                dot.abs() < 1e-5,
                "tangent dot normal should be ~0, got {}",
                dot
            );
        }
    }
}
