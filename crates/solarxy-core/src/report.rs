//! Analyzer report data: mesh / material / texture / bounds summaries used
//! by `solarxy-cli analyze` (TUI rows + JSON output via [`crate::json`]).
//!
//! Available with the `serialization` feature.

use std::fmt;

use crate::format_number;
use crate::project_config::AssetCategory;
pub use crate::validation::{IssueScope, Severity, ValidationIssue, ValidationReport};

#[derive(Debug, Clone)]
pub struct MeshSummary {
    pub index: usize,
    /// The name the file gave this mesh, empty when the format carries
    /// none. Loaders have always had it; before 0.8.2 the analyzer dropped
    /// it during conversion, so a report could only ever say `Mesh [2]`.
    pub name: String,
    pub vertex_count: usize,
    pub index_count: usize,
    pub triangle_count: usize,
    pub normal_count: usize,
    pub texcoord_count: usize,
    pub material_name: Option<String>,
    pub material_id: Option<usize>,
    /// Indices of this mesh's degenerate faces, empty when it has none.
    ///
    /// Validation has always computed these and the analyzer discarded
    /// them, keeping only the count embedded in an issue message. Surfaces
    /// print a count and a bounded sample rather than the whole list: a
    /// broken asset can have millions.
    pub degenerate_faces: Vec<u32>,
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
    /// The lowercased extension the model was loaded as, empty when the
    /// path had none.
    pub source_format: String,
    /// Size of the model file itself, excluding any companion it
    /// references. `None` when the file could not be measured.
    pub file_size_bytes: Option<u64>,
    /// What the project's filename rules classified this model as.
    ///
    /// `None` means nothing classified it, which is not the same as
    /// [`AssetCategory::Default`], meaning it was classified and matched no
    /// rule. Before 0.8.2 both the category and the budget below were
    /// resolved, used to raise one issue, and then discarded.
    pub asset_category: Option<AssetCategory>,
    /// The triangle budget for that category, or `None` when the project's
    /// budget check is switched off.
    pub triangle_budget: Option<u32>,
}

impl fmt::Display for AnalysisReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.validation.is_clean() {
            writeln!(f, "VALIDATION\n")?;
            for issue in &self.validation.issues {
                writeln!(f, "  {} {}: {}", issue.severity, issue.scope, issue.message)?;
            }
            writeln!(f)?;
            writeln!(f, "  By kind:")?;
            for (kind, count) in self.validation.ranked_kinds() {
                writeln!(f, "    {:28} {}", format!("{}:", kind), count)?;
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
        writeln!(
            f,
            "Source Format:    {}",
            describe_format(&self.source_format)
        )?;
        writeln!(
            f,
            "File Size:        {}",
            describe_size(self.file_size_bytes)
        )?;
        writeln!(
            f,
            "Asset Category:   {}",
            describe_category(self.asset_category)
        )?;
        writeln!(
            f,
            "Triangle Budget:  {}",
            describe_budget(self.triangle_budget)
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
                writeln!(f, "  Name:            {}", describe_name(&mesh.name))?;
                writeln!(
                    f,
                    "  Degenerate Faces: {}",
                    describe_degenerate(&mesh.degenerate_faces)
                )?;

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

// Every recovered fact prints its line whether or not the fact is present.
// A line that disappears when the value is absent is invisible to a script
// testing for presence, so absence gets an explicit word instead.

fn describe_format(format: &str) -> &str {
    if format.is_empty() { "unknown" } else { format }
}

fn describe_size(bytes: Option<u64>) -> String {
    bytes.map_or_else(
        || "unknown".to_owned(),
        |b| {
            format!(
                "{} bytes",
                format_number(usize::try_from(b).unwrap_or(usize::MAX))
            )
        },
    )
}

fn describe_category(category: Option<AssetCategory>) -> String {
    category.map_or_else(|| "not classified".to_owned(), |c| c.to_string())
}

fn describe_budget(budget: Option<u32>) -> String {
    budget.map_or_else(
        || "not configured".to_owned(),
        |b| format_number(usize::try_from(b).unwrap_or(usize::MAX)),
    )
}

fn describe_name(name: &str) -> &str {
    if name.is_empty() { "(unnamed)" } else { name }
}

/// A count plus a bounded sample.
///
/// A broken asset can carry millions of degenerate faces, and printing the
/// whole list into the one surface the append-only rule exists to keep
/// greppable would defeat the point of keeping it.
fn describe_degenerate(faces: &[u32]) -> String {
    const SAMPLE: usize = 10;

    if faces.is_empty() {
        return "0".to_owned();
    }
    let shown: Vec<String> = faces.iter().take(SAMPLE).map(u32::to_string).collect();
    let more = if faces.len() > SAMPLE { ", ..." } else { "" };
    format!(
        "{} [{}{}]",
        format_number(faces.len()),
        shown.join(", "),
        more
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validation::{IssueKind, ValidationIssue};

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
            file_size_bytes: None,
            asset_category: None,
            triangle_budget: None,
        }
    }

    fn mesh_summary(
        vertex_count: usize,
        normal_count: usize,
        texcoord_count: usize,
    ) -> MeshSummary {
        MeshSummary {
            index: 0,
            name: String::new(),
            vertex_count,
            index_count: 36,
            triangle_count: 12,
            normal_count,
            texcoord_count,
            material_name: None,
            material_id: None,
            degenerate_faces: Vec::new(),
        }
    }

    #[test]
    fn display_contains_model_overview() {
        let output = format!("{}", empty_report());
        assert!(output.contains("MODEL OVERVIEW"));
    }

    #[test]
    fn display_validation_before_overview() {
        let mut r = empty_report();
        r.validation.issues.push(ValidationIssue {
            severity: Severity::Error,
            scope: IssueScope::Model,
            kind: IssueKind::EmptyIndices,
            message: "test".to_owned(),
        });
        let output = format!("{}", r);
        let val_pos = output.find("VALIDATION").unwrap();
        let overview_pos = output.find("MODEL OVERVIEW").unwrap();
        assert!(val_pos < overview_pos);
    }

    #[test]
    fn display_clean_no_validation() {
        let output = format!("{}", empty_report());
        assert!(!output.contains("VALIDATION"));
    }

    #[test]
    fn display_bounds_formatting() {
        let mut r = empty_report();
        r.bounds = Some(BoundsSummary {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 2.0, 3.0],
            size: [1.0, 2.0, 3.0],
            center: [0.5, 1.0, 1.5],
            diagonal: 3.742,
        });
        let output = format!("{}", r);
        assert!(output.contains("[0.000, 0.000, 0.000]"));
        assert!(output.contains("[1.000, 2.000, 3.000]"));
        assert!(output.contains("3.742"));
    }

    #[test]
    fn display_normal_checkmark() {
        let mut r = empty_report();
        r.meshes.push(mesh_summary(100, 100, 100));
        let output = format!("{}", r);
        assert!(output.contains("\u{2713}"));
    }

    #[test]
    fn display_normal_warning() {
        let mut r = empty_report();
        r.meshes.push(mesh_summary(100, 50, 100));
        let output = format!("{}", r);
        assert!(output.contains("\u{26a0}"));
    }

    #[test]
    fn display_texcoord_zero_cross() {
        let mut r = empty_report();
        r.meshes.push(mesh_summary(100, 100, 0));
        let output = format!("{}", r);
        assert!(output.contains("\u{2717}"));
    }

    #[test]
    fn display_no_materials() {
        let output = format!("{}", empty_report());
        assert!(output.contains("No materials found"));
    }

    #[test]
    fn display_missing_texture() {
        let mut r = empty_report();
        r.materials.push(MaterialSummary {
            index: 0,
            name: "mat".to_owned(),
            ambient: [0.0; 3],
            diffuse: [0.5; 3],
            specular: [1.0; 3],
            shininess: None,
            dissolve: None,
            optical_density: None,
            textures: vec![TextureEntry {
                slot: "diffuse".to_owned(),
                path: "missing.png".to_owned(),
                exists: false,
            }],
        });
        let output = format!("{}", r);
        assert!(output.contains("[MISSING]"));
    }

    /// Every non-blank line the report emitted before the recovered facts
    /// were added, in the order it emitted them.
    ///
    /// This is the whole content of the append-only rule: a pipeline
    /// grepping the text output keeps working because none of these lines
    /// changed shape and none of them moved past another.
    const PRE_CHANGE_LINES: &[&str] = &[
        "VALIDATION",
        "  [ERROR] Model: test",
        "MODEL OVERVIEW",
        "Model Name:       test.obj",
        "Mesh Count:       1",
        "Material Count:   1",
        "Total Vertices:   8",
        "Total Indices:    36",
        "Total Triangles:  12",
        "Bounding Box:",
        "  Min:        [0.000, 0.000, 0.000]",
        "  Max:        [1.000, 2.000, 3.000]",
        "  Size:       [1.000, 2.000, 3.000]",
        "  Center:     [0.500, 1.000, 1.500]",
        "  Diagonal:   3.742",
        "MESH DETAILS",
        "Mesh [0]:",
        "  Vertices:        8",
        "  Indices:         36",
        "  Triangles:       12",
        "  Normals:         8 \u{2713}",
        "  Texture Coords:  8 \u{2713}",
        "  Material:        'wood' (ID: 0)",
        "MATERIAL DETAILS",
        "Material [0]: 'wood'",
        "  Ambient:  [0.000, 0.000, 0.000]",
        "  Diffuse:  [0.500, 0.500, 0.500]",
        "  Specular: [1.000, 1.000, 1.000]",
        "  Shininess: 32.000",
        "  Textures:",
        "    diffuse:       'd.png'",
    ];

    /// Exercises every branch the pinned lines above come from: a validation
    /// block, bounds, one mesh with a material, one material with a texture.
    fn populated_report() -> AnalysisReport {
        let mut r = empty_report();
        r.mesh_count = 1;
        r.material_count = 1;
        r.total_vertices = 8;
        r.total_indices = 36;
        r.total_triangles = 12;
        r.validation.issues.push(ValidationIssue {
            severity: Severity::Error,
            scope: IssueScope::Model,
            kind: IssueKind::EmptyIndices,
            message: "test".to_owned(),
        });
        r.bounds = Some(BoundsSummary {
            min: [0.0, 0.0, 0.0],
            max: [1.0, 2.0, 3.0],
            size: [1.0, 2.0, 3.0],
            center: [0.5, 1.0, 1.5],
            diagonal: 3.742,
        });
        let mut mesh = mesh_summary(8, 8, 8);
        mesh.index_count = 36;
        mesh.triangle_count = 12;
        mesh.material_name = Some("wood".to_owned());
        mesh.material_id = Some(0);
        r.meshes.push(mesh);
        r.materials.push(MaterialSummary {
            index: 0,
            name: "wood".to_owned(),
            ambient: [0.0; 3],
            diffuse: [0.5; 3],
            specular: [1.0; 3],
            shininess: Some(32.0),
            dissolve: None,
            optical_density: None,
            textures: vec![TextureEntry {
                slot: "diffuse".to_owned(),
                path: "d.png".to_owned(),
                exists: true,
            }],
        });
        r
    }

    #[test]
    fn no_pre_existing_line_changed_shape_or_moved() {
        let output = format!("{}", populated_report());
        let actual: Vec<&str> = output.lines().filter(|l| !l.trim().is_empty()).collect();
        let mut remaining = actual.iter();
        for expected in PRE_CHANGE_LINES {
            assert!(
                remaining.any(|line| *line == *expected),
                "the report no longer carries `{expected}` in its original position, so a \
                 consumer grepping this output would break.\n\nFull output:\n{output}"
            );
        }
    }

    #[test]
    fn overview_carries_the_recovered_facts() {
        let mut r = empty_report();
        r.source_format = "glb".to_owned();
        r.file_size_bytes = Some(2_500_000);
        r.asset_category = Some(AssetCategory::Hero);
        r.triangle_budget = Some(100_000);
        let output = format!("{}", r);
        assert!(output.contains("Source Format:    glb"));
        assert!(output.contains("File Size:        2,500,000 bytes"));
        assert!(output.contains("Asset Category:   Hero"));
        assert!(output.contains("Triangle Budget:  100,000"));
    }

    /// An omitted line is invisible to a script testing for presence, so a
    /// fact that is absent says so rather than disappearing.
    #[test]
    fn an_absent_fact_still_emits_its_line() {
        let mut r = empty_report();
        r.source_format = String::new();
        let output = format!("{}", r);
        assert!(output.contains("Source Format:    unknown"));
        assert!(output.contains("File Size:        unknown"));
        assert!(output.contains("Asset Category:   not classified"));
        assert!(output.contains("Triangle Budget:  not configured"));
    }

    #[test]
    fn mesh_name_and_degenerate_faces_reach_the_report() {
        let mut r = empty_report();
        let mut mesh = mesh_summary(100, 100, 100);
        mesh.name = "door_frame".to_owned();
        mesh.degenerate_faces = vec![3, 7, 11];
        r.meshes.push(mesh);
        let output = format!("{}", r);
        assert!(output.contains("  Name:            door_frame"));
        assert!(output.contains("  Degenerate Faces: 3 [3, 7, 11]"));
    }

    #[test]
    fn an_unnamed_and_undegenerate_mesh_says_so() {
        let mut r = empty_report();
        r.meshes.push(mesh_summary(100, 100, 100));
        let output = format!("{}", r);
        assert!(output.contains("  Name:            (unnamed)"));
        assert!(output.contains("  Degenerate Faces: 0"));
    }

    /// A broken asset can carry millions of these. The report prints a count
    /// and a sample, never the list.
    #[test]
    fn a_long_degenerate_list_is_sampled_not_dumped() {
        let mut r = empty_report();
        let mut mesh = mesh_summary(100, 100, 100);
        mesh.degenerate_faces = (0..5000).collect();
        r.meshes.push(mesh);
        let output = format!("{}", r);
        assert!(output.contains("  Degenerate Faces: 5,000 [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, ...]"));
        assert!(!output.contains("4999"), "the whole list was printed");
    }

    #[test]
    fn the_validation_block_groups_by_kind_most_frequent_first() {
        let mut r = empty_report();
        for kind in [
            IssueKind::MissingUvs,
            IssueKind::MissingUvs,
            IssueKind::EmptyIndices,
        ] {
            r.validation.issues.push(ValidationIssue {
                severity: Severity::Warning,
                scope: IssueScope::Model,
                kind,
                message: "test".to_owned(),
            });
        }
        let output = format!("{}", r);
        assert!(output.contains("  By kind:"));
        let uvs = output.find("Missing UVs:").expect("kind row");
        let empty = output.find("Empty indices:").expect("kind row");
        assert!(uvs < empty, "the more frequent kind must come first");
    }
}
