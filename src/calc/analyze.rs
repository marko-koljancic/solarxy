use std::path::{Path, PathBuf};

use anyhow::Result;
use solarxy::cgi::geometry::RawModelData;

use super::geometry::compute_bounds;
use super::report::{
    AnalysisReport, IssueScope, MaterialSummary, MeshSummary, Severity, TextureEntry,
    ValidationIssue, ValidationReport,
};

pub struct AnalyzerMesh {
    pub positions: Vec<f32>,
    pub indices: Vec<u32>,
    pub normals: Vec<f32>,
    pub texcoords: Vec<f32>,
    pub material_id: Option<usize>,
}

pub struct AnalyzerMaterial {
    pub name: String,
    pub ambient: Option<[f32; 3]>,
    pub diffuse: Option<[f32; 3]>,
    pub specular: Option<[f32; 3]>,
    pub shininess: Option<f32>,
    pub dissolve: Option<f32>,
    pub optical_density: Option<f32>,
    pub diffuse_texture: Option<String>,
    pub ambient_texture: Option<String>,
    pub specular_texture: Option<String>,
    pub normal_texture: Option<String>,
    pub shininess_texture: Option<String>,
    pub dissolve_texture: Option<String>,
}

pub struct ModelAnalyzer {
    pub model_name: String,
    pub meshes: Vec<AnalyzerMesh>,
    pub materials: Vec<AnalyzerMaterial>,
    pub obj_dir: Option<PathBuf>,
    base_validation: ValidationReport,
}

fn raw_to_analyzer(raw: &RawModelData) -> (Vec<AnalyzerMesh>, Vec<AnalyzerMaterial>) {
    let meshes = raw
        .meshes
        .iter()
        .map(|m| {
            let positions: Vec<f32> = m.positions.iter().flat_map(|p| p.iter().copied()).collect();
            let normals: Vec<f32> = m
                .normals
                .as_ref()
                .map(|ns| ns.iter().flat_map(|n| n.iter().copied()).collect())
                .unwrap_or_default();
            let texcoords: Vec<f32> = m
                .tex_coords
                .as_ref()
                .map(|tcs| tcs.iter().flat_map(|tc| tc.iter().copied()).collect())
                .unwrap_or_default();
            AnalyzerMesh {
                positions,
                indices: m.indices.clone(),
                normals,
                texcoords,
                material_id: m.material_index,
            }
        })
        .collect();

    let materials = raw
        .materials
        .iter()
        .map(|m| AnalyzerMaterial {
            name: m.name.clone(),
            ambient: m.ambient,
            diffuse: m.diffuse,
            specular: m.specular,
            shininess: m.shininess,
            dissolve: m.dissolve,
            optical_density: m.optical_density,
            diffuse_texture: m.diffuse_texture_name.clone(),
            ambient_texture: m.ambient_texture_name.clone(),
            specular_texture: m.specular_texture_name.clone(),
            normal_texture: m.normal_texture_name.clone(),
            shininess_texture: m.shininess_texture_name.clone(),
            dissolve_texture: m.dissolve_texture_name.clone(),
        })
        .collect();

    (meshes, materials)
}

impl ModelAnalyzer {
    pub fn new(path: &str) -> Result<Self> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let raw = match ext.as_str() {
            "stl" => solarxy::cgi::loader_stl::load_stl(path)?,
            "ply" => solarxy::cgi::loader_ply::load_ply(path)?,
            "gltf" | "glb" => solarxy::cgi::loader_gltf::load_gltf(path)?,
            _ => solarxy::cgi::loader_obj::load_obj(path)?,
        };

        let model_name = Path::new(path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(path)
            .to_string();

        let base_validation = solarxy::validation::validate_raw_model(&raw, &ext).report;
        let (meshes, materials) = raw_to_analyzer(&raw);

        Ok(ModelAnalyzer {
            model_name,
            meshes,
            materials,
            obj_dir: Path::new(path).parent().map(|p| p.to_path_buf()),
            base_validation,
        })
    }

    pub fn generate_report(&self) -> AnalysisReport {
        let mut issues = self.base_validation.issues.clone();

        let total_vertices: usize = self.meshes.iter().map(|m| m.positions.len() / 3).sum();
        let total_indices: usize = self.meshes.iter().map(|m| m.indices.len()).sum();
        let total_triangles: usize = self.meshes.iter().map(|m| m.indices.len() / 3).sum();
        let meshes: Vec<MeshSummary> = self
            .meshes
            .iter()
            .enumerate()
            .map(|(i, mesh)| {
                let vertex_count = mesh.positions.len() / 3;
                let index_count = mesh.indices.len();
                let normal_count = mesh.normals.len() / 3;
                let texcoord_count = mesh.texcoords.len() / 2;

                let (material_name, material_id) = if let Some(mat_id) = mesh.material_id {
                    if mat_id < self.materials.len() {
                        (Some(self.materials[mat_id].name.clone()), Some(mat_id))
                    } else {
                        (None, Some(mat_id))
                    }
                } else {
                    (None, None)
                };

                MeshSummary {
                    index: i,
                    vertex_count,
                    index_count,
                    triangle_count: index_count / 3,
                    normal_count,
                    texcoord_count,
                    material_name,
                    material_id,
                }
            })
            .collect();

        let materials: Vec<MaterialSummary> = self
            .materials
            .iter()
            .enumerate()
            .map(|(i, mat)| {
                let mut textures = Vec::new();
                let tex_fields: &[(&str, &Option<String>)] = &[
                    ("Diffuse", &mat.diffuse_texture),
                    ("Ambient", &mat.ambient_texture),
                    ("Specular", &mat.specular_texture),
                    ("Normal", &mat.normal_texture),
                    ("Shininess", &mat.shininess_texture),
                    ("Dissolve", &mat.dissolve_texture),
                ];
                for &(slot, tex_opt) in tex_fields {
                    if let Some(entry) = check_texture(&self.obj_dir, tex_opt, slot, &mut issues, i)
                    {
                        textures.push(entry);
                    }
                }

                MaterialSummary {
                    index: i,
                    name: mat.name.clone(),
                    ambient: mat.ambient.unwrap_or([0.0; 3]),
                    diffuse: mat.diffuse.unwrap_or([0.0; 3]),
                    specular: mat.specular.unwrap_or([0.0; 3]),
                    shininess: mat.shininess,
                    dissolve: mat.dissolve,
                    optical_density: mat.optical_density,
                    textures,
                }
            })
            .collect();

        let bounds = compute_bounds(&self.meshes);

        AnalysisReport {
            model_name: self.model_name.clone(),
            mesh_count: self.meshes.len(),
            material_count: self.materials.len(),
            total_vertices,
            total_indices,
            total_triangles,
            bounds,
            meshes,
            materials,
            validation: ValidationReport { issues },
        }
    }
}

fn check_texture(
    obj_dir: &Option<PathBuf>,
    tex_path: &Option<String>,
    slot: &str,
    issues: &mut Vec<ValidationIssue>,
    mat_index: usize,
) -> Option<TextureEntry> {
    let path = tex_path.as_ref()?;
    if path.starts_with("texture_index:") {
        return Some(TextureEntry {
            slot: slot.to_string(),
            path: path.clone(),
            exists: true,
        });
    }

    let exists = obj_dir.as_ref().is_some_and(|dir| dir.join(path).exists());

    if !exists {
        issues.push(ValidationIssue {
            severity: Severity::Error,
            scope: IssueScope::Material(mat_index),
            kind: solarxy::validation::IssueKind::MissingTexture,
            message: format!("Texture file not found: '{}'", path),
        });
    }

    Some(TextureEntry {
        slot: slot.to_string(),
        path: path.clone(),
        exists,
    })
}
