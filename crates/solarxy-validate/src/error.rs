//! Error type for [`crate::run_validation`] and the surrounding orchestration
//! functions.
//!
//! Per the workspace convention documented in the root `CLAUDE.md`,
//! library crates use `thiserror` rather than `anyhow`. Consumers can
//! convert with `anyhow::Error::from(err)` if they prefer anyhow
//! ergonomics.

use std::path::PathBuf;

use thiserror::Error;

use crate::adapter::AdapterFormat;

/// Errors raised by the validation orchestration pipeline.
///
/// Each variant carries the offending path / pattern in a structured field
/// so callers can route messages without parsing strings. The `Display`
/// impl produces a human-readable single-line message suitable for CLI
/// stderr output.
#[derive(Error, Debug)]
pub enum ValidationRunError {
    #[error("read config '{path}': {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("config '{path}' is not utf-8: {source}")]
    ConfigNotUtf8 {
        path: PathBuf,
        #[source]
        source: std::str::Utf8Error,
    },

    #[error("parse config '{path}': {source}")]
    ConfigParse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("discover config from '{path}': {source}")]
    ConfigDiscover {
        path: PathBuf,
        #[source]
        source: solarxy_core::project_config::ProjectConfigError,
    },

    #[error("invalid regex in solarxy.toml filename classifier: {0}")]
    InvalidClassifierRegex(String),

    #[error("invalid glob '{pattern}': {source}")]
    InvalidGlob {
        pattern: String,
        #[source]
        source: glob::PatternError,
    },

    #[error("no model files matched the given --paths patterns")]
    NoMatchingPaths,

    #[error("write artifact '{path}': {source}")]
    ArtifactWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("write report '{path}': {source}")]
    OutputWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("stdout write: {0}")]
    StdoutWrite(#[source] std::io::Error),

    #[error("serialize report: {0}")]
    Serialize(#[from] serde_json::Error),

    #[error("adapter '{adapter}' does not support format '{format:?}'; use a compatible adapter")]
    UnsupportedFormat {
        adapter: &'static str,
        format: AdapterFormat,
    },
}
