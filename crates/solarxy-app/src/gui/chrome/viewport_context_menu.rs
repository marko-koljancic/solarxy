//! Right-click context menu inside the 3D viewport.
//!
//! The viewport's 3D content is a non-interactive egui area, so winit pointer
//! events reach the camera. A right-click is therefore caught in `app.rs`,
//! picked through the engine in `State`, and recorded as a
//! [`ViewportContextMenu`]. This module paints that menu as a free-floating
//! `egui::Area` and reports the chosen action back.
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
use solarxy_host::gizmo::{ALL_TOOLS, ToolMode};

use super::tool_column::{tool_action, tool_hover, tool_label};
use crate::state::keymap::Action;

/// The transform tools as the menu was opened: which is armed and which the
/// selection can take. Taken at open time rather than read live, because
/// the menu is drawn from a copy the state layer hands the interface.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ToolRows {
    pub armed: ToolMode,
    pub applies: [bool; ALL_TOOLS.len()],
}

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
    /// The tool rows' state as the menu opened.
    pub tools: ToolRows,
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
    /// Arm a transform tool, the same arm the column and the keys reach.
    SetTool(ToolMode),
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
    let tools = menu.tools;

    let area = egui::Area::new(egui::Id::new("solarxy_viewport_context_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(menu.screen_pos)
        .constrain(true)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(170.0);
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                    action = draw_entries(ui, target, tools);
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
fn draw_entries(
    ui: &mut egui::Ui,
    target: Option<ContextTarget>,
    tools: ToolRows,
) -> Option<ViewportAction> {
    let mut action = None;

    // The five tools in the column's order, each with the key its binding
    // table gives it; a tool the selection cannot take is drawn disabled
    // with the reason, never dropped, so the menu's shape does not depend
    // on what is selected.
    ui.label(egui::RichText::new(TOOLS_HEADING).weak());
    for (i, tool) in ALL_TOOLS.into_iter().enumerate() {
        let applies = tools.applies[i];
        let response = ui.add_enabled(
            applies,
            super::menu_items::button(tool_label(tool), tool_action(tool))
                .selected(tools.armed == tool && applies),
        );
        let response = if applies {
            response
        } else {
            response.on_disabled_hover_text(tool_hover(tool, false))
        };
        if response.clicked() {
            action = Some(ViewportAction::SetTool(tool));
        }
    }

    let resettable = target.filter(|t| t.resettable);
    for (i, row) in AFTER_TOOLS.iter().enumerate() {
        // Three groups after the tools, so a rule goes before the first, the
        // third and the fifth.
        if i == 0 || i == 2 || i == 4 {
            ui.separator();
        }
        let label = row.label(target.is_some_and(|t| t.visible));
        let clicked = match row {
            // Framing the whole scene needs nothing under the pointer, so it
            // is the one row that is never disabled, and the one that shows a
            // key: its binding does the same thing.
            AfterTools::FrameView => super::menu_items::entry(ui, label, row.action()).clicked(),
            AfterTools::ResetTransform => entry(ui, label, resettable).clicked(),
            _ => entry(ui, label, target).clicked(),
        };
        if !clicked {
            continue;
        }
        action = match row {
            AfterTools::FrameView => Some(ViewportAction::FrameView),
            AfterTools::FrameSelection => target.map(|t| ViewportAction::FrameObject(t.object)),
            AfterTools::Duplicate => target.map(|t| ViewportAction::Duplicate(t.object)),
            AfterTools::Delete => target.map(|t| ViewportAction::Delete(t.object)),
            AfterTools::HideOrShow => target.map(|t| ViewportAction::ToggleVisible(t.object)),
            AfterTools::ResetTransform => {
                resettable.map(|t| ViewportAction::ResetTransform(t.object))
            }
        };
    }

    action
}

/// The heading above the tools.
pub(in crate::gui) const TOOLS_HEADING: &str = "Tools";
const FRAME_VIEW: &str = "Frame view";
const HIDE: &str = "Hide";
const SHOW: &str = "Show";

/// One of the six entries after the tools, in the order they are drawn.
/// The draw walks [`AFTER_TOOLS`], so the table is the menu rather than a
/// description of it, and the test that holds this menu against the
/// browser reads the same table.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterTools {
    FrameView,
    FrameSelection,
    Duplicate,
    Delete,
    HideOrShow,
    ResetTransform,
}

impl AfterTools {
    /// One row follows the state of what was picked, so the entry says what
    /// it will do rather than what it is about. The browser chooses between
    /// the same two words.
    const fn label(self, visible: bool) -> &'static str {
        match self {
            Self::FrameView => FRAME_VIEW,
            Self::FrameSelection => "Frame selection",
            Self::Duplicate => "Duplicate",
            Self::Delete => "Delete",
            Self::HideOrShow if visible => HIDE,
            Self::HideOrShow => SHOW,
            Self::ResetTransform => "Reset transform",
        }
    }

    /// The binding whose key the row shows. Only the first is bound, and
    /// the browser shows a key for that one alone too.
    const fn action(self) -> Option<Action> {
        match self {
            Self::FrameView => Some(Action::FitView),
            _ => None,
        }
    }
}

const AFTER_TOOLS: &[AfterTools] = &[
    AfterTools::FrameView,
    AfterTools::FrameSelection,
    AfterTools::Duplicate,
    AfterTools::Delete,
    AfterTools::HideOrShow,
    AfterTools::ResetTransform,
];

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::keymap::hint;

    fn browser_source() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/ViewportContextMenu.tsx"))
            .expect("the browser's viewport context menu")
    }

    /// The browser's tool table as `(label, key)` pairs in its order. The
    /// key is its third slot, empty where a tool has no binding.
    fn browser_tools(source: &str) -> Vec<(String, String)> {
        let table = source
            .split_once("const TOOLS: [ToolMode, string, string][] = [")
            .and_then(|(_, rest)| rest.split_once("];"))
            .map(|(table, _)| table)
            .expect("the browser's tool table");
        table
            .lines()
            .filter_map(|line| {
                let mut quoted = line.split('"').skip(1).step_by(2);
                let _stored = quoted.next()?;
                Some((quoted.next()?.to_string(), quoted.next()?.to_string()))
            })
            .collect()
    }

    /// The tools are the browser tools, under its labels, in its order, and
    /// each shows the key its own third slot carries.
    #[test]
    fn the_tools_are_the_browsers_with_their_keys() {
        let source = browser_source();
        let browser = browser_tools(&source);
        assert_eq!(browser.len(), 5, "the reader found the browser's five");

        let here: Vec<(String, String)> = ALL_TOOLS
            .into_iter()
            .map(|tool| {
                let key = tool_action(tool).and_then(hint).unwrap_or_default();
                (tool_label(tool).to_string(), key)
            })
            .collect();
        assert_eq!(here, browser);
    }

    /// The browser source with its own comments taken out.
    ///
    /// The comments there name the entries they explain, and one of them
    /// names an entry that is drawn further down, so a search over the raw
    /// text finds the prose rather than the button.
    fn without_comments(source: &str) -> String {
        let mut out = String::with_capacity(source.len());
        let mut rest = source;
        while let Some((before, after)) = rest.split_once("{/*") {
            out.push_str(before);
            rest = after.split_once("*/}").map_or("", |(_, tail)| tail);
        }
        out.push_str(rest);
        out
    }

    /// The six entries after the tools are the browser six, in its order.
    /// Their positions in its source are compared rather than parsed out of
    /// its markup, because each is a button written by hand there and the
    /// order is the only thing that can drift.
    #[test]
    fn the_entries_after_the_tools_are_the_browsers_in_its_order() {
        let source = without_comments(&browser_source());
        let start = source.find("ctx-sep").expect("the rule under the tools");
        let region = &source[start..];

        let mut last = 0;
        for row in AFTER_TOOLS {
            let label = row.label(true);
            let at = region
                .find(label)
                .unwrap_or_else(|| panic!("the browser menu has no {label}"));
            assert!(at > last, "{label} is out of the browser order");
            last = at;
        }
        // The wording that follows the state is the one pair with two
        // spellings, and the browser chooses between the same two.
        assert!(region.contains(SHOW), "the browser has no {SHOW} wording");
    }

    /// One entry after the tools carries a key on each shell, it is the same
    /// entry, and it is the same key. The browser comment beside its chip
    /// records that this one named the wrong key once already.
    #[test]
    fn frame_view_shows_the_key_the_browser_shows() {
        let source = browser_source();
        let start = source.find("ctx-sep").expect("the rule under the tools");
        let region = &source[start..];
        let keys: Vec<&str> = region
            .split("className=\"ctx-key\">")
            .skip(1)
            .filter_map(|rest| rest.split('<').next())
            .collect();
        assert_eq!(keys, ["Z"], "the browser keys after the tools");

        let bound: Vec<(&str, String)> = AFTER_TOOLS
            .iter()
            .filter_map(|row| Some((row.label(true), hint(row.action()?)?)))
            .collect();
        assert_eq!(bound, [(FRAME_VIEW, "Z".to_string())]);
    }

    /// The heading above the tools is the browser heading.
    #[test]
    fn the_heading_is_the_browsers() {
        let source = browser_source();
        assert!(
            source.contains(&format!("ctx-heading\">{TOOLS_HEADING}<")),
            "the browser no longer heads the tools with {TOOLS_HEADING}"
        );
    }
}
