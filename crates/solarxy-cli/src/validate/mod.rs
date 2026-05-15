//! In-CLI validation orchestration.
//!
//! Public entry: [`run_validation`]. Expands glob patterns, loads each model
//! through `solarxy-formats`, runs `validate_raw_model_with_config` against
//! the supplied `ProjectConfig`, assembles a [`report::ValidationRunReport`],
//! formats it via the requested [`adapter::PipelineAdapter`], and writes the
//! result to stdout or a file.
//!
//! L2 — this module lives inside `solarxy-cli` (not a new crate). Extract to
//! `solarxy-validate` later when a vendor needs library access.

pub mod adapter;
mod adapters;
mod formats;
pub mod report;

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::Context;
use sha2::{Digest, Sha256};
use solarxy_core::json::JsonIssue;
use solarxy_core::project_config::{self, AssetCategory, FilenameClassifier, ProjectConfig};
use solarxy_core::validation::{IssueScope, Severity, ValidationReport};

pub use adapter::{
    AdapterFormat, AdapterName, AdapterOutput, Artifact, FailOn, PipelineAdapter, resolve_adapter,
};
pub use report::{FileFinding, FileStatus, RunSummary, ValidationRunReport};

/// `--output` destination.
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
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("read config '{}'", path.display()))?;
        let raw = std::str::from_utf8(&bytes)
            .with_context(|| format!("config '{}' is not utf-8", path.display()))?;
        let config: ProjectConfig =
            toml::from_str(raw).with_context(|| format!("parse config '{}'", path.display()))?;
        let hash = hex_encode(&Sha256::digest(&bytes));
        Ok(Self {
            config,
            path: Some(path.to_path_buf()),
            hash: Some(hash),
        })
    }

    /// Discovers a config starting at `start` (typically `.`).
    pub fn discover(start: &Path) -> anyhow::Result<Self> {
        match project_config::discover(start, None)
            .with_context(|| format!("discover config from '{}'", start.display()))?
        {
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
) -> anyhow::Result<i32> {
    let started_at = format_now();
    let started_instant = Instant::now();

    let classifier_rules =
        source.config.filenames.compile_rules().map_err(|e| {
            anyhow::anyhow!("invalid regex in solarxy.toml filename classifier: {e}")
        })?;

    let expanded = expand_globs(paths)?;
    if expanded.is_empty() {
        anyhow::bail!("no model files matched the given --paths patterns");
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
        std::fs::write(&artifact.path, &artifact.content)
            .with_context(|| format!("write artifact '{}'", artifact.path.display()))?;
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

/// Used by tests as well — keep this `Filenam`-private API stable.
fn expand_globs(patterns: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = Vec::new();
    for pattern in patterns {
        let mut matched = 0;
        for entry in glob::glob(pattern).with_context(|| format!("invalid glob '{pattern}'"))? {
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

fn write_output(text: &str, output: &Output) -> anyhow::Result<()> {
    match output {
        Output::Stdout => {
            io::stdout().write_all(text.as_bytes())?;
            if !text.ends_with('\n') {
                io::stdout().write_all(b"\n")?;
            }
        }
        Output::File(p) => {
            std::fs::write(p, text).with_context(|| format!("write report '{}'", p.display()))?;
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
