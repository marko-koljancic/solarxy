//! Output formatters consumed by the pipeline adapters.
//!
//! - [`render_json`] — pretty-printed [`ValidationRunReport`]; the canonical
//!   on-disk format.
//! - [`render_text`] — human-readable summary for terminal viewing.
//! - [`render_tap`] — TAP version 14 stream (one ok/not-ok line per file).
//! - [`render_gha_commands`] — `::error file=…::msg` workflow commands. Logs
//!   surface inline in GitHub Actions PRs.
//! - [`render_sarif`] — SARIF 2.1.0 document for GitHub Code Scanning.

use serde_json::json;

use crate::validate::report::{FileStatus, ValidationRunReport};

pub fn render_json(report: &ValidationRunReport) -> anyhow::Result<String> {
    serde_json::to_string_pretty(report).map_err(anyhow::Error::from)
}

pub fn render_text(report: &ValidationRunReport) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "Solarxy validation run {}", report.run_id);
    let _ = writeln!(
        out,
        "  solarxy {}  started_at {}",
        report.solarxy_version, report.started_at
    );
    if let Some(p) = &report.config_path {
        let _ = writeln!(out, "  config: {}", p.display());
    }
    let _ = writeln!(
        out,
        "  files: {} total ({} clean, {} warnings, {} errors, {} load failed)  elapsed {}ms",
        report.summary.files_total,
        report.summary.files_clean,
        report.summary.files_with_warnings,
        report.summary.files_with_errors,
        report.summary.files_load_failed,
        report.summary.elapsed_ms,
    );
    for f in &report.findings {
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "{} {}  ({} error, {} warning)",
            status_glyph(f.status),
            f.path.display(),
            f.error_count,
            f.warning_count,
        );
        if let Some(err) = &f.load_error {
            let _ = writeln!(out, "    load failed: {err}");
            continue;
        }
        for issue in &f.issues {
            let _ = writeln!(
                out,
                "    [{}] {} ({}): {}",
                issue.severity, issue.kind, issue.scope, issue.message
            );
        }
    }
    out
}

pub fn render_tap(report: &ValidationRunReport) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "TAP version 14");
    let _ = writeln!(out, "1..{}", report.findings.len());
    for (i, f) in report.findings.iter().enumerate() {
        let n = i + 1;
        let ok = matches!(f.status, FileStatus::Clean | FileStatus::Warning);
        let prefix = if ok { "ok" } else { "not ok" };
        let _ = writeln!(out, "{prefix} {n} - {}", f.path.display());
        if !ok || f.warning_count > 0 || f.load_error.is_some() {
            let _ = writeln!(out, "  ---");
            if let Some(err) = &f.load_error {
                let _ = writeln!(out, "  load_error: {err}");
            }
            let _ = writeln!(out, "  errors: {}", f.error_count);
            let _ = writeln!(out, "  warnings: {}", f.warning_count);
            let _ = writeln!(out, "  issues:");
            for issue in &f.issues {
                let _ = writeln!(
                    out,
                    "    - [{}] {}: {}",
                    issue.severity, issue.kind, issue.message
                );
            }
            let _ = writeln!(out, "  ...");
        }
    }
    out
}

pub fn render_gha_commands(report: &ValidationRunReport) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for f in &report.findings {
        let path = f.path.display();
        if let Some(err) = &f.load_error {
            let _ = writeln!(
                out,
                "::error file={path},line=0,col=0,title=LoadFailed::{}",
                escape_gha_message(err)
            );
            continue;
        }
        for issue in &f.issues {
            let level = if issue.severity == "error" {
                "error"
            } else {
                "warning"
            };
            let _ = writeln!(
                out,
                "::{level} file={path},line=0,col=0,title={}::{}",
                issue.kind,
                escape_gha_message(&issue.message)
            );
        }
    }
    out
}

fn escape_gha_message(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

pub fn render_sarif(report: &ValidationRunReport) -> anyhow::Result<String> {
    let mut rule_ids: Vec<String> = report
        .findings
        .iter()
        .flat_map(|f| f.issues.iter().map(|i| i.kind.clone()))
        .collect();
    rule_ids.sort();
    rule_ids.dedup();

    let rules: Vec<_> = rule_ids
        .iter()
        .map(|id| {
            json!({
                "id": id,
                "name": id,
                "shortDescription": { "text": id },
                "helpUri": "https://github.com/marko-koljancic/solarxy/wiki/Troubleshooting",
            })
        })
        .collect();

    let results: Vec<_> = report
        .findings
        .iter()
        .flat_map(|f| {
            let path = f.path.display().to_string();
            f.issues.iter().map(move |issue| {
                json!({
                    "ruleId": issue.kind,
                    "level": sarif_level(&issue.severity),
                    "message": { "text": issue.message },
                    "locations": [{
                        "physicalLocation": {
                            "artifactLocation": { "uri": &path },
                        }
                    }],
                })
            })
        })
        .collect();

    let sarif = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "solarxy",
                    "version": report.solarxy_version,
                    "informationUri": "https://github.com/marko-koljancic/solarxy",
                    "rules": rules,
                }
            },
            "results": results,
        }]
    });
    serde_json::to_string_pretty(&sarif).map_err(anyhow::Error::from)
}

fn sarif_level(severity: &str) -> &'static str {
    match severity {
        "error" => "error",
        "warning" => "warning",
        _ => "note",
    }
}

fn status_glyph(s: FileStatus) -> &'static str {
    match s {
        FileStatus::Clean => "OK",
        FileStatus::Warning => "WARN",
        FileStatus::Error => "FAIL",
        FileStatus::LoadFailed => "LOAD",
    }
}
