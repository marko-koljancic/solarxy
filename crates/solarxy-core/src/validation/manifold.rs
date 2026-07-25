//! Edge-manifold validation.
//!
//! Builds an undirected edge → face-count map for each mesh, then flags
//! anything that's not a clean two-face interior edge:
//!
//! - `count == 1`: boundary edge. Reported as a `Warning` only when
//!   `allow_open_mesh` is `false`. Closed manifolds (game characters / props)
//!   should never have boundaries; arch-viz / scanned data often do.
//! - `count >= 3`: non-manifold junction. Always an `Error` — three or more
//!   faces sharing an edge indicates a topological defect (Z-fighting,
//!   inconsistent winding, modeler error).
//!
//! Output is capped at [`PER_MESH_EDGE_LIMIT`] edges per mesh; an extra
//! summary issue records the truncation count.

use std::collections::HashMap;

use crate::geometry::RawMeshData;
use crate::validation::types::{IssueKind, IssueScope, Severity, ValidationIssue};

/// Per-mesh cap on emitted edge issues; keeps reports bounded on pathological
/// meshes (e.g. exporters that lose every face's neighbor information).
pub(super) const PER_MESH_EDGE_LIMIT: usize = 1000;

pub(super) fn check_non_manifold_edges(
    mesh_index: usize,
    mesh: &RawMeshData,
    allow_open_mesh: bool,
) -> Vec<ValidationIssue> {
    let mut edges: HashMap<(u32, u32), u32> = HashMap::new();
    for tri in mesh.indices.chunks_exact(3) {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        if a == b || b == c || a == c {
            continue;
        }
        for (u, v) in [(a, b), (b, c), (a, c)] {
            let key = if u < v { (u, v) } else { (v, u) };
            *edges.entry(key).or_insert(0) += 1;
        }
    }

    let mut issues = Vec::new();
    let mut emitted = 0;
    let mut truncated = 0;
    for ((u, v), count) in edges {
        let emit = match count {
            1 => !allow_open_mesh,
            2 => false,
            _ => true,
        };
        if !emit {
            continue;
        }
        if emitted >= PER_MESH_EDGE_LIMIT {
            truncated += 1;
            continue;
        }
        let (severity, label) = match count {
            1 => (Severity::Warning, "boundary edge"),
            n if n >= 3 => (Severity::Error, "edge shared by 3+ faces"),
            _ => continue,
        };
        issues.push(ValidationIssue {
            severity,
            scope: IssueScope::Edge {
                mesh_index,
                vertices: [u, v],
            },
            kind: IssueKind::NonManifoldEdge,
            message: format!("{label} ({u}-{v}, {count} face(s))"),
        });
        emitted += 1;
    }

    if truncated > 0 {
        issues.push(ValidationIssue {
            severity: Severity::Warning,
            scope: IssueScope::Mesh(mesh_index),
            kind: IssueKind::NonManifoldEdge,
            message: format!(
                "...and {truncated} more non-manifold edge(s) elided (cap {})",
                PER_MESH_EDGE_LIMIT
            ),
        });
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{MeshTopology, RawMeshData};

    fn cube_mesh() -> RawMeshData {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.0, 0.0, 1.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 1.0],
            [0.0, 1.0, 1.0],
        ];

        let indices = vec![
            0, 1, 2, 0, 2, 3, // -Z
            4, 6, 5, 4, 7, 6, // +Z
            0, 4, 5, 0, 5, 1, // -Y
            3, 2, 6, 3, 6, 7, // +Y
            0, 3, 7, 0, 7, 4, // -X
            1, 5, 6, 1, 6, 2, // +X
        ];
        RawMeshData {
            name: "cube".into(),
            positions,
            indices,
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        }
    }

    fn open_quad_mesh() -> RawMeshData {
        RawMeshData {
            name: "quad".into(),
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        }
    }

    #[test]
    fn cube_is_manifold() {
        let mesh = cube_mesh();
        let issues = check_non_manifold_edges(0, &mesh, false);
        assert!(issues.is_empty(), "got: {issues:#?}");
    }

    #[test]
    fn open_quad_warns_on_boundary_when_not_allowed() {
        let mesh = open_quad_mesh();
        let issues = check_non_manifold_edges(0, &mesh, false);
        assert_eq!(issues.len(), 4);
        for issue in &issues {
            assert_eq!(issue.severity, Severity::Warning);
            assert_eq!(issue.kind, IssueKind::NonManifoldEdge);
            assert!(matches!(
                issue.scope,
                IssueScope::Edge { mesh_index: 0, .. }
            ));
        }
    }

    #[test]
    fn open_quad_clean_when_open_mesh_allowed() {
        let mesh = open_quad_mesh();
        let issues = check_non_manifold_edges(0, &mesh, true);
        assert!(issues.is_empty());
    }

    #[test]
    fn t_junction_errors_on_shared_edge() {
        let mesh = RawMeshData {
            name: "t".into(),
            positions: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
                [0.5, 0.0, 0.5],
            ],
            indices: vec![0, 1, 2, 1, 0, 3, 0, 1, 4],
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        };
        let issues = check_non_manifold_edges(0, &mesh, true);
        let errors: Vec<_> = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .collect();
        assert_eq!(errors.len(), 1, "expected one error edge, got: {issues:#?}");
        let issue = errors[0];
        assert_eq!(issue.kind, IssueKind::NonManifoldEdge);
        match issue.scope {
            IssueScope::Edge { vertices, .. } => {
                assert_eq!(vertices, [0, 1]);
            }
            _ => panic!("expected Edge scope, got {:?}", issue.scope),
        }
    }

    #[test]
    fn truncation_emits_summary() {
        let count = PER_MESH_EDGE_LIMIT + 50;
        let mut positions = Vec::with_capacity(count * 3);
        let mut indices = Vec::with_capacity(count * 3);
        for i in 0..count {
            let base = (i * 3) as u32;
            positions.push([i as f32, 0.0, 0.0]);
            positions.push([i as f32 + 1.0, 0.0, 0.0]);
            positions.push([i as f32, 1.0, 0.0]);
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
        }
        let mesh = RawMeshData {
            name: "boundary_storm".into(),
            positions,
            indices,
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        };
        let issues = check_non_manifold_edges(0, &mesh, false);
        let summary: Vec<_> = issues
            .iter()
            .filter(|i| matches!(i.scope, IssueScope::Mesh(_)))
            .collect();
        assert_eq!(summary.len(), 1);
        assert!(summary[0].message.contains("elided"));
    }
}
