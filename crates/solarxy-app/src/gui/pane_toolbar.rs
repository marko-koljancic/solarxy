//! Per-pane viewport toolbar — the slim strip atop each pane.
//!
//! Every pane gets its own toolbar whose dropdowns mutate that pane's
//! display settings. The **active** pane writes through the
//! [`GuiSnapshot`] (so `apply_to_state` carries the change — it rewrites
//! every active-pane field unconditionally); the other panes write their
//! [`PaneDisplaySettings`] directly. Projection lives on the camera, not
//! `PaneDisplaySettings`, so a change is recorded as a request the state
//! layer applies after the egui pass.

use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, CustomBackground, InspectionMode, LineWeight, MaterialOverride,
    NormalsMode, PaneMode, ProjectionMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_core::view_config::PANE_TOOLBAR_HEIGHT;

use super::snapshot::GuiSnapshot;
use super::theme::Theme;
use crate::state::view_state::{BoundsMode, PaneDisplaySettings};

/// Below this pane width (logical px) the less-used controls collapse
/// into a `⋯` overflow menu.
const OVERFLOW_WIDTH: f32 = 360.0;

const PANE_MODES: [PaneMode; 2] = [PaneMode::Scene3D, PaneMode::UvMap];
const PROJECTIONS: [ProjectionMode; 2] =
    [ProjectionMode::Perspective, ProjectionMode::Orthographic];

/// A `ComboBox` listing the builtin backgrounds (`HDRI Sky` gated on an
/// HDRI being loaded) then, under a separator, every user custom
/// background. Shared by the per-pane toolbar and the menu bar.
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
    /// Latest UV-shell overlap percentage, shown beside the UV-map
    /// `Overlap` toggle. `None` until a readback completes.
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

        ui.painter().rect_filled(strip, 0.0, theme.bg_elevated);

        let projection = data.projections[i];
        let mut new_projection: Option<ProjectionMode> = None;
        {
            let mut fields = if is_active {
                PaneFields::from_snapshot(snap)
            } else {
                PaneFields::from_pane(&mut data.pane_settings[i])
            };
            let compact = strip.width() < OVERFLOW_WIDTH;
            ui.scope_builder(
                egui::UiBuilder::new().max_rect(strip.shrink2(egui::vec2(6.0, 2.0))),
                |ui| {
                    ui.horizontal_centered(|ui| {
                        draw_controls(
                            ui,
                            i,
                            &mut fields,
                            projection,
                            &mut new_projection,
                            compact,
                            hdri_available,
                            customs,
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

#[allow(clippy::too_many_arguments)]
fn draw_controls(
    ui: &mut egui::Ui,
    idx: usize,
    f: &mut PaneFields,
    projection: ProjectionMode,
    new_projection: &mut Option<ProjectionMode>,
    compact: bool,
    hdri_available: bool,
    customs: &[CustomBackground],
    uv_overlap_pct: Option<f32>,
) {
    combo(ui, (idx, "pm"), "Pane mode", f.pane_mode, &PANE_MODES);

    if *f.pane_mode == PaneMode::Scene3D {
        combo(ui, (idx, "sh"), "Shading", f.view_mode, ViewMode::ALL);
        combo(
            ui,
            (idx, "in"),
            "Inspection mode",
            f.inspection_mode,
            InspectionMode::ALL,
        );

        if compact {
            ui.menu_button("\u{22ef}", |ui| {
                draw_overflow_controls(
                    ui,
                    idx,
                    f,
                    projection,
                    new_projection,
                    hdri_available,
                    customs,
                );
            })
            .response
            .on_hover_text("More controls");
        } else {
            draw_overflow_controls(
                ui,
                idx,
                f,
                projection,
                new_projection,
                hdri_available,
                customs,
            );
        }
    } else {
        draw_uv_controls(ui, idx, f, uv_overlap_pct);
    }
}

/// UV-map-pane toolbar controls — the per-pane home for what was the
/// sidebar's UV-map section. Shown when the pane is in UV Map mode.
fn draw_uv_controls(
    ui: &mut egui::Ui,
    idx: usize,
    f: &mut PaneFields,
    uv_overlap_pct: Option<f32>,
) {
    combo(
        ui,
        (idx, "uvbg"),
        "UV map background",
        f.uv_bg,
        UvMapBackground::ALL,
    );
    ui.checkbox(f.show_uv_overlap, "Overlap")
        .on_hover_text("UV shell overlap heatmap");
    if *f.show_uv_overlap
        && let Some(pct) = uv_overlap_pct
    {
        ui.label(format!("{pct:.1}%"))
            .on_hover_text("Overlapping UV area");
    }
    combo(
        ui,
        (idx, "uvlw"),
        "Wireframe weight",
        f.line_weight,
        LineWeight::ALL,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_overflow_controls(
    ui: &mut egui::Ui,
    idx: usize,
    f: &mut PaneFields,
    projection: ProjectionMode,
    new_projection: &mut Option<ProjectionMode>,
    hdri_available: bool,
    customs: &[CustomBackground],
) {
    combo(
        ui,
        (idx, "ov"),
        "Material override",
        f.material_override,
        MaterialOverride::ALL,
    );

    let mut proj = projection;
    combo(ui, (idx, "pr"), "Projection", &mut proj, &PROJECTIONS);
    if proj != projection {
        *new_projection = Some(proj);
    }

    ui.menu_button("Show", |ui| {
        ui.checkbox(f.show_grid, "Grid");
        ui.checkbox(f.show_axis_gizmo, "Axis Gizmo");
        ui.checkbox(f.show_local_axes, "Local Axes");
        ui.checkbox(f.show_validation, "Validation Overlay");
        ui.separator();
        combo(ui, (idx, "nm"), "Normals", f.normals_mode, NormalsMode::ALL);
        combo(ui, (idx, "uv"), "UV Overlay", f.uv_mode, UvMode::ALL);
        combo(ui, (idx, "bn"), "Bounds", f.bounds_mode, BoundsMode::ALL);
        combo(
            ui,
            (idx, "lw"),
            "Wireframe Weight",
            f.line_weight,
            LineWeight::ALL,
        );
    })
    .response
    .on_hover_text("Overlays");

    background_combo(ui, (idx, "bg"), f.background_mode, customs, hdri_available);
}

/// A compact `ComboBox` that picks one of `all` into `current`.
fn combo<T: PartialEq + Copy + std::fmt::Display>(
    ui: &mut egui::Ui,
    id: (usize, &str),
    tooltip: &str,
    current: &mut T,
    all: &[T],
) {
    egui::ComboBox::from_id_salt(id)
        .selected_text(current.to_string())
        .show_ui(ui, |ui| {
            for &variant in all {
                ui.selectable_value(current, variant, variant.to_string());
            }
        })
        .response
        .on_hover_text(tooltip);
}
