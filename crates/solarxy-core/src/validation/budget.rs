//! Triangle-budget validation.
//!
//! Compares the total triangle count of a model against a budget resolved by
//! the caller (typically by classifying the model's file path with
//! `project_config::FilenameClassifier` then looking the result up
//! in `project_config::Budgets` — both available with the `serialization` feature).
//!
//! Bands (where `B` = budget, `T` = `tolerance_percent / 100`):
//! - `count <= B`           → clean (no issue emitted).
//! - `B < count <= B*(1+T)` → warning.
//! - `count > B*(1+T)`      → error.
//!
//! The check is per-**file** (sum of triangles across all meshes), with
//! [`IssueScope::Model`] so the GUI surfaces it as a toast / sidebar entry
//! rather than mesh tinting (mesh tinting wouldn't tell the user which mesh
//! is the offender — a separate per-mesh "biggest contributors" UI may
//! arrive later).

use crate::geometry::RawModelData;
use crate::validation::types::{IssueKind, IssueScope, Severity, ValidationIssue};

pub(super) fn check_triangle_budget(
    raw: &RawModelData,
    budget: u32,
    tolerance_percent: f32,
) -> Option<ValidationIssue> {
    if budget == 0 {
        return None;
    }
    let count: usize = raw.meshes.iter().map(|m| m.indices.len() / 3).sum();
    let count_u32 = u32::try_from(count).unwrap_or(u32::MAX);
    let budget_f = budget as f32;
    let warning_threshold = budget_f * (1.0 + tolerance_percent / 100.0);
    let count_f = count_u32 as f32;
    if count_u32 <= budget {
        return None;
    }
    let severity = if count_f <= warning_threshold {
        Severity::Warning
    } else {
        Severity::Error
    };
    let over_pct = ((count_f - budget_f) / budget_f) * 100.0;
    Some(ValidationIssue {
        severity,
        scope: IssueScope::Model,
        kind: IssueKind::TriangleBudgetExceeded,
        message: format!("{count_u32} triangles exceeds budget of {budget} ({over_pct:.1}% over)"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{RawMeshData, RawModelData};

    fn raw_with_triangles(n: usize) -> RawModelData {
        let mut positions = Vec::with_capacity(n * 3);
        let mut indices = Vec::with_capacity(n * 3);
        for i in 0..n {
            let base = (i * 3) as u32;
            positions.push([i as f32, 0.0, 0.0]);
            positions.push([i as f32 + 1.0, 0.0, 0.0]);
            positions.push([i as f32, 1.0, 0.0]);
            indices.push(base);
            indices.push(base + 1);
            indices.push(base + 2);
        }
        RawModelData {
            meshes: vec![RawMeshData {
                name: "fixture".into(),
                positions,
                indices,
                normals: None,
                tex_coords: None,
                material_index: None,
            }],
            materials: Vec::new(),
            polygon_count: n,
        }
    }

    #[test]
    fn under_budget_is_clean() {
        let raw = raw_with_triangles(50);
        assert!(check_triangle_budget(&raw, 100, 20.0).is_none());
    }

    #[test]
    fn warning_band_emits_warning() {
        let raw = raw_with_triangles(115);
        let issue = check_triangle_budget(&raw, 100, 20.0).expect("must flag");
        assert_eq!(issue.severity, Severity::Warning);
        assert_eq!(issue.kind, IssueKind::TriangleBudgetExceeded);
        assert!(matches!(issue.scope, IssueScope::Model));
    }

    #[test]
    fn over_warning_band_emits_error() {
        let raw = raw_with_triangles(200);
        let issue = check_triangle_budget(&raw, 100, 20.0).expect("must flag");
        assert_eq!(issue.severity, Severity::Error);
    }

    #[test]
    fn zero_budget_disables_check() {
        let raw = raw_with_triangles(10_000);
        assert!(check_triangle_budget(&raw, 0, 20.0).is_none());
    }
}
