//! Right-click context menu inside the 3D viewport.
//!
//! The viewport's 3D content is a non-interactive egui area (so winit
//! pointer events reach the camera). A right-click is therefore caught in
//! `app.rs`, raycast in `State`, and — if it landed on a mesh — recorded
//! as a [`ViewportContextMenu`]. This module paints that menu as a
//! free-floating `egui::Area` and reports the chosen action back as an
//! [`OutlinerAction`] (reused — the actions are identical to the
//! Outliner's).

use super::outliner::OutlinerAction;

/// A pending viewport context menu — set by `State` on a right-click that
/// hit a mesh, cleared once the menu is dismissed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewportContextMenu {
    /// Model mesh index the right-click landed on.
    pub mesh_index: usize,
    /// Egui-logical position to anchor the menu at (the cursor).
    pub screen_pos: egui::Pos2,
    /// Skips the dismiss check on the first frame so the opening
    /// right-click doesn't immediately close the menu.
    pub suppress_dismiss: bool,
}

/// Result of painting the context menu for one frame.
pub(super) struct ContextMenuOutcome {
    /// The action chosen, if a menu item was clicked.
    pub action: Option<OutlinerAction>,
    /// `true` once the menu should be dismissed (item clicked, click
    /// outside, or Esc).
    pub close: bool,
}

/// Paint the viewport context menu. Returns the chosen action (if any)
/// and whether the menu should now close.
pub(super) fn draw_viewport_context_menu(
    ctx: &egui::Context,
    menu: &mut ViewportContextMenu,
) -> ContextMenuOutcome {
    let mut action = None;
    let mut close = false;
    let mesh = menu.mesh_index;

    let area = egui::Area::new(egui::Id::new("solarxy_viewport_context_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    if ui.button("Frame").clicked() {
                        action = Some(OutlinerAction::FrameMesh(mesh));
                        close = true;
                    }
                    if ui.button("Hide").clicked() {
                        action = Some(OutlinerAction::HideMesh(mesh));
                        close = true;
                    }
                    if ui.button("Hide Others").clicked() {
                        action = Some(OutlinerAction::IsolateMesh(mesh));
                        close = true;
                    }
                    ui.separator();
                    if ui.button("Show All").clicked() {
                        action = Some(OutlinerAction::ShowAll);
                        close = true;
                    }
                });
            });
        });

    if menu.suppress_dismiss {
        // The opening right-click is still in egui's input this frame —
        // don't let it dismiss the menu it just opened.
        menu.suppress_dismiss = false;
    } else {
        let menu_rect = area.response.rect;
        let clicked_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer
                    .interact_pos()
                    .is_none_or(|p| !menu_rect.contains(p))
        });
        let esc = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if clicked_outside || esc {
            close = true;
        }
    }

    ContextMenuOutcome { action, close }
}
