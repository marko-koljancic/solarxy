//! Asserts that the checked-in JSON Schema matches what `schemars` would
//! generate from the current `ProjectConfig` type. Drift is caught here so
//! reviewers see a failing test instead of stale schemas leaking out.

#![cfg(feature = "schemars-gen")]

use std::path::PathBuf;

use solarxy_core::project_config::ProjectConfig;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/solarxy-core → workspace root
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

#[test]
fn solarxy_config_schema_matches_disk() {
    let schema = schemars::schema_for!(ProjectConfig);
    let generated =
        serde_json::to_string_pretty(&schema).expect("serialize schema") + "\n";
    let path = workspace_root().join("schemas/solarxy-config.v1.json");
    let on_disk =
        std::fs::read_to_string(&path).expect("schemas/solarxy-config.v1.json must exist");
    assert_eq!(
        on_disk.trim_end(),
        generated.trim_end(),
        "schemas/solarxy-config.v1.json is out of date. Regenerate with:\n\
         \n  cargo run -p solarxy-core --features schemars-gen --example gen_schemas > {}\n",
        path.display(),
    );
}
