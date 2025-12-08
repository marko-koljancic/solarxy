use tobj;

pub struct ModelAnalyzer {
    pub model_name: String,
    pub meshes: Vec<tobj::Mesh>,
    pub materials: Vec<tobj::Material>,
}

impl ModelAnalyzer {
    pub fn new(path: &str) -> Result<Self, tobj::LoadError> {
        let model_name = path.split('/').last().unwrap_or(path).to_string();
        let (models, materials) = tobj::load_obj(path, &tobj::GPU_LOAD_OPTIONS)?;

        let meshes = models.into_iter().map(|m| m.mesh).collect();
        let materials = materials.unwrap_or_default();

        Ok(ModelAnalyzer {
            model_name,
            meshes,
            materials,
        })
    }

    pub fn run_analysis(&self) {
        self.print_header();
        self.print_model_summary();
        self.print_mesh_details();
        self.print_material_details();
        self.print_footer();
    }

    fn print_header(&self) {
        println!("\n{}", "=".repeat(80));
        println!("{:^80}", "MODEL ANALYSIS REPORT");
        println!("{}", "=".repeat(80));
    }

    fn print_footer(&self) {
        println!("{}", "=".repeat(80));
        println!();
    }

    fn print_model_summary(&self) {
        println!("\n┌─ MODEL OVERVIEW {}", "─".repeat(62));
        println!("│");
        println!("│  Model Name:       {}", self.model_name);
        println!("│  Mesh Count:       {}", self.meshes.len());
        println!("│  Material Count:   {}", self.materials.len());

        let total_vertices: usize = self.meshes.iter().map(|m| m.positions.len() / 3).sum();
        let total_indices: usize = self.meshes.iter().map(|m| m.indices.len()).sum();
        let total_triangles: usize = self.meshes.iter().map(|m| m.indices.len() / 3).sum();

        println!("│  Total Vertices:   {}", format_number(total_vertices));
        println!("│  Total Indices:    {}", format_number(total_indices));
        println!("│  Total Triangles:  {}", format_number(total_triangles));
        println!("│");
        println!("└{}", "─".repeat(79));
    }

    fn print_mesh_details(&self) {
        if self.meshes.is_empty() {
            return;
        }

        println!("\n┌─ MESH DETAILS {}", "─".repeat(64));
        println!("│");

        for (i, mesh) in self.meshes.iter().enumerate() {
            let vertex_count = mesh.positions.len() / 3;
            let index_count = mesh.indices.len();
            let triangle_count = index_count / 3;
            let normal_count = mesh.normals.len() / 3;
            let texcoord_count = mesh.texcoords.len() / 2;

            println!("│  [{:3}] Mesh Statistics:", i);
            println!("│       ├─ Vertices:        {:>10}", format_number(vertex_count));
            println!("│       ├─ Indices:         {:>10}", format_number(index_count));
            println!("│       ├─ Triangles:       {:>10}", format_number(triangle_count));
            println!(
                "│       ├─ Normals:         {:>10} {}",
                format_number(normal_count),
                if normal_count == vertex_count { "✓" } else { "⚠" }
            );
            println!(
                "│       ├─ Texture Coords:  {:>10} {}",
                format_number(texcoord_count),
                if texcoord_count == vertex_count {
                    "✓"
                } else if texcoord_count == 0 {
                    "✗"
                } else {
                    "⚠"
                }
            );

            // Material reference
            if let Some(mat_id) = mesh.material_id {
                if mat_id < self.materials.len() {
                    println!(
                        "│       └─ Material:        '{}' (ID: {})",
                        self.materials[mat_id].name, mat_id
                    );
                } else {
                    println!("│       └─ Material:        Invalid ID: {}", mat_id);
                }
            } else {
                println!("│       └─ Material:        None");
            }

            if i < self.meshes.len() - 1 {
                println!("│");
            }
        }

        println!("│");
        println!("└{}", "─".repeat(79));
    }

    fn print_material_details(&self) {
        if self.materials.is_empty() {
            println!("\n┌─ MATERIALS {}", "─".repeat(67));
            println!("│");
            println!("│  No materials found (.mtl file not provided or empty)");
            println!("│");
            println!("└{}", "─".repeat(79));
            return;
        }

        println!("\n┌─ MATERIAL DETAILS {}", "─".repeat(60));
        println!("│");

        for (i, mat) in self.materials.iter().enumerate() {
            println!("│  [{:3}] Material: '{}'", i, mat.name);
            println!(
                "│       ├─ Ambient:  [{:.3}, {:.3}, {:.3}]",
                mat.ambient.unwrap_or([0.0, 0.0, 0.0])[0],
                mat.ambient.unwrap_or([0.0, 0.0, 0.0])[1],
                mat.ambient.unwrap_or([0.0, 0.0, 0.0])[2]
            );
            println!(
                "│       ├─ Diffuse:  [{:.3}, {:.3}, {:.3}]",
                mat.diffuse.unwrap_or([0.0, 0.0, 0.0])[0],
                mat.diffuse.unwrap_or([0.0, 0.0, 0.0])[1],
                mat.diffuse.unwrap_or([0.0, 0.0, 0.0])[2]
            );
            println!(
                "│       ├─ Specular: [{:.3}, {:.3}, {:.3}]",
                mat.specular.unwrap_or([0.0, 0.0, 0.0])[0],
                mat.specular.unwrap_or([0.0, 0.0, 0.0])[1],
                mat.specular.unwrap_or([0.0, 0.0, 0.0])[2]
            );

            if let Some(shininess) = mat.shininess {
                println!("│       ├─ Shininess: {:.3}", shininess);
            }

            if let Some(dissolve) = mat.dissolve {
                println!("│       ├─ Dissolve (opacity): {:.3}", dissolve);
            }

            if let Some(optical_density) = mat.optical_density {
                println!("│       ├─ Optical Density: {:.3}", optical_density);
            }

            // Texture maps
            println!("│       └─ Textures:");

            let mut has_textures = false;

            if let Some(ref tex) = mat.diffuse_texture {
                println!("│          ├─ Diffuse:         '{}'", tex);
                has_textures = true;
            }

            if let Some(ref tex) = mat.ambient_texture {
                println!("│          ├─ Ambient:         '{}'", tex);
                has_textures = true;
            }

            if let Some(ref tex) = mat.specular_texture {
                println!("│          ├─ Specular:        '{}'", tex);
                has_textures = true;
            }

            if let Some(ref tex) = mat.normal_texture {
                println!("│          ├─ Normal:          '{}'", tex);
                has_textures = true;
            }

            if let Some(ref tex) = mat.shininess_texture {
                println!("│          ├─ Shininess:       '{}'", tex);
                has_textures = true;
            }

            if let Some(ref tex) = mat.dissolve_texture {
                println!("│          ├─ Dissolve:        '{}'", tex);
                has_textures = true;
            }

            if !has_textures {
                println!("│          └─ None");
            } else {
                println!("│          └─ [End of textures]");
            }

            if i < self.materials.len() - 1 {
                println!("│");
            }
        }

        println!("│");
        println!("└{}", "─".repeat(79));
    }
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();

    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }

    result
}
