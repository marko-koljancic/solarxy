use crate::console::ConsoleState;

pub(super) fn draw_console_docked(
    ctx: &egui::Context,
    console: &mut ConsoleState,
    visible: &mut bool,
) {
    egui::TopBottomPanel::bottom("console_panel")
        .resizable(true)
        .default_height(150.0)
        .min_height(80.0)
        .max_height(400.0)
        .show_animated(ctx, *visible, |ui| {
            draw_console_content(ui, console);
        });
}

pub(super) fn draw_console_floating(
    ctx: &egui::Context,
    console: &mut ConsoleState,
    visible: &mut bool,
) {
    let mut open = *visible;
    egui::Window::new("Console")
        .open(&mut open)
        .resizable(true)
        .collapsible(true)
        .default_size([520.0, 220.0])
        .default_pos([240.0, 400.0])
        .show(ctx, |ui| {
            draw_console_content(ui, console);
        });
    *visible = open;
}

fn draw_console_content(ui: &mut egui::Ui, console: &mut ConsoleState) {
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("console_filter")
            .selected_text(console.min_level.as_str())
            .width(72.0)
            .show_ui(ui, |ui| {
                for level in [
                    tracing::Level::ERROR,
                    tracing::Level::WARN,
                    tracing::Level::INFO,
                    tracing::Level::DEBUG,
                ] {
                    ui.selectable_value(&mut console.min_level, level, level.as_str());
                }
            });
        if ui.small_button("Clear").clicked() {
            console.clear();
        }
        ui.checkbox(&mut console.auto_scroll, "Auto-scroll");

        ui.separator();
        ui.label(egui::RichText::new("\u{1F50D}").small());
        let search_resp = ui.add(
            egui::TextEdit::singleline(&mut console.search)
                .hint_text("filter messages")
                .desired_width(160.0),
        );
        if !console.search.is_empty() && ui.small_button("\u{00D7}").clicked() {
            console.search.clear();
        }
        if search_resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            console.search.clear();
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if console.docked {
                "\u{2197} Detach"
            } else {
                "\u{2199} Dock"
            };
            if ui.small_button(label).clicked() {
                console.docked = !console.docked;
            }
        });
    });
    ui.separator();

    let min_level = console.min_level;
    let auto_scroll = console.auto_scroll;
    let search_lower = console.search.to_lowercase();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(auto_scroll)
        .show(ui, |ui| {
            if let Ok(buf) = console.buffer.lock() {
                for entry in buf.iter() {
                    if entry.level > min_level {
                        continue;
                    }
                    if !search_lower.is_empty()
                        && !entry.message.to_lowercase().contains(&search_lower)
                    {
                        continue;
                    }
                    let (level_color, msg_color) = match entry.level {
                        tracing::Level::ERROR => (
                            egui::Color32::from_rgb(255, 100, 100),
                            egui::Color32::from_rgb(255, 150, 150),
                        ),
                        tracing::Level::WARN => (
                            egui::Color32::from_rgb(255, 200, 80),
                            egui::Color32::from_rgb(235, 210, 160),
                        ),
                        tracing::Level::INFO => (
                            egui::Color32::from_white_alpha(180),
                            egui::Color32::from_white_alpha(200),
                        ),
                        _ => (
                            egui::Color32::from_white_alpha(110),
                            egui::Color32::from_white_alpha(130),
                        ),
                    };
                    let row = ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&entry.timestamp)
                                .monospace()
                                .small()
                                .color(egui::Color32::from_white_alpha(110)),
                        );
                        ui.label(
                            egui::RichText::new(format!("{:>5}", entry.level.as_str()))
                                .monospace()
                                .small()
                                .color(level_color),
                        );
                        ui.label(egui::RichText::new(&entry.message).small().color(msg_color));
                    });
                    row.response
                        .interact(egui::Sense::click())
                        .context_menu(|ui| {
                            if ui.button("Copy message").clicked() {
                                ui.ctx().copy_text(entry.message.clone());
                                ui.close();
                            }
                            if ui.button("Copy full line").clicked() {
                                let full = format!(
                                    "{} {:>5}  {}",
                                    entry.timestamp,
                                    entry.level.as_str(),
                                    entry.message
                                );
                                ui.ctx().copy_text(full);
                                ui.close();
                            }
                        });
                }
            }
        });
}
