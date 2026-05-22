//! Per-pane viewport toolbar — the slim strip atop each pane.
//!
//! Every pane gets its own toolbar whose controls mutate that pane's
//! display settings. The **active** pane writes through the
//! [`GuiSnapshot`] (so `apply_to_state` carries the change — it rewrites
//! every active-pane field unconditionally); the other panes write their
//! [`PaneDisplaySettings`] directly. Projection lives on the camera, not
//! `PaneDisplaySettings`, so a change is recorded as a request the state
//! layer applies after the egui pass.
//!
//! The controls are 3ds Max-style **viewport label menus**: a few frameless
//! bracketed text labels (`[ Scene 3D ]` / `[ Shaded ]` / `[ Perspective ]`;
//! a UV pane shows `[ UV Map ]` / `[ Display ]`) that **float directly on
//! the 3D scene** — no strip fill — and open a dropdown with nested
//! submenus on click. Idle text is `theme.fg`; hover / open shifts it to
//! the amber accent (no pill). The label shows the control's current
//! value, so the row stays uncluttered even in a 4-up quad.

use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, CustomBackground, InspectionMode, LineWeight, MaterialOverride,
    NormalsMode, PaneMode, ProjectionMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::view_config::PANE_TOOLBAR_HEIGHT;

use super::snapshot::GuiSnapshot;
use super::theme::Theme;
use crate::state::view_state::{BoundsMode, PaneDisplaySettings};

const PANE_MODES: [PaneMode; 2] = [PaneMode::Scene3D, PaneMode::UvMap];
const PROJECTIONS: [ProjectionMode; 2] =
    [ProjectionMode::Perspective, ProjectionMode::Orthographic];

/// A `ComboBox` listing the builtin backgrounds (`HDRI Sky` gated on an
/// HDRI being loaded) then, under a separator, every user custom
/// background. Used by the Preferences modal's custom-background editor.
pub(super) fn background_combo(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    current: &mut BackgroundMode,
    customs: &[CustomBackground],
    hdri_available: bool,
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.label(customs))
        .show_ui(ui, |ui| {
            for &builtin in BuiltinBg::ALL {
                if builtin == BuiltinBg::HdriSky && !hdri_available {
                    continue;
                }
                ui.selectable_value(
                    current,
                    BackgroundMode::Builtin(builtin),
                    builtin.to_string(),
                );
            }
            if !customs.is_empty() {
                ui.separator();
                for custom in customs {
                    ui.selectable_value(current, BackgroundMode::Custom(custom.id), &custom.name);
                }
            }
        })
        .response
        .on_hover_text("Background");
}

/// Per-frame data the per-pane toolbars need. `rects` are the full pane
/// rects (toolbar strip + 3D content) in egui-logical space.
pub(crate) struct PaneToolbarData<'a> {
    pub rects: &'a [egui::Rect],
    pub active: usize,
    pub pane_settings: &'a mut [PaneDisplaySettings; 4],
    pub projections: [ProjectionMode; 4],
    /// Set to `(pane, mode)` when a toolbar changes a pane's projection;
    /// the state layer applies it to that pane's camera after `render_ui`.
    pub projection_change: &'a mut Option<(usize, ProjectionMode)>,
    /// `true` once an HDRI is loaded — gates the `HDRI Sky` background.
    pub hdri_available: bool,
    /// User custom backgrounds, listed in every Background dropdown.
    pub customs: &'a [CustomBackground],
    /// Latest UV-shell overlap percentage, shown in the UV `Display`
    /// label when overlap is on. `None` until a readback completes.
    pub uv_overlap_pct: Option<f32>,
}

/// Mutable handles to the per-pane fields a toolbar edits.
struct PaneFields<'a> {
    pane_mode: &'a mut PaneMode,
    view_mode: &'a mut ViewMode,
    inspection_mode: &'a mut InspectionMode,
    material_override: &'a mut MaterialOverride,
    background_mode: &'a mut BackgroundMode,
    normals_mode: &'a mut NormalsMode,
    uv_mode: &'a mut UvMode,
    bounds_mode: &'a mut BoundsMode,
    line_weight: &'a mut LineWeight,
    show_grid: &'a mut bool,
    show_axis_gizmo: &'a mut bool,
    show_local_axes: &'a mut bool,
    show_validation: &'a mut bool,
    uv_bg: &'a mut UvMapBackground,
    show_uv_overlap: &'a mut bool,
}

impl<'a> PaneFields<'a> {
    fn from_snapshot(s: &'a mut GuiSnapshot) -> Self {
        Self {
            pane_mode: &mut s.pane_mode,
            view_mode: &mut s.view_mode,
            inspection_mode: &mut s.inspection_mode,
            material_override: &mut s.material_override,
            background_mode: &mut s.background_mode,
            normals_mode: &mut s.normals_mode,
            uv_mode: &mut s.uv_mode,
            bounds_mode: &mut s.bounds_mode,
            line_weight: &mut s.line_weight,
            show_grid: &mut s.show_grid,
            show_axis_gizmo: &mut s.show_axis_gizmo,
            show_local_axes: &mut s.show_local_axes,
            show_validation: &mut s.show_validation,
            uv_bg: &mut s.uv_bg,
            show_uv_overlap: &mut s.show_uv_overlap,
        }
    }

    fn from_pane(p: &'a mut PaneDisplaySettings) -> Self {
        Self {
            pane_mode: &mut p.pane_mode,
            view_mode: &mut p.view_mode,
            inspection_mode: &mut p.inspection_mode,
            material_override: &mut p.material_override,
            background_mode: &mut p.background_mode,
            normals_mode: &mut p.normals_mode,
            uv_mode: &mut p.uv_mode,
            bounds_mode: &mut p.bounds_mode,
            line_weight: &mut p.line_weight,
            show_grid: &mut p.show_grid,
            show_axis_gizmo: &mut p.show_axis_gizmo,
            show_local_axes: &mut p.show_local_axes,
            show_validation: &mut p.show_validation,
            uv_bg: &mut p.uv_bg,
            show_uv_overlap: &mut p.show_uv_overlap,
        }
    }
}

/// Draw the toolbar strip atop every pane. Called inside the Viewport
/// dock-tab's `ui()` callback.
pub(super) fn draw_pane_toolbars(
    ui: &mut egui::Ui,
    data: &mut PaneToolbarData,
    snap: &mut GuiSnapshot,
    theme: Theme,
) {
    let hdri_available = data.hdri_available;
    let uv_overlap_pct = data.uv_overlap_pct;
    let customs = data.customs;
    for i in 0..data.rects.len() {
        let rect = data.rects[i];
        let strip =
            egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), PANE_TOOLBAR_HEIGHT));
        let is_active = i == data.active;

        // No strip fill — the 3D scene renders the full pane and the
        // labels float on top of it (3ds Max style).

        let projection = data.projections[i];
        let mut new_projection: Option<ProjectionMode> = None;
        {
            let mut fields = if is_active {
                PaneFields::from_snapshot(snap)
            } else {
                PaneFields::from_pane(&mut data.pane_settings[i])
            };
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(strip.shrink2(egui::vec2(8.0, 2.0))),
                |ui| {
                    style_frameless_labels(ui, theme);
                    ui.horizontal_centered(|ui| {
                        draw_controls(
                            ui,
                            i,
                            &mut fields,
                            projection,
                            &mut new_projection,
                            customs,
                            hdri_available,
                            uv_overlap_pct,
                        );
                    });
                },
            );
        }
        if let Some(p) = new_projection {
            *data.projection_change = Some((i, p));
        }

        if is_active {
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0, theme.accent),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// Style the toolbar `Ui` so its `menu_button`s render as bare floating
/// text — no fill or outline in any state. The hover / open cue is the
/// text colour shifting to the amber accent. Scoped to the toolbar; the
/// dropdown popups open with the global Ayu style untouched.
fn style_frameless_labels(ui: &mut egui::Ui, theme: Theme) {
    ui.spacing_mut().item_spacing.x = 10.0;
    ui.spacing_mut().button_padding = egui::vec2(6.0, 1.0);
    let transparent = egui::Color32::TRANSPARENT;
    let w = &mut ui.style_mut().visuals.widgets;
    w.inactive.bg_fill = transparent;
    w.inactive.weak_bg_fill = transparent;
    w.inactive.bg_stroke = egui::Stroke::NONE;
    w.inactive.fg_stroke = egui::Stroke::new(1.0, theme.fg);
    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_fill = transparent;
        s.weak_bg_fill = transparent;
        s.bg_stroke = egui::Stroke::NONE;
        s.fg_stroke = egui::Stroke::new(1.0, theme.accent);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_controls(
    ui: &mut egui::Ui,
    idx: usize,
    f: &mut PaneFields,
    projection: ProjectionMode,
    new_projection: &mut Option<ProjectionMode>,
    customs: &[CustomBackground],
    hdri_available: bool,
    uv_overlap_pct: Option<f32>,
) {
    // Label 1 — pane mode.
    let pane_mode = f.pane_mode.to_string();
    label_menu(ui, (idx, "pm"), &pane_mode, |ui| {
        for &pm in &PANE_MODES {
            ui.radio_value(f.pane_mode, pm, pm.to_string());
        }
    });

    if *f.pane_mode == PaneMode::Scene3D {
        // Label 2 — display: shading + inspection + override.
        let display = display_label(f);
        label_menu(ui, (idx, "disp"), &display, |ui| {
            for &vm in ViewMode::ALL {
                ui.radio_value(f.view_mode, vm, vm.to_string());
            }
            ui.separator();
            ui.menu_button("Inspection", |ui| {
                for &im in InspectionMode::ALL {
                    ui.radio_value(f.inspection_mode, im, im.to_string());
                }
            });
            ui.menu_button("Material Override", |ui| {
                for &mo in MaterialOverride::ALL {
                    ui.radio_value(f.material_override, mo, mo.to_string());
                }
            });
        });

        // Label 3 — view: projection + overlays + background.
        label_menu(ui, (idx, "view"), &projection.to_string(), |ui| {
            let mut proj = projection;
            for &p in &PROJECTIONS {
                ui.radio_value(&mut proj, p, p.to_string());
            }
            if proj != projection {
                *new_projection = Some(proj);
            }
            ui.separator();
            ui.menu_button("Overlays", |ui| draw_overlays_menu(ui, f));
            ui.menu_button("Background", |ui| {
                background_menu_body(ui, f.background_mode, customs, hdri_available);
            });
        });
    } else {
        // UV pane — pane mode + a consolidated Display menu.
        let display = match (*f.show_uv_overlap, uv_overlap_pct) {
            (true, Some(pct)) => format!("Display \u{00b7} {pct:.0}%"),
            _ => "Display".to_string(),
        };
        label_menu(ui, (idx, "uvd"), &display, |ui| {
            ui.menu_button("Background", |ui| {
                for &b in UvMapBackground::ALL {
                    ui.radio_value(f.uv_bg, b, b.to_string());
                }
            });
            ui.checkbox(f.show_uv_overlap, "Overlap")
                .on_hover_text("UV shell overlap heatmap");
            ui.menu_button("Wireframe weight", |ui| {
                for &w in LineWeight::ALL {
                    ui.radio_value(f.line_weight, w, w.descriptive_label());
                }
            });
        });
    }
}

/// A frameless viewport label that opens `contents` as a dropdown on
/// click. The label text is bracketed (`"[ value ]"`, 3ds Max style) —
/// the brackets are the click affordance, no caret glyph. `push_id`
/// keeps the popup state collision-free across the four pane toolbars.
fn label_menu(
    ui: &mut egui::Ui,
    id: (usize, &str),
    value: &str,
    contents: impl FnOnce(&mut egui::Ui),
) {
    ui.push_id(id, |ui| {
        ui.menu_button(format!("[ {value} ]"), contents);
    });
}

/// The `Shaded` label's text — the dominant on-screen display mode:
/// inspection mode if not the default, else material override if not
/// `None`, else the shading mode.
fn display_label(f: &PaneFields) -> String {
    if *f.inspection_mode != InspectionMode::Shaded {
        f.inspection_mode.to_string()
    } else if *f.material_override != MaterialOverride::None {
        f.material_override.to_string()
    } else {
        f.view_mode.to_string()
    }
}

/// Body of the `Overlays ▸` submenu: scene-overlay toggles plus the
/// per-overlay mode submenus.
fn draw_overlays_menu(ui: &mut egui::Ui, f: &mut PaneFields) {
    ui.checkbox(f.show_grid, "Grid");
    ui.checkbox(f.show_axis_gizmo, "Axis Gizmo");
    ui.checkbox(f.show_local_axes, "Local Axes");
    ui.checkbox(f.show_validation, "Validation Overlay");
    ui.separator();
    ui.menu_button("Normals", |ui| {
        for &m in NormalsMode::ALL {
            ui.radio_value(f.normals_mode, m, m.to_string());
        }
    });
    ui.menu_button("UV Overlay", |ui| {
        for &m in UvMode::ALL {
            ui.radio_value(f.uv_mode, m, m.to_string());
        }
    });
    ui.menu_button("Bounds", |ui| {
        for &m in BoundsMode::ALL {
            ui.radio_value(f.bounds_mode, m, m.to_string());
        }
    });
    ui.menu_button("Wireframe Weight", |ui| {
        for &w in LineWeight::ALL {
            ui.radio_value(f.line_weight, w, w.descriptive_label());
        }
    });
}

/// Body of the `Background ▸` submenu: builtins (`HDRI Sky` gated on a
/// loaded HDRI) then, under a separator, every user custom background.
fn background_menu_body(
    ui: &mut egui::Ui,
    current: &mut BackgroundMode,
    customs: &[CustomBackground],
    hdri_available: bool,
) {
    for &builtin in BuiltinBg::ALL {
        if builtin == BuiltinBg::HdriSky && !hdri_available {
            continue;
        }
        ui.radio_value(
            current,
            BackgroundMode::Builtin(builtin),
            builtin.to_string(),
        );
    }
    if !customs.is_empty() {
        ui.separator();
        for custom in customs {
            ui.radio_value(current, BackgroundMode::Custom(custom.id), &custom.name);
        }
    }
}
