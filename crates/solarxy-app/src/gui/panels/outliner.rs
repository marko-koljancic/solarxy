//! The Outliner panel, a docked tab listing the open document's objects,
//! each expanding to its own meshes and materials. An object row carries a
//! visibility checkbox; clicking a row name frames the active camera on it.
//!
//! Lights and cameras are intentionally absent here: they are nodes, and the
//! Node Tree is where a document's nodes are listed.
//!
//! The panel is read-only; mutations are raised as [`OutlinerAction`]s onto
//! the intent queue, drained by `state/intents.rs` after the egui pass.

use solarxy_core::scene::SceneObjectId;
use solarxy_renderer::model::Model;
use solarxy_renderer::scene_objects::SceneObjects;

use crate::gui::intent::{Intents, PanelIntent};

/// One Outliner interaction, raised during an egui pass.
///
/// **Visibility is object-level and nothing finer.** A document's geometry
/// belongs to the engine and is re-emitted on every cook, so the only
/// visibility that survives a user's next edit is a `visible` parameter on
/// the owning node. The shell offered per-mesh and per-material hiding until
/// 0.10.0, when the second root that owned those meshes went away.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OutlinerAction {
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
#[derive(Clone, Copy)]
pub(crate) enum OutlinerSource<'a> {
    /// Nothing is open.
    Empty,
    /// The open document's objects, each expanding to its own meshes and
    /// materials. `names` is parallel to the objects' iteration order.
    Scene {
        objects: &'a SceneObjects,
        names: &'a [(SceneObjectId, String)],
    },
}

/// Render the Outliner content into `ui` (the `egui_dock` `Outliner` tab
/// supplies the `Ui`).
pub(in crate::gui) fn draw_outliner_content(
    ui: &mut egui::Ui,
    source: OutlinerSource<'_>,
    intents: &mut Intents,
) {
    match source {
        OutlinerSource::Empty => {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Nothing open").weak());
            });
        }
        OutlinerSource::Scene { objects, names } => {
            draw_scene_outliner(ui, objects, names, intents);
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
    intents: &mut Intents,
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
            draw_object_section(ui, *id, &name, object, intents);
        }
        ui.add_space(8.0);
    });
}

fn draw_object_section(
    ui: &mut egui::Ui,
    id: SceneObjectId,
    name: &str,
    object: &solarxy_renderer::scene_objects::SceneObject,
    intents: &mut Intents,
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
                        draw_scene_mesh_row(ui, id, i, mesh, model, intents);
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
        intents.panel(PanelIntent::Outliner(OutlinerAction::ToggleObject(id)));
    }

    if response.header_response.clicked() {
        intents.panel(PanelIntent::Outliner(OutlinerAction::FrameObject(id)));
    }
    response.header_response.context_menu(|ui| {
        if ui.button("Frame").clicked() {
            intents.panel(PanelIntent::Outliner(OutlinerAction::FrameObject(id)));
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
    intents: &mut Intents,
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
            intents.panel(PanelIntent::Outliner(OutlinerAction::FrameObjectMesh(
                object, idx,
            )));
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
/// Display name for a mesh — falls back to `Mesh N` for unnamed meshes
/// (common in OBJ files without `o`/`g` groups).
fn mesh_display_name(mesh: &solarxy_renderer::model::Mesh, idx: usize) -> String {
    if mesh.name.trim().is_empty() {
        format!("Mesh {idx}")
    } else {
        mesh.name.clone()
    }
}
