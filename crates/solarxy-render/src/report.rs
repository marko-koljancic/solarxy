//! The machine-readable result.
//!
//! Versioned from its first release, following the validation report, because a
//! consumer that parses this has no other way to know whether a field it is
//! missing was removed or was never there. The rule that comes with the version
//! is the same one: adding a field is a minor change, removing or retyping one
//! is a version bump.

use serde::Serialize;

/// The schema version stamped into every report.
///
/// Bump when a field is removed or changes type. Adding one does not.
pub const RENDER_REPORT_SCHEMA_VERSION: u32 = 1;

/// What a finished render did, for a pipeline to read off stdout.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderReport {
    pub schema_version: u32,
    pub solarxy_version: &'static str,
    /// Where the image went. Absolute, because a relative path means nothing to
    /// whoever reads the report on another machine.
    pub output: String,
    pub width: u32,
    pub height: u32,
    /// `"raster"` or `"pathTraced"`, spelled the way the boundary spells it
    /// everywhere else.
    pub engine: &'static str,
    /// Samples per pixel actually rendered. One for a rasterized still, which
    /// is the honest answer rather than zero.
    pub samples: u32,
    pub tiles: u32,
    pub elapsed_ms: u64,
    /// Non-fatal things the run wants a reader to know: a scene with no camera,
    /// a topology the tracer skipped. Empty is the ordinary case.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    /// The auxiliary passes written beside the image, absolute, in the order
    /// they were asked for. Absent when none were.
    ///
    /// An added field, so the schema version does not move: a reader that does
    /// not know about it is not wrong about anything it does read.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aovs: Vec<String>,
}

impl RenderReport {
    /// Serializes to a single line of JSON.
    ///
    /// One line rather than pretty-printed: the consumer is a build system
    /// reading a stream, and a multi-line object interleaves badly with
    /// anything else that reaches the same file.
    ///
    /// # Errors
    /// Only if serialization fails, which for this shape it cannot.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }
}
