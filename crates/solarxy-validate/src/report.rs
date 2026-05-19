//! Public data types for a validation run.
//!
//! [`ValidationRunReport`] is the canonical structure emitted by
//! [`crate::run_validation`]; adapters consume it and format it into their
//! adapter-specific output (JSON, SARIF, GHA workflow commands).
//!
//! The on-disk JSON shape carries [`schema_version`](ValidationRunReport::schema_version)
//! so CI consumers can lock against a compatible version. The current
//! version is [`RUN_REPORT_SCHEMA_VERSION`] (`1` in 0.6.0).
//!
//! # Stability
//!
//! Adding fields is a minor version bump (consumers must tolerate unknown
//! fields). Removing or renaming a field requires a `schema_version` bump
//! with a documented migration path.

use std::path::PathBuf;

use serde::Serialize;
use solarxy_core::json::JsonIssue;
use solarxy_core::project_config::AssetCategory;

/// Wire-format version of [`ValidationRunReport`].
///
/// CI consumers should compare this against their expected version and
/// bail rather than silently misinterpret a future-shaped report.
pub const RUN_REPORT_SCHEMA_VERSION: u32 = 1;

/// Top-level report emitted by a validation run.
///
/// Serializes to JSON via the (private) `formats::render_json` renderer;
/// consumed by adapters to emit format-specific output.
#[derive(Debug, Clone, Serialize)]
pub struct ValidationRunReport {
    pub schema_version: u32,
    pub run_id: String,
    pub solarxy_version: &'static str,
    pub started_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_hash: Option<String>,
    pub findings: Vec<FileFinding>,
    pub summary: RunSummary,
}

/// Per-file result. One entry per model path expanded from `--paths`.
#[derive(Debug, Clone, Serialize)]
pub struct FileFinding {
    pub path: PathBuf,
    pub status: FileStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<AssetCategory>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triangles: Option<u32>,
    pub error_count: u32,
    pub warning_count: u32,
    pub issues: Vec<JsonIssue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub load_error: Option<String>,
}

/// Top-level status for a single file. `LoadFailed` distinguishes a tool
/// error (couldn't parse) from an asset error (validated and failed).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FileStatus {
    Clean,
    Warning,
    Error,
    LoadFailed,
}

/// Aggregated counts for the entire run.
#[derive(Debug, Clone, Serialize)]
pub struct RunSummary {
    pub files_total: u32,
    pub files_clean: u32,
    pub files_with_warnings: u32,
    pub files_with_errors: u32,
    pub files_load_failed: u32,
    pub elapsed_ms: u64,
}

impl RunSummary {
    pub fn from_findings(findings: &[FileFinding], elapsed_ms: u64) -> Self {
        let mut clean = 0;
        let mut warns = 0;
        let mut errs = 0;
        let mut failed = 0;
        for f in findings {
            match f.status {
                FileStatus::Clean => clean += 1,
                FileStatus::Warning => warns += 1,
                FileStatus::Error => errs += 1,
                FileStatus::LoadFailed => failed += 1,
            }
        }
        Self {
            files_total: findings.len() as u32,
            files_clean: clean,
            files_with_warnings: warns,
            files_with_errors: errs,
            files_load_failed: failed,
            elapsed_ms,
        }
    }
}
