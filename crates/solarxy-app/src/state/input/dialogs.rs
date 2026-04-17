use crate::gui::ToastSeverity;
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::resources;
use solarxy_core::preferences::PaneMode;

use super::super::view_state::ViewLayout;
use super::super::State;

impl State {
    pub fn handle_dropped_file(&mut self, path: std::path::PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && (ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr"))
        {
            let device = self.device.clone();
            let queue = self.queue.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(IblState::from_hdri(&device, &queue, &path));
            });
            self.gui.set_loading_message("Loading HDRI...");
            self.pending_hdri = Some(rx);
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

    pub fn set_view_layout(&mut self, layout: ViewLayout) {
        match layout {
            ViewLayout::Single => {
                if self.view.display.layout != ViewLayout::Single {
                    if self.view.active_pane == 1 {
                        if let Some(sec) = self.view.secondary_cam.take()
                            && let Some(scene) = &mut self.scene
                        {
                            scene.cam = sec;
                        }
                    } else {
                        self.view.secondary_cam = None;
                    }
                }
                if self.view.active_pane == 1 {
                    self.view.pane_settings[0] = self.view.pane_settings[1];
                }
                self.view.active_pane = 0;
                self.view.display.layout = ViewLayout::Single;
                self.gui
                    .set_toast("Single Viewport", ToastSeverity::Success);
            }
            ViewLayout::SplitVertical | ViewLayout::SplitHorizontal => {
                if self.view.display.layout == ViewLayout::Single {
                    self.view.pane_settings[1] = self.view.pane_settings[0];
                    self.view.pane_settings[0].pane_mode = PaneMode::UvMap;
                    self.view.pane_settings[0].uv_offset = [0.0, 0.0];
                    self.view.pane_settings[0].uv_zoom = 1.0;
                    self.view.pane_settings[1].pane_mode = PaneMode::Scene3D;
                    if let Some(scene) = &self.scene {
                        self.view.secondary_cam =
                            Some(scene.cam.clone_with_new_resources(
                                &self.device,
                                &self.renderer.layouts.camera,
                            ));
                    }
                }
                self.view.display.layout = layout;
                let msg = if matches!(layout, ViewLayout::SplitVertical) {
                    "Split Vertical"
                } else {
                    "Split Horizontal"
                };
                self.gui.set_toast(msg, ToastSeverity::Success);
            }
        }
        let (tw, th) = self.target_dimensions();
        self.resize_render_targets(tw, th);
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
