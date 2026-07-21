//! UV-buffer checks: count mismatch against positions, and missing UVs for
//! formats that are expected to carry them (OBJ / glTF / GLB). PLY and STL
//! intentionally exempt — they don't model textures at the format level.

use super::types::{IssueKind, IssueScope, Severity, ValidationIssue};
use crate::geometry::RawMeshData;

/// Whether a file extension is expected to carry per-vertex UVs. Used to
/// suppress `MissingUvs` warnings on formats where the absence is normal.
pub(super) fn supports_uvs(file_ext: &str) -> bool {
    matches!(
        file_ext.to_ascii_lowercase().as_str(),
        "obj" | "gltf" | "glb"
    )
}

/// UV checks for one mesh: count-mismatch warning + missing-UV warning.
/// The missing-UV warning fires when the source format is expected to carry
/// UVs (OBJ / glTF / GLB) or when `forced` is set (the validate node's
/// opt-in `require_uvs`, which flags UV-less geometry regardless of format).
pub(super) fn check_uvs(
    mesh_index: usize,
    mesh: &RawMeshData,
    file_ext: &str,
    forced: bool,
) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let vertex_count = mesh.positions.len();

    if let Some(ref tex_coords) = mesh.tex_coords
        && tex_coords.len() != vertex_count
    {
        issues.push(ValidationIssue {
            severity: Severity::Warning,
            scope: IssueScope::Mesh(mesh_index),
            kind: IssueKind::UvMismatch,
            message: format!(
                "Texture coordinate count ({}) does not match vertex count ({})",
                tex_coords.len(),
                vertex_count
            ),
        });
    }

    if mesh.tex_coords.is_none() && (forced || supports_uvs(file_ext)) {
        issues.push(ValidationIssue {
            severity: Severity::Warning,
            scope: IssueScope::Mesh(mesh_index),
            kind: IssueKind::MissingUvs,
            message: "No texture coordinates".to_string(),
        });
    }

    issues
}

#[cfg(test)]
mod tests {
    use super::super::test_helpers::single_triangle_raw;
    use super::super::validate_raw_model;
    use super::*;

    #[test]
    fn supports_uvs_by_format() {
        assert!(supports_uvs("obj"));
        assert!(supports_uvs("gltf"));
        assert!(supports_uvs("glb"));
        assert!(supports_uvs("OBJ"));
        assert!(!supports_uvs("stl"));
        assert!(!supports_uvs("ply"));
        assert!(!supports_uvs("fbx"));
    }

    #[test]
    fn uv_count_mismatch() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].tex_coords = Some(vec![[0.0, 0.0]; 2]);
        let result = validate_raw_model(&raw, "obj");
        let uv_issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::UvMismatch)
            .collect();
        assert_eq!(uv_issues.len(), 1);
        assert_eq!(uv_issues[0].severity, Severity::Warning);
    }

    #[test]
    fn missing_uvs_obj() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].tex_coords = None;
        let result = validate_raw_model(&raw, "obj");
        let missing: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::MissingUvs)
            .collect();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].severity, Severity::Warning);
    }

    #[test]
    fn missing_uvs_stl_no_warning() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].tex_coords = None;
        let result = validate_raw_model(&raw, "stl");
        let missing: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::MissingUvs)
            .collect();
        assert!(missing.is_empty());
    }

    fn missing_count(issues: &[ValidationIssue]) -> usize {
        issues
            .iter()
            .filter(|i| i.kind == IssueKind::MissingUvs)
            .count()
    }

    #[test]
    fn forced_flags_missing_uvs_regardless_of_format() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].tex_coords = None;
        // STL normally exempts missing UVs; forcing overrides the format gate.
        let issues = check_uvs(0, &raw.meshes[0], "stl", true);
        assert_eq!(missing_count(&issues), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
    }

    #[test]
    fn forced_off_stays_quiet_on_unsupported_format() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].tex_coords = None;
        let issues = check_uvs(0, &raw.meshes[0], "stl", false);
        assert_eq!(missing_count(&issues), 0);
    }

    #[test]
    fn forced_flags_cooked_geometry_with_no_source_format() {
        // The validate node cooks with file_ext = ""; forcing makes the
        // otherwise-unreachable check fire, and leaving it off keeps the
        // pre-0.7.2 behavior (empty ext suppresses the warning).
        let mut raw = single_triangle_raw();
        raw.meshes[0].tex_coords = None;
        assert_eq!(missing_count(&check_uvs(0, &raw.meshes[0], "", true)), 1);
        assert_eq!(missing_count(&check_uvs(0, &raw.meshes[0], "", false)), 0);
    }
}
