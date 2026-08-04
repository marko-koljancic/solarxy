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

use solarxy_core::scene::SceneObjectId;
use solarxy_renderer::model::Model;
use solarxy_renderer::scene_objects::SceneObjects;

/// One Outliner interaction, raised during an egui pass.
///
/// The mutating variants address the **file-loaded model**, whose meshes
/// the app owns outright. A scene's geometry belongs to the engine and is
/// re-emitted on every cook, so the only visibility a scene object can
/// offer durably is object-level, and it travels as a parameter change
/// rather than as one of these. See `ToggleObject` below.
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
    /// Frame the active camera on one scene object's bounds.
    FrameObject(SceneObjectId),
    /// Frame the active camera on one mesh inside a scene object.
    FrameObjectMesh(SceneObjectId, usize),
    /// Flip a scene object's visibility.
    ///
    /// Lowered to a `visible` parameter change on the owning node, not to a
    /// write on the renderer's copy: the scene delta re-emits every
    /// object's render flags on each cook, so a direct write would be
    /// undone by the user's next parameter edit.
    ToggleObject(SceneObjectId),
}

/// What the Outliner draws.
///
/// The two roots are mutually exclusive, so this is which one is open
/// rather than a pair.
#[derive(Clone, Copy)]
pub(crate) enum OutlinerSource<'a> {
    /// Nothing is open.
    Empty,
    /// A single model file: meshes and materials, fully mutable.
    Model(&'a Model),
    /// A cooked scene: objects, each expanding to its own meshes and
    /// materials. `names` is parallel to the objects' iteration order.
    Scene {
        objects: &'a SceneObjects,
        names: &'a [(SceneObjectId, String)],
    },
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
    source: OutlinerSource<'_>,
    events: &mut OutlinerEvents,
) {
    match source {
        OutlinerSource::Empty => {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Nothing open").weak());
            });
        }
        OutlinerSource::Model(model) => draw_model_outliner(ui, model, events),
        OutlinerSource::Scene { objects, names } => {
            draw_scene_outliner(ui, objects, names, events);
        }
    }
}

/// The scene variant: one collapsed header per object over the same mesh
/// and material rows the model variant draws.
///
/// Objects start collapsed. A scene can hold any number of them, and an
/// outliner whose top level does not fit on screen has stopped being an
/// outline.
fn draw_scene_outliner(
    ui: &mut egui::Ui,
    objects: &SceneObjects,
    names: &[(SceneObjectId, String)],
    events: &mut OutlinerEvents,
) {
    if objects.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(egui::RichText::new("Scene is empty").weak());
        });
        return;
    }

    ui.add_space(2.0);
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);
        for (id, object) in objects.iter() {
            let name = names
                .iter()
                .find(|(oid, _)| oid == id)
                .map_or_else(|| format!("Object {}", id.0), |(_, n)| n.clone());
            draw_object_section(ui, *id, &name, object, events);
        }
        ui.add_space(8.0);
    });
}

fn draw_object_section(
    ui: &mut egui::Ui,
    id: SceneObjectId,
    name: &str,
    object: &solarxy_renderer::scene_objects::SceneObject,
    events: &mut OutlinerEvents,
) {
    let model = &object.model;
    let summary = format!(
        "{} ({}, {})",
        name,
        plural(model.meshes.len(), "mesh", "meshes"),
        plural(model.materials.len(), "material", "materials"),
    );

    let header = egui::CollapsingHeader::new(summary)
        .id_salt(id.0)
        .default_open(false);

    let response = header.show(ui, |ui| {
        if !model.meshes.is_empty() {
            egui::CollapsingHeader::new(format!("Meshes ({})", model.meshes.len()))
                .id_salt((id.0, "meshes"))
                .default_open(true)
                .show(ui, |ui| {
                    for (i, mesh) in model.meshes.iter().enumerate() {
                        draw_scene_mesh_row(ui, id, i, mesh, model, events);
                    }
                });
        }
        if !model.materials.is_empty() {
            egui::CollapsingHeader::new(format!("Materials ({})", model.materials.len()))
                .id_salt((id.0, "materials"))
                .default_open(true)
                .show(ui, |ui| {
                    for (m, material) in model.materials.iter().enumerate() {
                        draw_scene_material_row(ui, m, &material.name, model);
                    }
                });
        }
    });

    // The visibility checkbox rides on the header row rather than inside
    // the body, so an object can be hidden without expanding it.
    let rect = response.header_response.rect;
    let mut visible = object.visible;
    let checkbox_rect = egui::Rect::from_min_size(
        egui::pos2(rect.right() - 20.0, rect.center().y - 8.0),
        egui::vec2(16.0, 16.0),
    );
    if ui
        .put(checkbox_rect, egui::Checkbox::new(&mut visible, ""))
        .on_hover_text("Toggle object visibility")
        .changed()
    {
        events.action = Some(OutlinerAction::ToggleObject(id));
    }

    if response.header_response.clicked() {
        events.action = Some(OutlinerAction::FrameObject(id));
    }
    response.header_response.context_menu(|ui| {
        if ui.button("Frame").clicked() {
            events.action = Some(OutlinerAction::FrameObject(id));
            ui.close();
        }
    });
}

fn plural(n: usize, one: &str, many: &str) -> String {
    if n == 1 {
        format!("{n} {one}")
    } else {
        format!("{n} {many}")
    }
}

/// A scene object's mesh row: framing only.
///
/// The visibility checkbox is present but disabled, because there is no
/// per-mesh operation in the scene delta and the object's render flags are
/// re-emitted on every cook. A checkbox that reverted on the user's next
/// parameter edit would be worse than one that plainly says why it cannot
/// be used.
fn draw_scene_mesh_row(
    ui: &mut egui::Ui,
    object: SceneObjectId,
    idx: usize,
    mesh: &solarxy_renderer::model::Mesh,
    model: &Model,
    events: &mut OutlinerEvents,
) {
    ui.horizontal(|ui| {
        let mut visible = true;
        ui.add_enabled(false, egui::Checkbox::new(&mut visible, ""))
            .on_disabled_hover_text(
                "Scene meshes hide per object, not per mesh: a cook rebuilds the object's \
                 geometry, so a per-mesh toggle would not survive one",
            );

        let name = mesh_display_name(mesh, idx);
        if ui
            .selectable_label(false, name)
            .on_hover_text("Click to frame")
            .clicked()
        {
            events.action = Some(OutlinerAction::FrameObjectMesh(object, idx));
        }

        if let Some(material) = model.materials.get(mesh.material)
            && !material.name.trim().is_empty()
        {
            ui.label(egui::RichText::new(&material.name).weak());
        }
    });
}

/// A scene object's material row: name and mesh count only.
///
/// Materials are node parameters in a scene, so there is nothing here to
/// toggle and nothing to frame that the mesh rows do not already offer.
fn draw_scene_material_row(ui: &mut egui::Ui, idx: usize, name: &str, model: &Model) {
    let mesh_count = model.meshes.iter().filter(|m| m.material == idx).count();
    ui.horizontal(|ui| {
        let label = if name.trim().is_empty() {
            format!("Material {idx}")
        } else {
            name.to_string()
        };
        ui.label(label);
        ui.label(egui::RichText::new(format!("({mesh_count})")).weak());
    });
}

fn draw_model_outliner(ui: &mut egui::Ui, model: &Model, events: &mut OutlinerEvents) {
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
