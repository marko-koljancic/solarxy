//! Asserts that the checked-in JSON Schema matches what `schemars` would
//! generate from the current `ProjectConfig` type. Drift is caught here so
//! reviewers see a failing test instead of stale schemas leaking out.
//!
//! Comparison is **parsed-JSON equality** (`serde_json::Value` round-trip),
//! not byte-for-byte string match. This keeps the test from false-firing
//! when a contributor's editor-on-save formatter re-flows whitespace or
//! reorders inline arrays in the checked-in `.json` file. Schema *content*
//! is what matters; presentation is the formatter's problem.

#![cfg(feature = "schemars-gen")]

use std::path::PathBuf;

use solarxy_core::project_config::schema_json;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));

    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

#[test]
fn solarxy_config_schema_matches_disk() {
    let generated_str = schema_json().expect("generate JSON schema");
    let path = workspace_root().join("schemas/solarxy-config.v1.json");
    let on_disk =
        std::fs::read_to_string(&path).expect("schemas/solarxy-config.v1.json must exist");

    let generated: serde_json::Value =
        serde_json::from_str(&generated_str).expect("generated schema parses as JSON");
    let on_disk_val: serde_json::Value =
        serde_json::from_str(&on_disk).expect("on-disk schema parses as JSON");

    assert_eq!(
        on_disk_val,
        generated,
        "schemas/solarxy-config.v1.json content drift. Regenerate with:\n\
         \n  cargo run -p solarxy-core --features schemars-gen --example gen_schemas > {}\n",
        path.display(),
    );
}
