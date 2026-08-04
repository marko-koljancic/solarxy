use std::path::{Path, PathBuf};

use anyhow::Result;
use solarxy_core::geometry::RawModelData;
use solarxy_core::project_config::{self, AssetCategory, ProjectConfig};

use super::geometry::compute_bounds;
use solarxy_core::report::{
    AnalysisReport, IssueScope, MaterialSummary, MeshSummary, Severity, TextureEntry,
    ValidationIssue, ValidationReport,
};

pub struct AnalyzerMesh {
    /// The name the file gave this mesh. Carried through conversion so a
    /// report can say which mesh it means rather than only its index.
    pub name: String,
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
    /// Per-mesh degenerate face indices, parallel to `meshes`.
    ///
    /// Validation computes these on every run. Until 0.8.2 they were
    /// projected away with the rest of the result and only their count
    /// survived, inside an issue message.
    degenerate_faces: Vec<Vec<u32>>,
    source_format: String,
    file_size_bytes: Option<u64>,
    /// Resolved for every model, not only when the budget check is on:
    /// classification is a fact about the file, and switching the check off
    /// should not make the report forget what the file is.
    asset_category: AssetCategory,
    triangle_budget: Option<u32>,
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
                name: m.name.clone(),
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
    /// Loads a model and runs validation with [`ProjectConfig::default`].
    /// Equivalent to [`ModelAnalyzer::new_with_config`] passing `None`.
    pub fn new(path: &str) -> Result<Self> {
        Self::new_with_config(path, None)
    }

    /// Loads a model and runs validation with a discovered or explicit
    /// `solarxy.toml`. Discovery starts in the model's parent directory.
    /// Emits a `tracing::info!` event when a config is loaded so the GUI
    /// console / CLI logging surfaces it.
    pub fn new_with_config(path: &str, config_path: Option<&Path>) -> Result<Self> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let raw = solarxy_formats::load_model(path)?;

        let model_name = Path::new(path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(path)
            .to_string();

        let model_dir = Path::new(path)
            .parent()
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf);

        let project_config = match project_config::discover(&model_dir, config_path) {
            Ok(Some((found, cfg))) => {
                tracing::info!(target: "solarxy::toast", "Loaded solarxy.toml from {}", found.display());
                cfg
            }
            Ok(None) => ProjectConfig::default(),
            Err(e) => {
                tracing::warn!("solarxy.toml load failed: {e}. Continuing with defaults.");
                ProjectConfig::default()
            }
        };

        let (asset_category, triangle_budget) = resolve_budget(&project_config, Path::new(path));
        let validation = solarxy_core::validation::validate_raw_model_with_config(
            &raw,
            &ext,
            &project_config.validation,
            &project_config.thresholds,
            triangle_budget,
        );
        let (meshes, materials) = raw_to_analyzer(&raw);

        Ok(ModelAnalyzer {
            model_name,
            meshes,
            materials,
            obj_dir: Path::new(path).parent().map(Path::to_path_buf),
            base_validation: validation.report,
            degenerate_faces: validation.degenerate_faces,
            source_format: ext,
            file_size_bytes: std::fs::metadata(path).ok().map(|m| m.len()),
            asset_category,
            triangle_budget,
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
                    name: mesh.name.clone(),
                    vertex_count,
                    index_count,
                    triangle_count: index_count / 3,
                    normal_count,
                    texcoord_count,
                    material_name,
                    material_id,
                    degenerate_faces: self.degenerate_faces.get(i).cloned().unwrap_or_default(),
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
                    if let Some(entry) = check_texture(
                        self.obj_dir.as_ref(),
                        tex_opt.as_ref(),
                        slot,
                        &mut issues,
                        i,
                    ) {
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
            source_format: self.source_format.clone(),
            file_size_bytes: self.file_size_bytes,
            asset_category: Some(self.asset_category),
            triangle_budget: self.triangle_budget,
        }
    }
}

/// Classifies the model by filename and looks the resulting category up in
/// the project's budget table.
///
/// The category is resolved unconditionally and the budget only when the
/// project enables the triangle-budget check, so a project that has
/// switched the check off still reports what kind of asset this is.
/// A filename matching no rule classifies as `AssetCategory::Default`,
/// which is an answer rather than a failure.
fn resolve_budget(project_config: &ProjectConfig, path: &Path) -> (AssetCategory, Option<u32>) {
    let category = project_config.filenames.classify(path);
    let budget = project_config
        .validation
        .triangle_budget
        .then(|| project_config.budgets.for_category(category));
    (category, budget)
}

fn check_texture(
    obj_dir: Option<&PathBuf>,
    tex_path: Option<&String>,
    slot: &str,
    issues: &mut Vec<ValidationIssue>,
    mat_index: usize,
) -> Option<TextureEntry> {
    let path = tex_path?;
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
            kind: solarxy_core::validation::IssueKind::MissingTexture,
            message: format!("Texture file not found: '{}'", path),
        });
    }

    Some(TextureEntry {
        slot: slot.to_string(),
        path: path.clone(),
        exists,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_budget_check(enabled: bool) -> ProjectConfig {
        let mut validation = ProjectConfig::default().validation;
        validation.triangle_budget = enabled;
        ProjectConfig {
            validation,
            ..ProjectConfig::default()
        }
    }

    /// Classification is a fact about the file. Switching the budget check
    /// off silences the budget, not the category, which before 0.8.2 was
    /// resolved inside the guard and lost with it.
    #[test]
    fn the_category_survives_the_budget_check_being_off() {
        let config = config_with_budget_check(false);
        let path = Path::new("tree_environment.obj");
        let (category, budget) = resolve_budget(&config, path);
        assert!(budget.is_none(), "the budget must stay off");
        assert_eq!(category, config.filenames.classify(path));
    }

    #[test]
    fn an_enabled_budget_comes_from_the_category() {
        let config = config_with_budget_check(true);
        let (category, budget) = resolve_budget(&config, Path::new("anything.obj"));
        assert_eq!(budget, Some(config.budgets.for_category(category)));
    }

    /// A filename matching no rule is classified, not left unclassified.
    #[test]
    fn an_unmatched_filename_classifies_as_default() {
        let config = config_with_budget_check(true);
        let (category, _) = resolve_budget(&config, Path::new("zzz.obj"));
        assert_eq!(category, AssetCategory::Default);
    }
}
