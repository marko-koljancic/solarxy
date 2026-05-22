//! The Outliner panel — a docked tab listing the loaded model's **Meshes**
//! and **Materials**. Each row has a visibility checkbox; clicking a row
//! name frames the active camera on it. Mesh rows also carry a right-click
//! context menu (Frame / Hide / Isolate / Show All).
//!
//! Lights / cameras are intentionally absent — the glTF loader parses
//! neither, and the three scene lights are a synthetic camera-relative
//! rig, not scene data (deferred to a later release).
//!
//! The panel is read-only over `&Model`; mutations are raised as
//! [`OutlinerAction`]s in [`OutlinerEvents`], drained by `state/render.rs`
//! after the egui pass.

use solarxy_renderer::model::Model;

/// One Outliner interaction, raised during an egui pass.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OutlinerAction {
    /// Frame the active camera on a mesh's bounds.
    FrameMesh(usize),
    /// Toggle a single mesh's visibility.
    ToggleMesh(usize),
    /// Hide a single mesh (context menu).
    HideMesh(usize),
    /// Hide every mesh except this one (context menu).
    IsolateMesh(usize),
    /// Make every mesh visible (context menu).
    ShowAll,
    /// Toggle visibility of every mesh using a material.
    ToggleMaterial(usize),
    /// Frame the active camera on the union of a material's meshes.
    FrameMaterial(usize),
}

/// Outliner out-events, drained by `state/render.rs` after `render_ui`.
/// At most one action per frame — the user clicks one thing.
#[derive(Debug, Default)]
pub(crate) struct OutlinerEvents {
    pub action: Option<OutlinerAction>,
}

/// Render the Outliner content into `ui` (the `egui_dock` `Outliner` tab
/// supplies the `Ui`).
pub(super) fn draw_outliner_content(
    ui: &mut egui::Ui,
    model: Option<&Model>,
    events: &mut OutlinerEvents,
) {
    let Some(model) = model else {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("No model loaded").weak());
        });
        return;
    };

    // Bulk-visibility action. "Show All" previously lived only in a
    // right-click menu and was undiscoverable — a persistent button is
    // the obvious recovery path after hiding meshes.
    let any_hidden = model.meshes.iter().any(|m| !m.visible);
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(any_hidden, egui::Button::new("Show All Meshes").small())
            .on_hover_text("Make every mesh visible (Alt+H)")
            .clicked()
        {
            events.action = Some(OutlinerAction::ShowAll);
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);

        if !model.meshes.is_empty() {
            egui::CollapsingHeader::new(format!("Meshes ({})", model.meshes.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for (i, mesh) in model.meshes.iter().enumerate() {
                        draw_mesh_row(ui, i, mesh, model, events);
                    }
                });
            ui.separator();
        }

        if !model.materials.is_empty() {
            egui::CollapsingHeader::new(format!("Materials ({})", model.materials.len()))
                .default_open(true)
                .show(ui, |ui| {
                    for (m, material) in model.materials.iter().enumerate() {
                        draw_material_row(ui, m, &material.name, model, events);
                    }
                });
        }

        ui.add_space(8.0);
    });
}

/// Display name for a mesh — falls back to `Mesh N` for unnamed meshes
/// (common in OBJ files without `o`/`g` groups).
fn mesh_display_name(mesh: &solarxy_renderer::model::Mesh, idx: usize) -> String {
    if mesh.name.trim().is_empty() {
        format!("Mesh {idx}")
    } else {
        mesh.name.clone()
    }
}

fn draw_mesh_row(
    ui: &mut egui::Ui,
    idx: usize,
    mesh: &solarxy_renderer::model::Mesh,
    model: &Model,
    events: &mut OutlinerEvents,
) {
    let row = ui.horizontal(|ui| {
        let mut visible = mesh.visible;
        if ui
            .checkbox(&mut visible, "")
            .on_hover_text("Toggle mesh visibility")
            .changed()
        {
            events.action = Some(OutlinerAction::ToggleMesh(idx));
        }

        let name = mesh_display_name(mesh, idx);
        if ui
            .selectable_label(false, name)
            .on_hover_text("Click to frame \u{2014} right-click for actions")
            .clicked()
        {
            events.action = Some(OutlinerAction::FrameMesh(idx));
        }

        if let Some(material) = model.materials.get(mesh.material)
            && !material.name.trim().is_empty()
        {
            ui.label(egui::RichText::new(&material.name).weak());
        }
    });

    // Context menu on the whole row, not just the name label — the RC2
    // build attached it to the label only, so a right-click anywhere
    // else on the row did nothing.
    row.response.context_menu(|ui| {
        if ui.button("Frame").clicked() {
            events.action = Some(OutlinerAction::FrameMesh(idx));
            ui.close();
        }
        if ui.button("Hide").clicked() {
            events.action = Some(OutlinerAction::HideMesh(idx));
            ui.close();
        }
        if ui.button("Isolate").clicked() {
            events.action = Some(OutlinerAction::IsolateMesh(idx));
            ui.close();
        }
        if ui.button("Show All").clicked() {
            events.action = Some(OutlinerAction::ShowAll);
            ui.close();
        }
    });
}

fn draw_material_row(
    ui: &mut egui::Ui,
    idx: usize,
    name: &str,
    model: &Model,
    events: &mut OutlinerEvents,
) {
    let mesh_count = model.meshes.iter().filter(|m| m.material == idx).count();
    let all_visible = model
        .meshes
        .iter()
        .filter(|m| m.material == idx)
        .all(|m| m.visible);

    ui.horizontal(|ui| {
        let mut visible = all_visible;
        if ui
            .checkbox(&mut visible, "")
            .on_hover_text("Toggle visibility of every mesh using this material")
            .changed()
        {
            events.action = Some(OutlinerAction::ToggleMaterial(idx));
        }

        let label = if name.trim().is_empty() {
            format!("Material {idx}")
        } else {
            name.to_string()
        };
        let resp = ui
            .selectable_label(false, label)
            .on_hover_text("Click to frame this material's meshes");
        if resp.clicked() {
            events.action = Some(OutlinerAction::FrameMaterial(idx));
        }

        ui.label(egui::RichText::new(format!("({mesh_count})")).weak());
    });
}
