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
//! summary issue records the truncation count. Edges are reported in
//! ascending vertex-pair order, and the cap applies to that order, so the
//! same mesh always reports the same issues: a report is something people
//! diff between runs and gate pipelines on.

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
    for tri in mesh.indices.as_chunks::<3>().0 {
        let (a, b, c) = (tri[0], tri[1], tri[2]);
        if a == b || b == c || a == c {
            continue;
        }
        for (u, v) in [(a, b), (b, c), (a, c)] {
            let key = if u < v { (u, v) } else { (v, u) };
            *edges.entry(key).or_insert(0) += 1;
        }
    }

    // Select the offending edges, then ORDER them before the cap applies.
    //
    // Iterating the map directly decided both the order and, once the cap
    // bit, the selection: a hash map's iteration order is seeded per
    // instance, so the same build validating the same file reported a
    // different thousand edges every run. That reached the analyze output,
    // the Properties panel, the browser's validation pane, and the JSON a
    // pipeline gates on.
    //
    // The sort is deliberately here rather than in the accumulation above.
    // A healthy mesh has no offenders and pays nothing, whereas an ordered
    // map would pay on every edge of every mesh to fix a problem that only
    // exists at the reporting end.
    let mut offenders: Vec<((u32, u32), u32)> = edges
        .into_iter()
        .filter(|&(_, count)| match count {
            1 => !allow_open_mesh,
            2 => false,
            _ => true,
        })
        .collect();
    offenders.sort_unstable();

    let mut issues = Vec::new();
    let mut truncated = 0;
    for ((u, v), count) in offenders {
        if issues.len() >= PER_MESH_EDGE_LIMIT {
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

    /// A mesh past the cap must report the SAME edges every time.
    ///
    /// The check accumulates into a hash map, whose iteration order is
    /// seeded per instance, so two calls in one process see different
    /// orders. Before the sort that meant a different thousand edges
    /// survived truncation on every run, and nothing anywhere said so: the
    /// counts matched, the severities matched, and only the identities
    /// moved.
    #[test]
    fn a_capped_report_is_the_same_report_every_time() {
        let mesh = boundary_storm(PER_MESH_EDGE_LIMIT + 500);
        let first = check_non_manifold_edges(0, &mesh, false);
        let second = check_non_manifold_edges(0, &mesh, false);

        assert!(
            first.len() > PER_MESH_EDGE_LIMIT,
            "the fixture has to exceed the cap for this to test anything"
        );
        let edges = |issues: &[ValidationIssue]| -> Vec<[u32; 2]> {
            issues
                .iter()
                .filter_map(|i| match i.scope {
                    IssueScope::Edge { vertices, .. } => Some(vertices),
                    _ => None,
                })
                .collect()
        };
        assert_eq!(
            edges(&first),
            edges(&second),
            "the same mesh reported a different set of edges on a second run"
        );
        assert_eq!(
            first.iter().map(|i| &i.message).collect::<Vec<_>>(),
            second.iter().map(|i| &i.message).collect::<Vec<_>>(),
        );
    }

    /// And the order is the documented one, so a reader (or a diff) can
    /// rely on it rather than on whatever the map happened to yield.
    #[test]
    fn edges_report_in_ascending_vertex_order() {
        let mesh = boundary_storm(40);
        let issues = check_non_manifold_edges(0, &mesh, false);
        let edges: Vec<[u32; 2]> = issues
            .iter()
            .filter_map(|i| match i.scope {
                IssueScope::Edge { vertices, .. } => Some(vertices),
                _ => None,
            })
            .collect();
        let mut sorted = edges.clone();
        sorted.sort_unstable();
        assert_eq!(edges, sorted, "edges are not in ascending order");
    }

    /// `count` disconnected triangles, so every edge is a boundary edge:
    /// `3 * count` offenders from one cheap fixture.
    fn boundary_storm(count: usize) -> RawMeshData {
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
        RawMeshData {
            name: "boundary_storm".into(),
            positions,
            indices,
            normals: None,
            tex_coords: None,
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        }
    }

    #[test]
    fn truncation_emits_summary() {
        let mesh = boundary_storm(PER_MESH_EDGE_LIMIT + 50);
        let issues = check_non_manifold_edges(0, &mesh, false);
        let summary: Vec<_> = issues
            .iter()
            .filter(|i| matches!(i.scope, IssueScope::Mesh(_)))
            .collect();
        assert_eq!(summary.len(), 1);
        assert!(summary[0].message.contains("elided"));
    }
}
