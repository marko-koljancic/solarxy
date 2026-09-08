//! Writing preferences back to disk, and the two flushes that run on the way
//! out.

use crate::gui::ToastSeverity;
use solarxy_core::preferences::{self};

use super::State;

impl State {
    /// Snapshot the live view / render / lighting state into
    /// `self.preferences` and write the config file. Returns the I/O
    /// result so callers can toast a context-appropriate message —
    /// [`Self::save_preferences`] is the standard toasting wrapper.
    pub(in crate::state) fn persist_preferences(&mut self) -> Result<(), String> {
        let pds = &self.view.pane_settings[0];
        self.preferences.display.background = pds.background_mode;
        self.preferences.display.view_mode = pds.view_mode;
        self.preferences.display.normals_mode = pds.normals_mode;
        self.preferences.display.grid_visible = pds.show_grid;
        self.preferences.display.axis_gizmo_visible = pds.show_axis_gizmo;
        self.preferences.display.local_axes_visible = pds.show_local_axes;
        self.preferences.display.bloom_enabled = self.renderer.post.bloom_enabled;
        self.preferences.display.ssao_enabled = self.renderer.post.ssao_enabled;
        self.preferences.display.uv_mode = pds.uv_mode;
        self.preferences.display.turntable_active = self.view.display.turntable_active;
        self.preferences.display.turntable_rpm = self.view.display.turntable_rpm;
        if let Some(cam) = &self.view.cameras[0] {
            self.preferences.display.projection_mode = cam.camera.projection;
        }
        self.preferences.rendering.wireframe_line_weight = pds.line_weight;
        self.preferences.lighting.lock = self.view.display.lights_locked;
        self.preferences.display.ibl_mode = self.renderer.ibl_res.ibl_mode;
        self.preferences.display.tone_mode = self.renderer.post.tone_mode;
        self.preferences.display.exposure = self.renderer.post.exposure;
        self.preferences.display.inspection_mode = pds.inspection_mode;
        self.preferences.display.texel_density_target = pds.texel_density_target;
        self.preferences
            .display
            .set_post_strengths(self.renderer.post.strengths());
        preferences::save(&self.preferences)
    }

    pub(in crate::state) fn save_preferences(&mut self) {
        match self.persist_preferences() {
            Ok(()) => {
                self.gui
                    .set_toast("Preferences saved", ToastSeverity::Success);
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Save failed: {}", e), ToastSeverity::Error);
            }
        }
    }

    /// Auto-save the current dock layout into `preferences.dock.last_layout_json`
    /// and flush preferences to disk. Called on app exit so the next launch
    /// restores the layout the user actually left behind. Silent on failure —
    /// the user is on their way out and a toast wouldn't be seen anyway.
    pub fn flush_dock_layout_on_exit(&mut self) {
        let Some(json) = self.gui.serialize_layout() else {
            return;
        };
        if self.preferences.dock.last_layout_json.as_ref() == Some(&json) {
            return;
        }
        self.preferences.dock.last_layout_json = Some(json);
        if let Err(e) = preferences::save(&self.preferences) {
            tracing::warn!("Failed to persist dock layout on exit: {e}");
        }
    }

    /// Flush unsaved review notes to the sidecar on app exit — a data-loss
    /// safety net so quitting mid-review never silently drops annotations.
    /// In-session saving stays manual (the Review panel's Save button /
    /// `Cmd/Ctrl+S`); this only fires when there is something unsaved.
    pub fn flush_review_on_exit(&mut self) {
        if self.review.dirty {
            self.save_review_sidecar();
        }
    }
}
