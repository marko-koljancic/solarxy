//! Model validation: detects degenerate triangles, zero-area faces,
//! missing UVs, mismatched normals, invalid material references, and other
//! findings that warrant a sidebar warning or analyzer flag.
//!
//! Entry points:
//! - [`validate_raw_model`] — defaults; the original API preserved for
//!   consumers that don't care about per-check toggles.
//! - [`validate_raw_model_with_config`] — used by the CLI when a
//!   `solarxy.toml` is in play.
//!
//! # Module layout
//!
//! - [`types`] — public taxonomy: `Severity`, `IssueScope`, `IssueKind`,
//!   `ValidationIssue`, `ValidationReport`, `ValidationResult`.
//! - [`config`] — `ValidationConfig` (per-check toggles) +
//!   `ValidationThresholds` (numeric knobs).
//! - `geometry` (private) — index-buffer and triangle-area checks
//!   (`EmptyIndices`, `NonTriangulated`, `DegenerateTriangles`).
//! - `normals` (private) — per-vertex normal checks (`NormalMismatch`,
//!   `FlippedNormals`).
//! - `manifold` (private) — edge-manifold checks (`NonManifoldEdge`).
//! - `budget` (private) — per-file triangle budget check.
//! - `uvs` (private) — UV-buffer checks (`UvMismatch`, `MissingUvs`).
//! - `materials` (private) — material-index range checks
//!   (`InvalidMaterialRef`).

pub mod config;
pub mod types;

mod budget;
mod geometry;
mod manifold;
mod materials;
mod normals;
mod uvs;

#[cfg(test)]
mod test_helpers;

pub use config::{ValidationConfig, ValidationThresholds};
pub use types::{IssueKind, IssueScope, Severity, ValidationIssue, ValidationReport, ValidationResult};

use crate::geometry::RawModelData;

/// Default entry point — convenience wrapper around
/// [`validate_raw_model_with_config`] using [`ValidationConfig::default`] and
/// [`ValidationThresholds::default`] with no triangle budget.
pub fn validate_raw_model(raw: &RawModelData, file_ext: &str) -> ValidationResult {
    let config = ValidationConfig::default();
    let thresholds = ValidationThresholds::default();
    validate_raw_model_with_config(raw, file_ext, &config, &thresholds, None)
}

/// Run validation checks against a raw model, honoring per-check toggles in
/// `config` and numeric tuning in `thresholds`. `triangle_budget`, when
/// `Some`, is the resolved per-file budget for the file's classified
/// `project_config::AssetCategory` (available with the `serialization` feature).
pub fn validate_raw_model_with_config(
    raw: &RawModelData,
    file_ext: &str,
    config: &ValidationConfig,
    thresholds: &ValidationThresholds,
    triangle_budget: Option<u32>,
) -> ValidationResult {
    let mut issues = Vec::new();
    let mut degenerate_faces = Vec::with_capacity(raw.meshes.len());

    if config.triangle_budget
        && let Some(budget) = triangle_budget
        && let Some(issue) =
            budget::check_triangle_budget(raw, budget, thresholds.triangle_budget_tolerance_percent)
    {
        issues.push(issue);
    }

    let diagonal = geometry::compute_diagonal(raw);
    let degen_epsilon = diagonal * diagonal * 1e-10;

    for (i, mesh) in raw.meshes.iter().enumerate() {
        // The triangle-only checks (index shape, degenerate area, manifold
        // edges, normals) are meaningless on line and point topologies and
        // would fire false errors (a point cloud has an "empty" index
        // buffer by definition). UV and material-reference checks stay
        // universal: they inspect per-vertex buffers and table indices,
        // not triangles.
        let is_triangles = mesh.topology == crate::geometry::MeshTopology::Triangles;

        if is_triangles
            && config.normal_mismatch
            && let Some(issue) = normals::check_normal_mismatch(i, mesh)
        {
            issues.push(issue);
        }

        if is_triangles
            && config.flipped_normals
            && let Some(issue) =
                normals::check_flipped_normals(i, mesh, thresholds.flipped_normal_dot)
        {
            issues.push(issue);
        }

        if config.uv_presence {
            issues.extend(uvs::check_uvs(i, mesh, file_ext, config.uv_presence_forced));
        }

        if is_triangles && config.index_buffer {
            issues.extend(geometry::check_indices(i, mesh));
        }

        if config.material_refs
            && let Some(issue) = materials::check_material_ref(i, mesh, raw.materials.len())
        {
            issues.push(issue);
        }

        if is_triangles && config.non_manifold_edges {
            issues.extend(manifold::check_non_manifold_edges(
                i,
                mesh,
                config.allow_open_mesh,
            ));
        }

        if is_triangles && config.degenerate_triangles {
            let (degen_issue, degen) = geometry::check_degenerate(i, mesh, degen_epsilon);
            if let Some(issue) = degen_issue {
                issues.push(issue);
            }
            degenerate_faces.push(degen);
        } else {
            degenerate_faces.push(Vec::new());
        }
    }

    ValidationResult {
        report: ValidationReport { issues },
        degenerate_faces,
    }
}

#[cfg(test)]
mod tests {
    use super::test_helpers::single_triangle_raw;
    use super::*;
    use crate::geometry::{MeshTopology, RawMeshData, RawModelData};

    fn permissive_config() -> ValidationConfig {
        ValidationConfig {
            allow_open_mesh: true,
            ..ValidationConfig::default()
        }
    }

    fn validate_default(raw: &RawModelData, file_ext: &str) -> ValidationResult {
        validate_raw_model_with_config(
            raw,
            file_ext,
            &permissive_config(),
            &ValidationThresholds::default(),
            None,
        )
    }

    #[test]
    fn clean_model_no_issues() {
        let raw = single_triangle_raw();
        let result = validate_default(&raw, "obj");
        assert!(result.report.is_clean());
        assert_eq!(result.report.error_count(), 0);
        assert_eq!(result.report.warning_count(), 0);
    }

    /// The W1d acceptance line: a point cloud has an empty index buffer by
    /// definition and a polyline's pair count is not divisible by three,
    /// so without the topology gate both would fire false Errors
    /// (EmptyIndices / NonTriangulated) plus manifold and normals noise.
    #[test]
    fn point_and_line_topologies_produce_no_false_triangle_issues() {
        let raw = RawModelData {
            meshes: vec![
                RawMeshData {
                    name: "cloud".to_string(),
                    positions: vec![[0.0; 3], [1.0; 3], [2.0; 3], [3.0; 3]],
                    indices: vec![],
                    normals: None,
                    tex_coords: None,
                    material_index: None,
                    topology: MeshTopology::Points,
                    colors: None,
                },
                RawMeshData {
                    name: "wire".to_string(),
                    positions: vec![[0.0; 3], [1.0; 3], [2.0; 3]],
                    indices: vec![0, 1, 1, 2],
                    normals: None,
                    tex_coords: None,
                    material_index: None,
                    topology: MeshTopology::Lines,
                    colors: None,
                },
            ],
            materials: vec![],
            polygon_count: 0,
        };
        let result = validate_default(&raw, "ply");
        assert!(
            result.report.is_clean(),
            "expected clean, got: {:?}",
            result.report.issues
        );
        assert_eq!(
            result.degenerate_faces.len(),
            2,
            "the per-mesh overlay vector stays aligned"
        );
    }

    #[test]
    fn normal_count_mismatch() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].normals = Some(vec![[0.0, 0.0, 1.0]; 2]);
        let result = validate_default(&raw, "obj");
        assert_eq!(result.report.error_count(), 1);
        let issue = &result.report.issues[0];
        assert_eq!(issue.severity, Severity::Error);
        assert_eq!(issue.kind, IssueKind::NormalMismatch);
    }

    #[test]
    fn multiple_issues_single_mesh() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].normals = Some(vec![[0.0, 0.0, 1.0]; 2]);
        raw.meshes[0].tex_coords = None;
        raw.meshes[0].material_index = Some(99);
        let result = validate_default(&raw, "obj");
        assert!(result.report.error_count() >= 2);
        assert!(result.report.warning_count() >= 1);
    }

    #[test]
    fn open_mesh_flagged_under_default_config() {
        let raw = single_triangle_raw();
        let result = validate_raw_model(&raw, "obj");
        let edge_issues = result
            .report
            .issues
            .iter()
            .filter(|i| matches!(i.scope, IssueScope::Edge { .. }))
            .count();
        assert_eq!(edge_issues, 3);
    }

    #[test]
    fn multi_mesh_validation() {
        let raw = RawModelData {
            meshes: vec![
                RawMeshData {
                    name: "clean".to_string(),
                    positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    indices: vec![0, 1, 2],
                    normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
                    tex_coords: Some(vec![[0.0, 0.0]; 3]),
                    material_index: None,
                    topology: MeshTopology::Triangles,
                    colors: None,
                },
                RawMeshData {
                    name: "broken".to_string(),
                    positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    indices: vec![],
                    normals: Some(vec![[0.0, 0.0, 1.0]; 2]),
                    tex_coords: None,
                    material_index: None,
                    topology: MeshTopology::Triangles,
                    colors: None,
                },
            ],
            materials: vec![],
            polygon_count: 1,
        };
        let result = validate_default(&raw, "obj");
        let mesh1_issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| matches!(i.scope, IssueScope::Mesh(1)))
            .collect();
        assert!(!mesh1_issues.is_empty());

        let mesh0_issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| matches!(i.scope, IssueScope::Mesh(0)))
            .collect();
        assert!(mesh0_issues.is_empty());
    }
}
