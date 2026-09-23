//! The traced viewport pane: what keeps a pane's path tracer in step with
//! the document and the view.
//!
//! A per-shell twin by structure, like the gizmo drive loop and
//! `trace_settings_for`: it reads an `Engine` and writes a renderer backend,
//! so it can live in neither `solarxy-host`, which has no engine, nor
//! `solarxy-graph`, which has no renderer. The browser host carries the
//! same code, and the two are held to one behaviour by reading rather than
//! by a shared type.

use solarxy_core::view_config::PaneEngine;
use solarxy_graph::Engine;
use solarxy_renderer::pathtrace::backend::PathBackend;

use crate::state::State;

impl State {
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
    }
}
