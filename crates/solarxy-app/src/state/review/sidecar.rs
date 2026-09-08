//! Reading and writing the review sidecar file.
//!
//! The only part of review that touches the disk, and the only part that is an
//! `impl State` rather than an `impl ReviewState`: where the file goes is a
//! question about the open model and the project configuration rather than
//! about the annotations.

use crate::state::State;
use crate::gui::ToastSeverity;

impl State {
    /// Compute hashes + resolve sidecar path + attempt load. Called from
    /// the model-load completion site in `state::update`. Replaces any
    /// previously-loaded review state.
    pub(crate) fn load_review_for_model(&mut self, model_path: &str) {
        self.review.clear_for_new_model();
        let path = std::path::Path::new(model_path);
        let model_hash = solarxy_core::review::hash_file(path).ok();
        let mesh_hashes: Vec<String> = self
            .scene
            .as_ref()
            .map(|s| {
                s.model
                    .cpu_meshes
                    .iter()
                    .map(|m| solarxy_core::review::hash_positions_indices(&m.positions, &m.indices))
                    .collect()
            })
            .unwrap_or_default();

        self.review
            .author
            .clone_from(&self.preferences.review.author);
        self.review.model_hash = model_hash;
        self.review.mesh_hashes.clone_from(&mesh_hashes);

        let project_root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        let discovered = solarxy_core::project_config::discover(project_root, None)
            .ok()
            .flatten();
        if let Some((cfg_path, _)) = discovered.as_ref()
            && self.last_project_config_toast.as_ref() != Some(cfg_path)
        {
            let label = cfg_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("solarxy.toml")
                .to_string();
            self.gui.set_toast(
                &format!("Loaded project config from {label}"),
                ToastSeverity::Info,
            );
            self.last_project_config_toast = Some(cfg_path.clone());
        }
        let sidecar_dir = discovered.and_then(|(_, cfg)| cfg.review.sidecar_dir);
        let sidecar_path = solarxy_core::sidecar_path_for(path, sidecar_dir.as_deref());
        self.review.sidecar_path = Some(sidecar_path.clone());

        if !sidecar_path.exists() {
            return;
        }
        match solarxy_core::review::ReviewFile::load(&sidecar_path) {
            Ok(file) => {
                let count = file.annotations.len();
                let stored_hashes = file.mesh_hashes.clone();
                self.review.annotations = file.annotations;
                let stale = self.review.apply_stale_flags(&stored_hashes, &mesh_hashes);
                // Freshly loaded annotations match the on-disk file — not
                // dirty. `dirty` flips only on a real edit afterwards.
                self.review.dirty = false;
                let msg = if stale == 0 {
                    format!("Loaded {count} review annotations")
                } else {
                    format!("Loaded {count} annotations ({stale} need re-anchor)")
                };
                let severity = if stale == 0 {
                    ToastSeverity::Success
                } else {
                    ToastSeverity::Warning
                };
                self.gui.set_toast(&msg, severity);
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Review load failed: {e}"), ToastSeverity::Error);
            }
        }
    }

    /// Persist the current annotation set + hashes to the resolved sidecar
    /// path. Toasts on success or failure.
    pub(crate) fn save_review_sidecar(&mut self) {
        let Some(path) = self.review.sidecar_path.clone() else {
            self.gui.set_toast(
                "Open a model before saving review notes",
                ToastSeverity::Warning,
            );
            return;
        };
        if self.review.annotations.is_empty() && !path.exists() {
            self.gui
                .set_toast("No annotations to save", ToastSeverity::Info);
            return;
        }

        let file = solarxy_core::review::ReviewFile {
            format_version: solarxy_core::review::FORMAT_VERSION_CURRENT,
            model_hash: self.review.model_hash.clone().unwrap_or_default(),
            mesh_hashes: self.review.mesh_hashes.clone(),
            annotations: self.review.annotations.clone(),
        };
        match file.save(&path) {
            Ok(()) => {
                self.review.dirty = false;
                let count = self.review.annotations.len();
                tracing::info!(
                    target: "solarxy::toast",
                    "Saved {} annotations to {}",
                    count,
                    path.display()
                );
                self.gui.set_toast(
                    &format!("Saved {count} annotations"),
                    ToastSeverity::Success,
                );
            }
            Err(e) => {
                self.gui
                    .set_toast(&format!("Save failed: {e}"), ToastSeverity::Error);
            }
        }
    }
}
