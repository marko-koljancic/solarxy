//! Cook mode and the explicit cook, as the header strip, the Render menu and
//! the Cmd+Enter chord ask for them.
//!
//! The engine owns both: it holds the mode, keeps the stale set, and decides
//! when a cook runs. This module only dispatches the two commands and keeps
//! the header's readout honest for the frame in between, since the readout
//! is otherwise refreshed from the engine once per frame in `drive_engine`.

use solarxy_graph::engine::{Command, CookMode};

use super::State;
use crate::gui::ToastSeverity;

impl State {
    /// Switch between automatic and manual cooking.
    ///
    /// Switching back to automatic cooks whatever is stale on the next frame;
    /// the engine does that on its own, and nothing here has to remember.
    pub(super) fn set_cook_mode(&mut self, mode: CookMode) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if let Err(e) = engine.apply(Command::SetCookMode { mode }) {
            self.gui
                .set_toast(&format!("Cook mode: {e}"), ToastSeverity::Error);
            return;
        }
        // Written now rather than left to the next frame's refresh, so the
        // toggle does not lag its own click by a frame.
        self.cook_readout.mode = mode;
    }

    /// Cook what is stale now. Meaningful only in manual mode with something
    /// stale; anywhere else the chord and the button do nothing, which is
    /// why the button is disabled there.
    pub fn cook_now(&mut self) {
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if engine.cook_mode() != CookMode::Manual || engine.dirty_nodes().is_empty() {
            return;
        }
        match engine.apply(Command::CookNow) {
            Ok(_) => self.cook_readout.cooking = true,
            Err(e) => self
                .gui
                .set_toast(&format!("Cook now: {e}"), ToastSeverity::Error),
        }
    }
}
