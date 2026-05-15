//! Pipeline adapter trait + dispatch.
//!
//! An adapter wraps the format + exit-code conventions of a target CI/CD
//! ecosystem. Two ship in 0.6.0:
//! - `generic` — JSON-by-default, CI-agnostic. Use this through scripts /
//!   recipes (Perforce, GitLab, Jenkins) that consume the JSON.
//! - `github-actions` — workflow-command-by-default, with SARIF for Code
//!   Scanning. Detects GHA via env vars; emits annotations directly.

use std::path::PathBuf;

use crate::validate::report::ValidationRunReport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum AdapterName {
    Generic,
    GithubActions,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum AdapterFormat {
    Json,
    Text,
    Tap,
    GhaCommands,
    Sarif,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "kebab-case")]
pub enum FailOn {
    Error,
    Warning,
    Never,
}

#[derive(Debug)]
pub struct AdapterOutput {
    /// Primary textual emission — written to `--output` (file or stdout).
    pub stdout: String,
    /// Side artifacts written to disk regardless of `--output` (e.g. SARIF
    /// when uploaded directly by the GHA `upload-sarif` action).
    pub artifacts: Vec<Artifact>,
}

#[derive(Debug)]
pub struct Artifact {
    pub path: PathBuf,
    pub content: String,
}

pub trait PipelineAdapter {
    fn name(&self) -> &'static str;
    fn default_format(&self) -> AdapterFormat;
    fn format_report(
        &self,
        report: &ValidationRunReport,
        format: AdapterFormat,
    ) -> anyhow::Result<AdapterOutput>;
    fn exit_code(&self, report: &ValidationRunReport, fail_on: FailOn) -> i32;
}

pub fn resolve_adapter(name: AdapterName) -> Box<dyn PipelineAdapter> {
    match name {
        AdapterName::Generic => Box::new(super::adapters::GenericAdapter),
        AdapterName::GithubActions => Box::new(super::adapters::GithubActionsAdapter),
    }
}
