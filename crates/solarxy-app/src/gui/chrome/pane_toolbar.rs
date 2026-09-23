//! Per-pane viewport toolbar — the slim strip atop each pane.
//!
//! Every pane gets its own toolbar whose controls change that pane's
//! display settings. Each change is raised as an `Intent` stamped with the
//! pane's index, and the drain writes the real settings after the egui
//! pass; projection lives on the camera rather than in
//! `PaneDisplaySettings`, so it travels the same way as its own intent.
//!
//! The controls are **viewport label menus**: frameless bracketed text
//! labels that **float directly on the 3D scene** (no strip fill) and open
//! a dropdown with nested submenus on click. Idle text is `theme.fg`;
//! hover and open shift it to the amber accent, with no pill. Each label
//! shows its own current value, so a pane can be read without opening
//! anything.
//!
//! A 3D pane carries **seven** labels: view mode, inspect, override,
//! projection, camera, views, display. Seven rather than three, and that
//! is the point rather than an arrangement preference: one label showing
//! whichever of three settings is non-default cannot show the other two,
//! so a 4-up quad was unreadable at a glance. The view-mode label reads
//! `Path Traced` while the pane traces, and its menu offers that row after
//! the four view modes. A UV pane keeps an inert `[ UV Map ]` label beside
//! `[ Display ]`, with its overlap percentage in a separate readout rather
//! than folded into a label.
//!
//! Entries whose capability is not built yet are drawn **disabled with a
//! tooltip naming what they wait for**, rather than omitted. An absent row
//! reads as unbuilt; a disabled one reads as sequenced, and the surface
//! comparison this shell is held to is checked row by row. The one ruled
//! exception is the `Path Traced` row, absent when the device cannot build
//! the tracer: that is the browser's gate, and a device that cannot trace
//! has nothing to sequence.

use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ProjectionMode, UvMapBackground, ViewMode,
};
use solarxy_core::view_config::PANE_TOOLBAR_HEIGHT;
use solarxy_core::scene::SceneObjectId;
use solarxy_host::cameras::StandardView;

use crate::gui::intent::{DisplayChange, Intent, Intents, PaneChange, PaneLookIntent};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;
use crate::state::view_state::{BoundsMode, PaneDisplaySettings};

const PROJECTIONS: [ProjectionMode; 2] =
    [ProjectionMode::Perspective, ProjectionMode::Orthographic];

/// The six standard views, in the order the Views label lists them.
const AXIS_VIEWS: [(StandardView, &str); 6] = [
    (StandardView::Top, "Top"),
    (StandardView::Bottom, "Bottom"),
    (StandardView::Front, "Front"),
    (StandardView::Back, "Back"),
    (StandardView::Left, "Left"),
    (StandardView::Right, "Right"),
];

/// The UV backgrounds a pane offers, in the browser's order.
///
/// Three of the five [`UvMapBackground`] variants. `Gray` and `Texture` are
/// this shell's alone and the browser lists no entry for either, so they are
/// not offered here; the variants stay, because a scene saved with one still
/// has to render.
const UV_BACKGROUNDS: [UvMapBackground; 3] = [
    UvMapBackground::Checker,
    UvMapBackground::Dark,
    UvMapBackground::Charcoal,
];

/// The turntable speed presets.
///
/// The speed is **scene-global** while the spin itself is per pane, on both
/// shells. That asymmetry is the browser's and is matched rather than
/// corrected, so this one Display entry is the exception to the rule that a
/// pane menu writes only its own pane.
const TURNTABLE_SPEEDS: [(&str, f32); 4] = [
    ("Slow (2 rpm)", 2.0),
    ("Normal (6 rpm)", 6.0),
    ("Fast (12 rpm)", 12.0),
    ("Very fast (30 rpm)", 30.0),
];

/// The preset label for an rpm, or a plain figure for a value no preset
/// names. A preference can hold any speed in range, so the fallback is
/// reachable rather than defensive.
fn turntable_speed_label(rpm: f32) -> String {
    TURNTABLE_SPEEDS
        .iter()
        .find(|(_, v)| (v - rpm).abs() < 0.01)
        .map_or_else(|| format!("{rpm} rpm"), |(label, _)| (*label).to_string())
}

/// Per-frame data the per-pane toolbars need. `rects` are the full pane
/// rects (toolbar strip + 3D content) in egui-logical space.
pub(crate) struct PaneToolbarData<'a> {
    pub rects: &'a [egui::Rect],
    pub active: usize,
    pub projections: [ProjectionMode; 4],
    /// `true` once an HDRI is loaded — gates the `HDRI Sky` background.
    pub hdri_available: bool,
    /// Latest UV-shell overlap percentage, shown in the UV `Display`
    /// label when overlap is on. `None` until a readback completes.
    pub uv_overlap_pct: Option<f32>,
    /// The open scene's camera nodes, `(scene object id, node name)`.
    /// Empty when no engine scene is open or the scene has no cameras, in
    /// which case the Look Through submenu simply does not appear.
    pub cameras: &'a [(u64, String)],
    /// Which camera each pane looks through, mirroring the state field.
    pub look_through: [Option<u64>; 4],
    /// Whether each bound pane is locked to its camera, as the shared rule
    /// answers it: never true for a free view.
    pub camera_locked: [bool; 4],
    /// The scene-global turntable speed, which the Display menu's speed
    /// submenu both reads and writes. Global because it is global in the
    /// browser too; see [`TURNTABLE_SPEEDS`].
    pub turntable_rpm: f32,
    /// Whether the device can build the tracer, which decides whether the
    /// view-mode menu offers `Path Traced` at all. Capability rather than
    /// identity, as the backend contract states it.
    pub tracing_available: bool,
}

/// A toolbar's requested look-through change for one pane: bind to a
/// camera node, return to a free view, or lock the bound camera.
#[derive(Debug, Clone, Copy)]
pub(crate) enum LookThroughChange {
    Bind(u64),
    Free,
    /// Lock or unlock the bound camera to the view, so navigating the pane
    /// writes its pose back to the node.
    Lock(bool),
}

/// A toolbar's requested framing for one pane, from its Views label.
///
/// Named separately from the View menu's framing because the two differ in
/// who they move: the menu follows the camera link, and this moves the one
/// pane its label was drawn on.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PaneView {
    /// Frame the scene's visible bounds, keeping the current orientation.
    Fit,
    /// Snap to one of the six standard views.
    Axis(StandardView),
    /// Jump to a camera node's authored pose, without binding to it.
    ///
    /// Framing rather than a binding, which is why it lives here: jumping to
    /// a camera and working through one are different things, wanted at
    /// different moments, and conflating them is the easy mistake.
    Bookmark(SceneObjectId),
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
                            hdri_available,
                            uv_overlap_pct,
                            cameras: data.cameras,
                            bound_camera,
                            camera_locked: data.camera_locked.get(i).copied().unwrap_or(false),
                            turntable_rpm: data.turntable_rpm,
                            tracing_available: data.tracing_available,
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
    hdri_available: bool,
    uv_overlap_pct: Option<f32>,
    cameras: &'a [(u64, String)],
    bound_camera: Option<u64>,
    camera_locked: bool,
    turntable_rpm: f32,
    tracing_available: bool,
}

/// The view-mode label while a pane traces, and its menu row. The browser's
/// literal, held to it by the parity test below.
const PATH_TRACED: &str = "Path Traced";

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
    if cx.pane.pane_mode == PaneMode::Scene3D {
        draw_scene_labels(ui, cx, intents);
    } else {
        draw_uv_labels(ui, cx, intents);
    }
}

/// The seven labels a 3D pane carries, in order.
///
/// Each is its own label rather than a submenu of a neighbour, because a
/// label is the pane's status line as much as its menu: one label can only
/// report one of the settings folded behind it.
fn draw_scene_labels(ui: &mut egui::Ui, cx: PaneControls<'_>, intents: &mut Intents) {
    let PaneControls {
        index,
        pane,
        projection,
        bound_camera,
        tracing_available,
        ..
    } = cx;

    // The label reads the engine while the pane traces, since no view mode
    // is what it is showing. The four view-mode rows stay radio rows, and
    // each returns a traced pane to the rasterizer through the drain; the
    // traced row sits after them, and only when the device can build the
    // tracer, which is the browser's gate and the one entry drawn absent
    // rather than disabled: a device that cannot trace has nothing to
    // sequence.
    let traced = pane.pane_engine == solarxy_core::view_config::PaneEngine::Traced;
    let view_label = if traced {
        PATH_TRACED.to_string()
    } else {
        pane.view_mode.to_string()
    };
    label_menu(ui, (index, "vm"), &view_label, |ui| {
        for mode in ViewMode::ALL.iter().copied() {
            let checked = !traced && pane.view_mode == mode;
            if ui.radio(checked, mode.to_string()).clicked() && !checked {
                intents.pane(index, PaneChange::ViewMode(mode));
                ui.close();
            }
        }
        if tracing_available && ui.radio(traced, PATH_TRACED).clicked() && !traced {
            intents.pane(
                index,
                PaneChange::Engine(solarxy_core::view_config::PaneEngine::Traced),
            );
            ui.close();
        }
    });

    // The default falls back to the word rather than to `Shaded`, which the
    // view-mode label immediately to the left already says.
    let inspect = if pane.inspection_mode == InspectionMode::Shaded {
        "Inspect".to_string()
    } else {
        pane.inspection_mode.to_string()
    };
    label_menu(ui, (index, "insp"), &inspect, |ui| {
        if let Some(v) = radio_pick(
            ui,
            pane.inspection_mode,
            InspectionMode::ALL.iter().copied(),
            |v| v.to_string(),
        ) {
            intents.pane(index, PaneChange::InspectionMode(v));
        }
    });

    // Same rule: `Textured` is the absence of an override, not a state worth
    // a label.
    let overridden = if pane.material_override == MaterialOverride::None {
        "Override".to_string()
    } else {
        pane.material_override.to_string()
    };
    label_menu(ui, (index, "ovr"), &overridden, |ui| {
        if let Some(v) = radio_pick(
            ui,
            pane.material_override,
            MaterialOverride::ALL.iter().copied(),
            |v| v.to_string(),
        ) {
            intents.pane(index, PaneChange::MaterialOverride(v));
        }
    });

    let projection_label = match projection {
        ProjectionMode::Perspective => "Persp",
        ProjectionMode::Orthographic => "Ortho",
    };
    label_menu(ui, (index, "proj"), projection_label, |ui| {
        if let Some(mode) = radio_pick(ui, projection, PROJECTIONS, |p| p.to_string()) {
            intents.raise(Intent::PaneProjection { pane: index, mode });
        }
    });

    // The asterisk says the pane is a shot rather than a free view. Which
    // camera is in the menu, on the checked row.
    let camera_label = if bound_camera.is_some() {
        "Camera*"
    } else {
        "Camera"
    };
    label_menu(ui, (index, "cam"), camera_label, |ui| {
        draw_camera_menu(ui, cx, intents);
    });

    label_menu(ui, (index, "views"), "Views", |ui| {
        if ui.button("Fit view").clicked() {
            intents.raise(Intent::PaneView {
                pane: index,
                view: PaneView::Fit,
            });
            ui.close();
        }
        for (view, label) in AXIS_VIEWS {
            if ui.button(label).clicked() {
                intents.raise(Intent::PaneView {
                    pane: index,
                    view: PaneView::Axis(view),
                });
                ui.close();
            }
        }
    });

    label_menu(ui, (index, "disp"), "Display", |ui| {
        draw_display_menu(ui, cx, intents);
    });
}

/// A dropdown section heading, for the two menus the browser groups.
fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).weak());
}

/// Body of the camera label's menu.
///
/// Free view, the look-through list, the lock, the bookmark jumps and
/// create-from-view, in the browser's order.
fn draw_camera_menu(ui: &mut egui::Ui, cx: PaneControls<'_>, intents: &mut Intents) {
    let PaneControls {
        index,
        cameras,
        bound_camera,
        camera_locked,
        ..
    } = cx;
    if ui.radio(bound_camera.is_none(), "Free view").clicked() && bound_camera.is_some() {
        intents.raise(Intent::LookThrough {
            pane: index,
            change: LookThroughChange::Free,
        });
        ui.close();
    }
    if !cameras.is_empty() {
        heading(ui, "Look through");
        for (id, name) in cameras {
            if ui.radio(bound_camera == Some(*id), name).clicked() && bound_camera != Some(*id) {
                intents.raise(Intent::LookThrough {
                    pane: index,
                    change: LookThroughChange::Bind(*id),
                });
                ui.close();
            }
        }
    }

    // The lock is what gates writing a navigated pose back onto the camera
    // node, so it means nothing on a free view. The browser hides the entry
    // then; here it is drawn disabled with the reason, so the menu's shape
    // does not depend on the pane.
    let mut locked = camera_locked;
    let lock = ui
        .add_enabled(
            bound_camera.is_some(),
            egui::Checkbox::new(&mut locked, "Lock camera to view"),
        )
        .on_disabled_hover_text("Look through a camera to lock it to the view");
    if lock.changed() {
        intents.raise(Intent::LookThrough {
            pane: index,
            change: LookThroughChange::Lock(locked),
        });
    }

    // Guarded on there being a camera, unlike the browser's, whose heading
    // renders over an empty list in a scene with none.
    if !cameras.is_empty() {
        heading(ui, "Bookmarks");
        for (id, name) in cameras {
            if ui.button(format!("Jump to {name}")).clicked() {
                intents.raise(Intent::PaneView {
                    pane: index,
                    view: PaneView::Bookmark(SceneObjectId(*id)),
                });
                ui.close();
            }
        }
    }

    if ui.button("Create camera from view").clicked() {
        intents.raise(Intent::CreateCameraFromView { pane: index });
        ui.close();
    }
}

/// Body of the display label's menu: the look editor, the overlay toggles,
/// the turntable, the four mode submenus, the background list, and the way
/// into the image layout.
fn draw_display_menu(ui: &mut egui::Ui, cx: PaneControls<'_>, intents: &mut Intents) {
    let PaneControls {
        index,
        pane,
        hdri_available,
        turntable_rpm,
        ..
    } = cx;

    if ui
        .button("Look\u{2026}")
        .on_hover_text("Exposure, tone mapping and the grade, for this pane")
        .clicked()
    {
        intents.raise(Intent::PaneLook(PaneLookIntent::Open(index)));
        ui.close();
    }

    for (current, label, make) in [
        (
            pane.show_grid,
            "Grid",
            PaneChange::ShowGrid as fn(bool) -> PaneChange,
        ),
        (pane.show_axis_gizmo, "Axes", PaneChange::ShowAxisGizmo),
        (
            pane.show_light_markers,
            "Light markers",
            PaneChange::ShowLightMarkers,
        ),
    ] {
        if let Some(v) = check(ui, current, label, "") {
            intents.pane(index, make(v));
        }
    }

    if let Some(v) = check(ui, pane.show_validation, "Validation overlay", "") {
        intents.pane(index, PaneChange::ShowValidation(v));
    }
    if let Some(v) = check(ui, pane.turntable_active, "Turntable", "") {
        intents.pane(index, PaneChange::TurntableActive(v));
    }
    ui.menu_button(
        format!("Turntable speed: {}", turntable_speed_label(turntable_rpm)),
        |ui| {
            for (label, rpm) in TURNTABLE_SPEEDS {
                if ui.button(label).clicked() {
                    intents.raise(Intent::Display(DisplayChange::TurntableRpm(rpm)));
                    ui.close();
                }
            }
        },
    );
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
    ui.menu_button("Bounds", |ui| {
        if let Some(v) = radio_pick(ui, pane.bounds_mode, BoundsMode::ALL.iter().copied(), |v| {
            v.to_string()
        }) {
            intents.pane(index, PaneChange::BoundsMode(v));
        }
    });
    ui.menu_button("Wireframe", |ui| {
        if let Some(v) = radio_pick(ui, pane.line_weight, LineWeight::ALL.iter().copied(), |v| {
            v.descriptive_label()
        }) {
            intents.pane(index, PaneChange::LineWeight(v));
        }
    });
    ui.menu_button("Background", |ui| {
        if let Some(v) = background_menu_body(ui, pane.background_mode, hdri_available) {
            intents.pane(index, PaneChange::BackgroundMode(v));
        }
    });
    if ui.button("UV Layout").clicked() {
        intents.pane(index, PaneChange::PaneMode(PaneMode::UvMap));
        ui.close();
    }
}

/// A UV pane's two labels plus its overlap readout.
///
/// The readout sits beside the labels rather than inside one, which is what
/// lets it say `computing...` while a readback is in flight: a label that
/// only appears once a number exists cannot report that it is waiting.
fn draw_uv_labels(ui: &mut egui::Ui, cx: PaneControls<'_>, intents: &mut Intents) {
    let PaneControls {
        index,
        pane,
        uv_overlap_pct,
        ..
    } = cx;

    // Inert. The way out is the Display menu's exit entry.
    ui.label("[ UV Map ]");

    label_menu(ui, (index, "uvd"), "Display", |ui| {
        heading(ui, "Background");
        if let Some(v) = radio_pick(ui, pane.uv_bg, UV_BACKGROUNDS, |v| v.to_string()) {
            intents.pane(index, PaneChange::UvBackground(v));
        }
        heading(ui, "Overlays");
        if let Some(v) = check(
            ui,
            pane.show_uv_overlap,
            "UV overlap",
            "UV shell overlap heatmap",
        ) {
            intents.pane(index, PaneChange::ShowUvOverlap(v));
        }
        if ui.button("Exit UV Layout").clicked() {
            intents.pane(index, PaneChange::PaneMode(PaneMode::Scene3D));
            ui.close();
        }
    });

    if pane.show_uv_overlap {
        let text = uv_overlap_pct.map_or_else(
            || "computing...".to_string(),
            |pct| format!("{pct:.1}% overlap"),
        );
        ui.label(text).on_hover_text("UV overlap percentage");
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

/// Why `HDRI Sky` cannot be picked yet. Shown on the disabled entry.
const NO_HDRI: &str = "Load an HDRI in the Environment dialog first";

/// Body of the Background submenu: the builtins, which are the whole list,
/// as they are in the browser.
///
/// `HDRI Sky` is always listed and cannot be picked until an HDRI is loaded.
/// It is disabled rather than dropped so the list is the same six whatever
/// the scene holds, and so the entry can say what it is waiting for.
fn background_menu_body(
    ui: &mut egui::Ui,
    current: BackgroundMode,
    hdri_available: bool,
) -> Option<BackgroundMode> {
    let mut picked = None;
    for &builtin in BuiltinBg::ALL {
        let mode = BackgroundMode::Builtin(builtin);
        let offered = builtin != BuiltinBg::HdriSky || hdri_available;
        if ui
            .add_enabled(
                offered,
                egui::RadioButton::new(current == mode, builtin.to_string()),
            )
            .on_disabled_hover_text(NO_HDRI)
            .clicked()
            && current != mode
        {
            picked = Some(mode);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The browser's per-pane background list, as `(stored name, label)`
    /// pairs in its order, read from the table its menu is drawn from.
    fn browser_backgrounds() -> Vec<(String, String)> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/components/PaneToolbar.tsx");
        let source = std::fs::read_to_string(&path).expect("the browser's pane toolbar reads");
        let table = source
            .split_once("const BACKGROUNDS = [")
            .and_then(|(_, rest)| rest.split_once("] as const;"))
            .map(|(table, _)| table)
            .expect("the browser's pane toolbar has its background table");
        table
            .lines()
            .filter_map(|line| {
                let mut quoted = line.split('"').skip(1).step_by(2);
                Some((quoted.next()?.to_string(), quoted.next()?.to_string()))
            })
            .collect()
    }

    /// The Background submenu lists what the browser's does: the same
    /// entries, under the same labels, in the same order. The stored name
    /// rides along so a label cannot be right on the wrong entry.
    #[test]
    fn the_background_list_is_the_browsers() {
        let browser = browser_backgrounds();
        assert_eq!(browser.len(), 6, "the reader found the browser's six");
        let here: Vec<(String, String)> = BuiltinBg::ALL
            .iter()
            .map(|builtin| {
                let stored = serde_json::to_string(builtin).expect("a builtin serializes");
                (stored.trim_matches('"').to_string(), builtin.to_string())
            })
            .collect();
        assert_eq!(here, browser);
    }

    fn browser_pane_toolbar() -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/src/components/PaneToolbar.tsx");
        std::fs::read_to_string(&path).expect("the browser's pane toolbar reads")
    }

    /// The browser's view-mode list, as `(stored name, label)` pairs in its
    /// order, read from the table its menu is drawn from.
    fn browser_view_modes(source: &str) -> Vec<(String, String)> {
        let table = source
            .split_once("const VIEW_MODES = [")
            .and_then(|(_, rest)| rest.split_once("] as const;"))
            .map(|(table, _)| table)
            .expect("the browser's pane toolbar has its view-mode table");
        table
            .lines()
            .filter_map(|line| {
                let mut quoted = line.split('"').skip(1).step_by(2);
                Some((quoted.next()?.to_string(), quoted.next()?.to_string()))
            })
            .collect()
    }

    /// The view-mode menu lists what the browser's does, in its order and
    /// under its labels, and the traced row that follows them carries the
    /// browser's literal. The traced row is not in the browser's table: it
    /// is a separate row under the same label, present only when the device
    /// can trace, so it is held here by its literal rather than by a table.
    #[test]
    fn the_view_mode_list_is_the_browsers() {
        let source = browser_pane_toolbar();
        let browser = browser_view_modes(&source);
        assert_eq!(browser.len(), 4, "the reader found the browser's four");
        let here: Vec<(String, String)> = ViewMode::ALL
            .iter()
            .map(|mode| {
                let stored = serde_json::to_string(mode).expect("a view mode serializes");
                (stored.trim_matches('"').to_string(), mode.to_string())
            })
            .collect();
        assert_eq!(here, browser);
        assert!(
            source.contains(&format!("label=\"{PATH_TRACED}\"")),
            "the browser's traced row carries the same label"
        );
    }
}
