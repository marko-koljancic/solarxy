use crate::cgi::resources::ModelStats;
use crate::format_number;

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

pub(super) fn draw_stats_window(ctx: &egui::Context, info: &ModelInfo, open: &mut bool) {
    egui::Window::new("Model Stats")
        .open(open)
        .resizable(true)
        .collapsible(true)
        .default_pos([240.0, 60.0])
        .default_width(260.0)
        .show(ctx, |ui| {
            egui::Grid::new("stats_file")
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

            egui::Grid::new("stats_geo")
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
            egui::Grid::new("stats_bounds")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("W \u{00d7} H \u{00d7} D");
                    ui.label(format!("{w:.3} \u{00d7} {h:.3} \u{00d7} {d:.3}"));
                    ui.end_row();
                });

            ui.separator();
            ui.strong("UV Data");

            egui::Grid::new("stats_uv")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("UV Mapping");
                    ui.label(if info.has_uvs { "Yes" } else { "No" });
                    ui.end_row();

                    ui.label("Coverage");
                    ui.label("N/A");
                    ui.end_row();
                });

            ui.separator();
            ui.strong("Validation");

            egui::Grid::new("stats_val")
                .num_columns(2)
                .spacing([8.0, 2.0])
                .show(ui, |ui| {
                    ui.label("Status");
                    ui.label("N/A");
                    ui.end_row();
                });
        });
}
