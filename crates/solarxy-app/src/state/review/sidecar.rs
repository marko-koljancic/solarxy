//! Writing the review sidecar file.
//!
//! The only part of review that touches the disk, and the only part that is an
//! `impl State` rather than an `impl ReviewState`: where the file goes is a
//! question about the open document and the project configuration rather than
//! about the annotations.
//!
//! Reading one lived here too, keyed on a file-loaded model's per-mesh hashes.
//! Those meshes were the second root's, and a document's are re-derived by the
//! cook, so the loader came out with the root rather than being pointed at
//! something it would have hashed differently. Save still works for a set
//! authored in this session; loading returns with the review repointing.

use crate::state::State;
use crate::gui::ToastSeverity;

impl State {
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
