//! The Properties panel: a docked tab with four sections. **Model** (or
//! **Scene**: geometry and file stats), **HDRI** (environment file, IBL mode
//! and rotation), **Validation** (the issue list; click a row to fly the
//! active camera to the defect), and **Actions** (the action parameters the
//! node selected in the Node Tree declares, one button each).
//!
//! Read-only except the HDRI controls and the action buttons, which raise
//! intents like everything else, drained by `state/intents.rs` after the
//! egui pass. The Actions section is the seed of the parameter panel that
//! replaces this panel later in the release: it interprets the registry's
//! declaration and branches on no node type, so a node that declares an
//! action in Rust gets its button with no change here.

use solarxy_core::format_number;
use solarxy_core::preferences::IblMode;
use solarxy_core::validation::ValidationReport;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::params::ParamSource;
use solarxy_graph::registry::param_spec::{ParamSpec, ParamType};
use solarxy_graph::registry::visibility::param_visible;
use solarxy_renderer::resources::ModelStats;

use crate::state::engine_scene::SceneGeometryCounts;
use crate::state::hdri_info::HdriInfo;

use crate::gui::intent::{DisplayChange, Intent, Intents, PanelIntent};
use crate::gui::settings::PanelSettings;

/// The Actions section's input: the selected node and what it declares.
///
/// Borrowed from the engine for the frame, like the Node Tree's source. The
/// whole parameter list rides rather than a filtered copy, so the view stays
/// `Copy` and the filter is one pure function the test can call.
#[derive(Clone, Copy, Default)]
pub(crate) struct NodeActionsView<'a> {
    /// The node the section is about, or nothing selected.
    pub node: Option<(GraphContext, NodeId)>,
    /// The node's display name; empty with nothing selected.
    pub name: &'a str,
    /// The node type's display name; empty with nothing selected.
    pub type_name: &'a str,
    /// Every parameter the node's type declares, in declaration order.
    pub params: &'a [ParamSpec],
    /// The node's stored parameter values, which the visibility rules read.
    /// `None` with nothing selected, and treated as empty, which resolves
    /// every clause against the declared defaults.
    pub stored: Option<&'a std::collections::BTreeMap<String, ParamSource>>,
}

/// The parameters that are buttons, and are currently visible.
///
/// Both halves are the registry's answer rather than this panel's: the type
/// says which parameters are actions, and the shared evaluator says which
/// clauses hold. Until the evaluator existed this filter could only do the
/// first half, and a test asserted that no action declared a condition so
/// that the gap could not bite.
pub(super) fn action_specs<'a>(
    params: &'a [ParamSpec],
    stored: Option<&std::collections::BTreeMap<String, ParamSource>>,
) -> Vec<&'a ParamSpec> {
    let empty = std::collections::BTreeMap::new();
    let values = stored.unwrap_or(&empty);
    params
        .iter()
        .filter(|spec| matches!(spec.ty, ParamType::Action))
        .filter(|spec| param_visible(spec, params, values))
        .collect()
}

/// The Validation section's input: the report to list, plus the owning
/// object's name per issue.
///
/// A file model has one owner, so `owners` is empty there. A scene's issue
/// scopes collide across objects (every object's first mesh renders as
/// `Mesh [0]`), so the merged list needs the owner to be readable at all.
#[derive(Clone, Copy, Default)]
pub(crate) struct ValidationView<'a> {
    pub report: Option<&'a ValidationReport>,
    /// Parallel to `report.issues` when non-empty.
    pub owners: &'a [String],
}

/// File + geometry stats for the open document. Owned by `EguiRenderer`,
/// populated via `EguiRenderer::update_model_info` for a model file and
/// `EguiRenderer::update_scene_info` for a scene.
pub(in crate::gui) struct ModelInfo {
    pub filename: String,
    pub file_path: String,
    pub file_size: u64,
    pub format: String,
    pub mesh_count: usize,
    pub material_count: usize,
    /// For a scene these are the **drawn** totals, with instanced geometry
    /// counted once per placement. `polys` is zero there and its row is
    /// skipped: cooked geometry is triangles, so a polygon count would only
    /// repeat the triangle count under a second name.
    pub stats: ModelStats,
    pub bounds_size: [f32; 3],
    pub has_uvs: bool,
    /// Present when the open document is a scene rather than a single model
    /// file. Carries the counters a multi-object scene has and a file model
    /// does not, and its presence is what switches this section's wording.
    pub scene: Option<SceneGeometryCounts>,
}

/// A drawn count, with the source count beside it when instancing makes
/// the two differ.
///
/// A scatter draws its prototype once per placement, so the drawn number
/// alone describes a ten-thousand-copy scatter as millions of triangles
/// with no hint that it is one small mesh, and the source number alone
/// describes it as that one small mesh with no hint of the scatter.
fn count_with_source(drawn: usize, source: Option<usize>) -> String {
    match source {
        Some(source) if source != drawn => {
            format!(
                "{} ({} unique)",
                format_number(drawn),
                format_number(source)
            )
        }
        _ => format_number(drawn),
    }
}

/// A numeric value cell, monospaced: the grid's numbers are read down a
/// column, and digit alignment is what makes two counts comparable at a
/// glance. Labels and words stay in the interface face.
fn value_label(ui: &mut egui::Ui, text: impl Into<String>) {
    ui.label(egui::RichText::new(text.into()).monospace());
}

fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * 1024;
    const GB: u64 = 1024 * 1024 * 1024;
    match bytes {
        b if b >= GB => format!("{:.1} GB", b as f64 / GB as f64),
        b if b >= MB => format!("{:.1} MB", b as f64 / MB as f64),
        b if b >= KB => format!("{:.1} KB", b as f64 / KB as f64),
        b => format!("{b} B"),
    }
}

/// Render the Properties panel content into `ui` (the `egui_dock`
/// `Properties` tab supplies the `Ui`).
pub(in crate::gui) fn draw_properties_content(
    ui: &mut egui::Ui,
    model_info: Option<&ModelInfo>,
    hdri_info: Option<&HdriInfo>,
    validation: ValidationView<'_>,
    actions: NodeActionsView<'_>,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);

        // The heading names what is actually open, because "Model" over a
        // scene's object counts reads as a mislabel rather than a synonym.
        let heading = if model_info.is_some_and(|i| i.scene.is_some()) {
            "Scene"
        } else {
            "Model"
        };
        egui::CollapsingHeader::new(heading)
            .default_open(true)
            .show(ui, |ui| match model_info {
                Some(info) => draw_model_section(ui, info),
                None => {
                    ui.label(egui::RichText::new("Nothing open").weak());
                }
            });

        ui.separator();

        egui::CollapsingHeader::new("HDRI")
            .default_open(true)
            .show(ui, |ui| draw_hdri_section(ui, hdri_info, settings, intents));

        ui.separator();

        egui::CollapsingHeader::new("Validation")
            .default_open(true)
            .show(ui, |ui| draw_validation_section(ui, validation, intents));

        ui.separator();

        egui::CollapsingHeader::new("Actions")
            .default_open(true)
            .show(ui, |ui| draw_actions_section(ui, actions, intents));

        ui.add_space(8.0);
    });
}

/// One button per action the selected node declares. The press is routed by
/// the drain, not here: the panel knows the declaration and nothing about
/// what any node does with it.
fn draw_actions_section(ui: &mut egui::Ui, view: NodeActionsView<'_>, intents: &mut Intents) {
    let Some((ctx, node)) = view.node else {
        ui.label(egui::RichText::new("Select a node in the Node Tree").weak());
        return;
    };
    ui.label(
        egui::RichText::new(format!("{} \u{00b7} {}", view.name, view.type_name))
            .small()
            .weak(),
    );
    let specs = action_specs(view.params, view.stored);
    if specs.is_empty() {
        ui.label(egui::RichText::new("This node declares no actions").weak());
        return;
    }
    ui.add_space(2.0);
    for spec in specs {
        if ui
            .button(&spec.label)
            .on_hover_text(format!(
                "{}\n\nActs on the node's last cook; in manual cook mode, cook first.",
                spec.doc
            ))
            .clicked()
        {
            intents.panel(PanelIntent::InvokeAction {
                ctx,
                node,
                key: spec.key.clone(),
            });
        }
    }
}

fn draw_model_section(ui: &mut egui::Ui, info: &ModelInfo) {
    egui::Grid::new("props_model_file")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            ui.label("File");
            ui.label(&info.filename);
            ui.end_row();

            ui.label("Path");
            ui.label(&info.file_path);
            ui.end_row();

            ui.label("Size");
            value_label(ui, format_file_size(info.file_size));
            ui.end_row();

            ui.label("Format");
            ui.label(&info.format);
            ui.end_row();
        });

    ui.separator();
    ui.strong("Geometry");

    egui::Grid::new("props_model_geo")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            // Cooked geometry is triangles, so a scene's polygon count would
            // only restate its triangle count. The row is dropped there
            // rather than shown as a duplicate or a zero.
            if info.scene.is_none() {
                ui.label("Polygons");
                value_label(ui, format_number(info.stats.polys));
                ui.end_row();
            }

            ui.label("Triangles");
            value_label(
                ui,
                count_with_source(info.stats.tris, info.scene.map(|s| s.unique_tris)),
            );
            ui.end_row();

            ui.label("Vertices");
            value_label(
                ui,
                count_with_source(info.stats.verts, info.scene.map(|s| s.unique_verts)),
            );
            ui.end_row();

            if let Some(scene) = info.scene {
                ui.label("Objects");
                value_label(ui, scene.objects.to_string());
                ui.end_row();

                // Only worth a row when something is actually placed more
                // than once; otherwise it just repeats the mesh count.
                if scene.is_instanced() {
                    ui.label("Instances");
                    value_label(ui, format_number(scene.instances));
                    ui.end_row();
                }
            }

            ui.label("Meshes");
            value_label(ui, info.mesh_count.to_string());
            ui.end_row();

            ui.label("Materials");
            value_label(ui, info.material_count.to_string());
            ui.end_row();
        });

    ui.separator();
    ui.strong("Bounds");

    let [w, h, d] = info.bounds_size;
    egui::Grid::new("props_model_bounds")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            ui.label("W \u{00d7} H \u{00d7} D");
            value_label(ui, format!("{w:.3} \u{00d7} {h:.3} \u{00d7} {d:.3}"));
            ui.end_row();

            ui.label("UV Mapping");
            ui.label(if info.has_uvs { "Yes" } else { "No" });
            ui.end_row();
        });
}

fn draw_hdri_section(
    ui: &mut egui::Ui,
    hdri_info: Option<&HdriInfo>,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
) {
    let Some(info) = hdri_info else {
        ui.label(egui::RichText::new("No HDRI loaded").weak());
        ui.add_space(4.0);
        if ui
            .button("Load HDRI\u{2026}")
            .on_hover_text("Open an .hdr / .exr environment map")
            .clicked()
        {
            intents.panel(PanelIntent::LoadHdri);
        }
        return;
    };

    egui::Grid::new("props_hdri")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            ui.label("File");
            ui.label(&info.filename);
            ui.end_row();

            ui.label("Path");
            ui.label(&info.path);
            ui.end_row();

            ui.label("Resolution");
            value_label(
                ui,
                format!("{} \u{00d7} {}", info.resolution.0, info.resolution.1),
            );
            ui.end_row();

            ui.label("Size");
            ui.label(format_file_size(info.file_size));
            ui.end_row();
        });

    ui.separator();

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("props_ibl_mode")
            .selected_text(settings.ibl_mode.to_string())
            .width(140.0)
            .show_ui(ui, |ui| {
                let mut mode = settings.ibl_mode;
                for &variant in IblMode::ALL {
                    if ui
                        .selectable_value(&mut mode, variant, variant.to_string())
                        .changed()
                    {
                        intents.raise(Intent::Ibl(variant));
                    }
                }
            });
        ui.label("IBL Mode").on_hover_text("I / Shift+I");
    });

    let mut degrees = settings.display.hdri_rotation.to_degrees();
    if ui
        .add(
            egui::Slider::new(&mut degrees, 0.0..=360.0)
                .suffix("\u{00b0}")
                .text("Rotation"),
        )
        .on_hover_text("Yaw the HDRI sky and the IBL it derives")
        .changed()
    {
        intents.raise(Intent::Display(DisplayChange::HdriRotation(
            degrees.to_radians(),
        )));
    }

    let mut intensity = settings.display.hdri_intensity;
    if ui
        .add(
            egui::Slider::new(
                &mut intensity,
                solarxy_core::view_config::MIN_HDRI_INTENSITY
                    ..=solarxy_core::view_config::MAX_HDRI_INTENSITY,
            )
            .text("Intensity"),
        )
        .on_hover_text("Scale the light the HDRI casts, without dimming the visible sky")
        .changed()
    {
        intents.raise(Intent::Display(DisplayChange::HdriIntensity(intensity)));
    }

    ui.add_space(4.0);
    if ui
        .button("Clear HDRI")
        .on_hover_text("Drop the HDRI — IBL falls back to the background gradient")
        .clicked()
    {
        intents.panel(PanelIntent::ClearHdri);
    }
}

fn draw_validation_section(
    ui: &mut egui::Ui,
    validation: ValidationView<'_>,
    intents: &mut Intents,
) {
    let Some(report) = validation.report else {
        ui.label(egui::RichText::new("Nothing open").weak());
        return;
    };

    if report.is_clean() {
        ui.label("No issues found");
        return;
    }

    ui.label(format!(
        "{} error(s), {} warning(s)",
        report.error_count(),
        report.warning_count()
    ));
    ui.add_space(2.0);

    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_color = ui.visuals().text_color();

    for (idx, issue) in report.issues.iter().enumerate() {
        let c = solarxy_renderer::validation::issue_category(issue).color();
        let dot_color = egui::Color32::from_rgb(
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
        );

        let mut job = egui::text::LayoutJob::default();
        job.append(
            "\u{25cf}  ",
            0.0,
            egui::TextFormat {
                color: dot_color,
                font_id: font.clone(),
                ..Default::default()
            },
        );
        // The owner leads, because the scope does not distinguish objects:
        // every object's first mesh renders as `Mesh [0]`, so a merged list
        // without the owner shows identical rows for different geometry.
        if let Some(owner) = validation.owners.get(idx) {
            job.append(
                &format!("{owner}  "),
                0.0,
                egui::TextFormat {
                    color: text_color.gamma_multiply(0.7),
                    font_id: font.clone(),
                    ..Default::default()
                },
            );
        }
        job.append(
            &format!("{} \u{2014} {}", issue.scope, issue.message),
            0.0,
            egui::TextFormat {
                color: text_color,
                font_id: font.clone(),
                ..Default::default()
            },
        );
        job.wrap.max_width = ui.available_width();

        let resp = ui
            .selectable_label(false, job)
            .on_hover_text("Click to frame this issue in the active viewport");
        if resp.clicked() {
            intents.panel(PanelIntent::FlyToIssue(idx));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::action_specs;
    use solarxy_graph::Engine;
    use solarxy_graph::params::{ParamSource, ParamValue};
    use solarxy_graph::registry::param_spec::{ParamSpec, ParamType, Pred};

    /// The buttons are read off the declaration, so the two export nodes and
    /// the render node get theirs and a geometry node gets none.
    #[test]
    fn only_action_params_become_buttons() {
        let engine = Engine::new().expect("engine");
        let keys = |ty: &str| -> Vec<String> {
            let descriptor = engine.registry().get(ty).unwrap_or_else(|| panic!("{ty}"));
            action_specs(&descriptor.params, None)
                .iter()
                .map(|s| s.key.clone())
                .collect()
        };
        assert_eq!(keys("geo_export"), ["save"]);
        assert_eq!(keys("image_export"), ["save"]);
        assert_eq!(keys("render"), ["render"]);
        assert!(keys("box").is_empty());
    }

    /// The half this section could not do until the shared evaluator
    /// existed. No registered action declares a condition today, so the
    /// case is built rather than found; the point is that the filter now
    /// asks instead of assuming.
    #[test]
    fn an_action_hidden_by_its_own_condition_is_not_drawn() {
        let params = vec![
            ParamSpec::new(
                "mode",
                "Mode",
                "general",
                ParamType::Enum { variants: vec![] },
                ParamValue::Enum("file".into()),
            ),
            ParamSpec::new(
                "save",
                "Save",
                "general",
                ParamType::Action,
                ParamValue::Bool(false),
            )
            .show_if("mode", Pred::Eq(ParamValue::Enum("file".into()))),
        ];
        let stored = |mode: &str| -> std::collections::BTreeMap<String, ParamSource> {
            std::iter::once((
                "mode".to_string(),
                ParamSource::Literal(ParamValue::Enum(mode.into())),
            ))
            .collect()
        };

        let shown = stored("file");
        assert_eq!(
            action_specs(&params, Some(&shown))
                .iter()
                .map(|s| s.key.as_str())
                .collect::<Vec<_>>(),
            ["save"]
        );

        let hidden = stored("stream");
        assert!(action_specs(&params, Some(&hidden)).is_empty());

        // No stored values resolves against the declared default, which is
        // the mode that shows the button.
        assert_eq!(action_specs(&params, None).len(), 1);
    }
}
