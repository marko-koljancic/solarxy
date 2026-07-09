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
    /// Non-manifold edges — orange-red. Distinct from `InvalidMaterial` so the
    /// GUI edge-highlight overlay (Stream B7) can paint edges with its own
    /// color, brighter than the face-highlight palette.
    NonManifoldEdge,
}

impl IssueCategory {
    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Error => [1.0, 0.0, 0.0, 0.4],
            Self::InvalidMaterial => [1.0, 0.45, 0.0, 0.4],
            Self::NormalMismatch => [0.0, 0.85, 1.0, 0.4],
            Self::MissingUvs => [1.0, 0.0, 0.8, 0.4],
            Self::DegenerateTriangles => [1.0, 0.9, 0.0, 0.4],
            Self::NonManifoldEdge => [1.0, 0.55, 0.1, 0.6],
        }
    }

    pub const ALL: &[Self] = &[
        Self::Error,
        Self::InvalidMaterial,
        Self::NormalMismatch,
        Self::MissingUvs,
        Self::DegenerateTriangles,
        Self::NonManifoldEdge,
    ];
}

/// Resolve a validation issue's scope to an AABB for camera fly-to.
/// Mesh-granular for every scope — the per-face / per-edge validation
/// overlay highlights the exact defect once its mesh is framed. Raw issue
/// mesh indices are remapped to GPU mesh indices via `raw_to_gpu`. Shared
/// by the desktop Properties-panel fly-to and the web report panel.
#[must_use]
pub fn resolve_issue_aabb(
    scope: &IssueScope,
    model: &crate::model::Model,
    raw_to_gpu: &[Option<usize>],
) -> Option<solarxy_core::AABB> {
    let gpu_mesh = |raw: usize| raw_to_gpu.get(raw).copied().flatten();
    match scope {
        IssueScope::Model => Some(model.bounds),
        IssueScope::Mesh(raw) | IssueScope::Face(raw, _) => {
            gpu_mesh(*raw).and_then(|g| model.mesh_bounds.get(g).copied())
        }
        IssueScope::Edge { mesh_index, .. } => {
            gpu_mesh(*mesh_index).and_then(|g| model.mesh_bounds.get(g).copied())
        }
        IssueScope::Material(mat) => material_meshes_aabb(model, *mat).or(Some(model.bounds)),
    }
}

/// Union of the bounds of every mesh using `material`, or `None` when no
/// mesh references it. Shared by validation fly-to and the Outliner's
/// frame-material action.
#[must_use]
pub fn material_meshes_aabb(
    model: &crate::model::Model,
    material: usize,
) -> Option<solarxy_core::AABB> {
    let mut acc: Option<solarxy_core::AABB> = None;
    for (i, mesh) in model.meshes.iter().enumerate() {
        if mesh.material == material
            && let Some(b) = model.mesh_bounds.get(i).copied()
        {
            acc = Some(acc.map_or(b, |a| union_aabb(a, b)));
        }
    }
    acc
}

/// Smallest AABB enclosing both inputs.
fn union_aabb(a: solarxy_core::AABB, b: solarxy_core::AABB) -> solarxy_core::AABB {
    solarxy_core::AABB {
        min: cgmath::Point3::new(
            a.min.x.min(b.min.x),
            a.min.y.min(b.min.y),
            a.min.z.min(b.min.z),
        ),
        max: cgmath::Point3::new(
            a.max.x.max(b.max.x),
            a.max.y.max(b.max.y),
            a.max.z.max(b.max.z),
        ),
    }
}

pub fn issue_category(issue: &ValidationIssue) -> IssueCategory {
    match issue.kind {
        IssueKind::InvalidMaterialRef => IssueCategory::InvalidMaterial,
        IssueKind::NormalMismatch | IssueKind::FlippedNormals => IssueCategory::NormalMismatch,
        IssueKind::MissingUvs | IssueKind::UvMismatch => IssueCategory::MissingUvs,
        IssueKind::DegenerateTriangles => IssueCategory::DegenerateTriangles,
        IssueKind::NonManifoldEdge => IssueCategory::NonManifoldEdge,
        _ => IssueCategory::Error,
    }
}

/// Builds the per-GPU-mesh edge index buffer source from
/// `IssueScope::Edge` issues. Returns one `Vec<u32>` per GPU mesh — the
/// flat `[v0, v1, v0, v1, ...]` shape consumed by `LineList` topology.
/// Empty when no edge issues touch a mesh.
pub fn build_mesh_edge_indices(
    report: &ValidationReport,
    gpu_mesh_count: usize,
    raw_to_gpu: &[Option<usize>],
) -> Vec<Vec<u32>> {
    let mut out: Vec<Vec<u32>> = vec![Vec::new(); gpu_mesh_count];
    for issue in &report.issues {
        if let IssueScope::Edge {
            mesh_index,
            vertices,
        } = &issue.scope
            && let Some(Some(gpu_idx)) = raw_to_gpu.get(*mesh_index)
        {
            let dst = &mut out[*gpu_idx];
            dst.push(vertices[0]);
            dst.push(vertices[1]);
        }
    }
    out
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
            IssueScope::Edge { mesh_index, .. } => *mesh_index,
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
            (IssueKind::FlippedNormals, IssueCategory::NormalMismatch),
            (IssueKind::MissingUvs, IssueCategory::MissingUvs),
            (IssueKind::UvMismatch, IssueCategory::MissingUvs),
            (
                IssueKind::DegenerateTriangles,
                IssueCategory::DegenerateTriangles,
            ),
            (IssueKind::NonManifoldEdge, IssueCategory::NonManifoldEdge),
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
