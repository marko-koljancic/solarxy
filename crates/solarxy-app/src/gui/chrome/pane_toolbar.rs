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

use crate::gui::intent::{Intent, Intents, PaneChange};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;
use crate::state::view_state::{BoundsMode, PaneDisplaySettings};

const PANE_MODES: [PaneMode; 2] = [PaneMode::Scene3D, PaneMode::UvMap];
const PROJECTIONS: [ProjectionMode; 2] =
    [ProjectionMode::Perspective, ProjectionMode::Orthographic];

/// Per-frame data the per-pane toolbars need. `rects` are the full pane
/// rects (toolbar strip + 3D content) in egui-logical space.
pub(crate) struct PaneToolbarData<'a> {
    pub rects: &'a [egui::Rect],
    pub active: usize,
    pub projections: [ProjectionMode; 4],
    /// `true` once an HDRI is loaded — gates the `HDRI Sky` background.
    pub hdri_available: bool,
    /// User custom backgrounds, listed in every Background dropdown.
    pub customs: &'a [CustomBackground],
    /// Latest UV-shell overlap percentage, shown in the UV `Display`
    /// label when overlap is on. `None` until a readback completes.
    pub uv_overlap_pct: Option<f32>,
    /// The open scene's camera nodes, `(scene object id, node name)`.
    /// Empty when no engine scene is open or the scene has no cameras, in
    /// which case the Look Through submenu simply does not appear.
    pub cameras: &'a [(u64, String)],
    /// Which camera each pane looks through, mirroring the state field.
    pub look_through: [Option<u64>; 4],
}

/// A toolbar's requested look-through change for one pane: bind to a
/// camera node, or return to a free view.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LookThroughChange {
    Bind(u64),
    Free,
}

/// Draw the toolbar strip atop every pane. Called inside the Viewport
/// dock-tab's `ui()` callback.
pub(in crate::gui) fn draw_pane_toolbars(
    ui: &mut egui::Ui,
    data: &PaneToolbarData,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
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
        let bound_camera = data.look_through.get(i).copied().flatten();
        // Every pane goes through one path. The active pane and the others
        // were written through two before this, chosen by a fifteen-field
        // borrow bundle with a constructor each.
        let pane = &settings.panes[i.min(3)];
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(strip.shrink2(egui::vec2(8.0, 2.0))),
            |ui| {
                style_frameless_labels(ui, theme);
                ui.horizontal_centered(|ui| {
                    draw_controls(
                        ui,
                        PaneControls {
                            index: i,
                            pane,
                            projection,
                            customs,
                            hdri_available,
                            uv_overlap_pct,
                            cameras: data.cameras,
                            bound_camera,
                        },
                        intents,
                    );
                });
            },
        );

        if is_active {
            ui.painter().rect_stroke(
                rect,
                0.0,
                egui::Stroke::new(1.0_f32, theme.accent),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// Style the toolbar `Ui` so its `menu_button`s render as bare floating
/// text — no fill or outline in any state. The hover / open cue is the
/// text colour shifting to the amber accent. Scoped to the toolbar; the
/// dropdown popups open with the global theme style untouched.
fn style_frameless_labels(ui: &mut egui::Ui, theme: Theme) {
    ui.spacing_mut().item_spacing.x = 10.0;
    ui.spacing_mut().button_padding = egui::vec2(6.0, 1.0);
    let transparent = egui::Color32::TRANSPARENT;
    let w = &mut ui.style_mut().visuals.widgets;
    w.inactive.bg_fill = transparent;
    w.inactive.weak_bg_fill = transparent;
    w.inactive.bg_stroke = egui::Stroke::NONE;
    w.inactive.fg_stroke = egui::Stroke::new(1.0_f32, theme.fg);
    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.bg_fill = transparent;
        s.weak_bg_fill = transparent;
        s.bg_stroke = egui::Stroke::NONE;
        s.fg_stroke = egui::Stroke::new(1.0_f32, theme.accent);
    }
}

/// One pane's worth of what its toolbar draws from.
#[derive(Clone, Copy)]
struct PaneControls<'a> {
    index: usize,
    pane: &'a PaneDisplaySettings,
    projection: ProjectionMode,
    customs: &'a [CustomBackground],
    hdri_available: bool,
    uv_overlap_pct: Option<f32>,
    cameras: &'a [(u64, String)],
    bound_camera: Option<u64>,
}

/// A row of radio buttons over a variant set, returning what the user picked.
///
/// Returns rather than writing through a borrow, which is what lets one
/// function serve every pane: the pane it acts on is the index the caller
/// stamps onto the intent, not a handle it was handed.
fn radio_pick<T, S>(
    ui: &mut egui::Ui,
    current: T,
    all: impl IntoIterator<Item = T>,
    label: impl Fn(T) -> S,
) -> Option<T>
where
    T: Copy + PartialEq,
    S: Into<egui::WidgetText>,
{
    let mut value = current;
    let mut picked = None;
    for variant in all {
        if ui
            .radio_value(&mut value, variant, label(variant))
            .changed()
        {
            picked = Some(variant);
        }
    }
    picked
}

/// A checkbox, returning the new value when the user flipped it. `hover` is
/// empty for the rows that carry no tooltip.
fn check(ui: &mut egui::Ui, current: bool, label: &str, hover: &str) -> Option<bool> {
    let mut value = current;
    let mut response = ui.checkbox(&mut value, label);
    if !hover.is_empty() {
        response = response.on_hover_text(hover);
    }
    response.changed().then_some(value)
}

fn draw_controls(ui: &mut egui::Ui, cx: PaneControls<'_>, intents: &mut Intents) {
    let PaneControls {
        index,
        pane,
        projection,
        customs,
        hdri_available,
        uv_overlap_pct,
        cameras,
        bound_camera,
    } = cx;

    // Label 1: pane mode.
    let pane_mode = pane.pane_mode.to_string();
    label_menu(ui, (index, "pm"), &pane_mode, |ui| {
        if let Some(v) = radio_pick(ui, pane.pane_mode, PANE_MODES, |v| v.to_string()) {
            intents.pane(index, PaneChange::PaneMode(v));
        }
    });

    if pane.pane_mode == PaneMode::Scene3D {
        // Label 2: display, which is shading plus inspection plus override.
        let display = display_label(pane);
        label_menu(ui, (index, "disp"), &display, |ui| {
            if let Some(v) = radio_pick(ui, pane.view_mode, ViewMode::ALL.iter().copied(), |v| {
                v.to_string()
            }) {
                intents.pane(index, PaneChange::ViewMode(v));
            }
            ui.separator();
            ui.menu_button("Inspection", |ui| {
                if let Some(v) = radio_pick(
                    ui,
                    pane.inspection_mode,
                    InspectionMode::ALL.iter().copied(),
                    |v| v.to_string(),
                ) {
                    intents.pane(index, PaneChange::InspectionMode(v));
                }
            });
            ui.menu_button("Material Override", |ui| {
                if let Some(v) = radio_pick(
                    ui,
                    pane.material_override,
                    MaterialOverride::ALL.iter().copied(),
                    |v| v.to_string(),
                ) {
                    intents.pane(index, PaneChange::MaterialOverride(v));
                }
            });
        });

        // Label 3: view, which is projection plus look-through plus overlays
        // plus background. A bound pane's label is the camera's name, which is
        // the visible cue that the pane is a shot rather than a free view.
        let bound_name = bound_camera
            .and_then(|id| cameras.iter().find(|(cid, _)| *cid == id))
            .map(|(_, name)| name.as_str());
        let view_label = bound_name.map_or_else(|| projection.to_string(), str::to_owned);
        label_menu(ui, (index, "view"), &view_label, |ui| {
            if let Some(mode) = radio_pick(ui, projection, PROJECTIONS, |p| p.to_string()) {
                intents.raise(Intent::PaneProjection { pane: index, mode });
            }
            if !cameras.is_empty() {
                ui.separator();
                ui.menu_button("Look Through", |ui| {
                    if ui.radio(bound_camera.is_none(), "Free View").clicked()
                        && bound_camera.is_some()
                    {
                        intents.raise(Intent::LookThrough {
                            pane: index,
                            change: LookThroughChange::Free,
                        });
                        ui.close();
                    }
                    for (id, name) in cameras {
                        if ui.radio(bound_camera == Some(*id), name).clicked()
                            && bound_camera != Some(*id)
                        {
                            intents.raise(Intent::LookThrough {
                                pane: index,
                                change: LookThroughChange::Bind(*id),
                            });
                            ui.close();
                        }
                    }
                });
            }
            ui.separator();
            ui.menu_button("Overlays", |ui| {
                draw_overlays_menu(ui, index, pane, intents);
            });
            ui.menu_button("Background", |ui| {
                if let Some(v) =
                    background_menu_body(ui, pane.background_mode, customs, hdri_available)
                {
                    intents.pane(index, PaneChange::BackgroundMode(v));
                }
            });
        });
    } else {
        // UV pane: pane mode plus a consolidated Display menu.
        let display = match (pane.show_uv_overlap, uv_overlap_pct) {
            (true, Some(pct)) => format!("Display \u{00b7} {pct:.0}%"),
            _ => "Display".to_string(),
        };
        label_menu(ui, (index, "uvd"), &display, |ui| {
            ui.menu_button("Background", |ui| {
                if let Some(v) =
                    radio_pick(ui, pane.uv_bg, UvMapBackground::ALL.iter().copied(), |v| {
                        v.to_string()
                    })
                {
                    intents.pane(index, PaneChange::UvBackground(v));
                }
            });
            if let Some(v) = check(
                ui,
                pane.show_uv_overlap,
                "Overlap",
                "UV shell overlap heatmap",
            ) {
                intents.pane(index, PaneChange::ShowUvOverlap(v));
            }
            ui.menu_button("Wireframe weight", |ui| {
                if let Some(v) =
                    radio_pick(ui, pane.line_weight, LineWeight::ALL.iter().copied(), |v| {
                        v.descriptive_label()
                    })
                {
                    intents.pane(index, PaneChange::LineWeight(v));
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

/// The `Shaded` label's text: the dominant on-screen display mode, which is
/// the inspection mode if it is not the default, else the material override
/// if it is not `None`, else the shading mode.
fn display_label(pane: &PaneDisplaySettings) -> String {
    if pane.inspection_mode != InspectionMode::Shaded {
        pane.inspection_mode.to_string()
    } else if pane.material_override != MaterialOverride::None {
        pane.material_override.to_string()
    } else {
        pane.view_mode.to_string()
    }
}

/// Body of the `Overlays ▸` submenu: scene-overlay toggles plus the
/// per-overlay mode submenus.
fn draw_overlays_menu(
    ui: &mut egui::Ui,
    index: usize,
    pane: &PaneDisplaySettings,
    intents: &mut Intents,
) {
    for (current, label, make) in [
        (
            pane.show_grid,
            "Grid",
            PaneChange::ShowGrid as fn(bool) -> PaneChange,
        ),
        (
            pane.show_axis_gizmo,
            "Axis Gizmo",
            PaneChange::ShowAxisGizmo,
        ),
        (
            pane.show_local_axes,
            "Local Axes",
            PaneChange::ShowLocalAxes,
        ),
        (
            pane.show_validation,
            "Validation Overlay",
            PaneChange::ShowValidation,
        ),
    ] {
        if let Some(v) = check(ui, current, label, "") {
            intents.pane(index, make(v));
        }
    }
    ui.separator();
    ui.menu_button("Normals", |ui| {
        if let Some(v) = radio_pick(
            ui,
            pane.normals_mode,
            NormalsMode::ALL.iter().copied(),
            |v| v.to_string(),
        ) {
            intents.pane(index, PaneChange::NormalsMode(v));
        }
    });
    ui.menu_button("UV Overlay", |ui| {
        if let Some(v) = radio_pick(ui, pane.uv_mode, UvMode::ALL.iter().copied(), |v| {
            v.to_string()
        }) {
            intents.pane(index, PaneChange::UvMode(v));
        }
    });
    ui.menu_button("Bounds", |ui| {
        if let Some(v) = radio_pick(ui, pane.bounds_mode, BoundsMode::ALL.iter().copied(), |v| {
            v.to_string()
        }) {
            intents.pane(index, PaneChange::BoundsMode(v));
        }
    });
    ui.menu_button("Wireframe Weight", |ui| {
        if let Some(v) = radio_pick(ui, pane.line_weight, LineWeight::ALL.iter().copied(), |v| {
            v.descriptive_label()
        }) {
            intents.pane(index, PaneChange::LineWeight(v));
        }
    });
}

/// Body of the `Background ▸` submenu: builtins (`HDRI Sky` gated on a
/// loaded HDRI) then, under a separator, every user custom background.
fn background_menu_body(
    ui: &mut egui::Ui,
    current: BackgroundMode,
    customs: &[CustomBackground],
    hdri_available: bool,
) -> Option<BackgroundMode> {
    let mut value = current;
    let mut picked = None;
    for &builtin in BuiltinBg::ALL {
        if builtin == BuiltinBg::HdriSky && !hdri_available {
            continue;
        }
        if ui
            .radio_value(
                &mut value,
                BackgroundMode::Builtin(builtin),
                builtin.to_string(),
            )
            .changed()
        {
            picked = Some(BackgroundMode::Builtin(builtin));
        }
    }
    if !customs.is_empty() {
        ui.separator();
        for custom in customs {
            if ui
                .radio_value(&mut value, BackgroundMode::Custom(custom.id), &custom.name)
                .changed()
            {
                picked = Some(BackgroundMode::Custom(custom.id));
            }
        }
    }
    picked
}
