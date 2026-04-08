use serde::Serialize;

use crate::report::{
    AnalysisReport, BoundsSummary, IssueScope, MaterialSummary, MeshSummary, Severity,
    TextureEntry, ValidationIssue, ValidationReport,
};

#[derive(Serialize)]
pub struct JsonVec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl From<&[f32; 3]> for JsonVec3 {
    fn from(v: &[f32; 3]) -> Self {
        Self {
            x: v[0],
            y: v[1],
            z: v[2],
        }
    }
}

#[derive(Serialize)]
pub struct JsonColor3 {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl From<&[f32; 3]> for JsonColor3 {
    fn from(c: &[f32; 3]) -> Self {
        Self {
            r: c[0],
            g: c[1],
            b: c[2],
        }
    }
}

#[derive(Serialize)]
pub struct JsonBounds {
    pub min: JsonVec3,
    pub max: JsonVec3,
    pub size: JsonVec3,
    pub center: JsonVec3,
    pub diagonal: f32,
}

impl From<&BoundsSummary> for JsonBounds {
    fn from(b: &BoundsSummary) -> Self {
        Self {
            min: JsonVec3::from(&b.min),
            max: JsonVec3::from(&b.max),
            size: JsonVec3::from(&b.size),
            center: JsonVec3::from(&b.center),
            diagonal: b.diagonal,
        }
    }
}

#[derive(Serialize)]
pub struct JsonMesh {
    pub index: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub triangle_count: usize,
    pub normal_count: usize,
    pub texcoord_count: usize,
    pub material_name: Option<String>,
    pub material_id: Option<usize>,
}

impl From<&MeshSummary> for JsonMesh {
    fn from(m: &MeshSummary) -> Self {
        Self {
            index: m.index,
            vertex_count: m.vertex_count,
            index_count: m.index_count,
            triangle_count: m.triangle_count,
            normal_count: m.normal_count,
            texcoord_count: m.texcoord_count,
            material_name: m.material_name.clone(),
            material_id: m.material_id,
        }
    }
}

#[derive(Serialize)]
pub struct JsonTexture {
    pub slot: String,
    pub path: String,
    pub exists: bool,
}

impl From<&TextureEntry> for JsonTexture {
    fn from(t: &TextureEntry) -> Self {
        Self {
            slot: t.slot.clone(),
            path: t.path.clone(),
            exists: t.exists,
        }
    }
}

#[derive(Serialize)]
pub struct JsonMaterial {
    pub index: usize,
    pub name: String,
    pub ambient: JsonColor3,
    pub diffuse: JsonColor3,
    pub specular: JsonColor3,
    pub shininess: Option<f32>,
    pub dissolve: Option<f32>,
    pub optical_density: Option<f32>,
    pub textures: Vec<JsonTexture>,
}

impl From<&MaterialSummary> for JsonMaterial {
    fn from(m: &MaterialSummary) -> Self {
        Self {
            index: m.index,
            name: m.name.clone(),
            ambient: JsonColor3::from(&m.ambient),
            diffuse: JsonColor3::from(&m.diffuse),
            specular: JsonColor3::from(&m.specular),
            shininess: m.shininess,
            dissolve: m.dissolve,
            optical_density: m.optical_density,
            textures: m.textures.iter().map(JsonTexture::from).collect(),
        }
    }
}

#[derive(Serialize)]
pub struct JsonIssue {
    pub severity: String,
    pub scope: String,
    pub scope_index: Option<usize>,
    pub message: String,
}

impl From<&ValidationIssue> for JsonIssue {
    fn from(i: &ValidationIssue) -> Self {
        let (scope, scope_index) = match &i.scope {
            IssueScope::Model => ("model", None),
            IssueScope::Mesh(idx) => ("mesh", Some(*idx)),
            IssueScope::Material(idx) => ("material", Some(*idx)),
            IssueScope::Face(mesh_idx, _) => ("mesh", Some(*mesh_idx)),
        };
        Self {
            severity: match i.severity {
                Severity::Error => "error".to_owned(),
                Severity::Warning => "warning".to_owned(),
            },
            scope: scope.to_owned(),
            scope_index,
            message: i.message.clone(),
        }
    }
}

#[derive(Serialize)]
pub struct JsonValidation {
    pub error_count: usize,
    pub warning_count: usize,
    pub issues: Vec<JsonIssue>,
}

impl From<&ValidationReport> for JsonValidation {
    fn from(v: &ValidationReport) -> Self {
        Self {
            error_count: v.error_count(),
            warning_count: v.warning_count(),
            issues: v.issues.iter().map(JsonIssue::from).collect(),
        }
    }
}

#[derive(Serialize)]
pub struct JsonReport {
    pub model_name: String,
    pub mesh_count: usize,
    pub material_count: usize,
    pub total_vertices: usize,
    pub total_indices: usize,
    pub total_triangles: usize,
    pub bounds: Option<JsonBounds>,
    pub meshes: Vec<JsonMesh>,
    pub materials: Vec<JsonMaterial>,
    pub validation: JsonValidation,
}

impl From<&AnalysisReport> for JsonReport {
    fn from(r: &AnalysisReport) -> Self {
        Self {
            model_name: r.model_name.clone(),
            mesh_count: r.mesh_count,
            material_count: r.material_count,
            total_vertices: r.total_vertices,
            total_indices: r.total_indices,
            total_triangles: r.total_triangles,
            bounds: r.bounds.as_ref().map(JsonBounds::from),
            meshes: r.meshes.iter().map(JsonMesh::from).collect(),
            materials: r.materials.iter().map(JsonMaterial::from).collect(),
            validation: JsonValidation::from(&r.validation),
        }
    }
}

pub fn report_to_json(report: &AnalysisReport) -> String {
    let json_report = JsonReport::from(report);
    serde_json::to_string_pretty(&json_report).expect("Failed to serialize report to JSON")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::{IssueKind, Severity, ValidationIssue};

    fn empty_report() -> AnalysisReport {
        AnalysisReport {
            model_name: "test.obj".to_owned(),
            mesh_count: 0,
            material_count: 0,
            total_vertices: 0,
            total_indices: 0,
            total_triangles: 0,
            bounds: None,
            meshes: vec![],
            materials: vec![],
            validation: ValidationReport::default(),
        }
    }

    fn make_issue(severity: Severity, scope: IssueScope, kind: IssueKind) -> ValidationIssue {
        ValidationIssue {
            severity,
            scope,
            kind,
            message: "test".to_owned(),
        }
    }

    #[test]
    fn json_mesh_all_fields() {
        let m = MeshSummary {
            index: 2,
            vertex_count: 100,
            index_count: 300,
            triangle_count: 100,
            normal_count: 80,
            texcoord_count: 50,
            material_name: Some("wood".to_owned()),
            material_id: Some(1),
        };
        let jm = JsonMesh::from(&m);
        assert_eq!(jm.index, 2);
        assert_eq!(jm.vertex_count, 100);
        assert_eq!(jm.index_count, 300);
        assert_eq!(jm.triangle_count, 100);
        assert_eq!(jm.normal_count, 80);
        assert_eq!(jm.texcoord_count, 50);
        assert_eq!(jm.material_name.as_deref(), Some("wood"));
        assert_eq!(jm.material_id, Some(1));
    }

    #[test]
    fn json_material_with_nested_textures() {
        let m = MaterialSummary {
            index: 0,
            name: "metal".to_owned(),
            ambient: [0.1, 0.1, 0.1],
            diffuse: [0.8, 0.8, 0.8],
            specular: [1.0, 1.0, 1.0],
            shininess: Some(32.0),
            dissolve: None,
            optical_density: None,
            textures: vec![TextureEntry {
                slot: "normal".to_owned(),
                path: "n.png".to_owned(),
                exists: false,
            }],
        };
        let jm = JsonMaterial::from(&m);
        assert_eq!(jm.name, "metal");
        assert!((jm.ambient.r - 0.1).abs() < f32::EPSILON);
        assert!((jm.diffuse.r - 0.8).abs() < f32::EPSILON);
        assert!((jm.specular.r - 1.0).abs() < f32::EPSILON);
        assert!((jm.shininess.unwrap() - 32.0).abs() < f32::EPSILON);
        assert!(jm.dissolve.is_none());
        assert!(jm.optical_density.is_none());
        assert_eq!(jm.textures.len(), 1);
        assert_eq!(jm.textures[0].slot, "normal");
        assert!(!jm.textures[0].exists);
    }

    #[test]
    fn json_issue_scope_mapping() {
        let ji = JsonIssue::from(&make_issue(
            Severity::Error,
            IssueScope::Model,
            IssueKind::EmptyIndices,
        ));
        assert_eq!(ji.scope, "model");
        assert!(ji.scope_index.is_none());
        assert_eq!(ji.severity, "error");

        let ji = JsonIssue::from(&make_issue(
            Severity::Warning,
            IssueScope::Mesh(3),
            IssueKind::NormalMismatch,
        ));
        assert_eq!(ji.scope, "mesh");
        assert_eq!(ji.scope_index, Some(3));
        assert_eq!(ji.severity, "warning");

        let ji = JsonIssue::from(&make_issue(
            Severity::Warning,
            IssueScope::Material(1),
            IssueKind::MissingTexture,
        ));
        assert_eq!(ji.scope, "material");
        assert_eq!(ji.scope_index, Some(1));

        let ji = JsonIssue::from(&make_issue(
            Severity::Warning,
            IssueScope::Face(2, 5),
            IssueKind::DegenerateTriangles,
        ));
        assert_eq!(ji.scope, "mesh");
        assert_eq!(ji.scope_index, Some(2));
    }

    #[test]
    fn json_validation_counts() {
        let v = ValidationReport {
            issues: vec![
                make_issue(Severity::Error, IssueScope::Model, IssueKind::EmptyIndices),
                make_issue(
                    Severity::Warning,
                    IssueScope::Mesh(0),
                    IssueKind::NormalMismatch,
                ),
                make_issue(
                    Severity::Error,
                    IssueScope::Model,
                    IssueKind::NonTriangulated,
                ),
            ],
        };
        let jv = JsonValidation::from(&v);
        assert_eq!(jv.error_count, 2);
        assert_eq!(jv.warning_count, 1);
        assert_eq!(jv.issues.len(), 3);
    }

    #[test]
    fn json_report_roundtrip() {
        let mut report = empty_report();
        report.mesh_count = 1;
        report.meshes.push(MeshSummary {
            index: 0,
            vertex_count: 3,
            index_count: 3,
            triangle_count: 1,
            normal_count: 3,
            texcoord_count: 0,
            material_name: None,
            material_id: None,
        });
        report.bounds = Some(BoundsSummary {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 2.0, 3.0],
            size: [1.0, 2.0, 3.0],
            center: [0.5, 1.0, 1.5],
            diagonal: 3.742,
        });
        let json_str = report_to_json(&report);
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("Should be valid JSON");
        assert_eq!(parsed["model_name"], "test.obj");
        assert_eq!(parsed["mesh_count"], 1);
        assert_eq!(parsed["meshes"][0]["vertex_count"], 3);
        assert!((parsed["bounds"]["diagonal"].as_f64().unwrap() - 3.742).abs() < 1e-3);
        assert_eq!(parsed["validation"]["error_count"], 0);
    }
}
