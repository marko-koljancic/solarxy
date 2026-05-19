//! Validation orchestration + pipeline adapters for Solarxy 3D asset checks.
//!
//! This crate sits one layer above [`solarxy_core::validation`] and
//! [`solarxy_formats`]:
//!
//! - Resolves a [`ProjectConfig`] (explicit path → `$SOLARXY_CONFIG` →
//!   discovery walk).
//! - Expands `--paths` glob patterns to a sorted, deduped list of model
//!   files.
//! - Loads each model via `solarxy_formats::load_model`, classifies it via
//!   the project config's filename rules, picks the per-category budget,
//!   and runs `solarxy_core::validation::validate_raw_model_with_config`.
//! - Assembles a [`ValidationRunReport`] (the canonical wire format —
//!   `schema_version: u32`, currently `1`).
//! - Hands the report to a [`PipelineAdapter`] which serialises it for a
//!   specific CI ecosystem (GitHub Actions, generic-JSON for Perforce /
//!   GitLab / Jenkins) and computes an exit code per `--fail-on`.
//!
//! # Stability
//!
//! Public types in this crate are part of Solarxy's stable wire format and
//! are guarded by `cargo-semver-checks` against the published baseline.
//! Adding fields to [`ValidationRunReport`] / [`FileFinding`] is a minor
//! version bump; removing or renaming is a major. The
//! `schema_version` field carried in [`ValidationRunReport`] gives
//! downstream consumers a runtime check independent of the Rust semver.
//!
//! # Library vs. CLI
//!
//! Vendors embedding this crate to surface validation results inside their
//! own product (DAM, asset store, training-data pipeline) depend on this
//! crate directly and never reach into `solarxy-cli`. The CLI is a thin
//! wrapper over [`run_validation`] that adds `clap` argument parsing and
//! routes stdout / file output.
//!
//! ```no_run
//! use std::path::Path;
//! use solarxy_validate::{ConfigSource, Output, run_validation};
//! use solarxy_validate::adapter::{AdapterName, FailOn, resolve_adapter};
//!
//! let source = ConfigSource::discover(Path::new("."))?;
//! let adapter = resolve_adapter(AdapterName::Generic);
//! let format = adapter.default_format();
//! let exit_code = run_validation(
//!     &["assets/**/*.glb".to_string()],
//!     source,
//!     adapter.as_ref(),
//!     format,
//!     FailOn::Error,
//!     &Output::Stdout,
//! )?;
//! # Ok::<(), solarxy_validate::ValidationRunError>(())
//! ```

#![warn(clippy::pedantic)]
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::doc_markdown,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::module_name_repetitions,
    clippy::must_use_candidate,
    clippy::uninlined_format_args
)]

pub mod adapter;
mod adapters;
pub mod error;
mod formats;
pub mod report;

use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};
use solarxy_core::json::JsonIssue;
use solarxy_core::project_config::{self, AssetCategory, FilenameClassifier, ProjectConfig};
use solarxy_core::validation::{IssueScope, Severity, ValidationReport};

pub use adapter::{
    AdapterFormat, AdapterName, AdapterOutput, Artifact, FailOn, PipelineAdapter, resolve_adapter,
};
pub use error::ValidationRunError;
pub use report::{FileFinding, FileStatus, RunSummary, ValidationRunReport};

/// Destination for the rendered adapter output.
///
/// The string `"-"` is treated as a stdout alias, matching the Unix CLI
/// convention (`grep`, `tar`, `jq`, etc.).
pub enum Output {
    Stdout,
    File(PathBuf),
}

impl Output {
    /// Accepts `"-"` as a stdout synonym; otherwise files.
    pub fn from_path(path: Option<&Path>) -> Self {
        match path {
            None => Output::Stdout,
            Some(p) if p.as_os_str() == "-" => Output::Stdout,
            Some(p) => Output::File(p.to_path_buf()),
        }
    }
}

/// Bundles a discovered config with its on-disk metadata so the run report
/// can record provenance (`config_path` + SHA-256 hash of the raw bytes).
pub struct ConfigSource {
    pub config: ProjectConfig,
    pub path: Option<PathBuf>,
    pub hash: Option<String>,
}

impl ConfigSource {
    /// Defaults — no on-disk source.
    pub fn defaults() -> Self {
        Self {
            config: ProjectConfig::default(),
            path: None,
            hash: None,
        }
    }

    /// Loads `path` and computes a SHA-256 of the raw TOML bytes.
    pub fn load(path: &Path) -> Result<Self, ValidationRunError> {
        let bytes = std::fs::read(path).map_err(|source| ValidationRunError::ConfigRead {
            path: path.to_path_buf(),
            source,
        })?;
        let raw =
            std::str::from_utf8(&bytes).map_err(|source| ValidationRunError::ConfigNotUtf8 {
                path: path.to_path_buf(),
                source,
            })?;
        let config: ProjectConfig =
            toml::from_str(raw).map_err(|source| ValidationRunError::ConfigParse {
                path: path.to_path_buf(),
                source,
            })?;
        let hash = hex_encode(&Sha256::digest(&bytes));
        Ok(Self {
            config,
            path: Some(path.to_path_buf()),
            hash: Some(hash),
        })
    }

    /// Discovers a config starting at `start` (typically `.`).
    pub fn discover(start: &Path) -> Result<Self, ValidationRunError> {
        match project_config::discover(start, None).map_err(|source| {
            ValidationRunError::ConfigDiscover {
                path: start.to_path_buf(),
                source,
            }
        })? {
            Some((path, _cfg)) => Self::load(&path),
            None => Ok(Self::defaults()),
        }
    }
}

/// Executes one validation run end-to-end and returns the adapter's chosen
/// exit code.
pub fn run_validation(
    paths: &[String],
    source: ConfigSource,
    adapter: &dyn PipelineAdapter,
    format: AdapterFormat,
    fail_on: FailOn,
    output: &Output,
) -> Result<i32, ValidationRunError> {
    let started_at = format_now();
    let started_instant = Instant::now();

    let classifier_rules = source
        .config
        .filenames
        .compile_rules()
        .map_err(|e| ValidationRunError::InvalidClassifierRegex(e.to_string()))?;

    let expanded = expand_globs(paths)?;
    if expanded.is_empty() {
        return Err(ValidationRunError::NoMatchingPaths);
    }

    let mut findings: Vec<FileFinding> = Vec::with_capacity(expanded.len());
    for path in expanded {
        findings.push(run_one(&path, &source.config, &classifier_rules));
    }

    let elapsed_ms = started_instant.elapsed().as_millis() as u64;
    let summary = RunSummary::from_findings(&findings, elapsed_ms);
    let report = ValidationRunReport {
        schema_version: report::RUN_REPORT_SCHEMA_VERSION,
        run_id: ulid::Ulid::new().to_string(),
        solarxy_version: env!("CARGO_PKG_VERSION"),
        started_at,
        config_path: source.path,
        config_hash: source.hash,
        findings,
        summary,
    };

    let rendered = adapter.format_report(&report, format)?;
    write_output(&rendered.stdout, output)?;
    for artifact in &rendered.artifacts {
        std::fs::write(&artifact.path, &artifact.content).map_err(|source| {
            ValidationRunError::ArtifactWrite {
                path: artifact.path.clone(),
                source,
            }
        })?;
    }

    Ok(adapter.exit_code(&report, fail_on))
}

fn run_one(
    path: &Path,
    config: &ProjectConfig,
    classifier_rules: &[(regex::Regex, AssetCategory)],
) -> FileFinding {
    let category = project_config::classify_compiled(classifier_rules, path);
    let budget = if config.validation.triangle_budget {
        Some(config.budgets.for_category(category))
    } else {
        None
    };

    match solarxy_formats::load_model(path.to_string_lossy().as_ref()) {
        Ok(raw) => {
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let result = solarxy_core::validation::validate_raw_model_with_config(
                &raw,
                &ext,
                &config.validation,
                &config.thresholds,
                budget,
            );
            let triangles = u32::try_from(
                raw.meshes
                    .iter()
                    .map(|m| m.indices.len() / 3)
                    .sum::<usize>(),
            )
            .ok();
            build_finding(path, category, triangles, &result.report)
        }
        Err(e) => FileFinding {
            path: path.to_path_buf(),
            status: FileStatus::LoadFailed,
            category: Some(category),
            triangles: None,
            error_count: 0,
            warning_count: 0,
            issues: Vec::new(),
            load_error: Some(e.to_string()),
        },
    }
}

fn build_finding(
    path: &Path,
    category: AssetCategory,
    triangles: Option<u32>,
    report: &ValidationReport,
) -> FileFinding {
    let issues: Vec<JsonIssue> = report.issues.iter().map(JsonIssue::from).collect();
    let mut errors: u32 = 0;
    let mut warnings: u32 = 0;
    for i in &report.issues {
        match i.severity {
            Severity::Error => errors += 1,
            Severity::Warning => warnings += 1,
        }
    }
    let status = if errors > 0 {
        FileStatus::Error
    } else if warnings > 0 {
        FileStatus::Warning
    } else {
        FileStatus::Clean
    };

    let _ = IssueScope::Model;
    FileFinding {
        path: path.to_path_buf(),
        status,
        category: Some(category),
        triangles,
        error_count: errors,
        warning_count: warnings,
        issues,
        load_error: None,
    }
}

fn expand_globs(patterns: &[String]) -> Result<Vec<PathBuf>, ValidationRunError> {
    let mut out: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        let mut matched = 0;
        for entry in glob::glob(pattern).map_err(|source| ValidationRunError::InvalidGlob {
            pattern: pattern.clone(),
            source,
        })? {
            match entry {
                Ok(path) if path.is_file() => {
                    out.push(path);
                    matched += 1;
                }
                Ok(_) => {}
                Err(e) => tracing::warn!("glob '{pattern}': {e}"),
            }
        }
        if matched == 0 {
            tracing::warn!("glob '{pattern}' matched no files");
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn write_output(text: &str, output: &Output) -> Result<(), ValidationRunError> {
    use std::io::Write;
    match output {
        Output::Stdout => {
            let mut stdout = std::io::stdout();
            stdout
                .write_all(text.as_bytes())
                .map_err(ValidationRunError::StdoutWrite)?;
            if !text.ends_with('\n') {
                stdout
                    .write_all(b"\n")
                    .map_err(ValidationRunError::StdoutWrite)?;
            }
        }
        Output::File(p) => {
            std::fs::write(p, text).map_err(|source| ValidationRunError::OutputWrite {
                path: p.clone(),
                source,
            })?;
        }
    }
    Ok(())
}

fn format_now() -> String {
    use time::OffsetDateTime;
    use time::format_description::well_known::Rfc3339;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "unknown".into())
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(hex_digit(b >> 4));
        s.push(hex_digit(b & 0xf));
    }
    s
}

fn hex_digit(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => '?',
    }
}

const _: fn() = || {
    let _ = std::mem::size_of::<FilenameClassifier>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_from_path_dash_means_stdout() {
        assert!(matches!(
            Output::from_path(Some(Path::new("-"))),
            Output::Stdout
        ));
        assert!(matches!(Output::from_path(None), Output::Stdout));
        assert!(matches!(
            Output::from_path(Some(Path::new("/tmp/out.json"))),
            Output::File(_)
        ));
    }

    #[test]
    fn hex_encode_roundtrip() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
    }
}
