use serde::Serialize;

use super::report::{
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
