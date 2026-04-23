use std::fmt;

use cgmath::InnerSpace;

use crate::geometry::RawModelData;

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

#[derive(Debug)]
pub struct ValidationResult {
    pub report: ValidationReport,
    pub degenerate_faces: Vec<Vec<u32>>,
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

pub fn validate_raw_model(raw: &RawModelData, file_ext: &str) -> ValidationResult {
    let mut issues = Vec::new();
    let mut degenerate_faces = Vec::with_capacity(raw.meshes.len());

    let diagonal = compute_diagonal(raw);
    let degen_epsilon = diagonal * diagonal * 1e-10;

    for (i, mesh) in raw.meshes.iter().enumerate() {
        let vertex_count = mesh.positions.len();
        let index_count = mesh.indices.len();

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

        if mesh.tex_coords.is_none() && supports_uvs(file_ext) {
            issues.push(ValidationIssue {
                severity: Severity::Warning,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::MissingUvs,
                message: "No texture coordinates".to_string(),
            });
        }

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

        if mesh.indices.is_empty() {
            issues.push(ValidationIssue {
                severity: Severity::Error,
                scope: IssueScope::Mesh(i),
                kind: IssueKind::EmptyIndices,
                message: "Empty index buffer".to_string(),
            });
        }

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

    ValidationResult {
        report: ValidationReport { issues },
        degenerate_faces,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{RawMeshData, RawModelData};

    fn single_triangle_raw() -> RawModelData {
        RawModelData {
            meshes: vec![RawMeshData {
                name: "test".to_string(),
                positions: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
                indices: vec![0, 1, 2],
                normals: Some(vec![[0.0, 0.0, 1.0]; 3]),
                tex_coords: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
                material_index: None,
            }],
            materials: vec![],
            polygon_count: 1,
        }
    }

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

    #[test]
    fn non_triangulated() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].indices = vec![0, 1];
        raw.meshes[0].normals = Some(vec![[0.0, 0.0, 1.0]; 3]);
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::NonTriangulated)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn empty_indices() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].indices = vec![];
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::EmptyIndices)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn invalid_material_ref() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].material_index = Some(5);
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::InvalidMaterialRef)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Error);
    }

    #[test]
    fn degenerate_triangles() {
        let mut raw = single_triangle_raw();
        raw.meshes[0].positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let result = validate_raw_model(&raw, "obj");
        let issues: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::DegenerateTriangles)
            .collect();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].severity, Severity::Warning);
        assert_eq!(result.degenerate_faces[0], vec![0]);
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
    fn compute_diagonal_single_point() {
        let raw = RawModelData {
            meshes: vec![RawMeshData {
                name: "pt".to_string(),
                positions: vec![[5.0, 5.0, 5.0]],
                indices: vec![0, 0, 0],
                normals: None,
                tex_coords: None,
                material_index: None,
            }],
            materials: vec![],
            polygon_count: 0,
        };
        assert!((compute_diagonal(&raw)).abs() < f32::EPSILON);
    }

    #[test]
    fn compute_diagonal_unit_cube() {
        let raw = RawModelData {
            meshes: vec![RawMeshData {
                name: "cube".to_string(),
                positions: vec![[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]],
                indices: vec![0, 1, 0],
                normals: None,
                tex_coords: None,
                material_index: None,
            }],
            materials: vec![],
            polygon_count: 0,
        };
        assert!((compute_diagonal(&raw) - 3.0_f32.sqrt()).abs() < 1e-6);
    }

    #[test]
    fn compute_diagonal_no_vertices() {
        let raw = RawModelData {
            meshes: vec![],
            materials: vec![],
            polygon_count: 0,
        };
        assert!((compute_diagonal(&raw) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn degenerate_triangle_collinear() {
        let positions = vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]];
        let indices = vec![0, 1, 2];
        let result = detect_degenerate_triangles(&positions, &indices, 1e-6);
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn degenerate_triangle_coincident() {
        let positions = vec![[1.0, 1.0, 1.0], [1.0, 1.0, 1.0], [1.0, 1.0, 1.0]];
        let indices = vec![0, 1, 2];
        let result = detect_degenerate_triangles(&positions, &indices, 1e-6);
        assert_eq!(result, vec![0]);
    }

    #[test]
    fn degenerate_large_model_epsilon_scaling() {
        let positions = vec![
            [0.0, 0.0, 0.0],
            [1000.0, 0.0, 0.0],
            [0.0, 1000.0, 0.0],
            [500.0, 500.0, 0.0],
            [500.1, 500.0, 0.0],
            [500.0, 500.1, 0.0],
        ];
        let indices = vec![0, 1, 2, 3, 4, 5];
        let diagonal = (1000.0_f32 * 1000.0 + 1000.0 * 1000.0_f32).sqrt();
        let epsilon = diagonal * diagonal * 1e-10;
        let result = detect_degenerate_triangles(&positions, &indices, epsilon);
        assert!(
            result.is_empty(),
            "Small valid triangle should not be flagged"
        );
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

    #[test]
    fn invalid_material_ref_at_boundary() {
        use crate::geometry::RawMaterialData;
        let mut raw = single_triangle_raw();
        raw.meshes[0].material_index = Some(1);
        raw.materials.push(RawMaterialData {
            name: "mat0".to_string(),
            diffuse_texture_path: None,
            normal_texture_path: None,
            diffuse_texture_data: None,
            normal_texture_data: None,
            metallic_roughness_texture_path: None,
            metallic_roughness_texture_data: None,
            occlusion_texture_path: None,
            occlusion_texture_data: None,
            emissive_texture_path: None,
            emissive_texture_data: None,
            roughness_factor: 0.5,
            metallic_factor: 0.0,
            emissive_factor: [0.0; 3],
            alpha_mode: 0,
            alpha_cutoff: 0.5,
            ambient: None,
            diffuse: None,
            specular: None,
            shininess: None,
            dissolve: None,
            optical_density: None,
            ambient_texture_name: None,
            diffuse_texture_name: None,
            specular_texture_name: None,
            normal_texture_name: None,
            shininess_texture_name: None,
            dissolve_texture_name: None,
        });

        let result = validate_raw_model(&raw, "obj");
        let invalid: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::InvalidMaterialRef)
            .collect();
        assert_eq!(invalid.len(), 1);

        raw.meshes[0].material_index = Some(0);
        let result = validate_raw_model(&raw, "obj");
        let invalid: Vec<_> = result
            .report
            .issues
            .iter()
            .filter(|i| i.kind == IssueKind::InvalidMaterialRef)
            .collect();
        assert!(invalid.is_empty());
    }
}
