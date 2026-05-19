//! Output formatters consumed by the pipeline adapters.
//!
//! - [`render_json`] — pretty-printed [`ValidationRunReport`]; the canonical
//!   on-disk format.
//! - [`render_text`] — human-readable summary for terminal viewing.
//! - [`render_tap`] — TAP version 14 stream (one ok/not-ok line per file).
//! - [`render_junit_xml`] — JUnit 4.x XML for GitLab CI's
//!   `artifacts:reports:junit` and the Jenkins JUnit Plugin.
//! - [`render_gha_commands`] — `::error file=…::msg` workflow commands. Logs
//!   surface inline in GitHub Actions PRs.
//! - [`render_sarif`] — SARIF 2.1.0 document for GitHub Code Scanning.

use serde_json::json;

use crate::error::ValidationRunError;
use crate::report::{FileStatus, ValidationRunReport};

pub fn render_json(report: &ValidationRunReport) -> Result<String, ValidationRunError> {
    serde_json::to_string_pretty(report).map_err(ValidationRunError::from)
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

/// Renders the run as a JUnit 4.x XML document, the dialect understood by
/// GitLab CI's `artifacts:reports:junit` and the Jenkins JUnit Plugin.
///
/// Shape: one `<testcase>` per file finding. Issues in error-status files
/// nest as `<failure>` elements; load failures nest as `<error>`; files
/// with only warnings stay green (no `<failure>`/`<error>`) and surface
/// their warning details under `<system-out>` so reviewers can drill in
/// from the test panel without the build being marked red. The exit-code
/// policy (`--fail-on`) still controls whether the CI job overall fails.
pub fn render_junit_xml(report: &ValidationRunReport) -> String {
    use std::fmt::Write;

    let elapsed_s = (report.summary.elapsed_ms as f64) / 1000.0;
    let suite_failures = report.summary.files_with_errors;
    let suite_errors = report.summary.files_load_failed;

    let mut out = String::new();
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(
        out,
        "<testsuites name=\"solarxy-validate\" tests=\"{}\" failures=\"{}\" errors=\"{}\" time=\"{:.3}\">",
        report.summary.files_total, suite_failures, suite_errors, elapsed_s,
    );
    let _ = writeln!(
        out,
        "  <testsuite name=\"solarxy-validate\" tests=\"{}\" failures=\"{}\" errors=\"{}\" skipped=\"0\" time=\"{:.3}\" timestamp=\"{}\">",
        report.summary.files_total,
        suite_failures,
        suite_errors,
        elapsed_s,
        xml_escape_attr(&report.started_at),
    );

    for f in &report.findings {
        let path = f.path.display().to_string();
        let name = xml_escape_attr(&path);
        let _ = writeln!(
            out,
            "    <testcase classname=\"solarxy.validate\" name=\"{name}\" time=\"0.000\">",
        );
        if let Some(err) = &f.load_error {
            let _ = writeln!(
                out,
                "      <error type=\"LoadFailed\" message=\"{}\">{}</error>",
                xml_escape_attr(err),
                xml_escape_text(err),
            );
        } else if f.error_count > 0 {
            for issue in &f.issues {
                if issue.severity != "error" {
                    continue;
                }
                let _ = writeln!(
                    out,
                    "      <failure type=\"{}\" message=\"{}\">{}</failure>",
                    xml_escape_attr(&issue.kind),
                    xml_escape_attr(&issue.message),
                    xml_escape_text(&format!(
                        "[{}] {} ({}): {}",
                        issue.severity, issue.kind, issue.scope, issue.message
                    )),
                );
            }
            // Surface any warnings on the same file in system-out so
            // reviewers see the full picture from a single drill-in.
            emit_warnings_system_out(&mut out, f);
        } else if f.warning_count > 0 {
            // Warning-only: keep the testcase green; surface details in
            // system-out so they're visible from the test panel.
            emit_warnings_system_out(&mut out, f);
        }
        let _ = writeln!(out, "    </testcase>");
    }

    let _ = writeln!(out, "  </testsuite>");
    let _ = writeln!(out, "</testsuites>");
    out
}

fn emit_warnings_system_out(out: &mut String, f: &super::report::FileFinding) {
    use std::fmt::Write;
    let warnings: Vec<_> = f
        .issues
        .iter()
        .filter(|i| i.severity == "warning")
        .collect();
    if warnings.is_empty() {
        return;
    }
    let _ = writeln!(out, "      <system-out>");
    for issue in warnings {
        let _ = writeln!(
            out,
            "{}",
            xml_escape_text(&format!(
                "[warning] {} ({}): {}",
                issue.kind, issue.scope, issue.message
            )),
        );
    }
    let _ = writeln!(out, "      </system-out>");
}

/// XML attribute-safe escaping. Quotes use `&quot;`; other entities
/// follow the XML 1.0 spec.
fn xml_escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

/// XML element-text escaping. Same as attribute escaping minus the quote
/// substitutions (quotes are legal in text nodes).
fn xml_escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

pub fn render_sarif(report: &ValidationRunReport) -> Result<String, ValidationRunError> {
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
    serde_json::to_string_pretty(&sarif).map_err(ValidationRunError::from)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::{FileFinding, FileStatus, RunSummary, ValidationRunReport};
    use solarxy_core::json::JsonIssue;

    fn sample_report_with_error_issue() -> ValidationRunReport {
        let issue = JsonIssue {
            severity: "error".into(),
            kind: "Bogus<Issue>".into(),
            scope: "Mesh \"0\"".into(),
            scope_index: Some(0),
            message: "value & quote \" and < > here".into(),
        };
        let finding = FileFinding {
            path: std::path::PathBuf::from("path/with \"quotes\".glb"),
            status: FileStatus::Error,
            category: None,
            triangles: None,
            error_count: 1,
            warning_count: 0,
            issues: vec![issue],
            load_error: None,
        };
        let summary = RunSummary::from_findings(std::slice::from_ref(&finding), 7);
        ValidationRunReport {
            schema_version: 1,
            run_id: "01ABC".into(),
            solarxy_version: "0.6.0-test",
            started_at: "2026-05-19T00:00:00Z".into(),
            config_path: None,
            config_hash: None,
            summary,
            findings: vec![finding],
        }
    }

    #[test]
    fn xml_attr_escape_handles_all_five_entities() {
        assert_eq!(
            xml_escape_attr("a & b < c > d \" e ' f"),
            "a &amp; b &lt; c &gt; d &quot; e &apos; f"
        );
    }

    #[test]
    fn xml_text_escape_handles_three_entities() {
        // Text nodes don't need to escape quotes; only &, <, >.
        assert_eq!(
            xml_escape_text("a & b < c > d \" e ' f"),
            "a &amp; b &lt; c &gt; d \" e ' f"
        );
    }

    #[test]
    fn junit_xml_escapes_metacharacters_in_attrs_and_text() {
        let report = sample_report_with_error_issue();
        let xml = render_junit_xml(&report);
        assert!(
            !xml.contains("Bogus<Issue>"),
            "raw `<` must not appear in kind: {xml}"
        );
        assert!(
            xml.contains("Bogus&lt;Issue&gt;"),
            "expected escaped kind attr: {xml}"
        );
        assert!(
            xml.contains("&quot;quotes&quot;"),
            "expected escaped quotes in path attr: {xml}"
        );
        assert!(xml.contains("&amp;"), "expected escaped ampersand: {xml}");
    }

    #[test]
    fn junit_xml_summary_counters_match_findings() {
        let report = sample_report_with_error_issue();
        let xml = render_junit_xml(&report);
        assert!(xml.contains("tests=\"1\""));
        assert!(xml.contains("failures=\"1\""));
        assert!(xml.contains("errors=\"0\""));
    }
}
