//! Native-style menu bar — the Houdini-inspired 7-menu layout:
//! `File / Edit / Render / View / Layout / Window / Help`.
//!
//! Per-pane controls (Shading, Inspection, Material Override, Background,
//! Projection) write through [`GuiSnapshot`], which mirrors the **active
//! pane** — so a menu click acts on whichever pane the cursor last
//! selected. Scene-global controls (post-processing, IBL, turntable)
//! write the same snapshot's global fields.

use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, CustomBackground, IblMode, InspectionMode, LineWeight,
    MaterialOverride, NormalsMode, PaneMode, ProjectionMode, ToneMode, UvMode, ViewMode,
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
    hdri_available: bool,
    customs: &[CustomBackground],
) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            draw_file_menu(ui, actions, has_model, recent_files);
            draw_edit_menu(ui, actions);
            draw_render_menu(ui, snap, actions, hdri_available, customs);
            draw_view_menu(ui, snap, actions);
            draw_layout_menu(ui, actions, vis);
            draw_window_menu(ui, vis, has_model);
            draw_help_menu(ui, actions);
        });
    });
}

/// Submenu of `T::ALL`-style variants as selectable rows writing the
/// chosen variant into `current`. Covers every plain enum-pick submenu;
/// the Inspection + Projection submenus stay inline (special behaviour).
fn variant_submenu<T: PartialEq + Copy + std::fmt::Display>(
    ui: &mut egui::Ui,
    label: &str,
    hover: &str,
    current: &mut T,
    all: &[T],
) {
    ui.menu_button(label, |ui| {
        for &variant in all {
            if ui
                .selectable_label(*current == variant, variant.to_string())
                .clicked()
            {
                *current = variant;
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(hover);
}

/// Background submenu — builtins (`HDRI Sky` gated on an HDRI being
/// loaded) then, under a separator, every user custom background.
fn background_submenu(
    ui: &mut egui::Ui,
    current: &mut BackgroundMode,
    customs: &[CustomBackground],
    hdri_available: bool,
) {
    ui.menu_button("Background", |ui| {
        for &builtin in BuiltinBg::ALL {
            if builtin == BuiltinBg::HdriSky && !hdri_available {
                continue;
            }
            let mode = BackgroundMode::Builtin(builtin);
            if ui
                .selectable_label(*current == mode, builtin.to_string())
                .clicked()
            {
                *current = mode;
                ui.close();
            }
        }
        if !customs.is_empty() {
            ui.separator();
            for custom in customs {
                let mode = BackgroundMode::Custom(custom.id);
                if ui
                    .selectable_label(*current == mode, &custom.name)
                    .clicked()
                {
                    *current = mode;
                    ui.close();
                }
            }
        }
    })
    .response
    .on_hover_text("B");
}

fn draw_file_menu(
    ui: &mut egui::Ui,
    actions: &mut MenuActions,
    has_model: bool,
    recent_files: &[String],
) {
    ui.menu_button("File", |ui| {
        if ui
            .add(egui::Button::new("Open Model\u{2026}").shortcut_text(format!("{MOD}+O")))
            .clicked()
        {
            actions.open_model = true;
            ui.close();
        }
        if ui
            .add(egui::Button::new("Import HDRI\u{2026}").shortcut_text(format!("{MOD}+Shift+O")))
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
}

fn draw_edit_menu(ui: &mut egui::Ui, actions: &mut MenuActions) {
    ui.menu_button("Edit", |ui| {
        if ui
            .add(egui::Button::new("Preferences\u{2026}").shortcut_text(format!("{MOD}+,")))
            .clicked()
        {
            actions.open_preferences = true;
            ui.close();
        }
        ui.separator();
        if ui
            .button("Save View Settings as Default")
            .on_hover_text("Persist the current display, rendering and lighting settings")
            .clicked()
        {
            actions.save_view_defaults = true;
            ui.close();
        }
    });
}

fn draw_render_menu(
    ui: &mut egui::Ui,
    snap: &mut GuiSnapshot,
    actions: &mut MenuActions,
    hdri_available: bool,
    customs: &[CustomBackground],
) {
    ui.menu_button("Render", |ui| {
        variant_submenu(ui, "Shading", "W", &mut snap.view_mode, ViewMode::ALL);

        ui.menu_button("Inspection", |ui| {
            for mode in InspectionMode::ALL {
                let selected = snap.pane_mode == PaneMode::Scene3D && snap.inspection_mode == *mode;
                let shortcut = match mode {
                    InspectionMode::Shaded => "1",
                    InspectionMode::MaterialId => "2",
                    InspectionMode::TexelDensity => "4",
                    InspectionMode::Depth => "5",
                    InspectionMode::Overdraw => "6",
                    InspectionMode::AoPreview => "7",
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

        variant_submenu(
            ui,
            "Material Override",
            "M / Shift+M",
            &mut snap.material_override,
            MaterialOverride::ALL,
        );

        ui.separator();

        variant_submenu(
            ui,
            "Tone Mapping",
            "Shift+T",
            &mut snap.tone_mode,
            ToneMode::ALL,
        );
        ui.checkbox(&mut snap.bloom_enabled, "Bloom")
            .on_hover_text("Shift+D");
        ui.checkbox(&mut snap.ssao_enabled, "SSAO")
            .on_hover_text("Shift+O");

        ui.separator();

        ui.menu_button("Lighting", |ui| {
            variant_submenu(
                ui,
                "IBL Mode",
                "I / Shift+I",
                &mut snap.ibl_mode,
                IblMode::ALL,
            );
            ui.checkbox(&mut snap.lights_locked, "Lock Lights")
                .on_hover_text("Shift+L");
        });
        background_submenu(ui, &mut snap.background_mode, customs, hdri_available);

        ui.separator();

        if ui
            .add(egui::Button::new("Save Screenshot\u{2026}").shortcut_text("C"))
            .clicked()
        {
            actions.save_screenshot = true;
            ui.close();
        }
    });
}

fn draw_view_menu(ui: &mut egui::Ui, snap: &mut GuiSnapshot, actions: &mut MenuActions) {
    ui.menu_button("View", |ui| {
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
        ui.checkbox(&mut snap.turntable_active, "Turntable")
            .on_hover_text("V");
        if snap.is_split {
            ui.checkbox(&mut snap.cameras_linked, "Link Cameras")
                .on_hover_text(format!("{MOD}+L"));
        }

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
            variant_submenu(ui, "Normals", "N", &mut snap.normals_mode, NormalsMode::ALL);
            variant_submenu(ui, "UV Overlay", "U", &mut snap.uv_mode, UvMode::ALL);
            variant_submenu(
                ui,
                "Bounds",
                "Shift+B",
                &mut snap.bounds_mode,
                BoundsMode::ALL,
            );
            variant_submenu(
                ui,
                "Wireframe Weight",
                "Shift+W",
                &mut snap.line_weight,
                LineWeight::ALL,
            );
        });
    });
}

fn draw_layout_menu(ui: &mut egui::Ui, actions: &mut MenuActions, vis: &MenuBarVisibility) {
    ui.menu_button("Layout", |ui| {
        for (layout, label, shortcut) in [
            (ViewLayout::Single, "Single", "F1"),
            (ViewLayout::SplitVertical, "Split Vertical", "F2"),
            (ViewLayout::SplitHorizontal, "Split Horizontal", "F3"),
            (ViewLayout::Quad, "Quad", "F4"),
            (ViewLayout::ThreeLeftBig, "Three-Left-Big", "F5"),
        ] {
            if ui
                .add(egui::Button::new(label).shortcut_text(shortcut))
                .clicked()
            {
                actions.set_layout = Some(layout);
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Save Layout").clicked() {
            actions.save_dock_layout = true;
            ui.close();
        }
        if ui
            .add_enabled(
                vis.has_saved_layout,
                egui::Button::new("Restore Saved Layout"),
            )
            .clicked()
        {
            actions.restore_saved_layout = true;
            ui.close();
        }
        if ui.button("Reset Layout to Default").clicked() {
            actions.reset_dock_layout = true;
            ui.close();
        }
    });
}

fn draw_window_menu(ui: &mut egui::Ui, vis: &mut MenuBarVisibility, has_model: bool) {
    ui.menu_button("Window", |ui| {
        if ui
            .add(
                egui::Button::new("Viewport")
                    .selected(vis.viewport_visible)
                    .shortcut_text(format!("{MOD}+1")),
            )
            .clicked()
        {
            vis.viewport_visible = !vis.viewport_visible;
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
            .add(egui::Button::new("Outliner").selected(vis.outliner_visible))
            .clicked()
        {
            vis.outliner_visible = !vis.outliner_visible;
            ui.close();
        }
        if ui
            .add(egui::Button::new("Properties").selected(vis.properties_visible))
            .clicked()
        {
            vis.properties_visible = !vis.properties_visible;
            ui.close();
        }
        if ui
            .add(egui::Button::new("Review Panel").selected(vis.review_panel_visible))
            .clicked()
        {
            vis.review_panel_visible = !vis.review_panel_visible;
            ui.close();
        }
        if ui
            .add_enabled(
                has_model,
                egui::Button::new("Material Inspector").selected(vis.material_inspector_visible),
            )
            .clicked()
        {
            vis.material_inspector_visible = !vis.material_inspector_visible;
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

        ui.separator();

        if ui
            .add(egui::Button::new("Status Bar").selected(vis.status_bar_visible))
            .clicked()
        {
            vis.status_bar_visible = !vis.status_bar_visible;
            ui.close();
        }
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
    });
}

fn draw_help_menu(ui: &mut egui::Ui, actions: &mut MenuActions) {
    ui.menu_button("Help", |ui| {
        if ui.button("Solarxy Wiki").clicked() {
            actions.open_wiki = true;
            ui.close();
        }
        if ui
            .add(egui::Button::new("Keyboard Shortcuts").shortcut_text("?"))
            .clicked()
        {
            actions.open_shortcuts_modal = true;
            ui.close();
        }
        ui.separator();
        if ui.button("Check for Updates\u{2026}").clicked() {
            actions.check_for_updates = true;
            ui.close();
        }
        if ui.button("About Solarxy").clicked() {
            actions.open_about = true;
            ui.close();
        }
    });
}
