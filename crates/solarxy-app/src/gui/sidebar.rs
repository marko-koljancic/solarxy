use solarxy_core::preferences::{MaterialOverride, ToneMode};

use super::snapshot::GuiSnapshot;

fn combo_with_tooltip<T>(ui: &mut egui::Ui, label: &str, shortcut: &str, current: &mut T, all: &[T])
where
    T: Copy + PartialEq + std::fmt::Display,
{
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt(label)
            .selected_text(current.to_string())
            .width(140.0)
            .show_ui(ui, |ui| {
                for &variant in all {
                    ui.selectable_value(current, variant, variant.to_string());
                }
            });
        ui.label(label).on_hover_text(shortcut);
    });
}

fn checkbox_with_tooltip(ui: &mut egui::Ui, value: &mut bool, label: &str, shortcut: &str) {
    ui.horizontal(|ui| {
        ui.checkbox(value, label);
        ui.small(shortcut)
            .on_hover_text(format!("Shortcut: {shortcut}"));
    });
}

/// Render the sidebar's collapsible-panels content directly into the
/// provided `ui` — the SidePanel/ScrollArea shell is the caller's job.
/// Lives this way so `gui::dock` can host the sidebar as an `egui_dock`
/// tab (which provides its own `Ui`).
///
/// RC2: the sidebar is the canonical surface for **scene-global**
/// display / post-processing / material settings only. Per-pane view
/// state lives on the per-pane toolbar; validation and HDRI/IBL moved to
/// the Properties panel.
pub(super) fn draw_sidebar_content(ui: &mut egui::Ui, s: &mut GuiSnapshot) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);

        egui::CollapsingHeader::new("Display")
            .default_open(true)
            .show(ui, |ui| {
                checkbox_with_tooltip(ui, &mut s.lights_locked, "Lock Lights", "Shift+L");
                checkbox_with_tooltip(ui, &mut s.turntable_active, "Turntable", "V");
                if s.turntable_active {
                    ui.indent("turntable_indent", |ui| {
                        ui.add(
                            egui::Slider::new(&mut s.turntable_rpm, 1.0..=60.0)
                                .text("RPM")
                                .logarithmic(true),
                        );
                    });
                }
            });

        ui.separator();

        egui::CollapsingHeader::new("Post-Processing")
            .default_open(true)
            .show(ui, |ui| {
                checkbox_with_tooltip(ui, &mut s.bloom_enabled, "Bloom", "Shift+D");
                checkbox_with_tooltip(ui, &mut s.ssao_enabled, "SSAO", "Shift+O");
                combo_with_tooltip(ui, "Tone Map", "Shift+T", &mut s.tone_mode, ToneMode::ALL);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Slider::new(&mut s.exposure, 0.1..=10.0)
                            .text("Exposure")
                            .logarithmic(true),
                    );
                })
                .response
                .on_hover_text("E / Shift+E");
            });

        ui.separator();

        egui::CollapsingHeader::new("Material")
            .default_open(false)
            .show(ui, |ui| {
                ui.add_enabled_ui(s.material_override == MaterialOverride::None, |ui| {
                    ui.add(
                        egui::Slider::new(&mut s.roughness_scale, 0.0..=1.0)
                            .text("Roughness Scale"),
                    );
                    ui.add(
                        egui::Slider::new(&mut s.metallic_scale, 0.0..=1.0).text("Metallic Scale"),
                    );
                    if ui.small_button("Reset").clicked() {
                        s.roughness_scale = 1.0;
                        s.metallic_scale = 1.0;
                    }
                });
                if s.material_override != MaterialOverride::None {
                    ui.label("(disabled in override modes)");
                }
            });

        ui.add_space(8.0);
    });
}
