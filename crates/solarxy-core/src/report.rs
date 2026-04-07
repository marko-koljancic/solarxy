use std::fmt;

use crate::format_number;
pub use crate::validation::{IssueScope, Severity, ValidationIssue, ValidationReport};

#[derive(Debug, Clone)]
pub struct MeshSummary {
    pub index: usize,
    pub vertex_count: usize,
    pub index_count: usize,
    pub triangle_count: usize,
    pub normal_count: usize,
    pub texcoord_count: usize,
    pub material_name: Option<String>,
    pub material_id: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct TextureEntry {
    pub slot: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone)]
pub struct MaterialSummary {
    pub index: usize,
    pub name: String,
    pub ambient: [f32; 3],
    pub diffuse: [f32; 3],
    pub specular: [f32; 3],
    pub shininess: Option<f32>,
    pub dissolve: Option<f32>,
    pub optical_density: Option<f32>,
    pub textures: Vec<TextureEntry>,
}

#[derive(Debug, Clone)]
pub struct BoundsSummary {
    pub min: [f32; 3],
    pub max: [f32; 3],
    pub size: [f32; 3],
    pub center: [f32; 3],
    pub diagonal: f32,
}

#[derive(Debug, Clone)]
pub struct AnalysisReport {
    pub model_name: String,
    pub mesh_count: usize,
    pub material_count: usize,
    pub total_vertices: usize,
    pub total_indices: usize,
    pub total_triangles: usize,
    pub bounds: Option<BoundsSummary>,
    pub meshes: Vec<MeshSummary>,
    pub materials: Vec<MaterialSummary>,
    pub validation: ValidationReport,
}

impl fmt::Display for AnalysisReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.validation.is_clean() {
            writeln!(f, "VALIDATION\n")?;
            for issue in &self.validation.issues {
                writeln!(f, "  {} {}: {}", issue.severity, issue.scope, issue.message)?;
            }
            writeln!(f)?;
        }

        writeln!(f, "MODEL OVERVIEW\n")?;
        writeln!(f, "Model Name:       {}", self.model_name)?;
        writeln!(f, "Mesh Count:       {}", self.mesh_count)?;
        writeln!(f, "Material Count:   {}", self.material_count)?;
        writeln!(
            f,
            "Total Vertices:   {}",
            format_number(self.total_vertices)
        )?;
        writeln!(f, "Total Indices:    {}", format_number(self.total_indices))?;
        writeln!(
            f,
            "Total Triangles:  {}",
            format_number(self.total_triangles)
        )?;

        if let Some(ref bounds) = self.bounds {
            writeln!(f)?;
            writeln!(f, "Bounding Box:")?;
            writeln!(
                f,
                "  Min:        [{:.3}, {:.3}, {:.3}]",
                bounds.min[0], bounds.min[1], bounds.min[2]
            )?;
            writeln!(
                f,
                "  Max:        [{:.3}, {:.3}, {:.3}]",
                bounds.max[0], bounds.max[1], bounds.max[2]
            )?;
            writeln!(
                f,
                "  Size:       [{:.3}, {:.3}, {:.3}]",
                bounds.size[0], bounds.size[1], bounds.size[2]
            )?;
            writeln!(
                f,
                "  Center:     [{:.3}, {:.3}, {:.3}]",
                bounds.center[0], bounds.center[1], bounds.center[2]
            )?;
            writeln!(f, "  Diagonal:   {:.3}", bounds.diagonal)?;
        }

        if !self.meshes.is_empty() {
            writeln!(f, "\n\nMESH DETAILS\n")?;
            for (i, mesh) in self.meshes.iter().enumerate() {
                writeln!(f, "Mesh [{}]:", mesh.index)?;
                writeln!(f, "  Vertices:        {}", format_number(mesh.vertex_count))?;
                writeln!(f, "  Indices:         {}", format_number(mesh.index_count))?;
                writeln!(
                    f,
                    "  Triangles:       {}",
                    format_number(mesh.triangle_count)
                )?;
                writeln!(
                    f,
                    "  Normals:         {} {}",
                    format_number(mesh.normal_count),
                    if mesh.normal_count == mesh.vertex_count {
                        "\u{2713}"
                    } else {
                        "\u{26a0}"
                    }
                )?;
                writeln!(
                    f,
                    "  Texture Coords:  {} {}",
                    format_number(mesh.texcoord_count),
                    if mesh.texcoord_count == mesh.vertex_count {
                        "\u{2713}"
                    } else if mesh.texcoord_count == 0 {
                        "\u{2717}"
                    } else {
                        "\u{26a0}"
                    }
                )?;

                match (&mesh.material_name, mesh.material_id) {
                    (Some(name), Some(id)) => {
                        writeln!(f, "  Material:        '{}' (ID: {})", name, id)?;
                    }
                    (None, Some(id)) => writeln!(f, "  Material:        Invalid ID: {}", id)?,
                    _ => writeln!(f, "  Material:        None")?,
                }

                if i < self.meshes.len() - 1 {
                    writeln!(f)?;
                }
            }
        }

        if self.materials.is_empty() {
            writeln!(f, "\n\nMATERIALS\n")?;
            writeln!(f, "No materials found (.mtl file not provided or empty)")?;
        } else {
            writeln!(f, "\n\nMATERIAL DETAILS\n")?;
            for (i, mat) in self.materials.iter().enumerate() {
                writeln!(f, "Material [{}]: '{}'", mat.index, mat.name)?;
                writeln!(
                    f,
                    "  Ambient:  [{:.3}, {:.3}, {:.3}]",
                    mat.ambient[0], mat.ambient[1], mat.ambient[2]
                )?;
                writeln!(
                    f,
                    "  Diffuse:  [{:.3}, {:.3}, {:.3}]",
                    mat.diffuse[0], mat.diffuse[1], mat.diffuse[2]
                )?;
                writeln!(
                    f,
                    "  Specular: [{:.3}, {:.3}, {:.3}]",
                    mat.specular[0], mat.specular[1], mat.specular[2]
                )?;

                if let Some(shininess) = mat.shininess {
                    writeln!(f, "  Shininess: {:.3}", shininess)?;
                }
                if let Some(dissolve) = mat.dissolve {
                    writeln!(f, "  Dissolve (opacity): {:.3}", dissolve)?;
                }
                if let Some(optical_density) = mat.optical_density {
                    writeln!(f, "  Optical Density: {:.3}", optical_density)?;
                }

                writeln!(f, "  Textures:")?;
                if mat.textures.is_empty() {
                    writeln!(f, "    None")?;
                } else {
                    for tex in &mat.textures {
                        let indicator = if tex.exists { "" } else { " [MISSING]" };
                        writeln!(
                            f,
                            "    {:14} '{}'{}",
                            format!("{}:", tex.slot),
                            tex.path,
                            indicator
                        )?;
                    }
                }

                if i < self.materials.len() - 1 {
                    writeln!(f)?;
                }
            }
        }

        Ok(())
    }
}
