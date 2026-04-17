use crate::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ToneMode, UvMapBackground, UvMode, ViewMode,
};
use crate::state::view_state::BoundsMode;

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

pub(super) fn draw_sidebar(
    ctx: &egui::Context,
    s: &mut GuiSnapshot,
    visible: bool,
    uv_overlap_pct: Option<f32>,
    validation_report: Option<&crate::validation::ValidationReport>,
) {
    egui::SidePanel::left("sidebar")
        .resizable(false)
        .default_width(220.0)
        .show_animated(ctx, visible, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(2.0);

                egui::CollapsingHeader::new("View")
                    .default_open(true)
                    .show(ui, |ui| {
                        if s.pane_mode == PaneMode::UvMap {
                            ui.label("UV Map");
                            combo_with_tooltip(
                                ui,
                                "Background",
                                "U",
                                &mut s.uv_bg,
                                UvMapBackground::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Weight",
                                "Shift+W",
                                &mut s.line_weight,
                                LineWeight::ALL,
                            );
                            checkbox_with_tooltip(ui, &mut s.show_uv_overlap, "Overlap", "O");
                            if s.show_uv_overlap
                                && let Some(pct) = uv_overlap_pct
                            {
                                ui.indent("overlap_stats", |ui| {
                                    ui.label(format!("Overlap: {:.1}%", pct));
                                });
                            }
                            if ui.small_button("Back to 3D (3)").clicked() {
                                s.pane_mode = PaneMode::Scene3D;
                            }
                        } else {
                            combo_with_tooltip(ui, "Mode", "W", &mut s.view_mode, ViewMode::ALL);
                            combo_with_tooltip(
                                ui,
                                "Inspection",
                                "1\u{2013}5",
                                &mut s.inspection_mode,
                                InspectionMode::ALL,
                            );
                            if s.inspection_mode == InspectionMode::TexelDensity {
                                ui.indent("texel_density_indent", |ui| {
                                    ui.add(
                                        egui::Slider::new(&mut s.texel_density_target, 0.01..=10.0)
                                            .logarithmic(true)
                                            .text("Target"),
                                    );
                                });
                            }
                            combo_with_tooltip(
                                ui,
                                "Material",
                                "M",
                                &mut s.material_override,
                                MaterialOverride::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Normals",
                                "N",
                                &mut s.normals_mode,
                                NormalsMode::ALL,
                            );
                            combo_with_tooltip(ui, "UV", "U", &mut s.uv_mode, UvMode::ALL);
                            combo_with_tooltip(
                                ui,
                                "Weight",
                                "Shift+W",
                                &mut s.line_weight,
                                LineWeight::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Background",
                                "B",
                                &mut s.background_mode,
                                BackgroundMode::ALL,
                            );
                            combo_with_tooltip(
                                ui,
                                "Bounds",
                                "Shift+B",
                                &mut s.bounds_mode,
                                BoundsMode::ALL,
                            );
                        }
                        if s.is_split {
                            checkbox_with_tooltip(
                                ui,
                                &mut s.cameras_linked,
                                "Link cameras",
                                "Ctrl+L",
                            );
                        }
                    });

                ui.separator();

                egui::CollapsingHeader::new("Display")
                    .default_open(true)
                    .show(ui, |ui| {
                        checkbox_with_tooltip(ui, &mut s.show_grid, "Grid", "G");
                        checkbox_with_tooltip(ui, &mut s.show_axis_gizmo, "Axis Gizmo", "A");
                        checkbox_with_tooltip(ui, &mut s.show_local_axes, "Local Axes", "Shift+A");
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

                if let Some(report) = validation_report {
                    egui::CollapsingHeader::new("Validation")
                        .default_open(false)
                        .show(ui, |ui| {
                            checkbox_with_tooltip(
                                ui,
                                &mut s.show_validation,
                                "Highlight issues on mesh",
                                "Shift+V",
                            );
                            if report.is_clean() {
                                ui.label("No issues found");
                            } else {
                                ui.label(format!(
                                    "{} error(s), {} warning(s)",
                                    report.error_count(),
                                    report.warning_count()
                                ));
                                for issue in &report.issues {
                                    let color = crate::validation::issue_category(issue).color();
                                    let egui_color = egui::Color32::from_rgba_unmultiplied(
                                        (color[0] * 255.0) as u8,
                                        (color[1] * 255.0) as u8,
                                        (color[2] * 255.0) as u8,
                                        255,
                                    );
                                    ui.horizontal(|ui| {
                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(8.0, 8.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(rect.center(), 4.0, egui_color);
                                        ui.label(format!("{} {}", issue.scope, issue.message));
                                    });
                                }
                            }
                        });

                    ui.separator();
                }

                egui::CollapsingHeader::new("Post-Processing")
                    .default_open(true)
                    .show(ui, |ui| {
                        checkbox_with_tooltip(ui, &mut s.bloom_enabled, "Bloom", "Shift+D");
                        checkbox_with_tooltip(ui, &mut s.ssao_enabled, "SSAO", "Shift+O");
                        combo_with_tooltip(
                            ui,
                            "Tone Map",
                            "Shift+T",
                            &mut s.tone_mode,
                            ToneMode::ALL,
                        );
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
                                egui::Slider::new(&mut s.metallic_scale, 0.0..=1.0)
                                    .text("Metallic Scale"),
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

                ui.separator();

                egui::CollapsingHeader::new("Lighting")
                    .default_open(true)
                    .show(ui, |ui| {
                        combo_with_tooltip(ui, "IBL", "I / Shift+I", &mut s.ibl_mode, IblMode::ALL);
                    });

                ui.add_space(8.0);
            });
        });
}
