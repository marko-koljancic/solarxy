//! Validation result types — severity ladder, issue taxonomy, per-finding
//! payload, and the aggregating report.
//!
//! Public re-exports live on [`crate::validation`] so consumers
//! (`solarxy-cli`, `solarxy-app`, `solarxy-renderer`) refer to them at
//! stable paths (`solarxy_core::validation::ValidationReport`).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Warning,
    Error,
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Warning => write!(f, "[WARN]"),
            Severity::Error => write!(f, "[ERROR]"),
        }
    }
}

#[derive(Debug, Clone)]
pub enum IssueScope {
    Mesh(usize),
    Material(usize),
    Model,
    Face(usize, usize),
    Edge {
        mesh_index: usize,
        vertices: [u32; 2],
    },
}

impl fmt::Display for IssueScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IssueScope::Mesh(i) => write!(f, "Mesh [{}]", i),
            IssueScope::Material(i) => write!(f, "Material [{}]", i),
            IssueScope::Model => write!(f, "Model"),
            IssueScope::Face(mesh, count) => {
                write!(f, "Mesh [{}]: {} degenerate faces", mesh, count)
            }
            IssueScope::Edge {
                mesh_index,
                vertices,
            } => write!(
                f,
                "Mesh [{}] edge {}-{}",
                mesh_index, vertices[0], vertices[1]
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    NormalMismatch,
    FlippedNormals,
    UvMismatch,
    MissingUvs,
    NonTriangulated,
    EmptyIndices,
    InvalidMaterialRef,
    DegenerateTriangles,
    MissingTexture,
    NonManifoldEdge,
    TriangleBudgetExceeded,
}

/// One row in a [`ValidationReport`]. Carries everything a renderer (3D
/// overlay, CLI output, CI annotation) needs to surface a single defect:
/// where it lives (`scope`), what went wrong (`kind`), how bad
/// (`severity`), and a human-readable explanation (`message`).
///
/// `kind` is enum-typed so downstream code can switch / filter without
/// parsing the message string; `message` is intended for display, not
/// programmatic dispatch.
#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub scope: IssueScope,
    pub kind: IssueKind,
    pub message: String,
}

/// The full result of a validation run: a flat list of [`ValidationIssue`]
/// entries with no implicit ordering. Helper accessors are provided for
/// summary counts; consumers wanting structured aggregation should iterate
/// `issues` and bucket by `kind` / `severity` themselves.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count()
    }

    pub fn is_clean(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug)]
pub struct ValidationResult {
    pub report: ValidationReport,
    pub degenerate_faces: Vec<Vec<u32>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_counts() {
        let report = ValidationReport {
            issues: vec![
                ValidationIssue {
                    severity: Severity::Error,
                    scope: IssueScope::Model,
                    kind: IssueKind::EmptyIndices,
                    message: String::new(),
                },
                ValidationIssue {
                    severity: Severity::Warning,
                    scope: IssueScope::Model,
                    kind: IssueKind::MissingUvs,
                    message: String::new(),
                },
                ValidationIssue {
                    severity: Severity::Error,
                    scope: IssueScope::Model,
                    kind: IssueKind::NonTriangulated,
                    message: String::new(),
                },
            ],
        };
        assert_eq!(report.error_count(), 2);
        assert_eq!(report.warning_count(), 1);
        assert!(!report.is_clean());
        assert!(ValidationReport::default().is_clean());
    }
}
