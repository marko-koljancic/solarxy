use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::Result;
use ply_rs_bw::ply::Property;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFormat {
    Obj,
    Stl,
    Ply,
    Gltf,
}

impl SourceFormat {
    pub fn supports_uvs(self) -> bool {
        matches!(self, SourceFormat::Obj | SourceFormat::Gltf)
    }
}

pub struct ModelAnalyzer {
    pub model_name: String,
    pub meshes: Vec<AnalyzerMesh>,
    pub materials: Vec<AnalyzerMaterial>,
    pub obj_dir: Option<PathBuf>,
    pub source_format: SourceFormat,
}

impl ModelAnalyzer {
    pub fn new(path: &str) -> Result<Self> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "stl" => Self::from_stl(path),
            "ply" => Self::from_ply(path),
            "gltf" | "glb" => Self::from_gltf(path),
            _ => Self::from_obj(path),
        }
    }

    fn parent_dir(path: &str) -> Option<PathBuf> {
        Path::new(path).parent().map(|p| p.to_path_buf())
    }

    fn from_obj(path: &str) -> Result<Self> {
        let model_name = path.split('/').next_back().unwrap_or(path).to_string();
        let (models, materials) = tobj::load_obj(path, &tobj::GPU_LOAD_OPTIONS)?;

        let meshes = models
            .into_iter()
            .map(|m| AnalyzerMesh {
                positions: m.mesh.positions,
                indices: m.mesh.indices,
                normals: m.mesh.normals,
                texcoords: m.mesh.texcoords,
                material_id: m.mesh.material_id,
            })
            .collect();

        let materials = materials
            .unwrap_or_default()
            .into_iter()
            .map(|m| AnalyzerMaterial {
                name: m.name,
                ambient: m.ambient,
                diffuse: m.diffuse,
                specular: m.specular,
                shininess: m.shininess,
                dissolve: m.dissolve,
                optical_density: m.optical_density,
                diffuse_texture: m.diffuse_texture,
                ambient_texture: m.ambient_texture,
                specular_texture: m.specular_texture,
                normal_texture: m.normal_texture,
                shininess_texture: m.shininess_texture,
                dissolve_texture: m.dissolve_texture,
            })
            .collect();

        Ok(ModelAnalyzer {
            model_name,
            meshes,
            materials,
            obj_dir: Self::parent_dir(path),
            source_format: SourceFormat::Obj,
        })
    }

    fn from_stl(path: &str) -> Result<Self> {
        let model_name = path.split('/').next_back().unwrap_or(path).to_string();
        let file = std::fs::File::open(path)?;
        let mut reader = BufReader::new(file);
        let indexed_mesh = stl_io::read_stl(&mut reader)?;

        let positions: Vec<f32> = indexed_mesh
            .vertices
            .iter()
            .flat_map(|v| [v[0], v[1], v[2]])
            .collect();

        let indices: Vec<u32> = indexed_mesh
            .faces
            .iter()
            .flat_map(|f| f.vertices.iter().map(|&i| i as u32))
            .collect();

        let mesh = AnalyzerMesh {
            positions,
            indices,
            normals: Vec::new(),
            texcoords: Vec::new(),
            material_id: None,
        };

        Ok(ModelAnalyzer {
            model_name,
            meshes: vec![mesh],
            materials: Vec::new(),
            obj_dir: Self::parent_dir(path),
            source_format: SourceFormat::Stl,
        })
    }

    fn from_ply(path: &str) -> Result<Self> {
        let model_name = path.split('/').next_back().unwrap_or(path).to_string();
        let file = std::fs::File::open(path)?;
        let mut reader = BufReader::new(file);
        let parser = ply_rs_bw::parser::Parser::<ply_rs_bw::ply::DefaultElement>::new();
        let ply = parser.read_ply(&mut reader)?;

        let ply_vertices = ply
            .payload
            .get("vertex")
            .ok_or_else(|| anyhow::anyhow!("PLY file has no 'vertex' element"))?;
        let ply_faces = ply
            .payload
            .get("face")
            .ok_or_else(|| anyhow::anyhow!("PLY file has no 'face' element"))?;

        let (has_normals, uv_keys) = if let Some(first) = ply_vertices.first() {
            let has_normals = first.get("nx").is_some();
            let uv_keys: Option<(&str, &str)> =
                if first.get("s").is_some() && first.get("t").is_some() {
                    Some(("s", "t"))
                } else if first.get("u").is_some() && first.get("v").is_some() {
                    Some(("u", "v"))
                } else if first.get("texture_u").is_some() && first.get("texture_v").is_some() {
                    Some(("texture_u", "texture_v"))
                } else {
                    None
                };
            (has_normals, uv_keys)
        } else {
            (false, None)
        };

        let mut positions: Vec<f32> = Vec::with_capacity(ply_vertices.len() * 3);
        let mut normals: Vec<f32> = Vec::new();
        let mut texcoords: Vec<f32> = Vec::new();

        if has_normals {
            normals.reserve(ply_vertices.len() * 3);
        }
        if uv_keys.is_some() {
            texcoords.reserve(ply_vertices.len() * 2);
        }

        for elem in ply_vertices {
            positions.push(ply_analyzer_prop_to_f32(elem.get("x")));
            positions.push(ply_analyzer_prop_to_f32(elem.get("y")));
            positions.push(ply_analyzer_prop_to_f32(elem.get("z")));

            if has_normals {
                normals.push(ply_analyzer_prop_to_f32(elem.get("nx")));
                normals.push(ply_analyzer_prop_to_f32(elem.get("ny")));
                normals.push(ply_analyzer_prop_to_f32(elem.get("nz")));
            }

            if let Some((u_key, v_key)) = uv_keys {
                texcoords.push(ply_analyzer_prop_to_f32(elem.get(u_key)));
                texcoords.push(ply_analyzer_prop_to_f32(elem.get(v_key)));
            }
        }

        let mut indices: Vec<u32> = Vec::new();
        for face in ply_faces {
            let vis = face
                .get("vertex_indices")
                .or_else(|| face.get("vertex_index"))
                .map(ply_analyzer_prop_to_indices)
                .unwrap_or_default();
            for i in 1..vis.len().saturating_sub(1) {
                indices.push(vis[0]);
                indices.push(vis[i] as u32);
                indices.push(vis[i + 1] as u32);
            }
        }

        let mesh = AnalyzerMesh {
            positions,
            indices,
            normals,
            texcoords,
            material_id: None,
        };

        Ok(ModelAnalyzer {
            model_name,
            meshes: vec![mesh],
            materials: Vec::new(),
            obj_dir: Self::parent_dir(path),
            source_format: SourceFormat::Ply,
        })
    }

    fn from_gltf(path: &str) -> Result<Self> {
        let model_name = path.split('/').next_back().unwrap_or(path).to_string();
        let (document, buffers, _images) = gltf::import(path)?;

        let mut meshes = Vec::new();
        for scene in document.scenes() {
            for node in scene.nodes() {
                Self::collect_gltf_meshes(&node, &buffers, &mut meshes);
            }
        }

        let materials = document
            .materials()
            .map(|mat| {
                let pbr = mat.pbr_metallic_roughness();
                let base_color = pbr.base_color_factor();
                AnalyzerMaterial {
                    name: mat.name().unwrap_or("gltf_material").to_string(),
                    ambient: None,
                    diffuse: Some([base_color[0], base_color[1], base_color[2]]),
                    specular: None,
                    shininess: None,
                    dissolve: Some(base_color[3]),
                    optical_density: None,
                    diffuse_texture: pbr
                        .base_color_texture()
                        .map(|t| format!("texture_index:{}", t.texture().source().index())),
                    ambient_texture: None,
                    specular_texture: None,
                    normal_texture: mat
                        .normal_texture()
                        .map(|t| format!("texture_index:{}", t.texture().source().index())),
                    shininess_texture: None,
                    dissolve_texture: None,
                }
            })
            .collect();

        Ok(ModelAnalyzer {
            model_name,
            meshes,
            materials,
            obj_dir: Self::parent_dir(path),
            source_format: SourceFormat::Gltf,
        })
    }

    fn collect_gltf_meshes(
        node: &gltf::Node,
        buffers: &[gltf::buffer::Data],
        meshes: &mut Vec<AnalyzerMesh>,
    ) {
        if let Some(mesh) = node.mesh() {
            for primitive in mesh.primitives() {
                let reader = primitive.reader(|buffer| Some(&buffers[buffer.index()]));

                let positions: Vec<f32> = reader
                    .read_positions()
                    .map(|iter| iter.flatten().collect())
                    .unwrap_or_default();
                let indices: Vec<u32> = reader
                    .read_indices()
                    .map(|iter| iter.into_u32().collect())
                    .unwrap_or_default();
                let normals: Vec<f32> = reader
                    .read_normals()
                    .map(|iter| iter.flatten().collect())
                    .unwrap_or_default();
                let texcoords: Vec<f32> = reader
                    .read_tex_coords(0)
                    .map(|iter| iter.into_f32().flatten().collect())
                    .unwrap_or_default();

                meshes.push(AnalyzerMesh {
                    positions,
                    indices,
                    normals,
                    texcoords,
                    material_id: primitive.material().index(),
                });
            }
        }
        for child in node.children() {
            Self::collect_gltf_meshes(&child, buffers, meshes);
        }
    }

    pub fn generate_report(&self) -> AnalysisReport {
        let mut issues = Vec::new();

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

                if !mesh.normals.is_empty() && normal_count != vertex_count {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        scope: IssueScope::Mesh(i),
                        message: format!(
                            "Normal count ({}) does not match vertex count ({})",
                            normal_count, vertex_count
                        ),
                    });
                }

                if !mesh.texcoords.is_empty() && texcoord_count != vertex_count {
                    issues.push(ValidationIssue {
                        severity: Severity::Warning,
                        scope: IssueScope::Mesh(i),
                        message: format!(
                            "Texture coordinate count ({}) does not match vertex count ({})",
                            texcoord_count, vertex_count
                        ),
                    });
                }

                if mesh.texcoords.is_empty() && self.source_format.supports_uvs() {
                    issues.push(ValidationIssue {
                        severity: Severity::Warning,
                        scope: IssueScope::Mesh(i),
                        message: "No texture coordinates".to_string(),
                    });
                }

                if index_count % 3 != 0 {
                    issues.push(ValidationIssue {
                        severity: Severity::Error,
                        scope: IssueScope::Mesh(i),
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
                        message: "Empty index buffer".to_string(),
                    });
                }

                let (material_name, material_id) = if let Some(mat_id) = mesh.material_id {
                    if mat_id < self.materials.len() {
                        (Some(self.materials[mat_id].name.clone()), Some(mat_id))
                    } else {
                        issues.push(ValidationIssue {
                            severity: Severity::Error,
                            scope: IssueScope::Mesh(i),
                            message: format!(
                                "Material ID {} is out of range (only {} materials available)",
                                mat_id,
                                self.materials.len()
                            ),
                        });
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
            message: format!("Texture file not found: '{}'", path),
        });
    }

    Some(TextureEntry {
        slot: slot.to_string(),
        path: path.clone(),
        exists,
    })
}

fn ply_analyzer_prop_to_f32(prop: Option<&Property>) -> f32 {
    match prop {
        Some(Property::Float(v)) => *v,
        Some(Property::Double(v)) => *v as f32,
        Some(Property::Int(v)) => *v as f32,
        Some(Property::UInt(v)) => *v as f32,
        Some(Property::Short(v)) => *v as f32,
        Some(Property::UShort(v)) => *v as f32,
        Some(Property::Char(v)) => *v as f32,
        Some(Property::UChar(v)) => *v as f32,
        _ => 0.0,
    }
}

fn ply_analyzer_prop_to_indices(prop: &Property) -> Vec<u32> {
    match prop {
        Property::ListInt(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUInt(v) => v.clone(),
        Property::ListShort(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUShort(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListUChar(v) => v.iter().map(|&i| i as u32).collect(),
        Property::ListChar(v) => v.iter().map(|&i| i as u32).collect(),
        _ => Vec::new(),
    }
}
