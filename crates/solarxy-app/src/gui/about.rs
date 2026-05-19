use solarxy_core::WIKI_URL;

const TAGLINE: &str = "3D model viewer, visual debugger, and validator.";

const BLURB: &str = "Solarxy loads OBJ, STL, PLY, and glTF/GLB models and renders \
them in real time with physically based shading, IBL, SSAO, and shadows. Switch \
between inspection modes — Material ID, UV Map, Texel Density, Depth — to see what \
the geometry, materials, and UVs really look like, and split the viewport to \
compare side-by-side. Run validation checks for non-manifold edges, inverted UVs, \
overlapping shells, and more.";

const REVIEW_BLURB: &str = "Annotate models directly with the Review System: drop \
notes on geometry, reply, resolve, and re-anchor as your meshes evolve.";

const TECH_LINE: &str = "Built in Rust with wgpu — macOS, Linux, Windows.";

pub(super) fn draw_about_modal(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
        return;
    }
    let default_pos = ctx.content_rect().center() - egui::vec2(220.0, 160.0);
    egui::Window::new("About Solarxy")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .default_pos(default_pos)
        .default_width(440.0)
        .movable(true)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Solarxy");
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
            });
            ui.add_space(10.0);

            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new(TAGLINE).italics());
            });
            ui.add_space(10.0);

            ui.label(BLURB);
            ui.add_space(8.0);
            ui.label(REVIEW_BLURB);
            ui.add_space(10.0);

            ui.vertical_centered(|ui| {
                ui.label(TECH_LINE);
            });
            ui.add_space(10.0);

            ui.separator();
            ui.add_space(6.0);

            ui.vertical_centered(|ui| {
                ui.label(format!("License: {}", env!("CARGO_PKG_LICENSE")));
                ui.horizontal(|ui| {
                    ui.add_space((ui.available_width() - 140.0).max(0.0) * 0.5);
                    ui.hyperlink_to("Repository", env!("CARGO_PKG_REPOSITORY"));
                    ui.label("·");
                    ui.hyperlink_to("Wiki", WIKI_URL);
                });
            });
        });
}
