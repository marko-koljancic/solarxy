//! Shape-level smoke for [`solarxy_cli::validate::run_validation`]. Uses the
//! existing `triangle.glb` fixture from `solarxy-formats`. We assert
//! structural invariants rather than byte-identical goldens: changes to
//! whitespace, run_id, started_at, etc. don't cause spurious failures.

use std::path::PathBuf;

use serde_json::Value;
use solarxy_cli::validate::{
    ConfigSource, Output, adapter::AdapterFormat, adapter::AdapterName, adapter::FailOn,
    resolve_adapter, run_validation,
};

fn fixture_path() -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .expect("crates/")
        .join("solarxy-formats/tests/fixtures/triangle.glb")
        .to_string_lossy()
        .to_string()
}

fn capture_to_file(adapter: AdapterName, format: AdapterFormat) -> (String, i32) {
    let dir = std::env::temp_dir().join(format!(
        "solarxy-validate-smoke-{}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default(),
        format,
    ));
    std::fs::create_dir_all(&dir).expect("tempdir");
    let out_path = dir.join("out");
    let paths = vec![fixture_path()];
    let source = ConfigSource::defaults();
    let adapter_box = resolve_adapter(adapter);
    let code = run_validation(
        &paths,
        source,
        adapter_box.as_ref(),
        format,
        FailOn::Error,
        &Output::File(out_path.clone()),
    )
    .expect("run_validation");
    let body = std::fs::read_to_string(&out_path).expect("read output");
    let _ = std::fs::remove_dir_all(&dir);
    (body, code)
}

#[test]
fn generic_json_emits_schema_versioned_report() {
    let (body, code) = capture_to_file(AdapterName::Generic, AdapterFormat::Json);
    let v: Value = serde_json::from_str(&body).expect("parses as JSON");
    assert_eq!(v["schema_version"], 1);
    assert!(v["run_id"].as_str().unwrap_or("").len() >= 16);
    assert_eq!(v["findings"].as_array().expect("findings array").len(), 1);
    let finding = &v["findings"][0];
    assert!(
        finding["path"]
            .as_str()
            .unwrap_or("")
            .ends_with("triangle.glb")
    );

    let issues = finding["issues"].as_array().expect("issues array");
    assert!(
        issues.iter().any(|i| i["kind"] == "NonManifoldEdge"),
        "expected NonManifoldEdge issue in {issues:#?}"
    );

    assert_eq!(code, 0);
}

#[test]
fn github_actions_emits_workflow_commands() {
    let (body, _) = capture_to_file(AdapterName::GithubActions, AdapterFormat::GhaCommands);
    let mut saw_warning_line = false;
    for line in body.lines() {
        if line.is_empty() {
            continue;
        }
        assert!(
            line.starts_with("::warning ") || line.starts_with("::error "),
            "line should be a GHA workflow command: {line:?}"
        );
        assert!(line.contains("file=") && line.contains("title="));
        if line.starts_with("::warning ") {
            saw_warning_line = true;
        }
    }
    assert!(saw_warning_line, "expected at least one ::warning line");
}

#[test]
fn sarif_emits_valid_2_1_0_envelope() {
    let (body, _) = capture_to_file(AdapterName::GithubActions, AdapterFormat::Sarif);
    let v: Value = serde_json::from_str(&body).expect("parses as SARIF JSON");
    assert_eq!(v["version"], "2.1.0");
    let runs = v["runs"].as_array().expect("runs array");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0]["tool"]["driver"]["name"], "solarxy");
    let results = runs[0]["results"].as_array().expect("results array");
    assert!(!results.is_empty());
    for r in results {
        assert!(r["ruleId"].is_string());
        assert!(matches!(
            r["level"].as_str(),
            Some("error" | "warning" | "note")
        ));
    }
}

#[test]
fn generic_rejects_gha_commands_format() {
    let dir = std::env::temp_dir().join("solarxy-validate-mismatch");
    std::fs::create_dir_all(&dir).expect("tempdir");
    let out_path = dir.join("out");
    let paths = vec![fixture_path()];
    let source = ConfigSource::defaults();
    let adapter = resolve_adapter(AdapterName::Generic);
    let err = run_validation(
        &paths,
        source,
        adapter.as_ref(),
        AdapterFormat::GhaCommands,
        FailOn::Error,
        &Output::File(out_path),
    )
    .expect_err("must error");
    assert!(err.to_string().contains("generic adapter"));
    let _ = std::fs::remove_dir_all(&dir);
}
