//! The viewport's tool column: the five transform tools down the left edge
//! of the 3D region, and the live readout under a drag.
//!
//! An overlay inside the viewport rather than a strip beside it, as the
//! browser's is, so the pane rects the render is derived from never move.
//! Select stands in its own group, then Move, Rotate, Scale and Aim; the
//! keys keep the browser's order so the positions people reach for by
//! muscle memory are the same on both shells, and Aim, which has no key
//! on either, comes last.
//!
//! A tool the selection cannot take is drawn unavailable rather than
//! dropped, because a button that vanishes reads as a broken viewport.
//! The armed tool stays armed underneath, so it looks unarmed while a
//! light is selected and comes back when a mesh is.

use egui::{Rect, Sense, pos2, vec2};

use solarxy_host::gizmo::{ALL_TOOLS, ToolMode};

use solarxy_core::view_config::PANE_TOOLBAR_HEIGHT;

use super::viewport_icons::paint_tool;
use crate::gui::intent::{Intent, Intents, ToolIntent};
use crate::gui::settings::PanelSettings;
use crate::gui::theme::Theme;
use crate::state::keymap::{Action, hint};

/// A button's side, logical pixels: the browser's 19-unit glyph in its
/// padded square.
const BUTTON_PX: f32 = 30.0;
/// Between two buttons in a group.
const BUTTON_GAP: f32 = 4.0;
/// Between the two groups.
const GROUP_GAP: f32 = 8.0;
/// From the viewport's left edge.
const INSET_X: f32 = 8.0;
/// Below the pane toolbar strip.
const INSET_Y: f32 = 8.0;

/// The column's entries in draw order: the shared solver's own order, with
/// Select first and Aim last.
const COLUMN: [ToolMode; ALL_TOOLS.len()] = ALL_TOOLS;

/// What a tool is called on a button's hover and in the context menu.
pub(in crate::gui) fn tool_label(tool: ToolMode) -> &'static str {
    match tool {
        ToolMode::Select => "Select",
        ToolMode::Move => "Move",
        ToolMode::Rotate => "Rotate",
        ToolMode::Scale => "Scale",
        ToolMode::Aim => "Aim",
    }
}

/// The binding that arms a tool, for its hint. Aim has none on either shell.
pub(in crate::gui) fn tool_action(tool: ToolMode) -> Option<Action> {
    match tool {
        ToolMode::Select => Some(Action::ToolSelect),
        ToolMode::Move => Some(Action::ToolMove),
        ToolMode::Rotate => Some(Action::ToolRotate),
        ToolMode::Scale => Some(Action::ToolScale),
        ToolMode::Aim => None,
    }
}

/// The hover text for a tool: its name and key, or why it cannot be armed.
pub(in crate::gui) fn tool_hover(tool: ToolMode, applies: bool) -> String {
    if !applies {
        return format!("{}: not available for this selection", tool_label(tool));
    }
    match tool_action(tool).and_then(hint) {
        Some(key) => format!("{} ({key})", tool_label(tool)),
        None => tool_label(tool).to_string(),
    }
}

/// The column's rect within `viewport`, so the pointer routing can keep a
/// click on a button from also reaching the camera and the pick.
fn column_rect(viewport: Rect) -> Rect {
    let groups = 2.0;
    let height =
        COLUMN.len() as f32 * BUTTON_PX + (COLUMN.len() as f32 - groups) * BUTTON_GAP + GROUP_GAP;
    Rect::from_min_size(
        pos2(
            viewport.left() + INSET_X,
            viewport.top() + PANE_TOOLBAR_HEIGHT + INSET_Y,
        ),
        vec2(BUTTON_PX, height),
    )
}

/// Draw the column over `viewport` and return the rect it occupies.
pub(in crate::gui) fn draw_tool_column(
    ui: &mut egui::Ui,
    viewport: Rect,
    settings: PanelSettings<'_>,
    intents: &mut Intents,
    theme: Theme,
) -> Rect {
    let rect = column_rect(viewport);
    let tools = settings.tools;
    let mut y = rect.top();
    for (i, tool) in COLUMN.into_iter().enumerate() {
        if i == 1 {
            y += GROUP_GAP;
        }
        let button = Rect::from_min_size(pos2(rect.left(), y), vec2(BUTTON_PX, BUTTON_PX));
        let applies = tools.applies_to(tool);
        let armed = tools.tool == tool && applies;
        let response = ui
            .interact(
                button,
                ui.id().with(("tool_column", i)),
                if applies {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            )
            .on_hover_text(tool_hover(tool, applies));
        let hovered = applies && response.hovered();
        let fill = if armed {
            theme.accent.gamma_multiply(0.85)
        } else if hovered {
            theme.widget_hover
        } else {
            theme.bg_elevated.gamma_multiply(0.9)
        };
        let glyph = if armed {
            theme.bg
        } else if applies {
            theme.fg
        } else {
            theme.muted.gamma_multiply(0.6)
        };
        let painter = ui.painter();
        painter.rect_filled(button, 4.0, fill);
        painter.rect_stroke(
            button,
            4.0,
            egui::Stroke::new(1.0_f32, theme.border),
            egui::StrokeKind::Inside,
        );
        paint_tool(painter, button.shrink(6.0), tool, glyph);
        if response.clicked() {
            intents.raise(Intent::Tool(ToolIntent::Set(tool)));
        }
        y += BUTTON_PX + BUTTON_GAP;
    }
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The column lists the solver's tools in the solver's order, Select
    /// first and Aim last, which is what the context menu lists too.
    #[test]
    fn the_column_is_the_solvers_tools_in_order() {
        assert_eq!(
            COLUMN,
            [
                ToolMode::Select,
                ToolMode::Move,
                ToolMode::Rotate,
                ToolMode::Scale,
                ToolMode::Aim
            ]
        );
    }

    /// The four keyed tools read their hint from the binding table, so a
    /// rebinding cannot leave a button naming a key that does nothing; Aim
    /// has no key on either shell and says only its name.
    #[test]
    fn a_hover_names_the_bound_key_or_the_reason() {
        assert_eq!(tool_hover(ToolMode::Move, true), "Move (W)");
        assert_eq!(tool_hover(ToolMode::Select, true), "Select (Q)");
        assert_eq!(tool_hover(ToolMode::Aim, true), "Aim");
        assert_eq!(
            tool_hover(ToolMode::Scale, false),
            "Scale: not available for this selection"
        );
    }

    /// The column starts strictly below the pane toolbar strip, inside the
    /// viewport's left edge, and is tall enough for five buttons, the three
    /// gaps within the groups and the gap between them. Stated against the
    /// strip and in whole numbers rather than re-derived from the constants,
    /// so a constant moved onto the strip fails here.
    #[test]
    fn the_column_sits_under_the_toolbar_strip() {
        let viewport = Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0));
        let rect = column_rect(viewport);
        let strip_bottom = viewport.top() + PANE_TOOLBAR_HEIGHT;
        assert!(
            rect.top() > strip_bottom,
            "{} is not below {strip_bottom}",
            rect.top()
        );
        assert!(rect.left() > viewport.left() && rect.right() < viewport.center().x);
        assert!(rect.bottom() < viewport.bottom());
        assert!((rect.height() - 170.0).abs() < f32::EPSILON);
    }
}
