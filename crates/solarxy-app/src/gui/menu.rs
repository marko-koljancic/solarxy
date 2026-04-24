use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ProjectionMode, ToneMode, UvMode, ViewMode,
};
use crate::state::view_state::{BoundsMode, ViewLayout};

use super::MOD;
use super::actions::{MenuActions, MenuBarVisibility};
use super::snapshot::GuiSnapshot;

pub(super) fn draw_menu_bar(
    ctx: &egui::Context,
    snap: &mut GuiSnapshot,
    actions: &mut MenuActions,
    vis: &mut MenuBarVisibility,
    has_model: bool,
    recent_files: &[String],
) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui
                    .add(egui::Button::new("Open Model\u{2026}").shortcut_text(format!("{MOD}+O")))
                    .clicked()
                {
                    actions.open_model = true;
                    ui.close();
                }
                if ui
                    .add(
                        egui::Button::new("Import HDRI\u{2026}")
                            .shortcut_text(format!("{MOD}+Shift+O")),
                    )
                    .clicked()
                {
                    actions.open_hdri = true;
                    ui.close();
                }
                if !recent_files.is_empty() {
                    ui.separator();
                    ui.menu_button("Recent Files", |ui| {
                        for path in recent_files.iter().take(10) {
                            let raw = std::path::Path::new(path)
                                .file_name()
                                .and_then(|f| f.to_str())
                                .unwrap_or(path);
                            let count = raw.chars().count();
                            let label: String = if count > 50 {
                                let tail: String = raw.chars().skip(count - 47).collect();
                                format!("\u{2026}{tail}")
                            } else {
                                raw.to_string()
                            };
                            if ui.button(&label).on_hover_text(path).clicked() {
                                actions.open_recent = Some(path.clone());
                                ui.close();
                            }
                        }
                    });
                }
                ui.separator();
                if ui
                    .add(egui::Button::new("Save Screenshot").shortcut_text("C"))
                    .clicked()
                {
                    actions.save_screenshot = true;
                    ui.close();
                }
                ui.separator();
                if ui
                    .add_enabled(has_model, egui::Button::new("Close Model"))
                    .clicked()
                {
                    actions.close_model = true;
                    ui.close();
                }
                if ui.button("Quit").clicked() {
                    actions.quit = true;
                    ui.close();
                }
            });

            ui.menu_button("Edit", |ui| {
                if ui
                    .add(egui::Button::new("Preferences\u{2026}").shortcut_text(format!("{MOD}+,")))
                    .clicked()
                {
                    actions.open_preferences = true;
                    ui.close();
                }
            });

            ui.menu_button("View", |ui| {
                ui.menu_button("Shading", |ui| {
                    for mode in ViewMode::ALL {
                        if ui
                            .selectable_label(snap.view_mode == *mode, mode.to_string())
                            .clicked()
                        {
                            snap.view_mode = *mode;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("W");

                ui.menu_button("Inspection", |ui| {
                    for mode in InspectionMode::ALL {
                        let selected =
                            snap.pane_mode == PaneMode::Scene3D && snap.inspection_mode == *mode;
                        let shortcut = match mode {
                            InspectionMode::Shaded => "1",
                            InspectionMode::MaterialId => "2",
                            InspectionMode::TexelDensity => "4",
                            InspectionMode::Depth => "5",
                        };
                        if ui
                            .selectable_label(selected, mode.to_string())
                            .on_hover_text(shortcut)
                            .clicked()
                        {
                            snap.inspection_mode = *mode;
                            snap.pane_mode = PaneMode::Scene3D;
                            ui.close();
                        }
                    }
                    let uv_selected = snap.pane_mode == PaneMode::UvMap;
                    if ui
                        .selectable_label(uv_selected, "UV Map")
                        .on_hover_text("3")
                        .clicked()
                    {
                        snap.pane_mode = PaneMode::UvMap;
                        ui.close();
                    }
                });

                ui.menu_button("Material Override", |ui| {
                    for mode in MaterialOverride::ALL {
                        if ui
                            .selectable_label(snap.material_override == *mode, mode.to_string())
                            .clicked()
                        {
                            snap.material_override = *mode;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("M / Shift+M");

                ui.separator();

                ui.menu_button("Show", |ui| {
                    ui.checkbox(&mut snap.show_grid, "Grid").on_hover_text("G");
                    ui.checkbox(&mut snap.show_axis_gizmo, "Axis Gizmo")
                        .on_hover_text("A");
                    ui.checkbox(&mut snap.show_local_axes, "Local Axes")
                        .on_hover_text("Shift+A");
                    ui.checkbox(&mut snap.show_validation, "Validation Overlay")
                        .on_hover_text("Shift+V");
                    ui.separator();
                    ui.menu_button("Normals", |ui| {
                        for mode in NormalsMode::ALL {
                            if ui
                                .selectable_label(snap.normals_mode == *mode, mode.to_string())
                                .clicked()
                            {
                                snap.normals_mode = *mode;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("N");
                    ui.menu_button("UV Overlay", |ui| {
                        for mode in UvMode::ALL {
                            if ui
                                .selectable_label(snap.uv_mode == *mode, mode.to_string())
                                .clicked()
                            {
                                snap.uv_mode = *mode;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("U");
                    ui.menu_button("Bounds", |ui| {
                        for mode in BoundsMode::ALL {
                            if ui
                                .selectable_label(snap.bounds_mode == *mode, mode.to_string())
                                .clicked()
                            {
                                snap.bounds_mode = *mode;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Shift+B");
                    ui.menu_button("Wireframe Weight", |ui| {
                        for mode in LineWeight::ALL {
                            if ui
                                .selectable_label(snap.line_weight == *mode, mode.to_string())
                                .clicked()
                            {
                                snap.line_weight = *mode;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Shift+W");
                });

                ui.menu_button("Background", |ui| {
                    for mode in BackgroundMode::ALL {
                        if ui
                            .selectable_label(snap.background_mode == *mode, mode.to_string())
                            .clicked()
                        {
                            snap.background_mode = *mode;
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("B");

                ui.separator();

                ui.menu_button("Lighting", |ui| {
                    ui.menu_button("IBL Mode", |ui| {
                        for mode in IblMode::ALL {
                            if ui
                                .selectable_label(snap.ibl_mode == *mode, mode.to_string())
                                .clicked()
                            {
                                snap.ibl_mode = *mode;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("I / Shift+I");
                    ui.checkbox(&mut snap.lights_locked, "Lock Lights")
                        .on_hover_text("Shift+L");
                });

                ui.menu_button("Post-Processing", |ui| {
                    ui.checkbox(&mut snap.bloom_enabled, "Bloom")
                        .on_hover_text("Shift+D");
                    ui.checkbox(&mut snap.ssao_enabled, "SSAO")
                        .on_hover_text("Shift+O");
                    ui.menu_button("Tone Mapping", |ui| {
                        for mode in ToneMode::ALL {
                            if ui
                                .selectable_label(snap.tone_mode == *mode, mode.to_string())
                                .clicked()
                            {
                                snap.tone_mode = *mode;
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("Shift+T");
                });

                ui.separator();

                ui.menu_button("Layout", |ui| {
                    if ui
                        .add(egui::Button::new("Single").shortcut_text("F1"))
                        .clicked()
                    {
                        actions.set_layout = Some(ViewLayout::Single);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Split Vertical").shortcut_text("F2"))
                        .clicked()
                    {
                        actions.set_layout = Some(ViewLayout::SplitVertical);
                        ui.close();
                    }
                    if ui
                        .add(egui::Button::new("Split Horizontal").shortcut_text("F3"))
                        .clicked()
                    {
                        actions.set_layout = Some(ViewLayout::SplitHorizontal);
                        ui.close();
                    }
                });

                ui.menu_button("Projection", |ui| {
                    for (mode, shortcut) in [
                        (ProjectionMode::Perspective, "P"),
                        (ProjectionMode::Orthographic, "O"),
                    ] {
                        if ui
                            .selectable_label(snap.projection_mode == mode, mode.to_string())
                            .on_hover_text(shortcut)
                            .clicked()
                        {
                            actions.set_projection = Some(mode);
                            ui.close();
                        }
                    }
                });

                if snap.is_split {
                    ui.checkbox(&mut snap.cameras_linked, "Link Cameras")
                        .on_hover_text(format!("{MOD}+L"));
                }
                ui.checkbox(&mut snap.turntable_active, "Turntable")
                    .on_hover_text("V");

                ui.separator();

                if ui.button("Keyboard Shortcuts").on_hover_text("?").clicked() {
                    actions.open_shortcuts_modal = true;
                    ui.close();
                }
            });

            ui.menu_button("Window", |ui| {
                if ui
                    .add(
                        egui::Button::new("Menu Bar")
                            .selected(vis.menu_bar_visible)
                            .shortcut_text("F10"),
                    )
                    .clicked()
                {
                    vis.menu_bar_visible = !vis.menu_bar_visible;
                    ui.close();
                }
                if ui
                    .add(
                        egui::Button::new("Sidebar")
                            .selected(vis.sidebar_visible)
                            .shortcut_text("Tab"),
                    )
                    .clicked()
                {
                    vis.sidebar_visible = !vis.sidebar_visible;
                    ui.close();
                }
                if ui
                    .add(
                        egui::Button::new("Console")
                            .selected(vis.console_visible)
                            .shortcut_text("`"),
                    )
                    .clicked()
                {
                    vis.console_visible = !vis.console_visible;
                    ui.close();
                }
                if ui
                    .add_enabled(
                        has_model,
                        egui::Button::new("Model Stats").selected(vis.stats_visible),
                    )
                    .clicked()
                {
                    vis.stats_visible = !vis.stats_visible;
                    ui.close();
                }
                if ui
                    .add(egui::Button::new("FPS HUD").selected(vis.fps_hud_visible))
                    .clicked()
                {
                    vis.fps_hud_visible = !vis.fps_hud_visible;
                    ui.close();
                }
            });

            ui.menu_button("Help", |ui| {
                if ui.button("Solarxy Wiki").clicked() {
                    actions.open_wiki = true;
                    ui.close();
                }
                if ui.button("Check for Updates\u{2026}").clicked() {
                    actions.check_for_updates = true;
                    ui.close();
                }
                ui.separator();
                if ui.button("About Solarxy").clicked() {
                    actions.open_about = true;
                    ui.close();
                }
            });
        });
    });
}
