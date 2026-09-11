//! Native-style menu bar — the Houdini-inspired layout:
//! `File / Edit / Render / Review / View / Layout / Window / Help`.
//! `Review` sits between `Render` and `View` — it is a viewport mode.
//!
//! Per-pane controls (Shading, Inspection, Material Override, Background,
//! Projection) act on the **active pane**, so a menu click applies to
//! whichever pane the cursor last selected. Scene-global controls
//! (post-processing, IBL, turntable) raise scene-global intents.
//!
//! Everything that is an action rather than a setting is raised as an
//! [`Intent`] and applied after the pass. The Window menu is a table of
//! panel rows rather than a struct with a flag per panel, so adding a panel
//! is adding a row.

use solarxy_core::preferences::{
    BackgroundMode, BuiltinBg, CustomBackground, IblMode, InspectionMode, LineWeight,
    MaterialOverride, NormalsMode, PaneMode, ProjectionMode, ToneMode, UvMode, ViewMode,
};
use crate::state::view_state::{BoundsMode, ViewLayout};

use crate::gui::MOD;
use crate::gui::dock::SolarxyTab;
use crate::gui::intent::{
    CaptureIntent, EditIntent, FileIntent, HelpIntent, Intent, Intents, LayoutIntent, ReviewIntent,
};
use crate::gui::intent::{DisplayChange, PaneChange, PostChange};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;

/// A checkbox with a shortcut hint, returning the new value when the user
/// flipped it.
fn checkbox(ui: &mut egui::Ui, current: bool, label: &str, hover: &str) -> Option<bool> {
    let mut value = current;
    let changed = ui
        .checkbox(&mut value, label)
        .on_hover_text(hover)
        .changed();
    changed.then_some(value)
}

/// What the menu bar needs to know about the shell to draw itself, as
/// against what it asks the shell to do, which travels as an [`Intent`].
#[derive(Clone, Copy)]
pub(in crate::gui) struct MenuContext<'a> {
    pub has_model: bool,
    pub still_renderable: bool,
    pub recent_files: &'a [String],
    pub hdri_available: bool,
    pub customs: &'a [CustomBackground],
    pub review_available: bool,
    pub review_active: bool,
    pub review_markers_hidden: bool,
    pub review_dirty: bool,
    pub menu_bar_visible: bool,
    pub status_bar_visible: bool,
    pub has_saved_layout: bool,
    pub theme: Theme,
}

pub(in crate::gui) fn draw_menu_bar(
    ctx: &egui::Context,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    present: &dyn Fn(SolarxyTab) -> bool,
    cx: MenuContext<'_>,
) {
    egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            draw_file_menu(ui, intents, cx.has_model, cx.recent_files);
            draw_edit_menu(ui, intents);
            draw_render_menu(ui, settings, intents, cx);
            // Review is a viewport mode, so it belongs between Render and
            // View rather than stranded out past Help.
            draw_review_menu(ui, intents, cx);
            draw_view_menu(ui, settings, intents);
            draw_layout_menu(ui, intents, cx.has_saved_layout);
            draw_window_menu(ui, intents, present, cx);
            draw_help_menu(ui, intents);
            draw_cook_strip(ui, settings.cook, intents);
        });
    });
}

/// The cook strip at the right end of the bar: the mode toggle, and in manual
/// mode the stale count and the Cook button. It lives here rather than in the
/// status bar because the status bar retires this release and the header is
/// where the browser keeps the same three controls.
///
/// Widgets are added right to left, so the first one added is the rightmost.
fn draw_cook_strip(ui: &mut egui::Ui, cook: crate::gui::CookReadout, intents: &mut Intents) {
    use solarxy_graph::engine::CookMode;

    if !cook.open {
        return;
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let manual = cook.mode == CookMode::Manual;
        if manual
            && ui
                .add_enabled(cook.can_cook(), egui::Button::new("Cook"))
                .on_hover_text(format!(
                    "Cook the stale nodes now ({}+Enter)",
                    crate::gui::MOD
                ))
                .on_disabled_hover_text("Nothing is stale, or a cook is already working")
                .clicked()
        {
            intents.raise(Intent::Cook(crate::gui::CookIntent::CookNow));
        }
        if let Some(label) = cook.status_label() {
            ui.label(egui::RichText::new(label).small());
        }
        let toggle = egui::Button::new(if manual { "Manual" } else { "Auto" }).selected(manual);
        if ui
            .add(toggle)
            .on_hover_text(
                "Cook mode. Manual holds the cook until you ask for it, which is \
                 how a heavy graph stays editable.",
            )
            .clicked()
        {
            let next = if manual {
                CookMode::Auto
            } else {
                CookMode::Manual
            };
            intents.raise(Intent::Cook(crate::gui::CookIntent::SetMode(next)));
        }
    });
}

/// `Review` menu, sat between `Render` and `View` — Review is a viewport
/// mode, not a utility. The button label turns amber `● Review` while
/// review mode is active.
fn draw_review_menu(ui: &mut egui::Ui, intents: &mut Intents, cx: MenuContext<'_>) {
    let MenuContext {
        review_available,
        review_active,
        review_markers_hidden,
        review_dirty,
        theme,
        ..
    } = cx;
    let label: egui::WidgetText = if review_active {
        egui::RichText::new("\u{25CF} Review")
            .color(theme.accent)
            .into()
    } else {
        "Review".into()
    };
    ui.menu_button(label, |ui| {
        // Both viewport affordances are honest about the open document: the
        // desktop's review anchors against a file-loaded model, so a scene
        // (or nothing) disables them rather than discarding clicks. The mode
        // toggle stays enabled while active, so it can always be turned off.
        if ui
            .add_enabled(
                review_available || review_active,
                egui::Button::selectable(review_active, "Review Mode"),
            )
            .on_hover_text("Shift+R")
            .on_disabled_hover_text("Review needs an open model file")
            .clicked()
        {
            intents.raise(Intent::Review(ReviewIntent::ToggleMode));
            ui.close();
        }
        if ui
            .add_enabled(
                review_available,
                egui::Button::selectable(!review_markers_hidden, "Show Markers"),
            )
            .on_hover_text("Show review markers in the viewport")
            .on_disabled_hover_text("Review needs an open model file")
            .clicked()
        {
            intents.raise(Intent::Review(ReviewIntent::ToggleMarkers));
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(review_dirty, egui::Button::new("Save Review Notes"))
            .on_hover_text("Write review notes to the sidecar file (Cmd/Ctrl+S)")
            .clicked()
        {
            intents.raise(Intent::Review(ReviewIntent::SaveNotes));
            ui.close();
        }
    });
}

/// Submenu of `T::ALL`-style variants as selectable rows, returning the one
/// the user picked. Covers every plain enum-pick submenu; the Inspection and
/// Projection submenus stay inline, because they do more than pick.
fn variant_submenu<T: PartialEq + Copy + std::fmt::Display>(
    ui: &mut egui::Ui,
    label: &str,
    hover: &str,
    current: T,
    all: &[T],
) -> Option<T> {
    let mut picked = None;
    ui.menu_button(label, |ui| {
        for &variant in all {
            if ui
                .selectable_label(current == variant, variant.to_string())
                .clicked()
            {
                picked = Some(variant);
                ui.close();
            }
        }
    })
    .response
    .on_hover_text(hover);
    picked
}

/// Background submenu — builtins (`HDRI Sky` gated on an HDRI being
/// loaded) then, under a separator, every user custom background.
fn background_submenu(
    ui: &mut egui::Ui,
    current: BackgroundMode,
    customs: &[CustomBackground],
    hdri_available: bool,
) -> Option<BackgroundMode> {
    let mut picked = None;
    ui.menu_button("Background", |ui| {
        for &builtin in BuiltinBg::ALL {
            if builtin == BuiltinBg::HdriSky && !hdri_available {
                continue;
            }
            let mode = BackgroundMode::Builtin(builtin);
            if ui
                .selectable_label(current == mode, builtin.to_string())
                .clicked()
            {
                picked = Some(mode);
                ui.close();
            }
        }
        if !customs.is_empty() {
            ui.separator();
            for custom in customs {
                let mode = BackgroundMode::Custom(custom.id);
                if ui.selectable_label(current == mode, &custom.name).clicked() {
                    picked = Some(mode);
                    ui.close();
                }
            }
        }
    })
    .response
    .on_hover_text("B");
    picked
}

fn draw_file_menu(
    ui: &mut egui::Ui,
    intents: &mut Intents,
    has_model: bool,
    recent_files: &[String],
) {
    ui.menu_button("File", |ui| {
        if ui
            .add(egui::Button::new("New Scene").shortcut_text(format!("{MOD}+N")))
            .clicked()
        {
            intents.raise(Intent::File(FileIntent::NewScene));
            ui.close();
        }
        if ui
            .add(egui::Button::new("Open\u{2026}").shortcut_text(format!("{MOD}+O")))
            .clicked()
        {
            intents.raise(Intent::File(FileIntent::OpenModel));
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(
                has_model,
                egui::Button::new("Save Scene").shortcut_text(format!("{MOD}+S")),
            )
            .clicked()
        {
            intents.raise(Intent::File(FileIntent::Save));
            ui.close();
        }
        if ui
            .add_enabled(
                has_model,
                egui::Button::new("Save Scene As\u{2026}").shortcut_text(format!("{MOD}+Shift+S")),
            )
            .clicked()
        {
            intents.raise(Intent::File(FileIntent::SaveAs));
            ui.close();
        }
        ui.separator();
        if ui
            .add(egui::Button::new("Import HDRI\u{2026}").shortcut_text(format!("{MOD}+Shift+O")))
            .clicked()
        {
            intents.raise(Intent::File(FileIntent::OpenHdri));
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
                        intents.raise(Intent::File(FileIntent::OpenRecent(path.clone())));
                        ui.close();
                    }
                }
            });
        }
        ui.separator();
        if ui
            .add_enabled(has_model, egui::Button::new("Close"))
            .clicked()
        {
            intents.raise(Intent::File(FileIntent::Close));
            ui.close();
        }
        if ui.button("Quit").clicked() {
            intents.raise(Intent::File(FileIntent::Quit));
            ui.close();
        }
    });
}

fn draw_edit_menu(ui: &mut egui::Ui, intents: &mut Intents) {
    ui.menu_button("Edit", |ui| {
        if ui
            .add(egui::Button::new("Preferences\u{2026}").shortcut_text(format!("{MOD}+,")))
            .clicked()
        {
            intents.raise(Intent::Edit(EditIntent::OpenPreferences));
            ui.close();
        }
        ui.separator();
        if ui
            .button("Save View Settings as Default")
            .on_hover_text("Persist the current display, rendering and lighting settings")
            .clicked()
        {
            intents.raise(Intent::Edit(EditIntent::SaveViewDefaults));
            ui.close();
        }
    });
}

fn draw_render_menu(
    ui: &mut egui::Ui,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    cx: MenuContext<'_>,
) {
    let MenuContext {
        still_renderable,
        hdri_available,
        customs,
        ..
    } = cx;
    let active = settings.active;
    let pane = settings.active_pane();
    ui.menu_button("Render", |ui| {
        // The still renders either root through the render node's
        // authority: a scene the engine cooked, or an open model through
        // the same synthesized document the terminal renders.
        if ui
            .add_enabled(still_renderable, egui::Button::new("Render Still\u{2026}"))
            .on_disabled_hover_text("Open a scene or a model to render a still")
            .clicked()
        {
            intents.raise(Intent::Capture(CaptureIntent::Still));
            ui.close();
        }
        ui.separator();

        // The cook, for discoverability; the header strip is the working
        // surface. Both raise the same intents.
        let cook = settings.cook;
        let manual = cook.mode == solarxy_graph::engine::CookMode::Manual;
        if ui
            .add_enabled(
                cook.open,
                egui::Button::new("Auto Cook").selected(cook.open && !manual),
            )
            .on_hover_text("Cook stale nodes as edits land. Off, the cook waits for Cook Now.")
            .clicked()
        {
            let next = if manual {
                solarxy_graph::engine::CookMode::Auto
            } else {
                solarxy_graph::engine::CookMode::Manual
            };
            intents.raise(Intent::Cook(crate::gui::CookIntent::SetMode(next)));
            ui.close();
        }
        if ui
            .add_enabled(
                cook.can_cook(),
                egui::Button::new("Cook Now").shortcut_text(format!("{}+Enter", crate::gui::MOD)),
            )
            .on_disabled_hover_text("In manual cook mode, cooks what is stale")
            .clicked()
        {
            intents.raise(Intent::Cook(crate::gui::CookIntent::CookNow));
            ui.close();
        }
        ui.separator();

        if let Some(v) = variant_submenu(ui, "Shading", "W", pane.view_mode, ViewMode::ALL) {
            intents.pane(active, PaneChange::ViewMode(v));
        }

        ui.menu_button("Inspection", |ui| {
            for mode in InspectionMode::ALL {
                let selected = pane.pane_mode == PaneMode::Scene3D && pane.inspection_mode == *mode;
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
                    intents.pane(active, PaneChange::InspectionMode(*mode));
                    intents.pane(active, PaneChange::PaneMode(PaneMode::Scene3D));
                    ui.close();
                }
            }
            let uv_selected = pane.pane_mode == PaneMode::UvMap;
            if ui
                .selectable_label(uv_selected, "UV Map")
                .on_hover_text("3")
                .clicked()
            {
                intents.pane(active, PaneChange::PaneMode(PaneMode::UvMap));
                ui.close();
            }
        });

        if let Some(v) = variant_submenu(
            ui,
            "Material Override",
            "M / Shift+M",
            pane.material_override,
            MaterialOverride::ALL,
        ) {
            intents.pane(active, PaneChange::MaterialOverride(v));
        }

        ui.separator();

        if let Some(v) = variant_submenu(
            ui,
            "Tone Mapping",
            "Shift+T",
            settings.post.tone_mode,
            ToneMode::ALL,
        ) {
            intents.raise(Intent::Post(PostChange::ToneMode(v)));
        }
        if let Some(v) = checkbox(ui, settings.post.bloom_enabled, "Bloom", "Shift+D") {
            intents.raise(Intent::Post(PostChange::Bloom(v)));
        }
        if let Some(v) = checkbox(ui, settings.post.ssao_enabled, "SSAO", "Shift+O") {
            intents.raise(Intent::Post(PostChange::Ssao(v)));
        }

        ui.separator();

        ui.menu_button("Lighting", |ui| {
            if let Some(v) = variant_submenu(
                ui,
                "IBL Mode",
                "I / Shift+I",
                settings.ibl_mode,
                IblMode::ALL,
            ) {
                intents.raise(Intent::Ibl(v));
            }
            if let Some(v) = checkbox(ui, settings.display.lights_locked, "Lock Lights", "Shift+L")
            {
                intents.raise(Intent::Display(DisplayChange::LightsLocked(v)));
            }
        });
        if let Some(v) = background_submenu(ui, pane.background_mode, customs, hdri_available) {
            intents.pane(active, PaneChange::BackgroundMode(v));
        }

        ui.separator();

        if ui
            .add(egui::Button::new("Save Screenshot\u{2026}").shortcut_text("C"))
            .clicked()
        {
            intents.raise(Intent::Capture(CaptureIntent::Screenshot));
            ui.close();
        }
    });
}

fn draw_view_menu(ui: &mut egui::Ui, settings: PanelSettings<'_>, intents: &mut Intents) {
    let active = settings.active;
    let pane = settings.active_pane();
    ui.menu_button("View", |ui| {
        ui.menu_button("Projection", |ui| {
            for (mode, shortcut) in [
                (ProjectionMode::Perspective, "P"),
                (ProjectionMode::Orthographic, "O"),
            ] {
                if ui
                    .selectable_label(settings.projection_mode == mode, mode.to_string())
                    .on_hover_text(shortcut)
                    .clicked()
                {
                    intents.raise(Intent::Projection(mode));
                    ui.close();
                }
            }
        });
        if let Some(v) = checkbox(ui, settings.display.turntable_active, "Turntable", "V") {
            intents.raise(Intent::Display(DisplayChange::TurntableActive(v)));
        }
        if settings.is_split
            && let Some(v) = checkbox(
                ui,
                settings.cameras_linked,
                "Link Cameras",
                &format!("{MOD}+L"),
            )
        {
            intents.raise(Intent::LinkCameras(v));
        }

        ui.separator();

        ui.menu_button("Show", |ui| {
            for (current, label, hover, make) in [
                (
                    pane.show_grid,
                    "Grid",
                    "G",
                    PaneChange::ShowGrid as fn(bool) -> PaneChange,
                ),
                (
                    pane.show_axis_gizmo,
                    "Axis Gizmo",
                    "A",
                    PaneChange::ShowAxisGizmo,
                ),
                (
                    pane.show_local_axes,
                    "Local Axes",
                    "Shift+A",
                    PaneChange::ShowLocalAxes,
                ),
                (
                    pane.show_validation,
                    "Validation Overlay",
                    "Shift+V",
                    PaneChange::ShowValidation,
                ),
            ] {
                if let Some(v) = checkbox(ui, current, label, hover) {
                    intents.pane(active, make(v));
                }
            }
            ui.separator();
            if let Some(v) =
                variant_submenu(ui, "Normals", "N", pane.normals_mode, NormalsMode::ALL)
            {
                intents.pane(active, PaneChange::NormalsMode(v));
            }
            if let Some(v) = variant_submenu(ui, "UV Overlay", "U", pane.uv_mode, UvMode::ALL) {
                intents.pane(active, PaneChange::UvMode(v));
            }
            if let Some(v) =
                variant_submenu(ui, "Bounds", "Shift+B", pane.bounds_mode, BoundsMode::ALL)
            {
                intents.pane(active, PaneChange::BoundsMode(v));
            }
            if let Some(v) = variant_submenu(
                ui,
                "Wireframe Weight",
                "Shift+W",
                pane.line_weight,
                LineWeight::ALL,
            ) {
                intents.pane(active, PaneChange::LineWeight(v));
            }
        });
    });
}

fn draw_layout_menu(ui: &mut egui::Ui, intents: &mut Intents, has_saved_layout: bool) {
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
                intents.raise(Intent::Layout(LayoutIntent::SetLayout(layout)));
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Save Layout").clicked() {
            intents.raise(Intent::Layout(LayoutIntent::SaveDock));
            ui.close();
        }
        if ui
            .add_enabled(has_saved_layout, egui::Button::new("Restore Saved Layout"))
            .clicked()
        {
            intents.raise(Intent::Layout(LayoutIntent::RestoreDock));
            ui.close();
        }
        if ui.button("Reset Layout to Default").clicked() {
            intents.raise(Intent::Layout(LayoutIntent::ResetDock));
            ui.close();
        }
    });
}

/// A panel's row in the Window menu.
///
/// **This table is the registration a new panel needs.** Before it, every
/// panel cost a field on a shared visibility struct, a line building that
/// struct, a line in a diff table, and a line applying the diff. Now it costs
/// a row, and the tick state is read from the dock rather than mirrored.
struct PanelRow {
    tab: SolarxyTab,
    label: &'static str,
    accel: Accel,
    /// Panels that inspect an imported file are disabled without one.
    needs_model: bool,
}

/// A row's accelerator. `Mod` is separate because the platform key is a
/// runtime string, so the label cannot be a plain constant.
enum Accel {
    None,
    Key(&'static str),
    Mod(&'static str),
}

impl Accel {
    fn label(&self) -> Option<String> {
        match self {
            Self::None => None,
            Self::Key(k) => Some((*k).to_string()),
            Self::Mod(k) => Some(format!("{MOD}+{k}")),
        }
    }
}

const PANEL_ROWS: &[PanelRow] = &[
    PanelRow {
        tab: SolarxyTab::Viewport,
        label: "Viewport",
        accel: Accel::Mod("1"),
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::Sidebar,
        label: "Sidebar",
        accel: Accel::Key("Tab"),
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::Outliner,
        label: "Outliner",
        accel: Accel::None,
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::NodeTree,
        label: "Node Tree",
        accel: Accel::None,
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::Nodes,
        label: "Nodes",
        accel: Accel::None,
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::Parameters,
        label: "Parameters",
        accel: Accel::None,
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::Properties,
        label: "Properties",
        accel: Accel::None,
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::ReviewPanel,
        label: "Review Panel",
        accel: Accel::None,
        needs_model: false,
    },
    PanelRow {
        tab: SolarxyTab::MaterialInspector,
        label: "Material Inspector",
        accel: Accel::None,
        needs_model: true,
    },
    PanelRow {
        tab: SolarxyTab::Console,
        label: "Console",
        accel: Accel::Key("`"),
        needs_model: false,
    },
];

fn draw_window_menu(
    ui: &mut egui::Ui,
    intents: &mut Intents,
    present: &dyn Fn(SolarxyTab) -> bool,
    cx: MenuContext<'_>,
) {
    ui.menu_button("Window", |ui| {
        for row in PANEL_ROWS {
            let mut button = egui::Button::new(row.label).selected(present(row.tab));
            if let Some(accel) = row.accel.label() {
                button = button.shortcut_text(accel);
            }
            if ui
                .add_enabled(!row.needs_model || cx.has_model, button)
                .clicked()
            {
                intents.raise(Intent::Layout(LayoutIntent::ToggleTab(row.tab)));
                ui.close();
            }
        }

        ui.separator();

        // The status bar and the menu bar are chrome the shell owns rather
        // than panels the dock holds, so they are written out rather than
        // squeezed into the table above.
        if ui
            .add(egui::Button::new("Status Bar").selected(cx.status_bar_visible))
            .clicked()
        {
            intents.raise(Intent::Layout(LayoutIntent::ToggleStatusBar));
            ui.close();
        }
        if ui
            .add(
                egui::Button::new("Menu Bar")
                    .selected(cx.menu_bar_visible)
                    .shortcut_text("F10"),
            )
            .clicked()
        {
            intents.raise(Intent::Layout(LayoutIntent::ToggleMenuBar));
            ui.close();
        }
    });
}

fn draw_help_menu(ui: &mut egui::Ui, intents: &mut Intents) {
    ui.menu_button("Help", |ui| {
        if ui.button("Solarxy Wiki").clicked() {
            intents.raise(Intent::Help(HelpIntent::OpenWiki));
            ui.close();
        }
        if ui
            .add(egui::Button::new("Keyboard Shortcuts").shortcut_text("?"))
            .clicked()
        {
            intents.raise(Intent::Help(HelpIntent::OpenShortcuts));
            ui.close();
        }
        ui.separator();
        if ui.button("Check for Updates\u{2026}").clicked() {
            intents.raise(Intent::Help(HelpIntent::CheckForUpdates));
            ui.close();
        }
        if ui.button("About Solarxy").clicked() {
            intents.raise(Intent::Help(HelpIntent::OpenAbout));
            ui.close();
        }
    });
}
