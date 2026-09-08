//! The native file pickers, and the two window-level actions that have
//! always sat beside them.
//!
//! Opening a file is not here: what a picker produces is a path, and what a
//! path becomes is `state::open`'s subject.

use crate::gui::ToastSeverity;

use super::super::view_state::ViewLayout;
use super::super::State;

impl State {
    /// One Open for both kinds of file. The first filter therefore lists
    /// scenes and models together, because a user who picks Open knows what
    /// they have rather than which of two dialogs the application wants.
    /// `open_file` routes on the extension, so this and a drag and drop
    /// reach the same place.
    pub fn open_model_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter(
                "Scenes and Models",
                &["slxy", "obj", "stl", "ply", "gltf", "glb"],
            )
            .add_filter("Solarxy Scenes", &["slxy"])
            .add_filter("3D Models", &["obj", "stl", "ply", "gltf", "glb"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.open_file(path);
        }
    }

    pub fn open_hdri_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("HDRI", &["hdr", "exr"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.open_file(path);
        }
    }

    /// Switch the viewport layout. Pane cameras and per-pane settings
    /// stay parked in their slots — `ensure_pane_cameras` fills any
    /// newly-used slot — so toggling between layouts is idempotent
    /// within a session (each pane keeps its own camera).
    pub fn set_view_layout(&mut self, layout: ViewLayout) {
        let prev = self.view.display.layout;
        self.view.display.layout = layout;
        if self.view.active_pane >= layout.pane_count() {
            self.view.active_pane = 0;
        }
        self.ensure_pane_cameras();
        let (tw, th) = self.target_dimensions();
        self.resize_render_targets(tw, th);
        if prev != layout {
            let msg = match layout {
                ViewLayout::Single => "Single Viewport",
                ViewLayout::SplitVertical => "Split Vertical",
                ViewLayout::SplitHorizontal => "Split Horizontal",
                ViewLayout::Quad => "Quad",
                ViewLayout::ThreeLeftBig => "Three-Left-Big",
            };
            self.gui.set_toast(msg, ToastSeverity::Success);
        }
    }

    pub fn toggle_fullscreen(&mut self) {
        use winit::window::Fullscreen;
        let new = if self.window.fullscreen().is_some() {
            None
        } else {
            Some(Fullscreen::Borderless(None))
        };
        self.window.set_fullscreen(new);
    }
}
