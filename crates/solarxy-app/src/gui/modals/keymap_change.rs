//! The one-time notice that the keyboard map changed.
//!
//! Shown on the first launch after an upgrade and never on a fresh install.
//! It exists because adopting the other shell's map is a user-visible break
//! across keys a desktop user has in muscle memory, and a break is worth
//! saying out loud once rather than leaving someone to discover it a key at
//! a time. The mitigation is this and a reference that can no longer be
//! stale, rather than a compatibility mode, because a second table to hold
//! in step is the thing the one table replaced.

use crate::state::keymap::{Action, hint};

/// A key whose meaning moved, as the notice names it.
struct Moved {
    action: Option<Action>,
    was: &'static str,
    now: &'static str,
}

/// What changed, in the order a user is likeliest to reach for it.
///
/// Not every retired binding: the ones whose key still does something else
/// now, plus the two that simply went. A complete list is the reference,
/// which this opens.
const MOVED: &[Moved] = &[
    Moved {
        action: Some(Action::Bypass),
        was: "cycled the background",
        now: "toggles bypass over the canvas, and is the Bottom view over the viewport",
    },
    Moved {
        action: Some(Action::DisplayFlag),
        was: "raised the exposure",
        now: "sets the display flag over the canvas",
    },
    Moved {
        action: Some(Action::EdgeStyle),
        was: "set Shaded",
        now: "cycles the connection style over the canvas",
    },
    Moved {
        action: Some(Action::CanvasMinimap),
        was: "cycled the material override",
        now: "toggles the minimap over the canvas",
    },
    Moved {
        action: Some(Action::NodeInfo),
        was: "cycled the image-based lighting",
        now: "shows the selected node's info over the canvas",
    },
    Moved {
        action: Some(Action::ToggleReviewPanel),
        was: "cycled the normals display",
        now: "toggles the review panel",
    },
    Moved {
        action: Some(Action::CanvasGrid),
        was: "toggled the scene grid",
        now: "toggles the canvas grid; the scene grid is on the pane's Display menu",
    },
    Moved {
        action: Some(Action::FitView),
        was: "was unbound",
        now: "frames the scene, which was H",
    },
    Moved {
        action: Some(Action::OpenNodePalette),
        was: "showed and hid the sidebar",
        now: "opens the node palette",
    },
    Moved {
        action: None,
        was: "R and W, the Right view and the shading cycle, are unbound",
        now: "reserved for the transform tools",
    },
];

/// One line of the notice.
fn line(moved: &Moved) -> String {
    match moved.action.and_then(hint) {
        Some(keys) => format!("{keys} {} and now {}.", moved.was, moved.now),
        None => format!("{} and now {}.", moved.was, moved.now),
    }
}

/// Every line, which is what the modal draws and what the test reads.
pub(crate) fn lines() -> Vec<String> {
    MOVED.iter().map(line).collect()
}

#[derive(Default)]
pub(in crate::gui) struct KeymapNoticeState {
    pub open: bool,
    /// Set when the reader asked for the full reference, taken by the shell.
    show_reference: bool,
    dismissed: bool,
}

impl KeymapNoticeState {
    pub(in crate::gui) fn open(&mut self) {
        self.open = true;
        self.show_reference = false;
        self.dismissed = false;
    }

    /// `true` once, on the frame the reader asked for the reference.
    pub(in crate::gui) fn take_show_reference(&mut self) -> bool {
        std::mem::take(&mut self.show_reference)
    }

    /// `true` once, on the frame the notice was answered, whichever way.
    pub(in crate::gui) fn take_dismissed(&mut self) -> bool {
        std::mem::take(&mut self.dismissed)
    }
}

pub(in crate::gui) fn draw_keymap_notice(ctx: &egui::Context, modal: &mut KeymapNoticeState) {
    if !modal.open {
        return;
    }
    let mut close = false;
    egui::Window::new("The keyboard map has changed")
        .id(egui::Id::new("solarxy_keymap_notice"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label("Both shells now share one keyboard map, so a key means the same thing wherever you are. Several keys moved:");
            ui.add_space(6.0);
            for text in lines() {
                ui.label(egui::RichText::new(format!("\u{2022} {text}")));
            }
            ui.add_space(8.0);
            ui.label("Keys now depend on what the pointer is over: the viewport, the node canvas, or neither.");
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button("Got it").clicked() {
                    close = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Show all shortcuts").clicked() {
                        modal.show_reference = true;
                        close = true;
                    }
                });
            });
        });

    if close {
        modal.open = false;
        modal.dismissed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every line names a key, except the one that names two keys which no
    /// longer do anything on their own.
    #[test]
    fn every_line_names_what_moved_and_what_it_does_now() {
        let lines = lines();
        assert_eq!(lines.len(), MOVED.len());
        for text in &lines {
            assert!(
                text.contains(" and now "),
                "{text} does not say what changed"
            );
            assert!(text.ends_with('.'), "{text} is not a sentence");
        }
        assert!(
            lines[0].starts_with('B'),
            "the first line should name B: {}",
            lines[0]
        );
        assert!(
            lines[7].starts_with('Z'),
            "fit moved to Z and the line should say so: {}",
            lines[7]
        );
    }

    /// Answered either way it closes, and asking for the reference is
    /// reported once.
    #[test]
    fn an_answer_closes_it_and_the_request_is_reported_once() {
        let mut modal = KeymapNoticeState::default();
        modal.open();
        assert!(modal.open);
        assert!(!modal.take_dismissed());
        modal.show_reference = true;
        assert!(modal.take_show_reference());
        assert!(!modal.take_show_reference(), "asked once, reported once");
    }
}
