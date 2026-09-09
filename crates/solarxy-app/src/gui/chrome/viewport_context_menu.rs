//! Right-click context menu inside the 3D viewport.
//!
//! The viewport's 3D content is a non-interactive egui area, so winit pointer
//! events reach the camera. A right-click is therefore caught in `app.rs`,
//! raycast in `State`, and recorded as a [`ViewportContextMenu`]. This module
//! paints that menu as a free-floating `egui::Area` and reports the chosen
//! action back.
//!
//! **The menu acts on what the pointer landed on, not on what is selected.**
//! That is what a user expects from a menu opened on top of something, and it
//! is the one place this shell is deliberately ahead of the browser, whose
//! right-click does not pick and whose menu therefore acts on a selection
//! that may be nothing to do with what is under the cursor. The difference is
//! recorded in the milestone document rather than left to be discovered.
//!
//! Right-clicking empty space still opens the menu. Framing the view makes
//! sense there and the rest does not, so the rest is disabled rather than the
//! menu being withheld: a menu that sometimes fails to appear reads as a
//! broken gesture.

use solarxy_core::scene::SceneObjectId;

/// The five transform tools, in the order the menu lists them.
///
/// Named here rather than taken from the manipulator's own enum because the
/// tools are not armed on this shell yet: this is the menu's promise of what
/// will be here, and the rows are drawn disabled until the viewport gains
/// them.
const TOOLS: [&str; 5] = ["Select", "Move", "Rotate", "Scale", "Aim"];

/// What the right-click landed on, when it landed on something.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ContextTarget {
    /// The scene object under the pointer.
    pub object: SceneObjectId,
    /// Whether it is currently visible, which decides between the Hide and
    /// the Show wording.
    pub visible: bool,
    /// Whether its node declares a transform at all. `false` disables the
    /// reset rather than offering a reset that would do nothing.
    pub resettable: bool,
}

/// A pending viewport context menu, set by `State` on a right-click and
/// cleared once the menu is dismissed.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ViewportContextMenu {
    /// What the click landed on, or `None` for empty space.
    pub target: Option<ContextTarget>,
    /// Egui-logical position to anchor the menu at (the cursor).
    pub screen_pos: egui::Pos2,
    /// Skips the dismiss check on the first frame so the opening
    /// right-click doesn't immediately close the menu.
    pub suppress_dismiss: bool,
}

/// What the menu asked for. Every variant but [`Self::FrameView`] names the
/// object it acts on, so nothing downstream has to look up what was picked.
#[derive(Debug, Clone, Copy)]
pub(crate) enum ViewportAction {
    /// Frame the active pane on the whole scene.
    FrameView,
    /// Frame the active pane on the picked object.
    FrameObject(SceneObjectId),
    /// Copy the picked object's node.
    Duplicate(SceneObjectId),
    /// Delete the picked object's node.
    Delete(SceneObjectId),
    /// Flip the picked object's visibility.
    ToggleVisible(SceneObjectId),
    /// Put every transform parameter the picked object's node declares back
    /// to its default, as one undo step.
    ResetTransform(SceneObjectId),
}

/// Result of painting the context menu for one frame.
pub(in crate::gui) struct ContextMenuOutcome {
    /// The action chosen, if a menu item was clicked.
    pub action: Option<ViewportAction>,
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
    let target = menu.target;

    let area = egui::Area::new(egui::Id::new("solarxy_viewport_context_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    action = draw_entries(ui, target);
                    close = action.is_some();
                });
            });
        });

    if menu.suppress_dismiss {
        // The opening right-click is still in egui's input this frame, so it
        // must not dismiss the menu it just opened.
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

/// The eleven entries, in four groups.
fn draw_entries(ui: &mut egui::Ui, target: Option<ContextTarget>) -> Option<ViewportAction> {
    let mut action = None;

    ui.label(egui::RichText::new("Tools").weak());
    for tool in TOOLS {
        ui.add_enabled(false, egui::Button::new(tool))
            .on_disabled_hover_text("The transform tools arrive with the viewport work.");
    }

    ui.separator();
    if ui.button("Frame view").clicked() {
        action = Some(ViewportAction::FrameView);
    }
    if entry(ui, "Frame selection", target).clicked()
        && let Some(t) = target
    {
        action = Some(ViewportAction::FrameObject(t.object));
    }

    ui.separator();
    if entry(ui, "Duplicate", target).clicked()
        && let Some(t) = target
    {
        action = Some(ViewportAction::Duplicate(t.object));
    }
    if entry(ui, "Delete", target).clicked()
        && let Some(t) = target
    {
        action = Some(ViewportAction::Delete(t.object));
    }

    ui.separator();
    // The wording follows the object's current state, so the entry says what
    // it will do rather than what it is about.
    let hide_label = if target.is_some_and(|t| t.visible) {
        "Hide"
    } else {
        "Show"
    };
    if entry(ui, hide_label, target).clicked()
        && let Some(t) = target
    {
        action = Some(ViewportAction::ToggleVisible(t.object));
    }
    let resettable = target.filter(|t| t.resettable);
    if entry(ui, "Reset transform", resettable).clicked()
        && let Some(t) = resettable
    {
        action = Some(ViewportAction::ResetTransform(t.object));
    }

    action
}

/// One entry, enabled only when there is something for it to act on.
///
/// The disabled tooltip says which of the two reasons it is off, because
/// "nothing under the pointer" and "this node has no transform" are different
/// problems with different fixes.
fn entry(ui: &mut egui::Ui, label: &str, target: Option<ContextTarget>) -> egui::Response {
    let response = ui.add_enabled(target.is_some(), egui::Button::new(label));
    if target.is_none() {
        response.on_disabled_hover_text("Nothing under the pointer to act on")
    } else {
        response
    }
}
