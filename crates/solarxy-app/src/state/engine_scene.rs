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

/// The open document's file facts. `None` on `State` whenever nothing is
/// open.
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
}

impl EngineSceneInfo {
    pub fn new(filename: String, path: String) -> Self {
        Self {
            filename,
            path,
            created: String::new(),
        }
    }
}
