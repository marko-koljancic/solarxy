//! Vertex-normal validation: structural mismatch (count) and geometric flip
//! detection (dot product against the geometric face normal).

use crate::geometry::RawMeshData;
use crate::validation::types::{IssueKind, IssueScope, Severity, ValidationIssue};

/// Flags when a mesh declares per-vertex normals but the count disagrees with
/// the position count — usually a loader / exporter bug.
pub(super) fn check_normal_mismatch(
    mesh_index: usize,
    mesh: &RawMeshData,
) -> Option<ValidationIssue> {
    let vertex_count = mesh.positions.len();
    let normals = mesh.normals.as_ref()?;
    if normals.len() == vertex_count {
        return None;
    }
    Some(ValidationIssue {
        severity: Severity::Error,
        scope: IssueScope::Mesh(mesh_index),
        kind: IssueKind::NormalMismatch,
        message: format!(
            "Normal count ({}) does not match vertex count ({})",
            normals.len(),
            vertex_count
        ),
    })
}

/// Flags meshes where the **averaged vertex normal** of a triangle points
/// noticeably away from the **geometric (winding-derived) normal**. A negative
/// dot product means the surface was authored with reversed normals — common
/// after a mirror modifier left without "recalculate normals."
///
/// `threshold_dot` is typically `-0.5` (≈120° between geometric and shading
/// normal). Triangles with degenerate area or zero-length vertex-normal
/// averages are skipped — they're caught by other checks.
pub(super) fn check_flipped_normals(
    mesh_index: usize,
    mesh: &RawMeshData,
    threshold_dot: f32,
) -> Option<ValidationIssue> {
    let normals = mesh.normals.as_ref()?;
    if normals.len() != mesh.positions.len() {
        return None;
    }
    let mut flipped = 0_u32;
    let mut total = 0_u32;
    for tri in mesh.indices.chunks_exact(3) {
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        let (Some(p0), Some(p1), Some(p2)) = (
            mesh.positions.get(i0),
            mesh.positions.get(i1),
            mesh.positions.get(i2),
        ) else {
            continue;
        };
        let (Some(n0), Some(n1), Some(n2)) = (normals.get(i0), normals.get(i1), normals.get(i2))
        else {
            continue;
        };

        let Some(geom) = geometric_normal(p0, p1, p2) else {
            continue;
        };
        let Some(avg) = average_normal(n0, n1, n2) else {
            continue;
        };

        total += 1;
        if dot3(&geom, &avg) < threshold_dot {
            flipped += 1;
        }
    }
    if flipped == 0 {
        return None;
    }
    Some(ValidationIssue {
        severity: Severity::Warning,
        scope: IssueScope::Mesh(mesh_index),
        kind: IssueKind::FlippedNormals,
        message: format!(
            "{flipped} of {total} triangle(s) have vertex normals opposing the geometric normal"
        ),
    })
}

fn geometric_normal(p0: &[f32; 3], p1: &[f32; 3], p2: &[f32; 3]) -> Option<[f32; 3]> {
    let e1 = sub3(p1, p0);
    let e2 = sub3(p2, p0);
    normalize3(&cross3(&e1, &e2))
}

fn average_normal(n0: &[f32; 3], n1: &[f32; 3], n2: &[f32; 3]) -> Option<[f32; 3]> {
    let sum = [
        n0[0] + n1[0] + n2[0],
        n0[1] + n1[1] + n2[1],
        n0[2] + n1[2] + n2[2],
    ];
    normalize3(&sum)
}

fn sub3(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: &[f32; 3], b: &[f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot3(a: &[f32; 3], b: &[f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: &[f32; 3]) -> Option<[f32; 3]> {
    let len_sq = dot3(v, v);
    if len_sq < 1e-20 {
        return None;
    }
    let inv = len_sq.sqrt().recip();
    Some([v[0] * inv, v[1] * inv, v[2] * inv])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{RawMeshData, RawModelData};

    fn single_triangle(normals: Option<Vec<[f32; 3]>>) -> RawMeshData {
        RawMeshData {
            name: "tri".into(),
            positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            indices: vec![0, 1, 2],
            normals,
            tex_coords: None,
            material_index: None,
        }
    }

    #[test]
    fn normal_mismatch_count_disagreement() {
        let mesh = single_triangle(Some(vec![[0.0, 0.0, 1.0]; 2]));
        let issue = check_normal_mismatch(0, &mesh).expect("must flag");
        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(issue.kind, IssueKind::NormalMismatch);
    }

    #[test]
    fn normal_mismatch_clean_when_counts_align() {
        let mesh = single_triangle(Some(vec![[0.0, 0.0, 1.0]; 3]));
        assert!(check_normal_mismatch(0, &mesh).is_none());
    }

    #[test]
    fn flipped_normals_returns_none_when_no_normals() {
        let mesh = single_triangle(None);
        assert!(check_flipped_normals(0, &mesh, -0.5).is_none());
    }

    #[test]
    fn flipped_normals_clean_when_aligned() {
        let mesh = single_triangle(Some(vec![[0.0, 0.0, 1.0]; 3]));
        assert!(check_flipped_normals(0, &mesh, -0.5).is_none());
    }

    #[test]
    fn flipped_normals_flags_reversed_triangle() {
        let mesh = single_triangle(Some(vec![[0.0, 0.0, -1.0]; 3]));
        let issue = check_flipped_normals(0, &mesh, -0.5).expect("must flag");
        assert_eq!(issue.severity, Severity::Warning);
        assert_eq!(issue.kind, IssueKind::FlippedNormals);
        assert!(issue.message.contains("1 of 1"));
    }

    #[test]
    fn flipped_normals_counts_within_mixed_mesh() {
        // Two coplanar triangles; vertex 0..2 with normals UP, 3..5 with normals DOWN.
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 0.5],
            [1.0, 0.0, 0.5],
            [0.0, 1.0, 0.5],
        ];
        let normals = vec![
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, -1.0],
            [0.0, 0.0, -1.0],
        ];
        let mesh = RawMeshData {
            name: "two_tris".into(),
            positions,
            indices: vec![0, 1, 2, 3, 4, 5],
            normals: Some(normals),
            tex_coords: None,
            material_index: None,
        };
        let issue = check_flipped_normals(0, &mesh, -0.5).expect("must flag");
        assert!(issue.message.contains("1 of 2"));
    }

    #[test]
    fn flipped_normals_skips_degenerate_triangle() {
        let mesh = RawMeshData {
            name: "degenerate".into(),
            positions: vec![[0.0, 0.0, 0.0], [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            indices: vec![0, 1, 2],
            normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
            tex_coords: None,
            material_index: None,
        };
        assert!(check_flipped_normals(0, &mesh, -0.5).is_none());
    }

    #[test]
    fn flipped_normals_returns_none_when_counts_mismatch() {
        // NormalMismatch handles the structural error; flipped_normals stays silent.
        let mesh = single_triangle(Some(vec![[0.0, 0.0, 1.0]; 2]));
        assert!(check_flipped_normals(0, &mesh, -0.5).is_none());
    }

    #[test]
    fn end_to_end_via_raw_model() {
        // Sanity: the helpers compose into the orchestrator without panicking.
        let raw = RawModelData {
            meshes: vec![single_triangle(Some(vec![[0.0, 0.0, -1.0]; 3]))],
            materials: Vec::new(),
            polygon_count: 1,
        };
        let result = crate::validation::validate_raw_model(&raw, "obj");
        let kinds: Vec<_> = result.report.issues.iter().map(|i| i.kind).collect();
        assert!(kinds.contains(&IssueKind::FlippedNormals), "got {kinds:?}");
    }
}
