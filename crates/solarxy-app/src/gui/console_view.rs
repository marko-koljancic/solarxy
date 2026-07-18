use super::theme::Theme;
use crate::console::ConsoleState;

/// Console content for hosting inside an `egui_dock` tab. The dock owns
/// docked/floating placement; this function paints the level filter,
/// search bar, and the scrolling log entries.
pub(super) fn draw_console_content(ui: &mut egui::Ui, console: &mut ConsoleState, theme: &Theme) {
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
                    // Every colour here comes from the theme.
                    //
                    // These used to be hardcoded: the severities were literal
                    // RGB, and INFO/DEBUG were `from_white_alpha`, i.e. WHITE
                    // at partial opacity. White ink only resolves on a dark
                    // ground, so the moment the light theme became warm cream
                    // paper the entire log went invisible — a faint smudge on
                    // an off-white panel.
                    let (level_color, msg_color) = match entry.level {
                        tracing::Level::ERROR => (theme.severity_error, theme.severity_error),
                        tracing::Level::WARN => (theme.severity_warn, theme.fg),
                        tracing::Level::INFO => (theme.muted, theme.fg),
                        // DEBUG / TRACE: present but recessive.
                        _ => (theme.muted, theme.muted),
                    };
                    let row = ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(&entry.timestamp)
                                .monospace()
                                .small()
                                .color(theme.muted),
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
