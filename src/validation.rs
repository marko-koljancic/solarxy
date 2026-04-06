use std::fmt;

use cgmath::InnerSpace;

use crate::cgi::geometry::RawModelData;

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
    /// (`mesh_index`, `degenerate_face_count`) — actual face indices in `ViewerValidation.degenerate_faces`
    Face(usize, usize),
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IssueKind {
    NormalMismatch,
    UvMismatch,
    MissingUvs,
    NonTriangulated,
    EmptyIndices,
    InvalidMaterialRef,
    DegenerateTriangles,
    MissingTexture,
}

#[derive(Debug, Clone)]
pub struct ValidationIssue {
    pub severity: Severity,
    pub scope: IssueScope,
    pub kind: IssueKind,
    pub message: String,
}

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

pub struct ViewerValidation {
    pub report: ValidationReport,
    pub degenerate_faces: Vec<Vec<u32>>,
    /// Maps raw mesh index → GPU mesh index. Populated by `upload_model()`.
    pub raw_to_gpu: Vec<Option<usize>>,
}

fn supports_uvs(file_ext: &str) -> bool {
    matches!(
        file_ext.to_ascii_lowercase().as_str(),
        "obj" | "gltf" | "glb"
    )
}

fn compute_diagonal(raw: &RawModelData) -> f32 {
    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    let mut any = false;
    for mesh in &raw.meshes {
        for p in &mesh.positions {
            any = true;
            for i in 0..3 {
                min[i] = min[i].min(p[i]);
                max[i] = max[i].max(p[i]);
            }
        }
    }
    if !any {
        return 1.0;
    }
    let dx = max[0] - min[0];
    let dy = max[1] - min[1];
    let dz = max[2] - min[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn detect_degenerate_triangles(positions: &[[f32; 3]], indices: &[u32], epsilon: f32) -> Vec<u32> {
    let mut degenerate = Vec::new();
    for (face_idx, tri) in indices.chunks(3).enumerate() {
        if tri.len() < 3 {
            continue;
        }
        let i0 = tri[0] as usize;
        let i1 = tri[1] as usize;
        let i2 = tri[2] as usize;
        if i0 >= positions.len() || i1 >= positions.len() || i2 >= positions.len() {
            continue;
        }
        let p0 = cgmath::Vector3::from(positions[i0]);
        let p1 = cgmath::Vector3::from(positions[i1]);
        let p2 = cgmath::Vector3::from(positions[i2]);
        let cross = (p1 - p0).cross(p2 - p0);
        let area = cross.magnitude() * 0.5;
        if area < epsilon {
            degenerate.push(face_idx as u32);
        }
    }
    degenerate
}

pub fn validate_raw_model(raw: &RawModelData, file_ext: &str) -> ViewerValidation {
    let mut issues = Vec::new();
    let mut degenerate_faces = Vec::with_capacity(raw.meshes.len());

    let diagonal = compute_diagonal(raw);
    let degen_epsilon = diagonal * diagonal * 1e-10;

    for (i, mesh) in raw.meshes.iter().enumerate() {
        let vertex_count = mesh.positions.len();
        let index_count = mesh.indices.len();

        // Normal count mismatch
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

        // UV count mismatch
        if let Some(ref tex_coords) = mesh.tex_coords
            && tex_coords.len() != vertex_count
        {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::UvMismatch,
                message: format!(
                    "Texture coordinate count ({}) does not match vertex count ({})",
                    tex_coords.len(),
                    vertex_count
                ),
            });
        }

        // Missing UVs
        if mesh.tex_coords.is_none() && supports_uvs(file_ext) {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::MissingUvs,
                message: "No texture coordinates".to_string(),
            });
        }

        // Non-triangulated
        if index_count % 3 != 0 {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::NonTriangulated,
                message: format!(
                    "Index count ({}) is not divisible by 3 (non-triangulated)",
                    index_count
                ),
            });
        }

        // Empty indices
        if mesh.indices.is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::EmptyIndices,
                message: "Empty index buffer".to_string(),
            });
        }

        // Bad material ref
        if let Some(mat_id) = mesh.material_index
            && mat_id >= raw.materials.len()
        {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::InvalidMaterialRef,
                message: format!(
                    "Material ID {} is out of range (only {} materials available)",
                    mat_id,
                    raw.materials.len()
                ),
            });
        }

        // Degenerate triangles
        let degen = detect_degenerate_triangles(&mesh.positions, &mesh.indices, degen_epsilon);
        if !degen.is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                scope: IssueScope::Face(i, degen.len()),
                kind: IssueKind::DegenerateTriangles,
                message: format!("{} degenerate triangles detected", degen.len()),
            });
        }
        degenerate_faces.push(degen);
    }

    ViewerValidation {
        report: ValidationReport { issues },
        degenerate_faces,
        raw_to_gpu: Vec::new(),
    }
}

/// Issue category for color mapping in the viewer overlay.
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
            Self::Error => [1.0, 0.0, 0.0, 0.35],
            Self::InvalidMaterial => [1.0, 0.5, 0.0, 0.35],
            Self::NormalMismatch => [0.0, 0.8, 1.0, 0.35],
            Self::MissingUvs => [1.0, 0.4, 0.6, 0.35],
            Self::DegenerateTriangles => [1.0, 0.9, 0.0, 0.35],
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

/// Classify a validation issue into a color category.
pub fn issue_category(issue: &ValidationIssue) -> IssueCategory {
    match issue.kind {
        IssueKind::InvalidMaterialRef => IssueCategory::InvalidMaterial,
        IssueKind::NormalMismatch => IssueCategory::NormalMismatch,
        IssueKind::MissingUvs | IssueKind::UvMismatch => IssueCategory::MissingUvs,
        IssueKind::DegenerateTriangles => IssueCategory::DegenerateTriangles,
        _ => IssueCategory::Error,
    }
}

/// Build a per-GPU-mesh category index map from validation issues.
/// Returns `Option<usize>` where `usize` is the index into `IssueCategory::ALL`.
/// `raw_to_gpu` maps raw mesh index → GPU mesh index (None if mesh was skipped).
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
            _ => continue, // Face issues handled separately
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
