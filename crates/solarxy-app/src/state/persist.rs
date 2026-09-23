//! The two flushes that run on the way out, and the one notice an upgraded
//! installation is given on the way in.
//!
//! Nothing here snapshots the live view into the configuration file. That
//! was the job of an entry the browser never had, and the display defaults
//! are written from the Preferences dialog alone.

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

impl State {
    /// Tell an existing installation, once, that the keyboard map changed.
    ///
    /// The flag distinguishes the two populations, and a configuration file
    /// on disk distinguishes them again: a file written before the two shells
    /// shared a map carries no flag, while a fresh installation has no file
    /// at all and is set without being told, because there is nothing it
    /// knew that changed.
    pub(crate) fn check_keymap_notice_on_launch(&mut self) {
        if self.preferences.ui.keymap_notice_seen {
            return;
        }
        let upgraded = solarxy_core::preferences::config_path()
            .is_some_and(|path| std::fs::metadata(path).is_ok());
        if upgraded {
            self.gui.open_keymap_notice();
        } else {
            self.preferences.ui.keymap_notice_seen = true;
        }
    }
}
