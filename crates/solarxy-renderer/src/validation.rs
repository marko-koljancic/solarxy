//! GPU-side resources for the validation overlay: per-mesh validation index
//! buffer + the bind group consumed by `validation.wgsl`. The CPU-side
//! findings live in `solarxy_core::validation`.

pub use solarxy_core::validation::*;
use solarxy_core::RawModelData;

pub struct ViewerValidation {
    pub report: ValidationReport,
    pub degenerate_faces: Vec<Vec<u32>>,
    pub raw_to_gpu: Vec<Option<usize>>,
}

pub fn validate_raw_model(raw: &RawModelData, file_ext: &str) -> ViewerValidation {
    let r: ValidationResult = solarxy_core::validation::validate_raw_model(raw, file_ext);
    ViewerValidation {
        report: r.report,
        degenerate_faces: r.degenerate_faces,
        raw_to_gpu: Vec::new(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IssueCategory {
    Error,
    InvalidMaterial,
    NormalMismatch,
    MissingUvs,
    DegenerateTriangles,
}

impl IssueCategory {
    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Error => [1.0, 0.0, 0.0, 0.4],
            Self::InvalidMaterial => [1.0, 0.45, 0.0, 0.4],
            Self::NormalMismatch => [0.0, 0.85, 1.0, 0.4],
            Self::MissingUvs => [1.0, 0.0, 0.8, 0.4],
            Self::DegenerateTriangles => [1.0, 0.9, 0.0, 0.4],
        }
    }

    pub const ALL: &[Self] = &[
        Self::Error,
        Self::InvalidMaterial,
        Self::NormalMismatch,
        Self::MissingUvs,
        Self::DegenerateTriangles,
    ];
}

pub fn issue_category(issue: &ValidationIssue) -> IssueCategory {
    match issue.kind {
        IssueKind::InvalidMaterialRef => IssueCategory::InvalidMaterial,
        IssueKind::NormalMismatch => IssueCategory::NormalMismatch,
        IssueKind::MissingUvs | IssueKind::UvMismatch => IssueCategory::MissingUvs,
        IssueKind::DegenerateTriangles => IssueCategory::DegenerateTriangles,
        _ => IssueCategory::Error,
    }
}

pub fn build_mesh_category_map(
    report: &ValidationReport,
    gpu_mesh_count: usize,
    raw_to_gpu: &[Option<usize>],
) -> Vec<Option<usize>> {
    let mut categories: Vec<Option<usize>> = vec![None; gpu_mesh_count];
    let mut priorities: Vec<u8> = vec![0; gpu_mesh_count];

    for issue in &report.issues {
        let raw_idx = match &issue.scope {
            IssueScope::Mesh(i) => *i,
            _ => continue,
        };
        let Some(Some(gpu_idx)) = raw_to_gpu.get(raw_idx) else {
            continue;
        };
        let cat = issue_category(issue);
        let cat_idx = IssueCategory::ALL
            .iter()
            .position(|c| *c == cat)
            .unwrap_or(0);
        let priority = match issue.severity {
            Severity::Error => 2,
            Severity::Warning => 1,
        };
        if priority > priorities[*gpu_idx] {
            priorities[*gpu_idx] = priority;
            categories[*gpu_idx] = Some(cat_idx);
        }
    }

    categories
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn issue_category_mapping() {
        let cases = [
            (
                IssueKind::InvalidMaterialRef,
                IssueCategory::InvalidMaterial,
            ),
            (IssueKind::NormalMismatch, IssueCategory::NormalMismatch),
            (IssueKind::MissingUvs, IssueCategory::MissingUvs),
            (IssueKind::UvMismatch, IssueCategory::MissingUvs),
            (
                IssueKind::DegenerateTriangles,
                IssueCategory::DegenerateTriangles,
            ),
            (IssueKind::NonTriangulated, IssueCategory::Error),
            (IssueKind::EmptyIndices, IssueCategory::Error),
            (IssueKind::MissingTexture, IssueCategory::Error),
        ];
        for (kind, expected_cat) in cases {
            let issue = ValidationIssue {
                severity: Severity::Warning,
                scope: IssueScope::Mesh(0),
                kind,
                message: String::new(),
            };
            assert_eq!(
                issue_category(&issue),
                expected_cat,
                "failed for {:?}",
                kind
            );
        }
    }

    #[test]
    fn build_mesh_category_map_priorities() {
        let report = ValidationReport {
            issues: vec![
                ValidationIssue {
                    severity: Severity::Warning,
                    scope: IssueScope::Mesh(0),
                    kind: IssueKind::MissingUvs,
                    message: String::new(),
                },
                ValidationIssue {
                    severity: Severity::Error,
                    scope: IssueScope::Mesh(0),
                    kind: IssueKind::NormalMismatch,
                    message: String::new(),
                },
            ],
        };
        let raw_to_gpu = vec![Some(0)];
        let cats = build_mesh_category_map(&report, 1, &raw_to_gpu);
        let cat = cats[0].unwrap();
        let expected = IssueCategory::ALL
            .iter()
            .position(|c| *c == IssueCategory::NormalMismatch)
            .unwrap();
        assert_eq!(cat, expected);
    }
}
