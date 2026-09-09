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
//!
//! Three files, because they answer three questions. This one is the job's
//! lifecycle. `settings` is what a document says a render should be, which is
//! a pure function of the document and so is where the cross-shell parity
//! check lives. `pixels` is what happens to the pixels once they arrive.

use solarxy_core::preferences::BackgroundMode;
use solarxy_core::view_config::{PaneDisplaySettings, PaneLook};
use solarxy_graph::nodes::{RenderEngine, RenderSettings};
use solarxy_host::still::{StillEngine, StillSpec, StillStep, TILE_BUDGET_PIXELS};
use solarxy_host::{StillCtx, StillRenderJob};
use solarxy_renderer::backend::RenderBackend;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::{CompositeLook, resolve_look};
use solarxy_renderer::pathtrace::backend::PathBackend;
use solarxy_graph::Engine;
use solarxy_renderer::scene::BackgroundModeExt;

use super::State;
use super::update::find_node_name;

// `pixels` rather than `image`: a module of that name shadows the crate of
// that name in every sibling, and the two are used side by side here.
mod pixels;
mod settings;

use pixels::{blit_rect, blit_tile, preview_of, still_filename, write_exr};
use settings::{denoise_settings_for, resolve_still_settings, trace_settings_for};

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
        let Some(settings) = self.resolve_still_request() else {
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
    fn resolve_still_request(&mut self) -> Option<RenderSettings> {
        use crate::gui::ToastSeverity;
        // One root renders. A model file is already the document it
        // synthesizes into by the time it is on screen, so the still no longer
        // rebuilds one at the moment Render is pressed and the picture is of
        // exactly what the viewport is showing.
        let Some(engine) = self.engine.as_deref() else {
            self.gui.set_toast(
                "Open a scene or a model to render a still",
                ToastSeverity::Warning,
            );
            return None;
        };

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
                        format!("Cannot render: {name} failed to cook: {reason} (and {more} more)")
                    }
                },
            );
            self.gui.set_toast(&message, ToastSeverity::Error);
            return None;
        }

        // Opened from a render node's own action, the still renders that
        // node; opened from the menu, the document's single render node,
        // with the refusals that rule carries.
        let resolved = match self.still_target {
            Some((ctx, node)) => engine.render_settings(ctx, node).map(|s| (s, None)),
            None => resolve_still_settings(engine),
        };
        match resolved {
            Ok((settings, note)) => {
                if let Some(note) = note {
                    self.gui.set_toast(&note, ToastSeverity::Info);
                }
                Some(settings)
            }
            Err(message) => {
                self.gui.set_toast(&message, ToastSeverity::Error);
                None
            }
        }
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
        let Some(settings) = self.resolve_still_request() else {
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
        solarxy_host::cameras::bind_look_luts(
            &self.device,
            &self.queue,
            &mut self.renderer,
            cam_look.as_ref(),
        );

        let engine_kind = match settings.engine {
            RenderEngine::PathTraced => StillEngine::PathTraced,
            RenderEngine::Raster => StillEngine::Raster,
        };
        // The raster job needs no ingest of its own: the session's scene
        // objects already hold the open document, because there is only one.
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
            let Some(delta) = self.engine.as_deref().map(Engine::scene_snapshot) else {
                return;
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
                self.still.take();
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
        if self.still.take().is_some() {
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
