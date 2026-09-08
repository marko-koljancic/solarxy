//! How a file becomes the open document.
//!
//! # One root, one adoption
//!
//! There is one kind of open document. A scene file already is a document; a
//! model file becomes one through [`solarxy_graph::model_document`], the
//! synthesis the terminal's render command and this shell's still render
//! already stand on. So both kinds arrive at [`State::adopt_document`], which
//! is the only place `engine` is assigned, and nothing downstream asks which
//! file was opened.
//!
//! # The build runs off the interface thread
//!
//! Natively an import parses *inside* the cook rather than in a job, so
//! cooking a synthesized document to quiescence blocks for the whole parse.
//! That is the right shape for the terminal and the wrong one for a window,
//! so the read, the staging, the synthesis and the cook all happen on a
//! worker and only the finished engine crosses back. The loading message and
//! the responsive viewport are what that buys, and it is the same arrangement
//! the file loader used before the second root went away.
//!
//! A scene file is not built here: it is cheap to load and cooks
//! progressively on the frame loop, which is what keeps a heavy scene
//! appearing rather than hanging.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;

use solarxy_graph::Engine;

use crate::gui::ToastSeverity;
use solarxy_renderer::ibl::IblState;
use solarxy_renderer::resources;

use super::engine_scene::EngineSceneInfo;
use super::{PendingHdri, PendingOpen, State, preferences};

/// A model built into a document on the worker, with whatever its companion
/// walk had to say about the files it could not read.
pub(crate) struct OpenedModel {
    engine: Box<Engine>,
    warnings: Vec<String>,
}

/// Builds the document a model file is: companions collected and staged, the
/// document synthesized, and the cook driven to quiescence.
///
/// Called on a worker thread and given no shell state, which is what lets it
/// be one function rather than a second copy of the sequence. Bytes are read
/// here rather than handed in, so the whole of the expensive half is on the
/// far side of the channel.
///
/// `cancel` reaches the cook itself, so opening another file or quitting
/// stops a long parse between nodes instead of waiting it out. The pass
/// callback the loop offers is deliberately unused: a synthesized document is
/// one import and settles in two or three passes, so a pass number describes
/// nothing a reader of a loading message would recognise.
fn build_model_document(path: &Path, cancel: &Arc<AtomicBool>) -> Result<OpenedModel, String> {
    use solarxy_graph::model_document;

    let bytes =
        std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model")
        .to_string();

    let mut engine = Engine::new().map_err(|e| e.to_string())?;
    // Companions before the primary, so a required companion that cannot be
    // read fails naming the file rather than later, inside the cook, as a
    // parse failure naming something else. The terminal's adapter states the
    // same rule at its own copy of this ordering.
    let companions =
        solarxy_formats::companions::collect(path, &ext, &bytes).map_err(|e| e.to_string())?;
    let warnings = companions.warnings;
    for asset in companions.assets {
        engine.stage_asset(asset.name, String::new(), asset.bytes);
    }
    model_document::synthesize_model_document(&mut engine, &name, &ext, bytes)
        .map_err(|e| e.to_string())?;

    let mut cancelled = || cancel.load(Ordering::Relaxed);
    model_document::cook_to_quiescence(&mut engine, &mut cancelled, &mut |_, _| {})
        .map_err(|e| e.to_string())?;

    Ok(OpenedModel {
        engine: Box::new(engine),
        warnings,
    })
}

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

    /// The one router for opening a file of any supported kind: HDRI, scene,
    /// or model. The Open dialogs, a drag and drop, the startup argument, and
    /// Recent Files all land here, so extension routing exists exactly once.
    pub fn open_file(&mut self, path: std::path::PathBuf) {
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && (ext.eq_ignore_ascii_case("hdr") || ext.eq_ignore_ascii_case("exr"))
        {
            let device = self.device.clone();
            let queue = self.queue.clone();
            let (tx, rx) = mpsc::channel();
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

        self.spawn_open_model(model_path);
    }

    /// Start building a model file into a document on a worker thread.
    ///
    /// An open already in flight is cancelled rather than orphaned: its cook
    /// stops between nodes and its result is dropped, so two quick opens cost
    /// one parse rather than two.
    pub(super) fn spawn_open_model(&mut self, model_path: String) {
        if let Some(previous) = self.pending_open.take() {
            previous.cancel.store(true, Ordering::Relaxed);
        }

        let filename = Path::new(&model_path)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(&model_path)
            .to_string();

        self.gui
            .set_loading_message(&format!("Loading {}...", filename));

        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let path = model_path.clone();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = build_model_document(Path::new(&path), &worker_cancel);
            let _ = tx.send(result);
        });

        self.pending_open = Some(PendingOpen {
            receiver: rx,
            filename,
            path: model_path,
            cancel,
        });
    }

    /// Poll the worker, and adopt what it built.
    ///
    /// Framing happens here rather than a frame later: the document arrived
    /// already cooked, so applying its delta at adoption makes the scene's
    /// bounds known immediately and the panes frame the geometry rather than
    /// the empty placeholder.
    pub(super) fn poll_pending_open(&mut self) {
        let Some(pending) = self.pending_open.take() else {
            return;
        };
        match pending.receiver.try_recv() {
            Ok(Ok(opened)) => {
                let file_size = std::fs::metadata(&pending.path).map_or(0, |m| m.len());
                for w in &opened.warnings {
                    tracing::warn!("{w}");
                }
                self.adopt_document(opened.engine, &pending.filename, &pending.path, file_size);
                self.view.cameras = [None, None, None, None];
                self.pending_frame = [false; 4];
                self.ensure_pane_cameras();
                self.reset_pane_zero_for_new_document();

                self.gui.clear_loading_message();
                if let Some(first) = opened.warnings.first() {
                    self.gui.set_toast(first, ToastSeverity::Warning);
                } else {
                    self.gui.set_toast(
                        &format!("Opened {}", pending.filename),
                        ToastSeverity::Success,
                    );
                }
            }
            Ok(Err(message)) => {
                self.gui.clear_loading_message();
                self.gui
                    .set_toast(&format!("Failed to load: {message}"), ToastSeverity::Error);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pending_open = Some(pending);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.gui.clear_loading_message();
                self.gui
                    .set_toast("Loading thread crashed", ToastSeverity::Error);
            }
        }
    }

    /// Open a scene file: adopt it, and take the cameras and environment it
    /// was saved with.
    ///
    /// Loaded into a fresh engine rather than the live one, so a file that
    /// fails the schema gate or the integrity check leaves whatever is
    /// already open untouched.
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

        let mut engine = match Engine::new() {
            Ok(e) => Box::new(e),
            Err(e) => {
                self.gui
                    .set_toast(&format!("Engine unavailable: {e}"), ToastSeverity::Error);
                return;
            }
        };

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

        let warnings = loaded.warnings.len();
        let view = loaded.sidecar.view.clone();
        let environment = loaded.sidecar.environment.clone();

        let display_path = path.canonicalize().map_or_else(
            |_| path.display().to_string(),
            |p| p.to_string_lossy().to_string(),
        );
        let file_size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        self.adopt_document(engine, &filename, &display_path, file_size);

        self.apply_scene_view(&view);
        self.restore_scene_environment(&environment);

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

        // Nothing has cooked, so `bounds` above is the previous document's or
        // the placeholder. Every pane the file does not supply a camera for is
        // therefore marked for framing once the cook produces some.
        self.pending_frame = [true; 4];

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
                self.pending_frame[i] = false;
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

    /// Frame any pane the open left waiting, once the document has bounds.
    ///
    /// One rule for both kinds of file: a pane frames on the cooked scene's
    /// visible bounds. What differs is only when those bounds exist, which is
    /// at adoption for a model file and some frames later for a scene file
    /// that is still cooking. The waiting slots are dropped rather than
    /// re-framed in place, so the seeding that runs every frame rebuilds them
    /// exactly as it would have with the bounds in hand, including the
    /// orthographic Top, Front and Left views slots one to three start on.
    ///
    /// A document that never produces bounds is empty, and an empty document
    /// keeps the placeholder framing it already has.
    pub(super) fn apply_pending_frame(&mut self) {
        if !self.pending_frame.iter().any(|p| *p) {
            return;
        }
        let Some(bounds) = self.raster.scene().visible_bounds() else {
            return;
        };
        for (slot, pending) in self.view.cameras.iter_mut().zip(self.pending_frame) {
            if pending {
                *slot = None;
            }
        }
        self.pending_frame = [false; 4];
        self.ensure_pane_cameras_with(&bounds);
    }

    /// Install a freshly built document as the open one.
    ///
    /// **The only place `engine` is assigned.** Everything a document swap has
    /// to forget lives here rather than at each opening site, which is what
    /// makes a model file and a scene file the same act: bindings name camera
    /// nodes of the outgoing document, the cook ledger describes it, the
    /// thumbnail cache is keyed only by material index and role, and the
    /// tree's dived context addresses node ids the new document need not
    /// contain. A still mid-render over the outgoing scene is cancelled
    /// rather than left rendering a document that no longer exists.
    ///
    /// Called only on success, so a failed open never reaches it.
    fn adopt_document(&mut self, engine: Box<Engine>, filename: &str, path: &str, file_size: u64) {
        self.clear_scene_objects();
        self.environment.invalidate();
        self.look_through = [None; 4];
        self.cook_health.clear();
        self.cancel_still_render();

        let mut engine = engine;
        // Queued behind the `Clear` above, so the outgoing document leaves and
        // the incoming one arrives at one commit point. Empty for a scene file,
        // which has not cooked yet and fills in over the frame loop.
        let delta = engine.take_scene_delta();
        if !delta.ops.is_empty() {
            self.pending_scene_deltas.push(delta);
        }

        self.engine = Some(engine);
        // Identity now; the counters and the merged report fill in as the
        // delta drains.
        self.engine_scene = Some(EngineSceneInfo::new(
            filename.to_string(),
            path.to_string(),
            file_size,
        ));
        self.gui.reset_node_tree();
        self.selected_object = None;

        preferences::add_recent_file(&mut self.preferences, path);
        self.window.set_title(&format!("Solarxy - {filename}"));

        // Applied here rather than left to the top of the next frame, so what
        // follows an open reads the document rather than the one before it. A
        // model file arrived cooked and its geometry lands now, which is what
        // lets its panes be framed on real bounds in this same call; a scene
        // file has only the `Clear` to apply and fills in over the frame loop.
        self.apply_pending_scene_deltas();
    }

    /// Put the primary pane back to the display settings a new document
    /// starts on.
    ///
    /// A scene file overrides these from its own saved view; a model file has
    /// none to override them with, so this is what it opens as.
    fn reset_pane_zero_for_new_document(&mut self) {
        use solarxy_core::preferences::{InspectionMode, PaneMode, UvMapBackground, ViewMode};

        let pane = &mut self.view.pane_settings[0];
        pane.view_mode = self.preferences.display.view_mode;
        pane.prev_non_ghosted_mode = ViewMode::Shaded;
        pane.ghosted_wireframe = false;
        pane.normals_mode = self.preferences.display.normals_mode;
        pane.uv_mode = self.preferences.display.uv_mode;
        pane.inspection_mode = InspectionMode::Shaded;
        pane.texel_density_target = 1.0;
        pane.pane_mode = PaneMode::Scene3D;
        pane.uv_bg = UvMapBackground::Dark;
        pane.uv_offset = [0.0, 0.0];
        pane.uv_zoom = 1.0;
        pane.show_uv_overlap = false;
        pane.show_validation = false;
        self.renderer.uv_overlap.overlap_pct = None;
        self.renderer.uv_overlap.stats_dirty = false;
        self.view.display.turntable_active = self.preferences.display.turntable_active;
    }

    /// Close the open document.
    pub fn close_document(&mut self) {
        self.engine = None;
        self.engine_scene = None;
        self.look_through = [None; 4];
        self.cook_health.clear();
        self.cancel_still_render();
        self.clear_scene_objects();
        self.environment.invalidate();
        self.reset_env_for_empty_scene();
        self.gui.clear_model_info();
        self.gui.reset_node_tree();
        self.selected_object = None;
        self.window.set_title("Solarxy");
        self.renderer.uv_overlap.overlap_pct = None;
        self.renderer.uv_overlap.stats_dirty = false;
    }
}

/// **Opening a model file and opening its equivalent scene produce the same
/// document.** That is the whole claim of the one-root change, and it is
/// checkable without a device because a document is data.
///
/// Compared through `serde_json::Value` rather than field by field: the
/// document's own serialized form is what a scene file carries, so a
/// comparison over it covers every field a future one gains without anybody
/// remembering to add it here. `DocumentSnapshot` cannot be used for this, and
/// the reason is worth knowing rather than rediscovering: it derives
/// `Serialize` but not `PartialEq`.
#[cfg(test)]
mod tests {
    use super::*;

    fn model(rel: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../res/models")
            .join(rel)
    }

    fn shape(engine: &Engine) -> serde_json::Value {
        serde_json::to_value(engine.document().to_data()).expect("a document serializes")
    }

    fn build(path: &std::path::Path) -> OpenedModel {
        let cancel = Arc::new(AtomicBool::new(false));
        build_model_document(path, &cancel)
            .unwrap_or_else(|e| panic!("{} did not open: {e}", path.display()))
    }

    /// The round trip, on a model that names companions beside itself, so the
    /// staging half is exercised rather than assumed.
    #[test]
    fn a_model_and_the_scene_it_saves_as_open_to_the_same_document() {
        let path = model("knot/knot.obj");
        assert!(path.exists(), "the bundled knot model is missing");
        let opened = build(&path);

        let bytes = opened
            .engine
            .save_slxy(&solarxy_graph::engine::SceneSidecar::default())
            .expect("the synthesized document saves");
        let mut reopened = Engine::new().expect("engine");
        reopened.load_slxy(&bytes).expect("the saved scene opens");

        assert_eq!(
            shape(&opened.engine),
            shape(&reopened),
            "a model file and the scene it saves as are not the same document"
        );
    }

    /// The other half of the same claim: this shell and the terminal's render
    /// command build one document from one file. They call the same synthesis,
    /// and this is what keeps a second path from quietly appearing beside it.
    #[test]
    fn this_shell_and_the_terminal_build_the_same_document() {
        let path = model("knot/knot.obj");
        let opened = build(&path);
        let headless = solarxy_render::input::load(&path, None, &mut solarxy_render::Silent)
            .expect("the terminal opens the same model");

        assert_eq!(
            shape(&opened.engine),
            shape(&headless.engine),
            "the two native shells build different documents from one model"
        );
    }

    /// A model that cannot be parsed reports why, and reports it as a failure
    /// rather than handing back an empty document that would open cleanly and
    /// render nothing.
    #[test]
    fn a_model_that_will_not_parse_reports_why() {
        let dir = std::env::temp_dir().join("solarxy-open-tests");
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("broken.obj");
        std::fs::write(&path, b"f 1 2 3\n").expect("write");

        let cancel = Arc::new(AtomicBool::new(false));
        match build_model_document(&path, &cancel) {
            Err(message) => assert!(!message.is_empty(), "the failure carries no reason"),
            Ok(_) => panic!("a faceless OBJ is not a model"),
        }
    }

    /// Cancellation reaches the cook, which is what lets a second open stop
    /// the first between nodes rather than waiting out its parse.
    #[test]
    fn a_cancelled_open_does_not_report_success() {
        let path = model("knot/knot.obj");
        let cancel = Arc::new(AtomicBool::new(true));
        assert!(
            build_model_document(&path, &cancel).is_err(),
            "a cancelled open must not report success"
        );
    }
}
