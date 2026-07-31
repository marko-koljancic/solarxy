//! The Properties panel — a docked tab with three sections: **Model**
//! (geometry / file stats, the former floating Stats window), **HDRI**
//! (environment file + IBL mode + rotation), and **Validation** (the
//! issue list, click-a-row to fly the active camera to the defect).
//!
//! Replaced `gui::stats` in RC2 — `ModelInfo` moved here unchanged; file
//! sizes format via `solarxy_core::format_number`. Read-only except the
//! HDRI IBL-mode / rotation controls (which write through [`GuiSnapshot`])
//! and the out-events in [`PropertiesEvents`], drained by
//! `state/render.rs` after the egui pass.

use solarxy_core::format_number;
use solarxy_core::preferences::IblMode;
use solarxy_core::validation::ValidationReport;
use solarxy_renderer::resources::ModelStats;

use crate::state::hdri_info::HdriInfo;

use super::snapshot::GuiSnapshot;

/// File + geometry stats for the loaded model. Owned by `EguiRenderer`,
/// populated via `EguiRenderer::update_model_info`.
pub(super) struct ModelInfo {
    pub filename: String,
    pub file_path: String,
    pub file_size: u64,
    pub format: String,
    pub mesh_count: usize,
    pub material_count: usize,
    pub stats: ModelStats,
    pub bounds_size: [f32; 3],
    pub has_uvs: bool,
}

/// Events raised by the Properties panel during an egui pass, drained by
/// `state/render.rs` after `render_ui` returns.
#[derive(Debug, Default)]
pub(crate) struct PropertiesEvents {
    /// Index into `ValidationReport::issues` whose row was clicked — the
    /// state layer flies the active pane's camera to frame it.
    pub fly_to_issue: Option<usize>,
    /// The HDRI `[Clear]` button was pressed.
    pub clear_hdri: bool,
    /// The `[Load HDRI…]` button (shown when none is loaded) was pressed.
    pub load_hdri: bool,
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
pub(super) fn draw_properties_content(
    ui: &mut egui::Ui,
    model_info: Option<&ModelInfo>,
    hdri_info: Option<&HdriInfo>,
    validation: Option<&ValidationReport>,
    snap: &mut GuiSnapshot,
    events: &mut PropertiesEvents,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);

        egui::CollapsingHeader::new("Model")
            .default_open(true)
            .show(ui, |ui| match model_info {
                Some(info) => draw_model_section(ui, info),
                None => {
                    ui.label(egui::RichText::new("No model loaded").weak());
                }
            });

        ui.separator();

        egui::CollapsingHeader::new("HDRI")
            .default_open(true)
            .show(ui, |ui| draw_hdri_section(ui, hdri_info, snap, events));

        ui.separator();

        egui::CollapsingHeader::new("Validation")
            .default_open(true)
            .show(ui, |ui| draw_validation_section(ui, validation, events));

        ui.add_space(8.0);
    });
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
            ui.label(format_file_size(info.file_size));
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
            ui.label("Polygons");
            ui.label(format_number(info.stats.polys));
            ui.end_row();

            ui.label("Triangles");
            ui.label(format_number(info.stats.tris));
            ui.end_row();

            ui.label("Vertices");
            ui.label(format_number(info.stats.verts));
            ui.end_row();

            ui.label("Meshes");
            ui.label(info.mesh_count.to_string());
            ui.end_row();

            ui.label("Materials");
            ui.label(info.material_count.to_string());
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
            ui.label(format!("{w:.3} \u{00d7} {h:.3} \u{00d7} {d:.3}"));
            ui.end_row();

            ui.label("UV Mapping");
            ui.label(if info.has_uvs { "Yes" } else { "No" });
            ui.end_row();
        });
}

fn draw_hdri_section(
    ui: &mut egui::Ui,
    hdri_info: Option<&HdriInfo>,
    snap: &mut GuiSnapshot,
    events: &mut PropertiesEvents,
) {
    let Some(info) = hdri_info else {
        ui.label(egui::RichText::new("No HDRI loaded").weak());
        ui.add_space(4.0);
        if ui
            .button("Load HDRI\u{2026}")
            .on_hover_text("Open an .hdr / .exr environment map")
            .clicked()
        {
            events.load_hdri = true;
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
            ui.label(format!(
                "{} \u{00d7} {}",
                info.resolution.0, info.resolution.1
            ));
            ui.end_row();

            ui.label("Size");
            ui.label(format_file_size(info.file_size));
            ui.end_row();
        });

    ui.separator();

    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("props_ibl_mode")
            .selected_text(snap.ibl_mode.to_string())
            .width(140.0)
            .show_ui(ui, |ui| {
                for &mode in IblMode::ALL {
                    ui.selectable_value(&mut snap.ibl_mode, mode, mode.to_string());
                }
            });
        ui.label("IBL Mode").on_hover_text("I / Shift+I");
    });

    let mut degrees = snap.hdri_rotation.to_degrees();
    if ui
        .add(
            egui::Slider::new(&mut degrees, 0.0..=360.0)
                .suffix("\u{00b0}")
                .text("Rotation"),
        )
        .on_hover_text("Yaw the HDRI sky and the IBL it derives")
        .changed()
    {
        snap.hdri_rotation = degrees.to_radians();
    }

    ui.add(
        egui::Slider::new(
            &mut snap.hdri_intensity,
            solarxy_core::view_config::MIN_HDRI_INTENSITY
                ..=solarxy_core::view_config::MAX_HDRI_INTENSITY,
        )
        .text("Intensity"),
    )
    .on_hover_text("Scale the light the HDRI casts, without dimming the visible sky");

    ui.add_space(4.0);
    if ui
        .button("Clear HDRI")
        .on_hover_text("Drop the HDRI — IBL falls back to the background gradient")
        .clicked()
    {
        events.clear_hdri = true;
    }
}

fn draw_validation_section(
    ui: &mut egui::Ui,
    validation: Option<&ValidationReport>,
    events: &mut PropertiesEvents,
) {
    let Some(report) = validation else {
        ui.label(egui::RichText::new("No model loaded").weak());
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
            events.fly_to_issue = Some(idx);
        }
    }
}
