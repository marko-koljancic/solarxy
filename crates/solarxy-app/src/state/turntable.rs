//! The desktop turntable export: one full turn of the active pane's view,
//! written as a numbered image sequence.
//!
//! A job beside the still render's, and driven the same way: while it runs it
//! owns the shared render targets, so the panes are not rendered and only the
//! interface draws. What differs is that a still is one picture and a turn is
//! a hundred, so this holds a fresh [`StillRenderJob`] per frame and starts
//! the next one when the last finishes.
//!
//! Three things about it are deliberate.
//!
//! **Every frame is turned from the base pose** rather than from the frame
//! before it, so a hundred and twenty floating-point additions cannot walk the
//! camera off the turn, and the pane's own camera is never written: the
//! viewport's turntable, its speed and the pane cameras are exactly as they
//! were when the export ends, whether it finished or was cancelled.
//!
//! **The frame's display settings are derived the way the browser derives a
//! screenshot's**, not the way a still derives its own.
//! [`PaneDisplaySettings::for_still`] forces the grid, the axes and the
//! validation overlay off, which would make the dialog's three Include boxes
//! inert; the browser instead copies the pane's settings and lets an unchecked
//! box force its feature off. So does this.
//!
//! **A cancelled export keeps the frames it already wrote** and says how many.
//! The browser cannot: it holds every frame in memory and zips them at the
//! end, so cancelling there produces nothing. Writing each frame as it lands
//! is the native shape, and it is what makes a long export interruptible
//! without losing the part that is done.

use std::path::PathBuf;

use solarxy_core::preferences::PaneMode;
use solarxy_core::view_config::{PaneDisplaySettings, PaneLook};
use solarxy_host::still::{StillEngine, StillReadback, StillSpec, StillStep, TILE_BUDGET_PIXELS};
use solarxy_host::{StillCtx, StillRenderJob};
use solarxy_renderer::camera::Camera;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::{CompositeLook, resolve_look};

use super::State;
use super::still::pixels::blit_tile;
use crate::gui::{ToastSeverity, TurntableIncludes, TurntableRequest, azimuth_deg, frame_count};

/// The smallest frame edge worth writing, matching the capture path's floor.
const MIN_EDGE: u32 = 16;

/// One turntable export in flight, owned by the shell.
pub(crate) struct TurntableState {
    /// The frame being rendered. A fresh job per frame, because a frame is a
    /// whole picture and the tiled job is what renders one of those.
    job: StillRenderJob,
    /// Job-owned camera, rebuilt per frame from the base pose. No pane is
    /// moved by an export.
    camera: CameraState,
    /// The pose every frame is turned from.
    base: Camera,
    look: CompositeLook,
    /// The pane's settings with the dialog's includes applied, resolved once:
    /// every frame of a turn is the same shot from a different angle.
    pds: PaneDisplaySettings,
    width: u32,
    height: u32,
    /// Which frame is rendering, and how many the turn is.
    index: u32,
    count: u32,
    /// How many are on disk, which is what a cancelled run reports.
    written: u32,
    folder: PathBuf,
    stem: String,
    /// The frame being assembled, RGBA8, `width * height * 4`.
    image: Vec<u8>,
    /// When the export began. The job takes a clock reading rather than
    /// reading one, because it also compiles for the browser.
    started: std::time::Instant,
}

/// The pane's settings as a frame of the export should carry them.
///
/// Only ever turns a feature off, which is the browser's screenshot rule
/// (`crates/solarxy-web/src/app/capture.rs:171-184`): the pane is the starting
/// point and an unchecked box forces its feature off for the export. A checked
/// box does not switch on something the pane has switched off, so what leaves
/// is a picture of the pane rather than of the dialog.
pub(super) fn frame_settings(
    base: PaneDisplaySettings,
    includes: TurntableIncludes,
) -> PaneDisplaySettings {
    let mut pds = base;
    if !includes.grid {
        pds.show_grid = false;
    }
    if !includes.axes {
        pds.show_axis_gizmo = false;
        pds.show_local_axes = false;
    }
    if !includes.validation {
        pds.show_validation = false;
    }
    // Markers stay out of a delivered image on both shells: they are an
    // aiming aid rather than something the picture is of.
    pds.show_light_markers = false;
    // The live spin is not the export's. Leaving it set would have the frame
    // carry a flag that only means something to the viewport's own clock.
    pds.turntable_active = false;
    pds
}

impl State {
    /// Open the export dialog. Nothing renders until Export is pressed, which
    /// is where the folder and the frame count are chosen.
    pub(super) fn open_turntable_dialog(&mut self) {
        if self.turntable.is_some() {
            return;
        }
        self.gui.open_turntable_modal();
    }

    /// Start the export the dialog asked for.
    pub(super) fn start_turntable_export(&mut self, request: TurntableRequest) {
        if self.turntable.is_some() {
            return;
        }
        let TurntableRequest {
            resolution,
            fps,
            duration,
            includes,
            folder,
            stem,
        } = request;

        let pane_index = self.view.active_pane;
        let base_pds = self.view.pane_settings[pane_index];
        // A turn is an orbit, and a UV pane has nothing to orbit. Refused
        // here rather than by withholding the menu entry, so the two shells'
        // menus keep the same shape whatever the scene is.
        if base_pds.pane_mode == PaneMode::UvMap {
            self.gui.fail_turntable("A turntable needs a 3D pane");
            return;
        }
        let Some(pane_camera) = self.view.cameras[pane_index].as_ref().map(|c| c.camera) else {
            self.gui.fail_turntable("This pane has no camera yet");
            return;
        };

        let Some((width, height)) = self.turntable_frame_size(pane_index, resolution.factor())
        else {
            self.gui.fail_turntable("This pane is too small to export");
            return;
        };

        // The shot's look, through the one precedence site, exactly as the
        // pane resolves its own: an exported frame should be the picture the
        // pane is showing, turned.
        let cam_look = solarxy_host::cameras::camera_look_for(
            self.raster.scene().cameras(),
            self.look_through[pane_index.min(3)],
        )
        .cloned();
        let look = resolve_look(
            cam_look.as_ref(),
            &PaneLook::from_tone(self.renderer.post.tone_mode, self.renderer.post.exposure),
        );
        solarxy_host::cameras::bind_look_luts(
            &self.device,
            &self.queue,
            &mut self.renderer,
            cam_look.as_ref(),
        );

        let count = frame_count(fps, duration);
        let mut base = pane_camera;
        base.aspect = width as f32 / height as f32;
        let camera = CameraState::from_camera(&self.device, &self.renderer.layouts.camera, base);
        let job = StillRenderJob::new(self.turntable_spec(width, height));
        let spec = job.spec();
        let image = vec![0u8; spec.width as usize * spec.height as usize * 4];

        self.gui.begin_turntable(count);
        self.turntable = Some(TurntableState {
            job,
            camera,
            base,
            look,
            pds: frame_settings(base_pds, includes),
            width: spec.width,
            height: spec.height,
            index: 0,
            count,
            written: 0,
            folder,
            stem,
            image,
            started: std::time::Instant::now(),
        });
        // The first frame's camera, which is the base pose unturned.
        self.aim_turntable_frame();
    }

    /// The frame size for `factor`, from the pane's own on-screen size.
    ///
    /// The browser budgets this against its capture ceiling; this shell has
    /// none deliberately, so the only bounds are the job's own edge clamp and
    /// a floor that keeps a sliver of a pane from producing a zero-width file.
    fn turntable_frame_size(&self, pane: usize, factor: f32) -> Option<(u32, u32)> {
        let panes = self.compute_panes();
        let rect = panes.get(pane)?.content(self.pane_toolbar_height_px());
        let w = (rect.width * factor).round();
        let h = (rect.height * factor).round();
        if !w.is_finite() || !h.is_finite() || w < 1.0 || h < 1.0 {
            return None;
        }
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(((w as u32).max(MIN_EDGE), (h as u32).max(MIN_EDGE)))
    }

    /// What one frame of the turn renders as.
    fn turntable_spec(&self, width: u32, height: u32) -> StillSpec {
        StillSpec {
            width,
            height,
            // Always the rasterizer, whatever engine the pane is on. The
            // browser's turntable runs through its ordinary screenshot path
            // and so rasterizes too, and a traced turn would be a sample
            // budget per frame times a hundred frames with nothing in the
            // dialog to bound it.
            engine: StillEngine::Raster,
            // Ignored by the raster engine, which draws each tile once.
            samples: 1,
            screen_space_post: self.renderer.post.bloom_enabled,
            tile_budget: TILE_BUDGET_PIXELS,
            readback: StillReadback::Display8,
            aux: false,
            depth: false,
            // Nobody is watching a per-frame preview: the dialog shows a
            // count, and a preview composite and readback four times a second
            // would be paid for on every frame of the turn.
            preview_interval_ms: 0,
            transparent: false,
        }
    }

    /// Point the job's camera at frame `index` of the turn.
    ///
    /// The upload is the load-bearing half: `CameraState::from_camera` writes
    /// the view projection once at construction and nothing re-writes it, so a
    /// pose set on the CPU alone would leave every frame of the turn rendering
    /// the picture the first one did.
    fn aim_turntable_frame(&mut self) {
        let queue = &self.queue;
        let Some(t) = self.turntable.as_mut() else {
            return;
        };
        let mut cam = t.base;
        solarxy_host::preview::orbit_yaw(&mut cam, azimuth_deg(t.index, t.count).to_radians());
        t.camera.camera = cam;
        t.camera.upload_pose(queue);
    }

    /// Advance the running export one step. Called once per frame instead of
    /// pane rendering: the job owns the shared render targets while it runs.
    pub(super) fn pump_turntable_export(&mut self) {
        let Some(pds) = self.turntable.as_ref().map(|t| t.pds) else {
            return;
        };
        // The same reason the still clears it every pump: the manipulator,
        // both helper channels and the light markers are host-fed, reach no
        // pane flag, and hold whatever the last ordinary frame left in them.
        self.renderer.clear_viewport_furniture();
        let background = Self::resolve_background(&pds);
        let bounds = self.scene_bounds();
        let format = self.config.format;

        let Some(t) = self.turntable.as_mut() else {
            return;
        };
        let Some(tile) = t.job.current() else {
            // Every tile of this frame is done and taken.
            self.finish_turntable_frame();
            return;
        };
        self.renderer
            .resize_targets(&self.device, tile.render.width, tile.render.height);

        let step = {
            let mut ctx = StillCtx {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
                camera: &mut t.camera,
                env: &self.env,
                pds: &pds,
                display: &self.view.display,
                background,
                bounds: Some(&bounds),
                look: t.look,
                format,
                scene_present: true,
                now_ms: t.started.elapsed().as_millis() as u64,
            };
            t.job.advance(&mut ctx, &mut self.raster)
        };

        let width = t.width;
        match step {
            StillStep::Working | StillStep::Preview => {}
            StillStep::Tile => {
                while let Some(tile) = t.job.take_tile() {
                    blit_tile(&mut t.image, width, &tile);
                }
            }
            StillStep::Done => self.finish_turntable_frame(),
            StillStep::Failed => {
                let written = t.written;
                self.gui
                    .fail_turntable("A tile readback failed; the sequence is incomplete");
                self.gui.set_toast(
                    &format!("Turntable export failed after {written} frames"),
                    ToastSeverity::Error,
                );
                self.turntable.take();
            }
        }
    }

    /// Write the finished frame and start the next, or finish the export.
    fn finish_turntable_frame(&mut self) {
        let Some(t) = self.turntable.as_mut() else {
            return;
        };
        let path = t.folder.join(format!("{}_{:04}.png", t.stem, t.index));
        let Some(image) =
            image::RgbaImage::from_raw(t.width, t.height, std::mem::take(&mut t.image))
        else {
            let written = t.written;
            self.gui.fail_turntable("The frame could not be assembled");
            self.gui.set_toast(
                &format!("Turntable export failed after {written} frames"),
                ToastSeverity::Error,
            );
            self.turntable.take();
            return;
        };
        if let Err(e) = image.save_with_format(&path, image::ImageFormat::Png) {
            let written = t.written;
            self.gui.fail_turntable("A frame could not be written");
            self.gui.set_toast(
                &format!("Turntable export failed after {written} frames: {e}"),
                ToastSeverity::Error,
            );
            self.turntable.take();
            return;
        }
        // The buffer was taken to build the image; give the next frame one.
        t.image = image.into_raw();
        t.image.fill(0);
        t.written += 1;
        t.index += 1;
        let (written, index, count) = (t.written, t.index, t.count);
        self.gui.set_turntable_progress(written, count);

        if index >= count {
            let folder = t.folder.clone();
            self.turntable.take();
            self.gui.finish_turntable();
            self.gui.set_toast(
                &format!("Exported {written} frames to {}", folder.display()),
                ToastSeverity::Info,
            );
            return;
        }
        // The next frame is a fresh job: the tile plan is per picture, and a
        // finished job has nothing left to advance.
        let spec = self.turntable_spec_for_next();
        if let Some(t) = self.turntable.as_mut() {
            t.job = StillRenderJob::new(spec);
        }
        self.aim_turntable_frame();
    }

    /// The spec for the next frame of the turn, which is the same shot again.
    fn turntable_spec_for_next(&self) -> StillSpec {
        let (width, height) = self
            .turntable
            .as_ref()
            .map_or((MIN_EDGE, MIN_EDGE), |t| (t.width, t.height));
        self.turntable_spec(width, height)
    }

    /// Drop the running export. The frames already written stay on disk,
    /// which is the whole point of writing them as they land.
    pub(super) fn cancel_turntable_export(&mut self) {
        let Some(t) = self.turntable.take() else {
            return;
        };
        self.gui.mark_turntable_cancelled(t.written);
        self.gui.set_toast(
            &format!("Turntable export cancelled; {} frames kept", t.written),
            ToastSeverity::Info,
        );
    }

    /// Drain the turntable dialog's deferred actions: the folder picker,
    /// the start, and cancel.
    pub(super) fn handle_turntable_modal(&mut self) {
        if self.gui.take_turntable_cancel() {
            self.cancel_turntable_export();
        }
        if self.gui.take_turntable_folder_request()
            && let Some(folder) = rfd::FileDialog::new()
                .set_title("Export turntable into")
                .pick_folder()
        {
            self.gui.set_turntable_folder(folder);
        }
        if let Some(request) = self.gui.take_turntable_start() {
            self.start_turntable_export(request);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A pane with everything the includes reach switched on, so a test can
    /// tell "left alone" from "forced off". `for_still` is the base every
    /// sibling test builds from; it starts with all of them off.
    fn base() -> PaneDisplaySettings {
        PaneDisplaySettings {
            show_grid: true,
            show_axis_gizmo: true,
            show_local_axes: true,
            show_validation: true,
            show_light_markers: true,
            turntable_active: true,
            ..PaneDisplaySettings::for_still(solarxy_core::preferences::BackgroundMode::GRADIENT)
        }
    }

    #[test]
    fn an_unchecked_include_forces_its_feature_off() {
        let pds = frame_settings(
            base(),
            TurntableIncludes {
                grid: false,
                axes: false,
                validation: false,
            },
        );
        assert!(!pds.show_grid);
        assert!(!pds.show_axis_gizmo);
        assert!(!pds.show_local_axes);
        assert!(!pds.show_validation);
    }

    #[test]
    fn a_checked_include_leaves_the_panes_own_setting() {
        let pds = frame_settings(
            base(),
            TurntableIncludes {
                grid: true,
                axes: true,
                validation: true,
            },
        );
        assert!(pds.show_grid);
        assert!(pds.show_axis_gizmo);
        assert!(pds.show_local_axes);
        assert!(pds.show_validation);
    }

    #[test]
    fn a_checked_include_cannot_switch_on_what_the_pane_has_off() {
        // The browser's rule: the boxes only ever subtract, so what leaves is
        // a picture of the pane rather than of the dialog.
        let off =
            PaneDisplaySettings::for_still(solarxy_core::preferences::BackgroundMode::GRADIENT);
        let pds = frame_settings(
            off,
            TurntableIncludes {
                grid: true,
                axes: true,
                validation: true,
            },
        );
        assert!(!pds.show_grid);
        assert!(!pds.show_axis_gizmo);
        assert!(!pds.show_validation);
    }

    #[test]
    fn markers_and_the_live_spin_never_reach_a_frame() {
        let pds = frame_settings(base(), TurntableIncludes::default());
        assert!(!pds.show_light_markers);
        assert!(!pds.turntable_active);
    }
}
