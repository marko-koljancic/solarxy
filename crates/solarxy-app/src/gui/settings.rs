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
use solarxy_renderer::frame::PostProcessing;

use crate::state::view_state::{DisplaySettings, PaneDisplaySettings};

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
}

impl PanelSettings<'_> {
    /// The pane a menu or the sidebar acts on, which is whichever one the
    /// cursor last selected.
    pub(crate) fn active_pane(&self) -> &PaneDisplaySettings {
        &self.panes[self.active.min(3)]
    }
}
