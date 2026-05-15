//! Model validation: detects degenerate triangles, zero-area faces,
//! missing UVs, mismatched normals, invalid material references, and other
//! findings that warrant a sidebar warning or analyzer flag.
//!
//! Entry point: [`validate_raw_model`]. Result: [`ValidationResult`] — a
//! [`ValidationReport`] (the full set of findings) plus per-mesh
//! degenerate-face index lists the renderer uses to drive the validation
//! overlay shader.
//!
//! # Module layout
//!
//! - [`types`] — public taxonomy: `Severity`, `IssueScope`, `IssueKind`,
//!   `ValidationIssue`, `ValidationReport`, `ValidationResult`
//!   (re-exported below; stable paths preserved for crate consumers).
//! - `geometry` (private) — index-buffer and triangle-area checks
//!   (`EmptyIndices`, `NonTriangulated`, `DegenerateTriangles`).
//! - `uvs` (private) — UV-buffer checks (`UvMismatch`, `MissingUvs`).
//! - `materials` (private) — material-index range checks
//!   (`InvalidMaterialRef`).
//!
//! The `NormalMismatch` check lives inline in the orchestrator below
//! until a dedicated `normals` module lands with `FlippedNormals` in a
//! later stream.

pub mod types;

mod geometry;
mod materials;
mod uvs;

#[cfg(test)]
mod test_helpers;

pub use types::{IssueKind, IssueScope, Severity, ValidationIssue, ValidationReport, ValidationResult};

use crate::geometry::RawModelData;

/// Run all validation checks against a raw model. `file_ext` (e.g. `"obj"`,
/// `"glb"`) is used to suppress format-inappropriate warnings — see the
/// private `uvs::supports_uvs` for the format whitelist.
pub fn validate_raw_model(raw: &RawModelData, file_ext: &str) -> ValidationResult {
    let mut issues = Vec::new();
    let mut degenerate_faces = Vec::with_capacity(raw.meshes.len());

    let diagonal = geometry::compute_diagonal(raw);
    let degen_epsilon = diagonal * diagonal * 1e-10;

    for (i, mesh) in raw.meshes.iter().enumerate() {
        let vertex_count = mesh.positions.len();

        if let Some(ref normals) = mesh.normals
            && normals.len() != vertex_count
        {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::NormalMismatch,
                message: format!(
                    "Normal count ({}) does not match vertex count ({})",
                    normals.len(),
                    vertex_count
                ),
            });
        }

        issues.extend(uvs::check_uvs(i, mesh, file_ext));
        issues.extend(geometry::check_indices(i, mesh));

        if let Some(issue) = materials::check_material_ref(i, mesh, raw.materials.len()) {
            issues.push(issue);
        }

        let (degen_issue, degen) = geometry::check_degenerate(i, mesh, degen_epsilon);
        if let Some(issue) = degen_issue {
            issues.push(issue);
        }
        degenerate_faces.push(degen);
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
    use crate::geometry::{RawMeshData, RawModelData};

    #[test]
    fn clean_model_no_issues() {
        let raw = single_triangle_raw();
        let result = validate_raw_model(&raw, "obj");
        assert!(result.report.is_clean());
        assert_eq!(result.report.error_count(), 0);
        assert_eq!(result.report.warning_count(), 0);
    }

    #[test]
    fn normal_count_mismatch() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].normals = Some(vec![[0.0, 0.0, 1.0]; 2]);
        let result = validate_raw_model(&raw, "obj");
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
        let result = validate_raw_model(&raw, "obj");
        assert!(result.report.error_count() >= 2);
        assert!(result.report.warning_count() >= 1);
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
                },
                RawMeshData {
                    name: "broken".to_string(),
                    positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                    indices: vec![],
                    normals: Some(vec![[0.0, 0.0, 1.0]; 2]),
                    tex_coords: None,
                    material_index: None,
                },
            ],
            materials: vec![],
            polygon_count: 1,
        };
        let result = validate_raw_model(&raw, "obj");
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
