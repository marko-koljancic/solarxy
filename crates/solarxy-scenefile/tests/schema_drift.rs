//! Asserts the checked-in `schemas/slxy-scene.v1.json` matches what
//! `schemars` generates from the current [`solarxy_scenefile::SceneJson`].
//! Drift is caught here so reviewers see a failing test instead of a stale
//! schema. Comparison is parsed-JSON equality (not byte-for-byte), so a
//! formatter re-flowing whitespace never false-fires.

#![cfg(feature = "schemars-gen")]

use std::path::PathBuf;

use solarxy_scenefile::schema_json;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

#[test]
fn slxy_scene_schema_matches_disk() {
    let generated_str = schema_json().expect("generate JSON schema");
    let path = workspace_root().join("schemas/slxy-scene.v1.json");
    let on_disk = std::fs::read_to_string(&path).expect("schemas/slxy-scene.v1.json must exist");

    let generated: serde_json::Value =
        serde_json::from_str(&generated_str).expect("generated schema parses");
    let on_disk_val: serde_json::Value =
        serde_json::from_str(&on_disk).expect("on-disk schema parses");

    assert_eq!(
        on_disk_val,
        generated,
        "schemas/slxy-scene.v1.json content drift. Regenerate with:\n\
         \n  cargo run -p solarxy-scenefile --features schemars-gen --example gen_schemas > {}\n",
        path.display(),
    );
}
