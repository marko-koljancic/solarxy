use crate::gui::ToastSeverity;
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::resources;

use super::super::view_state::ViewLayout;
use super::super::{PendingHdri, State};

/// Point a camera at what a saved pane was looking at.
///
/// A near-copy of the web shell's equivalent. The shared home for it would
/// have to know both the scene format and the renderer, and no crate beneath
/// the two shells knows either, so it stays doubled rather than dragging the
/// format into the renderer.
fn apply_camera_json(
    cam: &mut solarxy_renderer::camera::Camera,
    json: &solarxy_scenefile::CameraJson,
) {
    let target = cgmath::Point3::new(json.target[0], json.target[1], json.target[2]);
    let cp = json.pitch.cos();
    let dir = cgmath::Vector3::new(cp * json.yaw.sin(), json.pitch.sin(), cp * json.yaw.cos());
    cam.target = target;
    cam.eye = target + dir * json.distance.max(1e-4);
    // A fixed +Y up is degenerate for a scene saved looking straight down,
    // where it is parallel to the view direction.
    cam.up = solarxy_renderer::camera::turntable_up(json.yaw, json.pitch);
    if json.fov_y > 0.0 {
        cam.fovy = json.fov_y.to_degrees();
    }
    cam.projection = if json.projection == "orthographic" {
        solarxy_core::preferences::ProjectionMode::Orthographic
    } else {
        solarxy_core::preferences::ProjectionMode::Perspective
    };
    if json.ortho_scale > 0.0 {
        cam.ortho_scale = json.ortho_scale;
    }
}

impl State {
    /// Drop every cooked object through the ordinary delta path, so the
    /// removal is applied at the same commit point as everything else rather
    /// than reaching into the renderer's state from a dialog handler.
    pub(crate) fn clear_scene_objects(&mut self) {
        self.pending_scene_deltas
            .push(solarxy_core::scene::SceneDelta {
                ops: vec![solarxy_core::scene::SceneOp::Clear],
            });
    }

    /// The one router for opening a file of any supported kind: HDRI,
    /// scene, or model. The Open dialogs, a drag and drop, the startup
    /// argument, and Recent Files all land here, so extension routing
    /// exists exactly once.
    pub fn open_file(&mut self, path: std::path::PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && (ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr"))
        {
            let device = self.device.clone();
            let queue = self.queue.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            let hdri_path = path.clone();
            std::thread::spawn(move || {
                // The channel carries anyhow (binary-crate convention); the
                // renderer's typed error converts at the boundary.
                let _ = tx
                    .send(IblState::from_hdri(&device, &queue, &path).map_err(anyhow::Error::from));
            });
            self.gui.set_loading_message("Loading HDRI...");
            self.pending_hdri = Some(PendingHdri {
                receiver: rx,
                path: hdri_path,
            });
            return;
        }

        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("slxy"))
        {
            self.open_scene(&path);
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

    /// Open a scene file: cook it, render it, and adopt the cameras it was
    /// saved with.
    ///
    /// A scene and a file model are mutually exclusive, so this closes any
    /// open model first, which also flushes unsaved review notes. The
    /// converse lives in `spawn_load`.
    pub fn open_scene(&mut self, path: &std::path::Path) {
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("scene")
            .to_string();

        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                self.gui.set_toast(
                    &format!("Cannot read {filename}: {e}"),
                    ToastSeverity::Error,
                );
                return;
            }
        };

        let mut engine = match solarxy_graph::Engine::new() {
            Ok(e) => Box::new(e),
            Err(e) => {
                self.gui
                    .set_toast(&format!("Engine unavailable: {e}"), ToastSeverity::Error);
                return;
            }
        };

        // Loaded into a fresh engine rather than the live one, so a file that
        // fails the schema gate or the integrity check leaves whatever is
        // already open untouched.
        let loaded = match engine.load_slxy(&bytes) {
            Ok(l) => l,
            Err(e) => {
                self.gui.set_toast(
                    &format!("Cannot open {filename}: {e}"),
                    ToastSeverity::Error,
                );
                return;
            }
        };

        if self.scene.is_some() {
            self.close_model();
        }
        self.clear_scene_objects();
        self.environment.invalidate();
        // Bindings name camera nodes of the previous document; a fresh scene
        // starts on free views. The cook ledger describes the previous
        // document too, and the new engine re-reports any failure. A still
        // mid-render over the outgoing scene is cancelled rather than left
        // rendering a document that no longer exists.
        self.look_through = [None; 4];
        self.cook_health.clear();
        self.cancel_still_render();

        let warnings = loaded.warnings.len();
        let view = loaded.sidecar.view.clone();
        let environment = loaded.sidecar.environment.clone();
        self.engine = Some(engine);
        // Identity now; the counters and the merged report fill in as the
        // first cook's delta drains, since nothing has cooked yet.
        self.engine_scene = Some(crate::state::engine_scene::EngineSceneInfo::new(
            filename.clone(),
            path.display().to_string(),
            u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        ));
        // A previous document's thumbnails are keyed only by material index
        // and role, so without this the new scene's slots would be served
        // stale textures.
        self.gui.reset_material_inspector();
        // The tree's dived context and folded set both address node ids,
        // and the new document need not contain any of them.
        self.gui.reset_node_tree();
        self.selected_object = None;

        self.apply_scene_view(&view);
        self.restore_scene_environment(&environment);

        // After every failure return, so a scene that did not open leaves
        // no entry. Canonicalized to the form the model loader records, so
        // deduplication works across both kinds in the one list.
        let recent = path.canonicalize().map_or_else(
            |_| path.display().to_string(),
            |p| p.to_string_lossy().to_string(),
        );
        solarxy_core::preferences::add_recent_file(&mut self.preferences, &recent);

        self.window.set_title(&format!("Solarxy - {filename}"));
        if warnings > 0 {
            // One toast for the batch. A scene that lost several assets would
            // otherwise stack a queue the user has to sit through, and the
            // queue caps at five, so the count is the honest summary.
            self.gui.set_toast(
                &format!("Opened {filename} with {warnings} warning(s)"),
                ToastSeverity::Warning,
            );
        } else {
            self.gui
                .set_toast(&format!("Opened {filename}"), ToastSeverity::Success);
        }
    }

    /// Adopt the per-pane cameras and display settings a scene was saved
    /// with, and leave the viewport arrangement alone.
    ///
    /// The file also carries a layout, a split ratio and an active pane, and
    /// this deliberately ignores all three. On the web the saved view is the
    /// whole window and restoring it is obviously right; here the arrangement
    /// is something the user set up around their own work, and clobbering it
    /// on every open would be a worse trade than opening a scene framed as
    /// authored inside the panes they already have.
    fn apply_scene_view(&mut self, view: &solarxy_scenefile::ViewJson) {
        let bounds = self.scene_bounds();
        let (tw, th) = self.target_dimensions();
        let aspect = tw as f32 / th.max(1) as f32;

        for (i, pane) in view.panes.iter().take(4).enumerate() {
            if !pane.display.is_empty() {
                let value = serde_json::Value::Object(pane.display.clone().into_iter().collect());
                if let Ok(mut settings) =
                    serde_json::from_value::<solarxy_core::view_config::PaneDisplaySettings>(value)
                {
                    // Both are session-temporary by the same rule the web
                    // applies: a reopened scene starts Textured and still,
                    // whatever it was saved mid-inspection as.
                    settings.material_override = solarxy_core::preferences::MaterialOverride::None;
                    settings.turntable_active = false;
                    self.view.pane_settings[i] = settings;
                }
            }
            if pane.camera.distance > 0.0 {
                let cam = self.view.cameras[i].get_or_insert_with(|| {
                    solarxy_renderer::camera_state::CameraState::new(
                        &self.device,
                        &self.renderer.layouts.camera,
                        &bounds,
                        aspect,
                    )
                });
                apply_camera_json(&mut cam.camera, &pane.camera);
            }
        }
        self.ensure_pane_cameras();
    }

    /// Put back the lighting environment a scene was saved with.
    ///
    /// Only the sidecar half. A scene whose environment comes from a node
    /// installs it through the ordinary delta path when that node cooks, and
    /// the node wins where the two disagree, so this returns early rather
    /// than racing it.
    fn restore_scene_environment(&mut self, env: &solarxy_scenefile::EnvironmentJson) {
        if let Some(rotation) = env
            .background
            .get("hdriRotation")
            .and_then(serde_json::Value::as_f64)
        {
            self.view.display.hdri_rotation = rotation as f32;
        }
        if self
            .engine
            .as_ref()
            .is_some_and(|e| e.has_environment_node())
        {
            return;
        }

        let Some(hash) = env.hdri_asset.clone() else {
            return;
        };
        let id = solarxy_graph::params::AssetId(hash);
        let Some(bytes) = self
            .engine
            .as_ref()
            .and_then(|e| e.asset_bytes(&id))
            .map(<[u8]>::to_vec)
        else {
            return;
        };

        // Decoded here rather than on a worker: this is the native path, the
        // decode is a few hundred milliseconds on a large map, and it happens
        // once during an open the user is already waiting on.
        let ibl = if bytes.starts_with(b"#?") {
            IblState::from_hdr_bytes(&self.device, &self.queue, &bytes)
        } else {
            IblState::from_exr_bytes(&self.device, &self.queue, &bytes)
        };
        match ibl {
            Ok(ibl) => {
                self.renderer.ibl_res.ibl = ibl;
                self.environment.invalidate();
                self.rebuild_light_bind_group();
            }
            Err(e) => {
                self.gui.set_toast(
                    &format!("Scene environment could not be restored: {e}"),
                    ToastSeverity::Warning,
                );
            }
        }
    }

    /// Close whichever of the two roots is open.
    pub fn close_document(&mut self) {
        if self.engine.is_some() {
            self.close_scene();
        } else {
            self.close_model();
        }
    }

    /// Drop the engine and everything it put on the GPU.
    pub fn close_scene(&mut self) {
        self.engine = None;
        self.engine_scene = None;
        self.look_through = [None; 4];
        self.cook_health.clear();
        self.cancel_still_render();
        self.clear_scene_objects();
        self.environment.invalidate();
        self.reset_env_for_empty_scene();
        self.gui.clear_model_info();
        // The thumbnail cache is keyed by (material index, role) with no
        // notion of which document filled it, so it has to be dropped on
        // the way out of a scene exactly as it is on a model swap.
        self.gui.reset_material_inspector();
        self.gui.reset_node_tree();
        self.selected_object = None;
        self.window.set_title("Solarxy");
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

    pub fn close_model(&mut self) {
        // Flush unsaved review notes before dropping the model's review
        // state — consistent with the model-swap / app-exit flushes.
        if self.review.dirty {
            self.save_review_sidecar();
        }
        self.review.clear_for_new_model();
        self.scene = None;
        self.reset_env_for_empty_scene();
        // The pane cameras deliberately survive. Closing a model leaves a
        // viewport, not a void: the background, grid, floor and axes keep
        // rendering through the full pass chain, and anything the
        // multi-object scene holds keeps drawing. Clearing them used to be
        // how the composite was kept off the closed model's last bloom and
        // occlusion textures, which the empty path leaves resident because it
        // encodes neither pass; `scene_present` answers that directly now, so
        // the cameras no longer have to disappear to make it true.
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
