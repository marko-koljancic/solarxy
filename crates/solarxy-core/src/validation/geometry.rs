//! Geometry-shape checks: empty / non-triangulated index buffers and
//! degenerate (zero-area) triangles, plus the per-model diagonal helper
//! used to scale per-mesh epsilons consistently across model scales.

use cgmath::InnerSpace;

use super::types::{IssueKind, IssueScope, Severity, ValidationIssue};
use crate::geometry::{RawMeshData, RawModelData};

/// World-space diagonal of the model's AABB. Used as a length scale so the
/// degenerate-triangle area threshold behaves consistently across model
/// sizes (a millimetre-scale prop and a kilometre-scale terrain shouldn't
/// share the same absolute epsilon).
pub(super) fn compute_diagonal(raw: &RawModelData) -> f32 {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut any = false;
    for mesh in &raw.meshes {
        for p in &mesh.positions {
            any = true;
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }
    if !any {
        return 1.0;
    }
    let dx = max[0] - min[0];
    let dy = max[1] - min[1];
    let dz = max[2] - min[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

/// Per-mesh face indices (0-based) for triangles whose cross-product area
/// is below `epsilon`. Skips faces whose vertex indices are out of range
/// or whose buffer slice is shorter than 3 entries.
pub(super) fn detect_degenerate_triangles(
    positions: &[[f32; 3]],
    indices: &[u32],
    epsilon: f32,
) -> Vec<u32> {
    let mut degenerate = Vec::new();
    for (face_idx, tri) in indices.chunks(3).enumerate() {
        if tri.len() < 3 {
            continue;
        }
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        let p0 = cgmath::Vector3::from(positions[i0]);
        let p1 = cgmath::Vector3::from(positions[i1]);
        let p2 = cgmath::Vector3::from(positions[i2]);
        let cross = (p1 - p0).cross(p2 - p0);
        let area = cross.magnitude() * 0.5;
        if area < epsilon {
            degenerate.push(face_idx as u32);
        }
    }
    degenerate
}

/// Index-buffer integrity check: empty buffer or length not divisible
/// by 3 (i.e. unmistakably non-triangulated).
pub(super) fn check_indices(mesh_index: usize, mesh: &RawMeshData) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let index_count = mesh.indices.len();

    if !index_count.is_multiple_of(3) {
        issues.push(ValidationIssue {
            severity: Severity::Error,
            scope: IssueScope::Mesh(mesh_index),
            kind: IssueKind::NonTriangulated,
            message: format!(
                "Index count ({}) is not divisible by 3 (non-triangulated)",
                index_count
            ),
        });
    }

    if mesh.indices.is_empty() {
        issues.push(ValidationIssue {
            severity: Severity::Error,
            scope: IssueScope::Mesh(mesh_index),
            kind: IssueKind::EmptyIndices,
            message: "Empty index buffer".to_string(),
        });
    }

    issues
}

/// Degenerate-triangle check: returns the aggregated issue (if any) plus
/// the per-mesh face-index list the renderer overlay consumes.
pub(super) fn check_degenerate(
    mesh_index: usize,
    mesh: &RawMeshData,
    degen_epsilon: f32,
) -> (Option<ValidationIssue>, Vec<u32>) {
    let degen = detect_degenerate_triangles(&mesh.positions, &mesh.indices, degen_epsilon);
    let issue = if degen.is_empty() {
        None
    } else {
        Some(ValidationIssue {
            severity: Severity::Warning,
            scope: IssueScope::Face(mesh_index, degen.len()),
            kind: IssueKind::DegenerateTriangles,
            message: format!("{} degenerate triangles detected", degen.len()),
        })
    };
    (issue, degen)
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::single_triangle_raw;
    use super::super::validate_raw_model;
    use super::*;
    use crate::geometry::{MeshTopology, RawMeshData, RawModelData};

    #[test]
    fn non_triangulated() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].indices = vec![0, 1];
        raw.meshes[0].normals = Some(vec![[0.0, 0.0, 1.0]; 3]);
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::NonTriangulated)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn empty_indices() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].indices = vec![];
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::EmptyIndices)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn degenerate_triangles() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::DegenerateTriangles)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(result.degenerate_faces[0], vec![0]);
    }

    #[test]
    fn compute_diagonal_single_point() {
        let raw = RawModelData {
            meshes: vec![RawMeshData {
                name: "pt".to_string(),
                positions: vec![[5.0, 5.0, 5.0]],
                indices: vec![0, 0, 0],
                normals: None,
                tex_coords: None,
                material_index: None,
                topology: MeshTopology::Triangles,
            }],
            materials: vec![],
            polygon_count: 0,
        };
        assert!((compute_diagonal(&raw)).abs() < f32::EPSILON);
    }

    #[test]
    fn compute_diagonal_unit_cube() {
        let raw = RawModelData {
            meshes: vec![RawMeshData {
                name: "cube".to_string(),
                positions: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
                indices: vec![0, 1, 0],
                normals: None,
                tex_coords: None,
                material_index: None,
                topology: MeshTopology::Triangles,
            }],
            materials: vec![],
            polygon_count: 0,
        };
        assert!((compute_diagonal(&raw) - 3.0_f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn compute_diagonal_no_vertices() {
        let raw = RawModelData {
            meshes: vec![],
            materials: vec![],
            polygon_count: 0,
        };
        assert!((compute_diagonal(&raw) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn degenerate_triangle_collinear() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let indices = vec![0, 1, 2];
        let result = detect_degenerate_triangles(&positions, &indices, 1e-6);
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn degenerate_triangle_coincident() {
        let positions = vec![[1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]];
        let indices = vec![0, 1, 2];
        let result = detect_degenerate_triangles(&positions, &indices, 1e-6);
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn degenerate_large_model_epsilon_scaling() {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [0.0, 1000.0, 0.0],
            [500.0, 500.0, 0.0],
            [500.1, 500.0, 0.0],
            [500.0, 500.1, 0.0],
        ];
        let indices = vec![0, 1, 2, 3, 4, 5];
        let diagonal = (1000.0_f32 * 1000.0 + 1000.0 * 1000.0_f32).sqrt();
        let epsilon = diagonal * diagonal * 1e-10;
        let result = detect_degenerate_triangles(&positions, &indices, epsilon);
        assert!(
            result.is_empty(),
            "Small valid triangle should not be flagged"
        );
    }
}
