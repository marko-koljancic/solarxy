//! Concrete pipeline adapters.
//!
//! Each adapter declares its default format, dispatches the requested format
//! through the [`crate::validate::formats`] renderers, and computes an exit
//! code per the `--fail-on` policy.

use anyhow::anyhow;

use crate::validate::adapter::{AdapterFormat, AdapterOutput, FailOn, PipelineAdapter};
use crate::validate::formats::{
    render_gha_commands, render_json, render_sarif, render_tap, render_text,
};
use crate::validate::report::{FileStatus, ValidationRunReport};

pub struct GenericAdapter;

impl PipelineAdapter for GenericAdapter {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn default_format(&self) -> AdapterFormat {
        AdapterFormat::Json
    }

    fn format_report(
        &self,
        report: &ValidationRunReport,
        format: AdapterFormat,
    ) -> anyhow::Result<AdapterOutput> {
        let stdout = match format {
            AdapterFormat::Json => render_json(report)?,
            AdapterFormat::Text => render_text(report),
            AdapterFormat::Tap => render_tap(report),
            AdapterFormat::GhaCommands | AdapterFormat::Sarif => {
                return Err(anyhow!(
                    "generic adapter does not support format '{format:?}'; \
                     use --adapter github-actions"
                ));
            }
        };
        Ok(AdapterOutput {
            stdout,
            artifacts: Vec::new(),
        })
    }

    fn exit_code(&self, report: &ValidationRunReport, fail_on: FailOn) -> i32 {
        exit_code_for(report, fail_on)
    }
}

pub struct GithubActionsAdapter;

impl PipelineAdapter for GithubActionsAdapter {
    fn name(&self) -> &'static str {
        "github-actions"
    }

    fn default_format(&self) -> AdapterFormat {
        AdapterFormat::GhaCommands
    }

    fn format_report(
        &self,
        report: &ValidationRunReport,
        format: AdapterFormat,
    ) -> anyhow::Result<AdapterOutput> {
        let stdout = match format {
            AdapterFormat::GhaCommands => render_gha_commands(report),
            AdapterFormat::Sarif => render_sarif(report)?,
            AdapterFormat::Json => render_json(report)?,
            AdapterFormat::Text => render_text(report),
            AdapterFormat::Tap => render_tap(report),
        };
        Ok(AdapterOutput {
            stdout,
            artifacts: Vec::new(),
        })
    }

    fn exit_code(&self, report: &ValidationRunReport, fail_on: FailOn) -> i32 {
        exit_code_for(report, fail_on)
    }
}

fn exit_code_for(report: &ValidationRunReport, fail_on: FailOn) -> i32 {
    let has_errors = report
        .findings
        .iter()
        .any(|f| matches!(f.status, FileStatus::Error | FileStatus::LoadFailed));
    let has_warnings = report
        .findings
        .iter()
        .any(|f| f.status == FileStatus::Warning);
    match fail_on {
        FailOn::Never => 0,
        FailOn::Warning => i32::from(has_errors || has_warnings),
        FailOn::Error => i32::from(has_errors),
    }
}
