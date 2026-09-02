//! The desktop still render: the Render menu's entry into the tiled
//! still job both shells share.
//!
//! The shape mirrors the web shell deliberately, piece for piece: the
//! render node is the one authority for what renders (engine, size,
//! samples, camera), the engine scene is snapshotted into the tracer on
//! **every** start (a tracer kept alive between stills sees no deltas,
//! which is the staleness defect the web shipped and fixed), the
//! environment syncs host-side because the traced scene cache drops the
//! environment op by design, and a lightless scene takes the synthesized
//! viewer rig as scene data so both renderers light it the same way.
//!
//! While a job runs it owns the frame: panes are not rendered, because
//! the job and the viewport would fight over the shared render targets
//! at different sizes every frame.

use solarxy_core::preferences::BackgroundMode;
use solarxy_core::view_config::{PaneDisplaySettings, PaneLook};
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::nodes::{RenderEngine, RenderSettings};
use solarxy_host::still::{StillEngine, StillSpec, StillStep, StillTile, TILE_BUDGET_PIXELS, TileRect};
use solarxy_host::{StillCtx, StillRenderJob};
use solarxy_renderer::backend::RenderBackend;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::{CompositeLook, resolve_look};
use solarxy_renderer::lut::LutSlot;
use solarxy_renderer::pathtrace::backend::{PathBackend, TraceSettings};
use solarxy_renderer::pathtrace::denoise::DenoiseSettings;
use solarxy_renderer::scene::BackgroundModeExt;

use super::State;
use super::update::find_node_name;

/// The largest edge of the modal's live preview. Small enough that the
/// per-tile nearest-neighbour downscale reads only preview-many pixels.
const PREVIEW_MAX_EDGE: u32 = 640;

/// One still render in flight, owned by the shell.
pub(crate) struct StillState {
    pub job: StillRenderJob,
    /// Job-owned camera. No pane is moved by a render.
    pub camera: CameraState,
    pub look: CompositeLook,
    /// The active pane's background at start time: a scene that authored
    /// a sky is shot against it.
    pub background: BackgroundMode,
    pub engine: StillEngine,
    /// The assembled picture, RGBA8, `width * height * 4`. What the modal
    /// previews and what a PNG is written from.
    pub image: Vec<u8>,
    /// The assembled floating-point picture, when the render is one. `None`
    /// for an eight-bit still, which costs nothing.
    pub float: Option<solarxy_host::still::FloatImage>,
    /// Whether the job ingested a synthesized model document into the
    /// session's scene objects, which the teardown then clears.
    pub synthesized_scene: bool,
    /// When the render began, which is the shell's own clock: the job takes a
    /// reading rather than reading one, because it also compiles for the
    /// browser where there is no `Instant`.
    pub started: std::time::Instant,
}

impl State {
    /// Open the still dialog without rendering anything.
    ///
    /// The menu's entry point. The job starts when Render is pressed, which is
    /// what gives the output format somewhere to be chosen: the format decides
    /// what the renderer reads back and so cannot be settled at save time. The
    /// browser dialog has always worked this way; this is the desktop catching
    /// up to it.
    pub(super) fn open_still_dialog(&mut self) {
        if self.still.is_some() {
            return;
        }
        let Some((_, settings)) = self.resolve_still_request() else {
            return;
        };
        self.gui.open_still_modal(
            settings.width,
            settings.height,
            settings.engine == RenderEngine::PathTraced,
            settings.samples,
            settings.denoise,
            still_filename(),
        );
    }

    /// What this render would be, or nothing with the reason already said.
    ///
    /// Shared by the dialog's opening and by the render it starts, and run
    /// again for the render rather than carried over from the opening: what
    /// runs is what the node says at the moment Render is pressed, which is the
    /// rule the browser host states for the same reason.
    fn resolve_still_request(
        &mut self,
    ) -> Option<(Option<Box<solarxy_graph::Engine>>, RenderSettings)> {
        use crate::gui::ToastSeverity;
        // Which root renders: the open engine as before, or a document
        // synthesized from the open model, so the desktop renders the file
        // it is displaying the way the terminal already does. One synthesis
        // in the product (`solarxy_graph::model_document`, shared with the
        // headless command); the throwaway engine lives only as long as
        // this start and never becomes `State::engine`, so the two-roots
        // invariant stands unamended.
        let synthesized: Option<Box<solarxy_graph::Engine>> = if self.engine.is_some() {
            None
        } else {
            let Some(path) = self.scene.as_ref().map(|s| s.model_path.clone()) else {
                self.gui.set_toast(
                    "Open a scene or a model to render a still",
                    ToastSeverity::Warning,
                );
                return None;
            };
            match engine_for_model(&path) {
                Ok((engine, warnings)) => {
                    // The terminal's channel and one toast: a missing
                    // optional companion should not be only in the log.
                    for w in &warnings {
                        tracing::warn!("{w}");
                    }
                    if let Some(first) = warnings.first() {
                        self.gui.set_toast(first, ToastSeverity::Warning);
                    }
                    Some(engine)
                }
                Err(message) => {
                    self.gui.set_toast(&message, ToastSeverity::Error);
                    return None;
                }
            }
        };

        // The health gate reads the session engine's cook. The synthesized
        // document was just cooked to quiescence, and a failure there has
        // already returned with its own message.
        if synthesized.is_none() {
            let engine = self.engine.as_ref()?;
            if !self.cook_health.is_healthy() {
                let failing = self.cook_health.failing();
                let message = failing.iter().next().map_or_else(
                    || "Cannot render: a cook failed".to_owned(),
                    |(id, reason)| {
                        let name = find_node_name(engine, *id);
                        let more = failing.len() - 1;
                        if more == 0 {
                            format!("Cannot render: {name} failed to cook: {reason}")
                        } else {
                            format!(
                                "Cannot render: {name} failed to cook: {reason} (and {more} more)"
                            )
                        }
                    },
                );
                self.gui.set_toast(&message, ToastSeverity::Error);
                return None;
            }
        }
        let engine: &solarxy_graph::Engine = match synthesized.as_deref() {
            Some(e) => e,
            None => self.engine.as_deref()?,
        };

        let settings = match resolve_still_settings(engine) {
            Ok((settings, note)) => {
                if let Some(note) = note {
                    self.gui.set_toast(&note, ToastSeverity::Info);
                }
                settings
            }
            Err(message) => {
                self.gui.set_toast(&message, ToastSeverity::Error);
                return None;
            }
        };
        Some((synthesized, settings))
    }

    /// Start the still render the dialog is showing.
    ///
    /// The refusals come first and each names its reason: a still over a
    /// scene whose cook silently failed would be a wrong picture behind
    /// a clean progress bar.
    pub(super) fn start_still_render(&mut self) {
        use crate::gui::ToastSeverity;

        if self.still.is_some() {
            return;
        }
        let Some((synthesized, settings)) = self.resolve_still_request() else {
            return;
        };

        let width = settings.width;
        let height = settings.height;
        let aspect = width as f32 / height as f32;

        // The shot's camera: the named camera node when the render node
        // names one and the cooked scene carries it, else the active
        // pane's view. Job-owned either way.
        let ap = self.view.active_pane;
        let mut camera = match self.view.cameras[ap].as_ref() {
            Some(c) => {
                CameraState::from_camera(&self.device, &self.renderer.layouts.camera, c.camera)
            }
            None => CameraState::new(
                &self.device,
                &self.renderer.layouts.camera,
                &self.scene_bounds(),
                aspect,
            ),
        };
        let mut cam_look = None;
        // The lens follows the look exactly, and for the same reason: both
        // describe the shot rather than the viewport, so both take the named
        // camera when there is one and the pane's camera when there is not.
        let mut cam_lens = None;
        if let Some(cam_node) = settings.camera
            && let Some(def) = self
                .raster
                .scene()
                .cameras()
                .and_then(|cams| cams.iter().find(|c| c.id.0 == cam_node.0))
        {
            solarxy_host::cameras::apply_camera_def(&mut camera.camera, def);
            cam_look = Some(def.look.clone());
            cam_lens = Some(solarxy_host::cameras::lens_for(def));
        }
        // A render node naming no camera means the shot is the active pane's
        // view, and that pane may itself be looking through a graded camera.
        // Falling back to tone and exposure alone would drop a grade the pane
        // is showing, so the still would not be a photograph of what was
        // framed. The pose needs no such fallback: it already came from the
        // pane's camera above, which follows the node it is bound to.
        if cam_look.is_none() {
            let bound = self.look_through[ap.min(3)].and_then(|id| {
                self.raster
                    .scene()
                    .cameras()
                    .and_then(|cams| cams.iter().find(|c| c.id == id))
                    .cloned()
            });
            cam_look = bound.as_ref().map(|c| c.look.clone());
            cam_lens = bound.as_ref().map(solarxy_host::cameras::lens_for);
        }
        camera.camera.aspect = aspect;

        // The shot's look, through the one precedence site, and the
        // shot's grading tables bound for the job's composites. A free
        // shot leaves the strengths at zero, so identity tables cost
        // nothing.
        let pane_look =
            PaneLook::from_tone(self.renderer.post.tone_mode, self.renderer.post.exposure);
        let look = resolve_look(cam_look.as_ref(), &pane_look);
        let (lut_a, lut_b) = cam_look
            .as_ref()
            .map_or((None, None), |l| (l.lut_a.clone(), l.lut_b.clone()));
        self.renderer
            .set_lut(&self.device, &self.queue, LutSlot::A, lut_a.as_deref());
        self.renderer
            .set_lut(&self.device, &self.queue, LutSlot::B, lut_b.as_deref());

        let engine_kind = match settings.engine {
            RenderEngine::PathTraced => StillEngine::PathTraced,
            RenderEngine::Raster => StillEngine::Raster,
        };
        // A synthesized document's geometry enters the session's scene
        // objects for the raster job's draw list, and leaves again when the
        // job ends. Applied directly to the backend rather than through the
        // pending-delta queue: the scene objects ignore the environment op
        // by design, so the session's lighting and background stay whatever
        // the viewer set, where the queue's drain would hand the synthesized
        // document's environment to the shell. A model session's scene
        // objects are empty (opening a model closed any scene), so the
        // teardown clears them wholesale. The traced job needs no ingest: it
        // snapshots the engine below, into its own scene.
        let synthesized_scene = match synthesized.as_deref() {
            Some(model_engine) if engine_kind == StillEngine::Raster => {
                let delta = model_engine.scene_snapshot();
                self.raster.apply(&self.device, &self.queue, &delta);
                true
            }
            _ => false,
        };
        if engine_kind == StillEngine::PathTraced {
            if self.tracer.is_none() {
                self.tracer = Some(PathBackend::new(&self.device, &self.queue));
                // A tracer built after the environment was installed
                // missed it, and the snapshot cannot carry it: the traced
                // scene cache drops the environment op by design.
                self.traced_env_dirty = true;
            }
            // On every start, not only at construction: the per-frame
            // delta feed goes to the raster backend alone, so a tracer
            // kept from a previous still has seen nothing since. Cheap,
            // because unchanged geometry stays a hierarchy-cache hit.
            let delta = match synthesized.as_deref() {
                Some(e) => e.scene_snapshot(),
                None => match self.engine.as_deref() {
                    Some(e) => e.scene_snapshot(),
                    None => return,
                },
            };
            if let Some(t) = self.tracer.as_mut() {
                t.apply_snapshot(&self.device, &self.queue, &delta);
            }
            // Said at the start rather than at the end, because the useful
            // moment to learn that your curves will not be in the picture is
            // before you wait for the picture.
            if let Some(note) = self
                .tracer
                .as_ref()
                .and_then(RenderBackend::skipped_primitives_warning)
            {
                self.gui.set_toast(&note, ToastSeverity::Warning);
            }
            self.sync_traced_environment();
            let shot_camera = camera.camera;
            self.light_traced_still_camera(&shot_camera);
            if let Some(t) = self.tracer.as_mut() {
                t.set_settings(trace_settings_for(&settings));
                t.set_denoise_settings(denoise_settings_for(&settings));
                // After the settings, which reset the lens to the pinhole
                // default: a free shot is a pinhole and a shot through a
                // camera is whatever that camera's aperture says.
                t.set_lens(cam_lens.unwrap_or_default());
                t.invalidate();
            }
        }

        let background = self.view.pane_settings[ap].background_mode;
        let (format, space) = self.gui.still_output_choice();
        let readback = solarxy_host::still::readback_for(format, space);
        let spec = StillSpec {
            width,
            height,
            engine: engine_kind,
            samples: settings.samples,
            // Bloom is the only screen-space pass a still keeps, and it
            // is what decides the tile apron.
            screen_space_post: self.renderer.post.bloom_enabled,
            tile_budget: TILE_BUDGET_PIXELS,
            // From the dialog, through the rule all three shells read, which is
            // what makes the choice spelled the same way on each of them.
            readback,
            aux: false,
            depth: false,
            // The modal is watching, so the job publishes what it has on the
            // shared interval rather than only when a tile lands. Without it a
            // still that fits in one tile shows a blank frame for its whole
            // duration.
            preview_interval_ms: solarxy_host::still::PREVIEW_INTERVAL_MS,
            transparent: settings.transparent_background,
        };
        let job = StillRenderJob::new(spec);
        let spec = job.spec();
        // The display buffer, always. A float render fills this too, through
        // the same clamp the browser shows a float still with, because the
        // modal's preview is a screen and a screen is eight bits.
        let image = vec![0u8; spec.width as usize * spec.height as usize * 4];
        let float = solarxy_host::still::FloatImage::new(
            readback,
            spec.width,
            spec.height,
            spec.transparent,
        );

        self.gui.begin_still(spec.transparent);
        self.still = Some(StillState {
            job,
            camera,
            look,
            background,
            engine: engine_kind,
            image,
            float,
            synthesized_scene,
            started: std::time::Instant::now(),
        });
    }

    /// Advance the running job one step. Called once per frame instead of
    /// pane rendering: the job owns the shared render targets while it
    /// runs.
    pub(super) fn pump_still_render(&mut self) {
        let Some(background_mode) = self.still.as_ref().map(|s| s.background) else {
            return;
        };
        let pds = PaneDisplaySettings::for_still(background_mode);
        let background = self.resolve_background(&pds);
        let bounds = self.scene_bounds();
        let format = self.config.format;

        let Some(still) = self.still.as_mut() else {
            return;
        };
        let Some(tile) = still.job.current() else {
            // Every tile is done and taken; finish below on the job's say.
            self.finish_still();
            return;
        };
        // The shell's half of the job's contract: targets sized to the
        // tile before advance.
        self.renderer
            .resize_targets(&self.device, tile.render.width, tile.render.height);

        let step = {
            let mut ctx = StillCtx {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
                camera: &mut still.camera,
                env: &self.env,
                pds: &pds,
                display: &self.view.display,
                background,
                bounds: Some(&bounds),
                look: still.look,
                format,
                scene_present: true,
                now_ms: still.started.elapsed().as_millis() as u64,
            };
            match still.engine {
                StillEngine::Raster => still.job.advance(&mut ctx, &mut self.raster),
                StillEngine::PathTraced => match self.tracer.as_mut() {
                    Some(t) => still.job.advance(&mut ctx, t),
                    None => StillStep::Failed,
                },
            }
        };

        let spec = still.job.spec();
        match step {
            StillStep::Working => {}
            StillStep::Tile => {
                while let Some(t) = still.job.take_tile() {
                    // A float tile lands in the image being assembled and
                    // reaches the modal as eight bits, which is the same
                    // arrangement the browser uses: the preview is a screen and
                    // a screen cannot show sixteen bytes a pixel.
                    if let Some(f) = still.float.as_mut() {
                        f.place(t.rect, &t.pixels);
                        let shown = solarxy_host::still::float_to_rgba8(&t.pixels);
                        blit_rect(&mut still.image, spec.width, t.rect, &shown);
                    } else {
                        blit_tile(&mut still.image, spec.width, &t);
                    }
                }
                self.gui
                    .set_still_preview(preview_of(&still.image, spec.width, spec.height));
            }
            // The tile so far, into the same buffer at the same place. The
            // finished tile overwrites it when it lands, so the modal shows one
            // picture that only ever improves.
            StillStep::Preview => {
                if let Some(p) = still.job.take_preview() {
                    blit_rect(&mut still.image, spec.width, p.rect, &p.pixels);
                    self.gui
                        .set_still_preview(preview_of(&still.image, spec.width, spec.height));
                }
            }
            StillStep::Done => {
                self.finish_still();
                return;
            }
            StillStep::Failed => {
                self.gui.fail_still();
                self.gui.set_toast(
                    "A tile readback failed; the render is incomplete",
                    crate::gui::ToastSeverity::Error,
                );
                if self.still.take().is_some_and(|s| s.synthesized_scene) {
                    self.clear_scene_objects();
                }
                return;
            }
        }
        let progress = still.job.progress();
        self.gui.set_still_progress(
            progress.tile,
            progress.tiles,
            progress.sample,
            progress.samples,
        );
        let elapsed_ms = still.started.elapsed().as_millis() as u64;
        self.gui.set_still_timing(
            elapsed_ms,
            solarxy_host::still::estimate_remaining_ms(progress.drawn, progress.total, elapsed_ms),
        );
    }

    /// Hand the finished picture to the modal and release the frame.
    fn finish_still(&mut self) {
        let Some(done) = self.still.take() else {
            return;
        };
        if done.synthesized_scene {
            self.clear_scene_objects();
        }
        // The final elapsed, set here rather than left at whatever the last
        // pump reported: the reading a person keeps looking at after a render
        // ends should be how long it actually took.
        self.gui
            .set_still_timing(done.started.elapsed().as_millis() as u64, None);
        let spec = done.job.spec();
        // Kept beside the modal's eight-bit picture rather than inside it: the
        // modal shows a screen image and the float one is only ever written,
        // so the two live where each is used.
        self.finished_float = done.float;
        let Some(image) = image::RgbaImage::from_raw(spec.width, spec.height, done.image) else {
            self.gui.fail_still();
            return;
        };
        self.gui.finish_still(image);
    }

    /// Drop the running job. Dropping frees everything the job owns; the
    /// next ordinary frame resizes the targets back to the panes.
    pub(super) fn cancel_still_render(&mut self) {
        if let Some(cancelled) = self.still.take() {
            if cancelled.synthesized_scene {
                self.clear_scene_objects();
            }
            self.gui.mark_still_cancelled();
            self.gui
                .set_toast("Render cancelled", crate::gui::ToastSeverity::Info);
        }
    }

    /// Drain the still modal's deferred actions: cancel, and the native
    /// save dialog for Save As. Mirrors the screenshot modal's shape:
    /// nothing is written until the user picks a path.
    pub(super) fn handle_still_modal(&mut self) {
        if self.gui.take_still_cancel() {
            self.cancel_still_render();
        }
        if self.gui.take_still_render_request() {
            self.start_still_render();
        }
        if !self.gui.take_still_save_request() {
            return;
        }
        // The float image is taken before the picker opens, because taking it
        // is also what closes the dialog and a cancelled picker should leave
        // both where they were. The eight-bit image follows the same rule.
        let float = self.finished_float.take();
        let is_float = self.gui.still_is_float() && float.is_some();
        let (filter, ext) = if is_float {
            ("OpenEXR image", "exr")
        } else {
            ("PNG image", "png")
        };
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(self.gui.still_suggested_filename())
            .add_filter(filter, &[ext])
            .save_file()
        else {
            // Put it back: the dialog is still open and Save can be pressed
            // again.
            self.finished_float = float;
            return;
        };
        let written = if let Some(f) = float.filter(|_| is_float) {
            self.gui.take_still_image();
            write_exr(&path, &f)
        } else {
            match self.gui.take_still_image() {
                Some(image) => image
                    .save_with_format(&path, image::ImageFormat::Png)
                    .map_err(|e| e.to_string()),
                None => Err("the picture is no longer available".to_owned()),
            }
        };
        match written {
            Ok(()) => {
                let name = path
                    .file_name()
                    .and_then(std::ffi::OsStr::to_str)
                    .unwrap_or("still")
                    .to_string();
                self.gui
                    .set_toast(&format!("Saved {name}"), crate::gui::ToastSeverity::Success);
            }
            Err(e) => {
                self.gui.set_toast(
                    &format!("Couldn't save still: {e}"),
                    crate::gui::ToastSeverity::Error,
                );
            }
        }
    }

    /// Bring the tracer's environment up to date with the scene's, which
    /// is what makes a traced still light the way the viewport does. The
    /// traced scene cache deliberately drops the environment op, so this
    /// is the host half of that decision, mirrored from the web shell.
    fn sync_traced_environment(&mut self) {
        if self.tracer.is_none() {
            return;
        }
        let intensity = self.view.display.hdri_intensity;
        let rotation = self.view.display.hdri_rotation;
        if !std::mem::take(&mut self.traced_env_dirty) {
            if let Some(tracer) = self.tracer.as_mut() {
                tracer.set_environment_params(intensity, rotation);
            }
            return;
        }
        // Resolved before the tracer is borrowed: resolving reads the
        // whole shell and the tracer is a field of it.
        let (top, bottom) = self
            .resolve_background(&self.view.pane_settings[0])
            .sky_colors();
        let ibl = &self.renderer.ibl_res.ibl;
        let built = match (ibl.equirect.as_ref(), ibl.distribution.as_ref()) {
            (Some(equirect), Some(distribution)) => Some(
                solarxy_renderer::pathtrace::environment::TraceEnvironment::from_shared_equirect(
                    &self.device,
                    &self.queue,
                    &equirect.view,
                    distribution,
                ),
            ),
            _ => None,
        };
        let Some(tracer) = self.tracer.as_mut() else {
            return;
        };
        match built {
            Some(environment) => {
                tracer.set_environment(&self.device, environment, intensity, rotation);
            }
            // No image is not black: the kernel's constant sky comes from
            // the same background the raster path resolves.
            None => tracer.set_sky(top, bottom),
        }
    }

    /// The tracer's half of the viewer rig: a scene with no light nodes
    /// is lit in the viewport by the rig the panes write into the lights
    /// uniform, which the tracer does not bind, so it takes the same
    /// three definitions as scene data, from the shot's camera.
    fn light_traced_still_camera(&mut self, camera: &solarxy_renderer::camera::Camera) {
        if let Some(t) = self.tracer.as_mut() {
            solarxy_host::apply_viewer_rig(
                &self.device,
                &self.queue,
                t,
                self.raster.scene(),
                camera,
            );
        }
    }
}

/// A throwaway engine holding the open model as the one-node document the
/// terminal renders through: companions collected and staged, the document
/// synthesized, and the cook driven to quiescence, all before the job
/// starts. Returns the staging warnings for the shell to surface.
///
/// Bytes are re-read from the model's path rather than reconstructed from
/// the GPU-side scene, which is simpler and less surprising: the import
/// cooks the same input the terminal would read.
fn engine_for_model(path_str: &str) -> Result<(Box<solarxy_graph::Engine>, Vec<String>), String> {
    use solarxy_graph::model_document;

    let path = std::path::Path::new(path_str);
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

    let mut engine = solarxy_graph::Engine::new().map_err(|e| e.to_string())?;
    let companions =
        solarxy_formats::companions::collect(path, &ext, &bytes).map_err(|e| e.to_string())?;
    let warnings = companions.warnings;
    for asset in companions.assets {
        engine.stage_asset(asset.name, String::new(), asset.bytes);
    }
    model_document::synthesize_model_document(&mut engine, &name, &ext, bytes)
        .map_err(|e| e.to_string())?;
    model_document::cook_to_quiescence(&mut engine, &mut || false, &mut |_, _| {})
        .map_err(|e| e.to_string())?;
    Ok((Box::new(engine), warnings))
}

/// The render node's settings, or the defaults when the scene has none.
///
/// The selection rule is the headless command's: zero render nodes means
/// the defaults with a note, one means that one, and several is a refusal
/// because a still renders exactly one and picking silently would render
/// the wrong one convincingly.
fn resolve_still_settings(
    engine: &solarxy_graph::Engine,
) -> Result<(RenderSettings, Option<String>), String> {
    let graph = engine
        .document()
        .graph(GraphContext::Root)
        .map_err(|e| e.to_string())?;
    let render_nodes: Vec<NodeId> = graph
        .nodes()
        .filter(|n| n.type_id == "render")
        .map(|n| n.id)
        .collect();
    match render_nodes.len() {
        0 => Ok((
            default_still_settings(),
            Some("The scene has no render node; rendering at the defaults".to_owned()),
        )),
        1 => engine
            .render_settings(GraphContext::Root, render_nodes[0])
            .map(|s| (s, None)),
        n => Err(format!(
            "The scene has {n} render nodes; the still renders exactly one"
        )),
    }
}

/// What a document with no render node renders at.
///
/// The node type's own defaults, read from its descriptor rather than
/// restated, so the desktop and the headless command answer with one set
/// of values by construction. Both carried a hand-written literal until
/// the render node's third version, kept in step only by a test comparing
/// fields one at a time.
fn default_still_settings() -> RenderSettings {
    RenderSettings::defaults()
}

/// The tracer configured the way the render node asks for.
///
/// Destructured exhaustively on purpose, and that is the point of the
/// function rather than a style choice: a value added to `RenderSettings`
/// stops this compiling until this shell says what happens to it. The
/// alternative is what the camera's aperture did earlier in this release,
/// where a value resolved correctly out of the document and then reached no
/// renderer at all, and every test passed because they all used an aperture
/// of zero. A test cannot catch that on its own; it can only catch a value
/// wired to the wrong place.
///
/// The three shells cannot share this. `solarxy-host` is where shared host
/// behaviour goes and it deliberately has no `solarxy-graph` dependency,
/// while the engine must not see the renderer, so the boundary forbids a
/// common home in both directions.
fn trace_settings_for(settings: &RenderSettings) -> TraceSettings {
    let RenderSettings {
        // The shot itself: read by the still spec and the job's camera, not
        // by the tracer.
        camera: _,
        width: _,
        height: _,
        engine: _,
        samples,
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        // The four that steer the filter travel by their own setter rather
        // than on here, because the filter is configured apart from the walk.
        // See `denoise_settings_for` below.
        denoise_strength: _,
        denoise_sigma_color: _,
        denoise_normal_power: _,
        denoise_sigma_albedo: _,
        denoise_level_falloff: _,
        // The film back reaches the tracer too: its kernel is what withholds
        // the environment from uncovered camera rays and counts the matte.
        transparent_background,
        // What leaves beside the picture is the still spec's business, not
        // the tracer's.
        aov_albedo: _,
        aov_normal: _,
        aov_depth: _,
    } = *settings;
    let samples = samples.max(1);
    TraceSettings {
        samples,
        // The browser paces at one sample per animation frame; native has no
        // frame to pace against, so a larger chunk is the same work in fewer
        // submissions.
        chunk: 8.min(samples),
        bounces,
        transmissive_bounces,
        firefly_clamp,
        seed,
        denoise,
        denoise_until_samples,
        transparent_background,
        // The lens is set immediately after this, by `set_lens`. Everything
        // else keeps the tracer's own default.
        ..TraceSettings::default()
    }
}

/// How the render node asks for the filter to be steered.
///
/// Strength multiplies the colour tolerance rather than being a fifth
/// independent number, because that tolerance is the value that most changes
/// the outcome: expressing it any other way would leave the advanced heading
/// holding a value the everyday control could contradict.
fn denoise_settings_for(settings: &RenderSettings) -> DenoiseSettings {
    DenoiseSettings {
        sigma_color: settings.denoise_sigma_color * settings.denoise_strength,
        normal_power: settings.denoise_normal_power,
        sigma_albedo: settings.denoise_sigma_albedo,
        level_falloff: settings.denoise_level_falloff,
    }
}

/// Suggested still file name, `still_<YYYYMMDD-HHMMSS>.png`, matching
/// the screenshot's stamp format.
fn still_filename() -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let stamp = now
        .format(&time::macros::format_description!(
            "[year][month][day]-[hour][minute][second]"
        ))
        .unwrap_or_default();
    // No extension: the dialog adds the one the chosen format decides, and a
    // name that said `.png` while EXR was selected would be a name arguing with
    // the file beside it.
    format!("still_{stamp}")
}

/// Writes the floating-point still, through the same encoder the browser and
/// the headless command write one with.
fn write_exr(
    path: &std::path::Path,
    image: &solarxy_host::still::FloatImage,
) -> Result<(), String> {
    // A matte still goes through the four-channel writer, which premultiplies
    // on the way out; an opaque one keeps writing three channels, because its
    // alpha would be a constant one pretending to be a matte.
    let bytes = if image.has_matte() {
        solarxy_formats::export::encode_exr_rgba_bytes(image.rgba(), image.width(), image.height())
    } else {
        let hdr = solarxy_core::geometry::RawImageHdr::new(
            image.rgb().to_vec(),
            image.width(),
            image.height(),
        );
        solarxy_formats::export::encode_exr_rgb_bytes(&hdr)
    }
    .map_err(|e| format!("the image could not be encoded: {e}"))?;
    std::fs::write(path, bytes).map_err(|e| e.to_string())
}

/// Copy one cropped tile into its place in the assembled picture.
fn blit_tile(image: &mut [u8], image_width: u32, tile: &StillTile) {
    blit_rect(image, image_width, tile.rect, &tile.pixels);
}

/// Copies one rectangle of eight-bit pixels into the assembled picture.
///
/// Takes the rect and the bytes rather than a tile, because a mid-render
/// preview covers the same rectangle in the same format and there is no reason
/// for it to travel a second path to the same buffer.
fn blit_rect(image: &mut [u8], image_width: u32, rect: TileRect, pixels: &[u8]) {
    let row = image_width as usize * 4;
    let tile_row = rect.width as usize * 4;
    for y in 0..rect.height as usize {
        let dst = (rect.y as usize + y) * row + rect.x as usize * 4;
        let src = y * tile_row;
        image[dst..dst + tile_row].copy_from_slice(&pixels[src..src + tile_row]);
    }
}

/// A nearest-neighbour downscale of the assembled picture for the modal's
/// live preview. Nearest, because it reads only preview-many pixels: a
/// box filter over a large still would cost more than the tile did.
fn preview_of(image: &[u8], width: u32, height: u32) -> image::RgbaImage {
    let scale = (PREVIEW_MAX_EDGE as f32 / width.max(height) as f32).min(1.0);
    let pw = ((width as f32 * scale) as u32).max(1);
    let ph = ((height as f32 * scale) as u32).max(1);
    let mut out = image::RgbaImage::new(pw, ph);
    for y in 0..ph {
        let sy = (u64::from(y) * u64::from(height) / u64::from(ph)) as usize;
        for x in 0..pw {
            let sx = (u64::from(x) * u64::from(width) / u64::from(pw)) as usize;
            let i = (sy * width as usize + sx) * 4;
            out.put_pixel(
                x,
                y,
                image::Rgba([image[i], image[i + 1], image[i + 2], image[i + 3]]),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use solarxy_host::still::TileRect;

    use super::*;

    /// Every value the render node authors for the walk reaches the tracer.
    ///
    /// The values are deliberately nothing like the defaults. A test written
    /// with the defaults passes with the assignment deleted, which is exactly
    /// how the camera's aperture reached no renderer for a whole release
    /// while every test stayed green.
    #[test]
    fn every_authored_value_reaches_the_tracer() {
        let mut s = RenderSettings::defaults();
        s.samples = 91;
        s.bounces = 13;
        s.transmissive_bounces = 7;
        s.firefly_clamp = 3.5;
        s.seed = 4242;
        s.denoise = true;
        s.transparent_background = true;

        let t = trace_settings_for(&s);
        assert_eq!(t.samples, 91);
        assert_eq!(t.bounces, 13);
        assert_eq!(t.transmissive_bounces, 7);
        assert!((t.firefly_clamp - 3.5).abs() < f32::EPSILON);
        assert_eq!(t.seed, 4242);
        assert!(t.denoise);
        assert!(t.transparent_background, "the film back reaches the kernel");
        // This shell's own pacing rather than the node's: eight samples to a
        // submission, native having no frame to pace against.
        assert_eq!(t.chunk, 8);
    }

    /// The four steering values reach the filter, and strength multiplies the
    /// one it is documented to multiply.
    ///
    /// Deliberately distinct values, so a field wired to its neighbour fails
    /// rather than passing on a coincidence.
    #[test]
    fn the_steering_values_reach_the_filter() {
        let mut s = RenderSettings::defaults();
        s.denoise_sigma_color = 2.0;
        s.denoise_normal_power = 33.0;
        s.denoise_sigma_albedo = 0.5;
        s.denoise_level_falloff = 3.0;
        s.denoise_strength = 1.5;

        let d = denoise_settings_for(&s);
        assert!(
            (d.sigma_color - 3.0).abs() < f32::EPSILON,
            "strength multiplies the colour tolerance: 2.0 at 1.5 is 3.0"
        );
        assert!((d.normal_power - 33.0).abs() < f32::EPSILON);
        assert!((d.sigma_albedo - 0.5).abs() < f32::EPSILON);
        assert!((d.level_falloff - 3.0).abs() < f32::EPSILON);
    }

    /// At the default strength the filter runs at exactly its measured values.
    ///
    /// The multiplier is what makes this worth asserting: a strength that
    /// defaulted to anything but one would silently retune every existing
    /// render the moment the control shipped.
    #[test]
    fn the_defaults_are_the_measured_values_untouched() {
        let d = denoise_settings_for(&RenderSettings::defaults());
        assert_eq!(d, DenoiseSettings::default());
    }

    /// The threshold reaches the walk's settings, where the gate reads it.
    #[test]
    fn the_denoise_threshold_reaches_the_tracer() {
        let mut s = RenderSettings::defaults();
        s.denoise = true;
        s.denoise_until_samples = 40;
        let t = trace_settings_for(&s);
        assert_eq!(t.denoise_until_samples, 40);
        assert!(t.filtering_at(40));
        assert!(!t.filtering_at(41));
    }

    /// A render shorter than a chunk submits the render, not the chunk.
    #[test]
    fn the_chunk_never_outruns_the_render() {
        let mut s = RenderSettings::defaults();
        s.samples = 3;
        assert_eq!(trace_settings_for(&s).chunk, 3);

        s.samples = 0;
        let t = trace_settings_for(&s);
        assert_eq!(
            (t.samples, t.chunk),
            (1, 1),
            "a render of no samples still draws one, rather than looping on a \
             chunk of zero"
        );
    }

    #[test]
    fn a_tile_lands_at_its_own_rect() {
        let mut image = vec![0u8; 4 * 4 * 4];
        let tile = StillTile {
            rect: TileRect {
                x: 2,
                y: 1,
                width: 2,
                height: 2,
            },
            pixels: vec![255u8; 2 * 2 * 4],
            aux: None,
            depth: None,
        };
        blit_tile(&mut image, 4, &tile);
        // Row 0 untouched, rows 1 and 2 filled from column 2.
        assert_eq!(&image[0..16], &[0u8; 16]);
        let row1 = &image[16..32];
        assert_eq!(&row1[0..8], &[0u8; 8]);
        assert_eq!(&row1[8..16], &[255u8; 8]);
    }

    #[test]
    fn defaults_match_the_headless_command() {
        // The headless command's `default_settings` is private to its
        // crate, so the agreement is pinned by value: a change there
        // must land here too, deliberately.
        let d = default_still_settings();
        assert_eq!((d.width, d.height), (1920, 1080));
        assert_eq!(d.engine, RenderEngine::Raster);
        assert_eq!((d.samples, d.bounces, d.transmissive_bounces), (64, 6, 4));
        assert!(!d.denoise);
        assert!(d.camera.is_none());
    }

    #[test]
    fn the_preview_never_exceeds_its_edge_and_samples_corners() {
        let width = 100u32;
        let height = 50u32;
        let mut image = vec![0u8; width as usize * height as usize * 4];
        // Mark the bottom-right source pixel.
        let last = ((height as usize - 1) * width as usize + (width as usize - 1)) * 4;
        image[last] = 200;
        let p = preview_of(&image, width, height);
        assert!(p.width() <= PREVIEW_MAX_EDGE && p.height() <= PREVIEW_MAX_EDGE);
        assert_eq!(p.width(), 100, "small images pass through unscaled");
        assert_eq!(p.get_pixel(99, 49)[0], 200);
    }
}

/// The desktop and the headless command read one document the same way.
///
/// **This is the automatable half of the cross-shell parity question, and it is
/// worth being precise about which half.** Three shells rendering "the same
/// image" can fail in two independent places: they can read different settings
/// out of one scene, or they can turn identical settings into different pixels.
/// The first is a pure function of the document and is checked here on every
/// `cargo test`, with no device. The second needs a real adapter on each of
/// three surfaces, one of which is a browser, and lives in
/// `Docs/qa/render-checklist.md` as a written check with named scenes.
///
/// The settings half is also the half that actually drifts. Pixels differ by
/// float reassociation, which this release already ruled on and bounded; a
/// shell reading a different sample count out of the same node is a defect that
/// no tolerance excuses.
#[cfg(test)]
mod parity {
    use super::*;

    /// Both shipped samples, which between them carry the four features a
    /// render can diverge on: transmission and an area light in the first, a
    /// procedural texture network and a graded camera in the second.
    const SCENES: [&str; 2] = ["cornell-box.slxy", "procedural-lookdev.slxy"];

    fn sample(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../web/public/samples")
            .join(name)
    }

    #[test]
    fn both_native_shells_resolve_one_scene_the_same_way() {
        for name in SCENES {
            let path = sample(name);
            assert!(path.exists(), "the shipped sample {name} is missing");

            let loaded = solarxy_render::input::load(&path, None, &mut solarxy_render::Silent)
                .unwrap_or_else(|e| panic!("{name} did not load: {e}"));

            // The headless command's answer, and this shell's own, over one
            // cooked document.
            let mut warnings = Vec::new();
            let headless = solarxy_render::resolve_settings(
                &loaded.engine,
                &solarxy_render::RenderOptions::default(),
                &mut warnings,
            )
            .unwrap_or_else(|e| panic!("{name} would not resolve headlessly: {e}"));

            let (desktop, _note) = resolve_still_settings(&loaded.engine)
                .unwrap_or_else(|e| panic!("{name} would not resolve on the desktop: {e}"));

            // Field by field rather than by equality, so a failure names what
            // disagreed instead of printing two structs and leaving the reader
            // to diff them.
            assert_eq!(desktop.width, headless.width, "{name}: width");
            assert_eq!(desktop.height, headless.height, "{name}: height");
            assert_eq!(desktop.engine, headless.engine, "{name}: engine");
            assert_eq!(desktop.samples, headless.samples, "{name}: samples");
            assert_eq!(desktop.bounces, headless.bounces, "{name}: bounces");
            assert_eq!(
                desktop.transmissive_bounces, headless.transmissive_bounces,
                "{name}: transmissive bounces"
            );
            assert_eq!(desktop.denoise, headless.denoise, "{name}: denoise");
            assert_eq!(desktop.seed, headless.seed, "{name}: seed");
            assert_eq!(
                desktop.firefly_clamp, headless.firefly_clamp,
                "{name}: firefly clamp"
            );
            assert_eq!(desktop.camera, headless.camera, "{name}: camera");
            assert_eq!(
                desktop.aov_albedo, headless.aov_albedo,
                "{name}: albedo pass"
            );
            assert_eq!(
                desktop.aov_normal, headless.aov_normal,
                "{name}: normal pass"
            );
            assert_eq!(desktop.aov_depth, headless.aov_depth, "{name}: depth pass");
            assert_eq!(
                desktop.transparent_background, headless.transparent_background,
                "{name}: transparent background"
            );
        }
    }

    /// A scene with no render node falls to the node type's own descriptor on
    /// both shells, rather than to two hand-written literals that agreed until
    /// somebody changed one.
    #[test]
    fn a_scene_with_no_render_node_falls_to_one_set_of_defaults() {
        assert_eq!(default_still_settings(), RenderSettings::defaults());
    }
}
