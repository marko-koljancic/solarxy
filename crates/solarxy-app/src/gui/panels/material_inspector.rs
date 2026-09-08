//! The Material Inspector, withdrawn.
//!
//! The panel was master and detail over a **file-loaded model's** materials,
//! with texture thumbnails and an open-externally action. It read the second
//! document root, and with one root there is no imported file standing beside
//! the document to inspect: a document's materials are node parameters, and
//! the parameter panel and the texture viewer are where they are read.
//!
//! What remains is the empty state, so a mounted tab says why it is empty
//! rather than looking broken while the viewport is plainly full of geometry.
//! The tab itself is retired with the rest of the surfaces the browser has no
//! counterpart for; it is kept for now because its variant name is serialized
//! into every user's saved dock layout, and dropping it silently costs a
//! reader their arrangement.

/// The Material Inspector's two empty states.
pub(in crate::gui) fn draw_material_inspector_content(ui: &mut egui::Ui, scene_open: bool) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        if scene_open {
            ui.label(egui::RichText::new("Materials live on their nodes").weak());
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new("Select a material node to read and edit its parameters.")
                    .weak()
                    .small(),
            );
        } else {
            ui.label(egui::RichText::new("Nothing open").weak());
        }
    });
}
