//! The traced viewport pane: what keeps a pane's path tracer in step with
//! the document and the view.
//!
//! A per-shell twin by structure, like the gizmo drive loop and
//! `trace_settings_for`: it reads an `Engine` and writes a renderer backend,
//! so it can live in neither `solarxy-host`, which has no engine, nor
//! `solarxy-graph`, which has no renderer. The browser host carries the
//! same code, and the two are held to one behaviour by reading rather than
//! by a shared type. What the two do share, the preview's terms and the
//! camera key, lives in `solarxy_host::traced_preview`.
//!
//! The reset set is the browser's, not a new one: a scene delta drops every
//! accumulation, an environment change re-syncs and drops every one, a
//! committed set of display defaults drops every one, a pane whose camera
//! moved drops its own at encode, an engine flip drops that pane, and a
//! still start drops every one. A look change deliberately drops none: the
//! look is applied after the resolve, so a converged mean re-composites
//! under the new look every frame without being thrown away, which is the
//! browser's behaviour too.

use solarxy_core::view_config::PaneEngine;
use solarxy_graph::Engine;
use solarxy_renderer::backend::RenderBackend as _;
use solarxy_renderer::pathtrace::backend::PathBackend;
use solarxy_renderer::pathtrace::denoise::DenoiseSettings;

use crate::state::State;

/// Whether the environment block installs this frame: never while a still
/// or a turntable owns the shared backend (the job's own environment would
/// be swapped out from under it and its mean reset), and otherwise when the
/// tracer's copy is flagged stale or the scalars moved. Leaving the flag
/// set while a job runs is what records the owed install once it lets go.
fn environment_install_due(
    job_running: bool,
    dirty: bool,
    params: (f32, f32),
    last_params: (f32, f32),
) -> bool {
    !job_running && (dirty || params != last_params)
}

impl State {
    /// Whether any pane in the layout traces, which is what every piece of
    /// housekeeping here is gated on: a session that never traces pays one
    /// scan of four settings.
    fn any_pane_traced(&self) -> bool {
        self.view.pane_settings.iter().any(|p| {
            p.pane_mode == solarxy_core::preferences::PaneMode::Scene3D
                && p.pane_engine == PaneEngine::Traced
        })
    }

    /// Drop every pane's accumulation and forget every pose, so each pane
    /// re-anchors on its next encode.
    fn invalidate_traced_panes(&mut self) {
        if let Some(t) = self.tracer.as_mut() {
            t.invalidate();
        }
        self.traced_cam_keys = [None; 4];
    }

    /// A pane's engine changed, either way.
    ///
    /// Flipping to the tracer builds it on first use and hands it the scene
    /// it has never seen: the per-frame delta feed only reaches it while a
    /// pane is traced, so a scene edited with every pane raster has moved
    /// on without it. The snapshot reconciles, and an unchanged scene is a
    /// hierarchy-cache hit. The environment is installed the same way the
    /// still installs it, so the first traced frame is lit.
    ///
    /// Either direction resets the pane's accumulation: the first traced
    /// frame is sample one rather than a stale mean, and a return to raster
    /// leaves nothing parked for a later flip back.
    pub(in crate::state) fn flip_pane_engine(&mut self, pane: usize, engine: PaneEngine) {
        if engine == PaneEngine::Traced {
            if self.tracer.is_none() {
                self.tracer = Some(PathBackend::new(&self.device, &self.queue));
                // A tracer built after the environment was installed missed
                // it, and the snapshot cannot carry it: the traced scene
                // cache drops the environment op by design.
                self.traced_env_dirty = true;
            }
            if let Some(delta) = self.engine.as_deref().map(Engine::scene_snapshot)
                && let Some(t) = self.tracer.as_mut()
            {
                t.apply_snapshot(&self.device, &self.queue, &delta);
            }
            self.sync_traced_environment();
        }
        if let Some(t) = self.tracer.as_mut() {
            t.invalidate_pane(pane);
        }
        if let Some(slot) = self.traced_cam_keys.get_mut(pane) {
            *slot = None;
        }
    }

    /// The same feed the raster backend gets, when a pane is traced: the
    /// delta lands and every accumulation resets, since the mean was of
    /// another scene. The camera keys reset with it so each pane re-anchors
    /// its pose on its next encode.
    pub(in crate::state) fn feed_traced_scene_delta(
        &mut self,
        delta: &solarxy_core::scene::SceneDelta,
    ) {
        if delta.ops.is_empty() || !self.any_pane_traced() {
            return;
        }
        let Some(t) = self.tracer.as_mut() else {
            return;
        };
        t.apply(&self.device, &self.queue, delta);
        self.invalidate_traced_panes();
    }

    /// Once per frame, after the deltas: keep a watched tracer's environment
    /// in step with the scene's. Does nothing while no pane is traced.
    pub(in crate::state) fn feed_traced_environment(&mut self) {
        if !self.any_pane_traced() || self.tracer.is_none() {
            return;
        }
        let params = (
            self.view.display.hdri_intensity,
            self.view.display.hdri_rotation,
        );
        let job_running = self.still.is_some() || self.turntable.is_some();
        if environment_install_due(
            job_running,
            self.traced_env_dirty,
            params,
            self.traced_env_params,
        ) {
            self.sync_traced_environment();
            self.traced_env_params = params;
            self.invalidate_traced_panes();
        }
    }

    /// A committed set of display defaults may steer the preview, so every
    /// accumulation starts over under the new terms.
    pub(in crate::state) fn traced_defaults_changed(&mut self) {
        if self.any_pane_traced() {
            self.invalidate_traced_panes();
        }
    }

    /// One traced pane's pre-encode housekeeping: assert the preview's
    /// settings, reset the accumulation when the pane's camera moved, and
    /// re-apply the viewer rig.
    pub(in crate::state) fn prepare_traced_pane(&mut self, i: usize) {
        // Asserted per encode rather than held, because the still job
        // authors its own settings on the same backend and whichever ran
        // last would otherwise win. `set_settings` drops every accumulation
        // only when the settings actually differ, which is what lets a pane
        // keep converging frame after frame.
        if let Some(t) = self.tracer.as_mut() {
            t.set_settings(solarxy_host::traced_preview::preview_trace_settings(true));
            // The filter's steering, for the same reason: a still authored
            // from a render node writes these four, so without this a
            // preview would inherit whatever the last still asked for. The
            // preview keeps the measured defaults.
            t.set_denoise_settings(DenoiseSettings::default());
        }
        // A pane bound to a camera previews through that camera's lens; a
        // free view is a pinhole. Asserted per encode like the settings
        // above and for the same reason. `set_lens` is a no-op when nothing
        // moved.
        let pane_lens = self.look_through[i.min(3)]
            .and_then(|id| {
                self.raster
                    .scene()
                    .cameras()
                    .and_then(|cams| cams.iter().find(|c| c.id == id))
                    .map(solarxy_host::cameras::lens_for)
            })
            .unwrap_or_default();
        if let Some(t) = self.tracer.as_mut() {
            t.set_lens(pane_lens);
        }
        let Some(cam) = self.view.cameras.get(i).and_then(Option::as_ref) else {
            return;
        };
        let camera = cam.camera;
        let key = solarxy_host::traced_preview::camera_key(&camera);
        if self.traced_cam_keys.get(i).copied().flatten() != Some(key) {
            if let Some(t) = self.tracer.as_mut() {
                t.invalidate_pane(i);
            }
            if let Some(slot) = self.traced_cam_keys.get_mut(i) {
                *slot = Some(key);
            }
        }
        // The viewer rig is re-applied every traced frame, not only on
        // reset: it is scene data shared by every pane, and with two traced
        // panes the last writer would otherwise win across frames. A no-op
        // under authored lights, and a constant write while the camera
        // rests, so it never disturbs a converging mean.
        if let Some(t) = self.tracer.as_mut() {
            solarxy_host::apply_viewer_rig(
                &self.device,
                &self.queue,
                t,
                self.raster.scene(),
                &camera,
            );
        }
    }

    /// The window went out of view, or came back. While it is out of view
    /// the frame loop stops asking for the next frame, so a traced pane
    /// accumulates nothing for nobody; coming back asks for one, and the
    /// loop resumes from there.
    pub fn set_occluded(&mut self, occluded: bool) {
        self.occluded = occluded;
        if !occluded {
            self.window.request_redraw();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_installs_when_flagged_or_moved_and_never_under_a_job() {
        let same = (1.0, 0.25);
        let moved = (1.5, 0.25);
        assert!(
            !environment_install_due(false, false, same, same),
            "nothing to do"
        );
        assert!(
            environment_install_due(false, true, same, same),
            "flagged stale"
        );
        assert!(
            environment_install_due(false, false, moved, same),
            "a scalar moved"
        );
        assert!(
            !environment_install_due(true, true, moved, same),
            "a running job owns the shared backend; the install is owed, not made"
        );
    }
}
