//! [`HdriInfo`]: what the shell remembers about the loaded HDRI, shown in
//! the Environment dialog and read back when a save stages the file. Built
//! when an HDRI finishes loading (`state/update.rs`) and stored on the GUI
//! side (`EguiRenderer`).

/// Metadata for the currently loaded HDRI environment map.
#[derive(Debug, Clone)]
pub(crate) struct HdriInfo {
    /// File name (no directory).
    pub filename: String,
    /// Full path as displayed to the user, and where a save reads the
    /// bytes to stage.
    pub path: String,
}
