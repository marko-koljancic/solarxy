//! Right-click context menu inside the 3D viewport.
//!
//! The viewport's 3D content is a non-interactive egui area, so winit pointer
//! events reach the camera. A right-click is therefore caught in `app.rs`,
//! raycast in `State`, and, if it landed on an object, recorded as a
//! [`ViewportContextMenu`]. This module paints that menu as a free-floating
//! `egui::Area` and reports the chosen action back as an [`OutlinerAction`],
//! reused because the actions are identical to the Outliner's.
//!
//! Frame and one Hide-or-Show toggle, which is what the browser's own
//! viewport menu offers. Hiding is a `visible` parameter change on the
//! object's node, so it survives the next cook.

use solarxy_core::scene::SceneObjectId;

use crate::gui::panels::outliner::OutlinerAction;

/// A pending viewport context menu, set by `State` on a right-click that hit
/// an object and cleared once the menu is dismissed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewportContextMenu {
    /// The scene object the right-click landed on.
    pub object: SceneObjectId,
    /// Whether that object is currently visible, which is what decides
    /// between the Hide and the Show wording.
    pub visible: bool,
    /// Egui-logical position to anchor the menu at (the cursor).
    pub screen_pos: egui::Pos2,
    /// Skips the dismiss check on the first frame so the opening
    /// right-click doesn't immediately close the menu.
    pub suppress_dismiss: bool,
}

/// Result of painting the context menu for one frame.
pub(in crate::gui) struct ContextMenuOutcome {
    /// The action chosen, if a menu item was clicked.
    pub action: Option<OutlinerAction>,
    /// `true` once the menu should be dismissed (item clicked, click
    /// outside, or Esc).
    pub close: bool,
}

/// Paint the viewport context menu. Returns the chosen action (if any)
/// and whether the menu should now close.
pub(in crate::gui) fn draw_viewport_context_menu(
    ctx: &egui::Context,
    menu: &mut ViewportContextMenu,
) -> ContextMenuOutcome {
    let mut action = None;
    let mut close = false;
    let object = menu.object;
    let hide_label = if menu.visible { "Hide" } else { "Show" };

    let area = egui::Area::new(egui::Id::new("solarxy_viewport_context_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    if ui.button("Frame").clicked() {
                        action = Some(OutlinerAction::FrameObject(object));
                        close = true;
                    }
                    if ui.button(hide_label).clicked() {
                        action = Some(OutlinerAction::ToggleObject(object));
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
