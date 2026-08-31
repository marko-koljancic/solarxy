//! `serde_json` schema for analyzer output (`solarxy-cli analyze --json`).
//! Mirrors [`crate::report::AnalysisReport`] in a stable, documented JSON
//! shape consumed by tooling.
//!
//! Available with the `serialization` feature.

use serde::Serialize;

use crate::project_config::AssetCategory;
use crate::report::{
    AnalysisReport, BoundsSummary, IssueScope, MaterialSummary, MeshSummary, Severity,
    TextureEntry, ValidationIssue, ValidationReport,
};
use crate::validation::IssueKind;

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct JsonMesh {
    pub index: usize,
    /// The name the file gave this mesh, empty when the format carries none.
    /// Schema version 2.
    pub name: String,
    pub vertex_count: usize,
    pub index_count: usize,
    pub triangle_count: usize,
    pub normal_count: usize,
    pub texcoord_count: usize,
    pub material_name: Option<String>,
    pub material_id: Option<usize>,
    /// Indices of this mesh's degenerate faces, empty when it has none.
    /// Schema version 2.
    pub degenerate_faces: Vec<u32>,
}

impl From<&MeshSummary> for JsonMesh {
    fn from(m: &MeshSummary) -> Self {
        Self {
            index: m.index,
            name: m.name.clone(),
            vertex_count: m.vertex_count,
            index_count: m.index_count,
            triangle_count: m.triangle_count,
            normal_count: m.normal_count,
            texcoord_count: m.texcoord_count,
            material_name: m.material_name.clone(),
            material_id: m.material_id,
            degenerate_faces: m.degenerate_faces.clone(),
        }
    }
}

/// Pixel dimensions of a texture, as an object rather than a bare pair so a
/// consumer reads `width` by name instead of by position.
#[derive(Debug, Serialize)]
pub struct JsonTextureDimensions {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Serialize)]
pub struct JsonTexture {
    pub slot: String,
    pub path: String,
    pub exists: bool,
    /// Pixel dimensions where knowable; `null` when the file is missing, is
    /// not an image, or is in a format this build cannot read. Schema
    /// version 2.
    pub dimensions: Option<JsonTextureDimensions>,
}

impl From<&TextureEntry> for JsonTexture {
    fn from(t: &TextureEntry) -> Self {
        Self {
            slot: t.slot.clone(),
            path: t.path.clone(),
            exists: t.exists,
            dimensions: t
                .dimensions
                .map(|(width, height)| JsonTextureDimensions { width, height }),
        }
    }
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Clone, Serialize)]
pub struct JsonIssue {
    pub severity: String,
    pub kind: String,
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
            IssueScope::Edge { mesh_index, .. } => ("edge", Some(*mesh_index)),
        };
        let kind = match i.kind {
            IssueKind::NormalMismatch => "NormalMismatch",
            IssueKind::FlippedNormals => "FlippedNormals",
            IssueKind::UvMismatch => "UvMismatch",
            IssueKind::MissingUvs => "MissingUvs",
            IssueKind::NonTriangulated => "NonTriangulated",
            IssueKind::EmptyIndices => "EmptyIndices",
            IssueKind::InvalidMaterialRef => "InvalidMaterialRef",
            IssueKind::DegenerateTriangles => "DegenerateTriangles",
            IssueKind::MissingTexture => "MissingTexture",
            IssueKind::NonManifoldEdge => "NonManifoldEdge",
            IssueKind::TriangleBudgetExceeded => "TriangleBudgetExceeded",
        };
        Self {
            severity: match i.severity {
                Severity::Error => "error".to_owned(),
                Severity::Warning => "warning".to_owned(),
            },
            kind: kind.to_owned(),
            scope: scope.to_owned(),
            scope_index,
            message: i.message.clone(),
        }
    }
}

#[derive(Debug, Serialize)]
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

/// JSON `schema_version` written by this build. Bump when the on-disk shape
/// changes incompatibly; new optional fields don't require a bump.
///
/// Version 2 lifted the deliberate v0.8.2 holdout: the report gained
/// `source_format`, `file_size_bytes`, `asset_category` and `triangle_budget`
/// at the top level, `name` and `degenerate_faces` per mesh, and `dimensions`
/// per texture. The facts had reached the text report and the terminal
/// workspace one release earlier; the wire format waited for this bump so a
/// consumer's parser could not change shape underneath it.
pub const JSON_REPORT_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Serialize)]
pub struct JsonReport {
    pub schema_version: u32,
    pub model_name: String,
    /// The lowercased extension the model was loaded as, empty when the path
    /// had none. Schema version 2.
    pub source_format: String,
    /// Size of the model file itself, excluding companions; `null` when the
    /// file could not be measured. Schema version 2.
    pub file_size_bytes: Option<u64>,
    /// What the project's filename rules classified this model as, in the
    /// same casing `solarxy.toml` uses; `null` when nothing classified it,
    /// which is not the same as `"default"` (classified, matched no rule).
    /// Schema version 2.
    pub asset_category: Option<AssetCategory>,
    /// The triangle budget for that category; `null` when the project's
    /// budget check is switched off. Schema version 2.
    pub triangle_budget: Option<u32>,
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
            schema_version: JSON_REPORT_SCHEMA_VERSION,
            model_name: r.model_name.clone(),
            source_format: r.source_format.clone(),
            file_size_bytes: r.file_size_bytes,
            asset_category: r.asset_category,
            triangle_budget: r.triangle_budget,
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

/// Serializes an [`AnalysisReport`] to a pretty-printed JSON string.
///
/// # Errors
/// Returns `Err` if any field contains values that don't serialize (e.g.
/// non-finite floats — currently impossible by construction).
pub fn report_to_json(report: &AnalysisReport) -> anyhow::Result<String> {
    let json_report = JsonReport::from(report);
    serde_json::to_string_pretty(&json_report).map_err(anyhow::Error::from)
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
            source_format: "obj".to_owned(),
            file_size_bytes: Some(1024),
            asset_category: Some(crate::project_config::AssetCategory::Hero),
            triangle_budget: Some(100_000),
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
            name: "trunk".to_owned(),
            vertex_count: 100,
            index_count: 300,
            triangle_count: 100,
            normal_count: 80,
            texcoord_count: 50,
            material_name: Some("wood".to_owned()),
            material_id: Some(1),
            degenerate_faces: vec![4, 9],
        };
        let jm = JsonMesh::from(&m);
        assert_eq!(jm.index, 2);
        assert_eq!(jm.name, "trunk");
        assert_eq!(jm.vertex_count, 100);
        assert_eq!(jm.index_count, 300);
        assert_eq!(jm.triangle_count, 100);
        assert_eq!(jm.normal_count, 80);
        assert_eq!(jm.texcoord_count, 50);
        assert_eq!(jm.material_name.as_deref(), Some("wood"));
        assert_eq!(jm.material_id, Some(1));
        assert_eq!(jm.degenerate_faces, vec![4, 9]);
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
                dimensions: Some((2048, 2048)),
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
        let dims = jm.textures[0]
            .dimensions
            .as_ref()
            .expect("the entry carries dimensions");
        assert_eq!((dims.width, dims.height), (2048, 2048));
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
            name: "tri".to_owned(),
            vertex_count: 3,
            index_count: 3,
            triangle_count: 1,
            normal_count: 3,
            texcoord_count: 0,
            material_name: None,
            material_id: None,
            degenerate_faces: Vec::new(),
        });
        report.bounds = Some(BoundsSummary {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 2.0, 3.0],
            size: [1.0, 2.0, 3.0],
            center: [0.5, 1.0, 1.5],
            diagonal: 3.742,
        });
        let json_str = report_to_json(&report).unwrap();
        let parsed: serde_json::Value =
            serde_json::from_str(&json_str).expect("Should be valid JSON");
        assert_eq!(parsed["model_name"], "test.obj");
        assert_eq!(parsed["mesh_count"], 1);
        assert_eq!(parsed["meshes"][0]["vertex_count"], 3);
        assert!((parsed["bounds"]["diagonal"].as_f64().unwrap() - 3.742).abs() < 1e-3);
        assert_eq!(parsed["validation"]["error_count"], 0);
    }

    /// This wire format is pinned: a parser is written against exactly this
    /// key set, so a field cannot reach the JSON report without failing here
    /// first and taking a deliberate `schema_version` bump with it.
    ///
    /// Version 2 lifted the v0.8.2 holdout and added the four recovered
    /// top-level facts. The pin stays, now guarding the version-2 set the
    /// same way it guarded version 1.
    #[test]
    fn the_json_report_key_set_is_pinned_to_the_schema_version() {
        let json_str = report_to_json(&empty_report()).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let mut keys: Vec<&str> = parsed
            .as_object()
            .expect("the report is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "asset_category",
                "bounds",
                "file_size_bytes",
                "material_count",
                "materials",
                "mesh_count",
                "meshes",
                "model_name",
                "schema_version",
                "source_format",
                "total_indices",
                "total_triangles",
                "total_vertices",
                "triangle_budget",
                "validation",
            ],
            "a field reached the JSON report without a schema_version bump"
        );
        assert_eq!(parsed["schema_version"], JSON_REPORT_SCHEMA_VERSION);
        assert_eq!(parsed["schema_version"], 2);
    }

    /// The v0.8.2 holdout, lifted behind the version-2 bump: the facts the
    /// analyzer recovered for the text report and the terminal workspace now
    /// reach the wire too, with the values the report carried rather than
    /// defaults. The nested key sets are pinned here the way the top level is
    /// pinned above, so a mesh or texture field cannot drift in silently
    /// either.
    #[test]
    fn the_recovered_facts_reach_the_json_under_version_two() {
        let mut report = empty_report();
        report.meshes.push(MeshSummary {
            index: 0,
            name: "trunk".to_owned(),
            vertex_count: 3,
            index_count: 3,
            triangle_count: 1,
            normal_count: 3,
            texcoord_count: 0,
            material_name: None,
            material_id: None,
            degenerate_faces: vec![4, 9],
        });
        report.materials.push(MaterialSummary {
            index: 0,
            name: "bark".to_owned(),
            ambient: [0.1; 3],
            diffuse: [0.8; 3],
            specular: [1.0; 3],
            shininess: None,
            dissolve: None,
            optical_density: None,
            textures: vec![TextureEntry {
                slot: "albedo".to_owned(),
                path: "bark.png".to_owned(),
                exists: true,
                dimensions: Some((2048, 1024)),
            }],
        });
        let json_str = report_to_json(&report).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();

        assert_eq!(parsed["source_format"], "obj");
        assert_eq!(parsed["file_size_bytes"], 1024);
        assert_eq!(parsed["asset_category"], "hero");
        assert_eq!(parsed["triangle_budget"], 100_000);
        assert_eq!(parsed["meshes"][0]["name"], "trunk");
        assert_eq!(
            parsed["meshes"][0]["degenerate_faces"],
            serde_json::json!([4, 9])
        );
        let dims = &parsed["materials"][0]["textures"][0]["dimensions"];
        assert_eq!(dims["width"], 2048);
        assert_eq!(dims["height"], 1024);

        let keys_of = |v: &serde_json::Value| -> Vec<String> {
            let mut keys: Vec<String> = v
                .as_object()
                .expect("a JSON object")
                .keys()
                .cloned()
                .collect();
            keys.sort_unstable();
            keys
        };
        assert_eq!(
            keys_of(&parsed["meshes"][0]),
            [
                "degenerate_faces",
                "index",
                "index_count",
                "material_id",
                "material_name",
                "name",
                "normal_count",
                "texcoord_count",
                "triangle_count",
                "vertex_count",
            ],
            "a mesh field reached the JSON report without a schema_version bump"
        );
        assert_eq!(
            keys_of(&parsed["materials"][0]["textures"][0]),
            ["dimensions", "exists", "path", "slot"],
            "a texture field reached the JSON report without a schema_version bump"
        );
    }

    /// The issue entry and its kind strings, pinned the way the mesh and
    /// texture entries are.
    ///
    /// The exhaustive match in [`JsonIssue::from`] only forces a NEW kind to
    /// choose a wire string; a renamed string still compiles and changes the
    /// published format silently, which is the expensive kind of change
    /// because build systems match on these. Every kind runs through the
    /// real serialization here, so either movement fails this test and takes
    /// a deliberate `schema_version` bump with it.
    #[test]
    fn the_issue_entry_and_its_kind_strings_are_pinned() {
        let mut report = empty_report();
        report.validation = ValidationReport {
            issues: IssueKind::ALL
                .iter()
                .map(|kind| make_issue(Severity::Warning, IssueScope::Model, *kind))
                .collect(),
        };
        let parsed: serde_json::Value =
            serde_json::from_str(&report_to_json(&report).unwrap()).unwrap();
        let issues = parsed["validation"]["issues"]
            .as_array()
            .expect("issues is a JSON array");

        let kinds: Vec<&str> = issues
            .iter()
            .map(|i| i["kind"].as_str().expect("kind is a string"))
            .collect();
        assert_eq!(
            kinds,
            [
                "NormalMismatch",
                "FlippedNormals",
                "UvMismatch",
                "MissingUvs",
                "NonTriangulated",
                "EmptyIndices",
                "InvalidMaterialRef",
                "DegenerateTriangles",
                "MissingTexture",
                "NonManifoldEdge",
                "TriangleBudgetExceeded",
            ],
            "an issue kind's wire string moved without a schema_version bump"
        );

        let mut keys: Vec<&str> = issues[0]
            .as_object()
            .expect("an issue is a JSON object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["kind", "message", "scope", "scope_index", "severity"],
            "an issue field reached the JSON report without a schema_version bump"
        );
    }
}
