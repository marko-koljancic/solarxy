//! What the shell knows about the open document as a file.
//!
//! The name it shows in the title bar, the path a save writes back to, and
//! the creation stamp a save carries forward. Everything about the
//! document's *content* is the engine's and is read from it directly.
//!
//! Until 0.10.0 this also collapsed every cooked object's geometry counts
//! and validation report into the single shapes the file-model panels took,
//! rebuilt on every scene delta. Those panels are gone: cook statistics are read
//! in the parameter panel's header and validation on its Validation tab,
//! both from the node's own report, so nothing was left reading the merge.

use std::collections::BTreeMap;

use solarxy_graph::engine::SceneSidecar;
use solarxy_scenefile::CanvasViewportJson;

/// The open document's file facts. `None` on `State` whenever nothing is
/// open.
///
/// The last three fields are carried rather than owned: this shell edits
/// none of them and has no surface that reads them, but the scene file
/// records them, and a save that dropped what it had loaded would make the
/// desktop a place a scene loses things by passing through. A browser that
/// saved a description or a canvas viewport gets them back after a desktop
/// save, whether or not anything here understood them.
pub(crate) struct EngineSceneInfo {
    /// The file name the title bar and the save dialogs show. `Untitled`
    /// for a document that has never been saved.
    pub filename: String,
    /// Where a save writes. Empty for a document with no file yet, which is
    /// what routes Save to Save As.
    pub path: String,
    /// When the document was first written, as the file's metadata records
    /// it. Empty for a document that has never been saved; a save keeps it
    /// and stamps only `modified`.
    pub created: String,
    /// The scene's description, as loaded.
    pub description: String,
    /// The project the scene belongs to, as loaded.
    pub project_id: String,
    /// The per-network canvas pan and zoom the file carried, keyed the way
    /// the browser keys them, as loaded.
    pub canvas_viewports: BTreeMap<String, CanvasViewportJson>,
}

impl EngineSceneInfo {
    pub fn new(filename: String, path: String) -> Self {
        Self {
            filename,
            path,
            created: String::new(),
            description: String::new(),
            project_id: String::new(),
            canvas_viewports: BTreeMap::new(),
        }
    }

    /// Take what a loaded scene file carried beside the document, so the
    /// next save writes it back.
    pub fn carry(&mut self, sidecar: &SceneSidecar) {
        self.created.clone_from(&sidecar.meta.created);
        self.description.clone_from(&sidecar.meta.description);
        self.project_id.clone_from(&sidecar.meta.project_id);
        self.canvas_viewports.clone_from(&sidecar.canvas_viewports);
    }
}
