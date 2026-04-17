pub(super) fn draw_about_modal(ctx: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }

    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        *open = false;
        return;
    }
    egui::Window::new("About Solarxy")
        .open(open)
        .resizable(false)
        .collapsible(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Solarxy");
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.add_space(8.0);
                ui.label(env!("CARGO_PKG_DESCRIPTION"));
                ui.add_space(8.0);
                ui.label(format!("License: {}", env!("CARGO_PKG_LICENSE")));
                ui.hyperlink_to("Repository", env!("CARGO_PKG_REPOSITORY"));
            });
        });
}
