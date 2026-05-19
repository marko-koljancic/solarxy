//! Pipeline adapter trait + dispatch.
//!
//! An adapter wraps the format + exit-code conventions of a target CI/CD
//! ecosystem. Two ship in 0.6.0:
//! - `generic` — JSON-by-default, CI-agnostic. Use this through scripts /
//!   recipes (Perforce, GitLab, Jenkins) that consume the JSON.
//! - `github-actions` — workflow-command-by-default, with SARIF for Code
//!   Scanning. Detects GHA via env vars; emits annotations directly.
//!
//! New adapters live in the (private) `adapters` module; add a variant to
//! [`AdapterName`] + a match arm in [`resolve_adapter`].

use std::path::PathBuf;

use crate::error::ValidationRunError;
use crate::report::ValidationRunReport;

/// Adapter selection (CLI: `--adapter`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "kebab-case"))]
pub enum AdapterName {
    Generic,
    GithubActions,
}

/// Output format selection (CLI: `--adapter-format`).
///
/// Not every adapter supports every format; mismatch yields
/// [`ValidationRunError::UnsupportedFormat`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "kebab-case"))]
pub enum AdapterFormat {
    Json,
    Text,
    Tap,
    /// JUnit XML 4.x dialect understood by GitLab CI's
    /// `artifacts:reports:junit` and the Jenkins JUnit Plugin. One
    /// `<testcase>` per file finding; issues nested as `<failure>` /
    /// `<error>`; warning-only files stay green and surface details in
    /// `<system-out>`.
    JunitXml,
    GhaCommands,
    Sarif,
}

/// Exit-code policy (CLI: `--fail-on`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "clap", derive(clap::ValueEnum))]
#[cfg_attr(feature = "clap", clap(rename_all = "kebab-case"))]
pub enum FailOn {
    Error,
    Warning,
    Never,
}

/// The adapter's rendered output, ready to be written to disk / stdout.
#[derive(Debug)]
pub struct AdapterOutput {
    /// Primary textual emission — written to `--output` (file or stdout).
    pub stdout: String,
    /// Side artifacts written to disk regardless of `--output` (e.g. SARIF
    /// when uploaded directly by the GHA `upload-sarif` action).
    pub artifacts: Vec<Artifact>,
}

/// A side artifact emitted by an adapter (e.g. a `.sarif` file alongside
/// stdout commands).
#[derive(Debug)]
pub struct Artifact {
    pub path: PathBuf,
    pub content: String,
}

/// Trait implemented by each pipeline adapter.
///
/// Adapters are stateless; instances are constructed by
/// [`resolve_adapter`] and dropped after the run. Adding a new adapter
/// means: implement the trait in the private `adapters` module, add a
/// variant to [`AdapterName`], and add a match arm to [`resolve_adapter`].
pub trait PipelineAdapter {
    fn name(&self) -> &'static str;
    fn default_format(&self) -> AdapterFormat;
    fn format_report(
        &self,
        report: &ValidationRunReport,
        format: AdapterFormat,
    ) -> Result<AdapterOutput, ValidationRunError>;
    fn exit_code(&self, report: &ValidationRunReport, fail_on: FailOn) -> i32;
}

/// Constructs the adapter implementation for `name`.
pub fn resolve_adapter(name: AdapterName) -> Box<dyn PipelineAdapter> {
    match name {
        AdapterName::Generic => Box::new(super::adapters::GenericAdapter),
        AdapterName::GithubActions => Box::new(super::adapters::GithubActionsAdapter),
    }
}
