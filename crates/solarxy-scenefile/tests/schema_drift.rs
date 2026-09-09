//! Asserts the checked-in schema for the CURRENT format version matches what
//! `schemars` generates from [`solarxy_scenefile::SceneJson`]. Drift is caught
//! here so reviewers see a failing test instead of a stale schema. Comparison
//! is parsed-JSON equality (not byte-for-byte), so a formatter re-flowing
//! whitespace never false-fires.
//!
//! The path is derived from the version constant rather than written out, so
//! a format bump repoints this test by construction. It used to be a literal,
//! and a bump that forgot to change it would have gone on validating the old
//! schema while the new one was checked by nothing.
//!
//! Older schemas are deliberately not checked. They describe a shape this
//! build no longer writes, they keep their published address forever, and the
//! only correct thing to do to one is leave it alone.

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
    let name = format!(
        "schemas/slxy-scene.v{}.json",
        solarxy_scenefile::SCHEMA_VERSION_CURRENT
    );
    let path = workspace_root().join(&name);
    let on_disk =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{name} must exist: {e}"));

    let generated: serde_json::Value =
        serde_json::from_str(&generated_str).expect("generated schema parses");
    let on_disk_val: serde_json::Value =
        serde_json::from_str(&on_disk).expect("on-disk schema parses");

    assert_eq!(
        on_disk_val,
        generated,
        "{name} content drift. Regenerate with:\n\
         \n  cargo run -p solarxy-scenefile --features schemars-gen --example gen_schemas > {}\n",
        path.display(),
    );
}
