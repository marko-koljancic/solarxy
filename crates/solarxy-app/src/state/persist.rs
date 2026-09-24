//! The flush that runs on the way out.
//!
//! Nothing here snapshots the live view into the configuration file. That
//! was the job of an entry the browser never had, and the display defaults
//! are written from the Preferences dialog alone. Nothing here tells an
//! upgraded installation anything either: the notice that the keys moved
//! was withdrawn in 0.10.0, since the reference the keys open is generated
//! from the binding table and the release notes carry the change.

use solarxy_core::preferences::{self};

use super::State;

impl State {
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
}
