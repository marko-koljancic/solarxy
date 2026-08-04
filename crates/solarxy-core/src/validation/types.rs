//! Validation result types — severity ladder, issue taxonomy, per-finding
//! payload, and the aggregating report.
//!
//! Public re-exports live on [`crate::validation`] so consumers
//! (`solarxy-cli`, `solarxy-app`, `solarxy-renderer`) refer to them at
//! stable paths (`solarxy_core::validation::ValidationReport`).

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
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
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
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
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
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

impl IssueKind {
    /// Every variant, in declaration order.
    ///
    /// The order is fixed and total on purpose: the text report, the
    /// terminal shell and the desktop panels each render a row per kind,
    /// and three independent orderings would disagree about which kind
    /// comes first for the same model.
    pub const ALL: [IssueKind; 11] = [
        IssueKind::NormalMismatch,
        IssueKind::FlippedNormals,
        IssueKind::UvMismatch,
        IssueKind::MissingUvs,
        IssueKind::NonTriangulated,
        IssueKind::EmptyIndices,
        IssueKind::InvalidMaterialRef,
        IssueKind::DegenerateTriangles,
        IssueKind::MissingTexture,
        IssueKind::NonManifoldEdge,
        IssueKind::TriangleBudgetExceeded,
    ];
}

/// The human-readable name of a kind, for any surface that groups issues
/// by what went wrong rather than by where.
///
/// Distinct from the wire spelling in `crate::json`, which is a stable
/// identifier consumers match on and must not be prettified.
impl fmt::Display for IssueKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            IssueKind::NormalMismatch => "Normal mismatch",
            IssueKind::FlippedNormals => "Flipped normals",
            IssueKind::UvMismatch => "UV mismatch",
            IssueKind::MissingUvs => "Missing UVs",
            IssueKind::NonTriangulated => "Non-triangulated",
            IssueKind::EmptyIndices => "Empty indices",
            IssueKind::InvalidMaterialRef => "Invalid material reference",
            IssueKind::DegenerateTriangles => "Degenerate triangles",
            IssueKind::MissingTexture => "Missing texture",
            IssueKind::NonManifoldEdge => "Non-manifold edge",
            IssueKind::TriangleBudgetExceeded => "Triangle budget exceeded",
        })
    }
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
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
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
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
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

    /// How many issues carry each kind, in [`IssueKind::ALL`] order.
    ///
    /// Total coverage, so a caller drawing a row per kind always draws the
    /// same rows in the same sequence whatever the model. Callers wanting
    /// only the kinds actually present want [`Self::ranked_kinds`].
    pub fn counts_by_kind(&self) -> [(IssueKind, usize); 11] {
        IssueKind::ALL.map(|kind| (kind, self.issues.iter().filter(|i| i.kind == kind).count()))
    }

    /// The kinds actually present, most frequent first.
    ///
    /// Ties break on [`IssueKind::ALL`] order (the sort is stable), so two
    /// kinds with equal counts always appear in the same sequence rather
    /// than in whatever order the issues happened to be raised.
    pub fn ranked_kinds(&self) -> Vec<(IssueKind, usize)> {
        let mut ranked: Vec<(IssueKind, usize)> = self
            .counts_by_kind()
            .into_iter()
            .filter(|&(_, count)| count > 0)
            .collect();
        ranked.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        ranked
    }
}

#[derive(Debug)]
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(rename_all = "camelCase")
)]
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

    fn issue(kind: IssueKind) -> ValidationIssue {
        ValidationIssue {
            severity: Severity::Warning,
            scope: IssueScope::Model,
            kind,
            message: String::new(),
        }
    }

    /// Every kind is represented whether or not it occurred, and the counts
    /// add up to the issue list. A surface drawing a row per kind depends on
    /// both halves.
    #[test]
    fn counts_by_kind_covers_every_variant() {
        let report = ValidationReport {
            issues: vec![
                issue(IssueKind::MissingUvs),
                issue(IssueKind::MissingUvs),
                issue(IssueKind::NonManifoldEdge),
            ],
        };
        let counts = report.counts_by_kind();
        assert_eq!(counts.len(), IssueKind::ALL.len());
        for (i, (kind, _)) in counts.iter().enumerate() {
            assert_eq!(*kind, IssueKind::ALL[i], "declaration order was not kept");
        }
        assert_eq!(
            counts.iter().map(|&(_, n)| n).sum::<usize>(),
            report.issues.len()
        );
        assert!(
            ValidationReport::default()
                .counts_by_kind()
                .iter()
                .all(|&(_, n)| n == 0)
        );
    }

    /// Most frequent first, and absent kinds are dropped rather than drawn
    /// as empty rows.
    #[test]
    fn ranked_kinds_orders_by_frequency() {
        let report = ValidationReport {
            issues: vec![
                issue(IssueKind::NonManifoldEdge),
                issue(IssueKind::MissingUvs),
                issue(IssueKind::MissingUvs),
                issue(IssueKind::MissingUvs),
                issue(IssueKind::FlippedNormals),
                issue(IssueKind::FlippedNormals),
            ],
        };
        assert_eq!(
            report.ranked_kinds(),
            vec![
                (IssueKind::MissingUvs, 3),
                (IssueKind::FlippedNormals, 2),
                (IssueKind::NonManifoldEdge, 1),
            ]
        );
        assert!(ValidationReport::default().ranked_kinds().is_empty());
    }

    /// A tie must not depend on the order the checks happened to run in,
    /// or the same model would render two different orderings on two runs.
    #[test]
    fn ranked_kinds_breaks_ties_on_declaration_order() {
        let report = ValidationReport {
            issues: vec![
                issue(IssueKind::MissingTexture),
                issue(IssueKind::UvMismatch),
            ],
        };
        assert_eq!(
            report.ranked_kinds(),
            vec![(IssueKind::UvMismatch, 1), (IssueKind::MissingTexture, 1)],
            "UvMismatch is declared first, so it wins an equal count"
        );
    }

    /// The display name is for humans; the wire spelling in `crate::json`
    /// is a separate, stable identifier and must not follow it.
    #[test]
    fn every_kind_has_a_distinct_display_name() {
        let mut names: Vec<String> = IssueKind::ALL.iter().map(ToString::to_string).collect();
        names.sort();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "two kinds share a display name");
        assert!(names.iter().all(|n| !n.is_empty()));
    }
}
