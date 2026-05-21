use crate::gui::ToastSeverity;
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::resources;

use super::super::view_state::ViewLayout;
use super::super::{PendingHdri, State};

impl State {
    pub fn handle_dropped_file(&mut self, path: std::path::PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && (ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr"))
        {
            let device = self.device.clone();
            let queue = self.queue.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let hdri_path = path.clone();
            std::thread::spawn(move || {
                let _ = tx.send(IblState::from_hdri(&device, &queue, &path));
            });
            self.gui.set_loading_message("Loading HDRI...");
            self.pending_hdri = Some(PendingHdri {
                receiver: rx,
                path: hdri_path,
            });
            return;
        }

        if !resources::is_supported_model_extension(&path) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("none");
            self.gui.set_toast(
                &format!("Unsupported format: .{}", ext),
                ToastSeverity::Error,
            );
            return;
        }

        let model_path = match path.canonicalize() {
            Ok(p) => p.to_string_lossy().to_string(),
            Err(e) => {
                self.gui
                    .set_toast(&format!("Invalid path: {}", e), ToastSeverity::Error);
                return;
            }
        };

        self.spawn_load(model_path);
    }

    pub fn open_model_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("3D Models", &["obj", "stl", "ply", "gltf", "glb"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.handle_dropped_file(path);
        }
    }

    pub fn open_hdri_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("HDRI", &["hdr", "exr"])
            .add_filter("All Files", &["*"])
            .pick_file()
        {
            self.handle_dropped_file(path);
        }
    }

    pub fn close_model(&mut self) {
        self.scene = None;
        self.gui.clear_model_info();
        self.window.set_title("Solarxy");
        self.renderer.uv_overlap.overlap_pct = None;
        self.renderer.uv_overlap.stats_dirty = false;
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
