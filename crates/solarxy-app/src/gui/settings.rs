//! What a panel reads about the shell's display state.
//!
//! This is the half of the retired mirror that was actually needed. A widget
//! has to show a value before it can change one, and the value it shows is
//! the real one the shell owns rather than a copy taken at the top of the
//! frame. Changing it travels the other way, as an [`Intent`].
//!
//! [`Intent`]: super::intent::Intent
//!
//! The mirror this replaces was a thirty-two field `Copy` struct rebuilt every
//! frame and written back every frame whether anything had moved or not. Two
//! things came with it that are gone rather than reimplemented. The write-back
//! was unconditional, so anything else that wrote a per-pane setting during
//! the same frame was silently overwritten from the top-of-frame copy. And
//! `texel_density_target` round-tripped through it without a single widget
//! ever writing it, which is a hazard wearing a feature's clothes.

use solarxy_core::preferences::{IblMode, ProjectionMode};
use solarxy_graph::engine::CookMode;
use solarxy_renderer::frame::PostProcessing;

use crate::state::view_state::{DisplaySettings, PaneDisplaySettings};

/// What the header strip says about the cook, read from the engine each
/// frame rather than mirrored from events, so a scene opened in manual mode
/// says so on its first frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CookReadout {
    /// Whether a document is open at all. The strip draws nothing otherwise.
    pub open: bool,
    pub mode: CookMode,
    /// Nodes currently stale. Counted only while the strip can show it or a
    /// cook is still working, because the count walks every node.
    pub stale: usize,
    /// Whether a cook is still owed: a manual cook armed and not yet drained,
    /// or an automatic cook that ran out of frame budget with work left.
    pub cooking: bool,
}

impl CookReadout {
    /// The strip's status text, or none when there is nothing to say. A
    /// working cook outranks the count, because the count is about to move.
    pub(crate) fn status_label(&self) -> Option<String> {
        if self.cooking {
            Some("Cooking\u{2026}".to_owned())
        } else if self.mode == CookMode::Manual && self.stale > 0 {
            Some(format!("{} stale", self.stale))
        } else {
            None
        }
    }

    /// Whether the Cook button can be pressed: manual mode, something stale,
    /// and no cook already working.
    pub(crate) fn can_cook(&self) -> bool {
        self.mode == CookMode::Manual && self.stale > 0 && !self.cooking
    }
}

/// The display state the panels draw from, borrowed rather than copied.
#[derive(Clone, Copy)]
pub(crate) struct PanelSettings<'a> {
    /// Every pane's settings. The toolbars read one each; the sidebar, the
    /// menus and the Properties panel read the active one.
    pub panes: &'a [PaneDisplaySettings; 4],
    pub active: usize,
    pub display: &'a DisplaySettings,
    pub post: &'a PostProcessing,
    pub ibl_mode: IblMode,
    pub cameras_linked: bool,
    /// Whether the layout has more than one pane. Read-only: it follows the
    /// layout rather than being set.
    pub is_split: bool,
    /// The active pane camera's projection. Read-only for the same reason:
    /// changing it is an intent, and the camera is what answers afterwards.
    pub projection_mode: ProjectionMode,
    /// The header strip's cook readout, refreshed from the engine each frame.
    pub cook: CookReadout,
    /// How the node canvas routes a wire. A reading preference rather
    /// than anything about the document, which is why it sits with the
    /// display settings and persists with them.
    pub canvas_routing: solarxy_core::preferences::WireRouting,
}

impl PanelSettings<'_> {
    /// The pane a menu or the sidebar acts on, which is whichever one the
    /// cursor last selected.
    pub(crate) fn active_pane(&self) -> &PaneDisplaySettings {
        &self.panes[self.active.min(3)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn readout(mode: CookMode, stale: usize, cooking: bool) -> CookReadout {
        CookReadout {
            open: true,
            mode,
            stale,
            cooking,
        }
    }

    /// The strip says "cooking" while a cook is owed in either mode, the
    /// stale count only in manual mode, and nothing when there is nothing
    /// to say. A working cook outranks the count, because the count is
    /// about to move.
    #[test]
    fn the_strip_says_cooking_only_while_a_cook_is_pending() {
        assert_eq!(
            readout(CookMode::Auto, 0, true).status_label().as_deref(),
            Some("Cooking\u{2026}")
        );
        assert_eq!(
            readout(CookMode::Manual, 3, true).status_label().as_deref(),
            Some("Cooking\u{2026}")
        );
        assert_eq!(
            readout(CookMode::Manual, 3, false)
                .status_label()
                .as_deref(),
            Some("3 stale")
        );
        assert_eq!(readout(CookMode::Manual, 0, false).status_label(), None);
        assert_eq!(readout(CookMode::Auto, 3, false).status_label(), None);
    }

    /// Cook needs manual mode, something stale, and no cook already working.
    #[test]
    fn the_cook_button_needs_manual_mode_and_stale_work() {
        assert!(readout(CookMode::Manual, 1, false).can_cook());
        assert!(!readout(CookMode::Manual, 0, false).can_cook());
        assert!(!readout(CookMode::Manual, 1, true).can_cook());
        assert!(!readout(CookMode::Auto, 1, false).can_cook());
        assert!(!CookReadout::default().can_cook());
    }
}
