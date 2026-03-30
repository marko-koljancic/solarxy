use std::io::BufReader;
use std::path::Path;

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
}

impl ModelAnalyzer {
    pub fn new(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "stl" => Self::from_stl(path),
            _ => Self::from_obj(path),
        }
    }

    fn from_obj(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
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
        })
    }

    fn from_stl(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let model_name = path.split('/').next_back().unwrap_or(path).to_string();
        let file = std::fs::File::open(path)?;
        let mut reader = BufReader::new(file);
        let indexed_mesh = stl_io::read_stl(&mut reader)?;

        let positions: Vec<f32> = indexed_mesh.vertices.iter().flat_map(|v| [v[0], v[1], v[2]]).collect();

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
        })
    }

    pub fn generate_report(&self) -> String {
        let mut output = String::new();

        output.push_str("MODEL OVERVIEW\n\n");
        output.push_str(&format!("Model Name:       {}\n", self.model_name));
        output.push_str(&format!("Mesh Count:       {}\n", self.meshes.len()));
        output.push_str(&format!("Material Count:   {}\n", self.materials.len()));

        let total_vertices: usize = self.meshes.iter().map(|m| m.positions.len() / 3).sum();
        let total_indices: usize = self.meshes.iter().map(|m| m.indices.len()).sum();
        let total_triangles: usize = self.meshes.iter().map(|m| m.indices.len() / 3).sum();

        output.push_str(&format!("Total Vertices:   {}\n", format_number(total_vertices)));
        output.push_str(&format!("Total Indices:    {}\n", format_number(total_indices)));
        output.push_str(&format!("Total Triangles:  {}\n", format_number(total_triangles)));

        if !self.meshes.is_empty() {
            output.push_str("\n\nMESH DETAILS\n\n");

            for (i, mesh) in self.meshes.iter().enumerate() {
                let vertex_count = mesh.positions.len() / 3;
                let index_count = mesh.indices.len();
                let triangle_count = index_count / 3;
                let normal_count = mesh.normals.len() / 3;
                let texcoord_count = mesh.texcoords.len() / 2;

                output.push_str(&format!("Mesh [{}]:\n", i));
                output.push_str(&format!("  Vertices:        {}\n", format_number(vertex_count)));
                output.push_str(&format!("  Indices:         {}\n", format_number(index_count)));
                output.push_str(&format!("  Triangles:       {}\n", format_number(triangle_count)));
                output.push_str(&format!(
                    "  Normals:         {} {}\n",
                    format_number(normal_count),
                    if normal_count == vertex_count { "✓" } else { "⚠" }
                ));
                output.push_str(&format!(
                    "  Texture Coords:  {} {}\n",
                    format_number(texcoord_count),
                    if texcoord_count == vertex_count {
                        "✓"
                    } else if texcoord_count == 0 {
                        "✗"
                    } else {
                        "⚠"
                    }
                ));

                if let Some(mat_id) = mesh.material_id {
                    if mat_id < self.materials.len() {
                        output.push_str(&format!(
                            "  Material:        '{}' (ID: {})\n",
                            self.materials[mat_id].name, mat_id
                        ));
                    } else {
                        output.push_str(&format!("  Material:        Invalid ID: {}\n", mat_id));
                    }
                } else {
                    output.push_str("  Material:        None\n");
                }

                if i < self.meshes.len() - 1 {
                    output.push('\n');
                }
            }
        }

        if self.materials.is_empty() {
            output.push_str("\n\nMATERIALS\n\n");
            output.push_str("No materials found (.mtl file not provided or empty)\n");
        } else {
            output.push_str("\n\nMATERIAL DETAILS\n\n");

            for (i, mat) in self.materials.iter().enumerate() {
                output.push_str(&format!("Material [{}]: '{}'\n", i, mat.name));
                output.push_str(&format!(
                    "  Ambient:  [{:.3}, {:.3}, {:.3}]\n",
                    mat.ambient.unwrap_or([0.0, 0.0, 0.0])[0],
                    mat.ambient.unwrap_or([0.0, 0.0, 0.0])[1],
                    mat.ambient.unwrap_or([0.0, 0.0, 0.0])[2]
                ));
                output.push_str(&format!(
                    "  Diffuse:  [{:.3}, {:.3}, {:.3}]\n",
                    mat.diffuse.unwrap_or([0.0, 0.0, 0.0])[0],
                    mat.diffuse.unwrap_or([0.0, 0.0, 0.0])[1],
                    mat.diffuse.unwrap_or([0.0, 0.0, 0.0])[2]
                ));
                output.push_str(&format!(
                    "  Specular: [{:.3}, {:.3}, {:.3}]\n",
                    mat.specular.unwrap_or([0.0, 0.0, 0.0])[0],
                    mat.specular.unwrap_or([0.0, 0.0, 0.0])[1],
                    mat.specular.unwrap_or([0.0, 0.0, 0.0])[2]
                ));

                if let Some(shininess) = mat.shininess {
                    output.push_str(&format!("  Shininess: {:.3}\n", shininess));
                }
                if let Some(dissolve) = mat.dissolve {
                    output.push_str(&format!("  Dissolve (opacity): {:.3}\n", dissolve));
                }
                if let Some(optical_density) = mat.optical_density {
                    output.push_str(&format!("  Optical Density: {:.3}\n", optical_density));
                }

                output.push_str("  Textures:\n");

                let mut has_textures = false;

                if let Some(ref tex) = mat.diffuse_texture {
                    output.push_str(&format!("    Diffuse:         '{}'\n", tex));
                    has_textures = true;
                }
                if let Some(ref tex) = mat.ambient_texture {
                    output.push_str(&format!("    Ambient:         '{}'\n", tex));
                    has_textures = true;
                }
                if let Some(ref tex) = mat.specular_texture {
                    output.push_str(&format!("    Specular:        '{}'\n", tex));
                    has_textures = true;
                }
                if let Some(ref tex) = mat.normal_texture {
                    output.push_str(&format!("    Normal:          '{}'\n", tex));
                    has_textures = true;
                }
                if let Some(ref tex) = mat.shininess_texture {
                    output.push_str(&format!("    Shininess:       '{}'\n", tex));
                    has_textures = true;
                }
                if let Some(ref tex) = mat.dissolve_texture {
                    output.push_str(&format!("    Dissolve:        '{}'\n", tex));
                    has_textures = true;
                }
                if !has_textures {
                    output.push_str("    None\n");
                }
                if i < self.materials.len() - 1 {
                    output.push('\n');
                }
            }
        }
        output
    }
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(',');
        }
        result.push(*c);
    }

    result
}
