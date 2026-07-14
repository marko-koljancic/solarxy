//! Asserts the checked-in `schemas/registry.json` matches what the current
//! builtin registry produces, and that every node type documents itself.
//!
//! The registry is the source of truth for the palette, the parameter panel, the
//! typed handles AND the generated wiki reference. A node added or changed
//! without regenerating means the published documentation silently describes a
//! different application. That is caught here, as a failing test with the
//! regeneration command in the message, rather than by a reader.
//!
//! Mirrors `solarxy-scenefile/tests/schema_drift.rs`.

use std::path::PathBuf;

use solarxy_graph::builtin_registry;
use solarxy_graph::engine::RegistrySnapshot;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

fn snapshot() -> RegistrySnapshot {
    let registry = builtin_registry().expect("builtin registry");
    RegistrySnapshot::capture(&registry)
}

/// Compared as TEXT, not as parsed `Value`s.
///
/// Parsed comparison looks more robust and is in fact weaker: `serde_json::Number`
/// holds `PosInt`/`NegInt`/`Float` as separate variants and compares them as
/// unequal ACROSS variants, so a whole-number float can round-trip through text
/// into a different variant and fail an equality check that means nothing. Text
/// comparison sidesteps that and asserts the stronger property we actually want:
/// the checked-in file is exactly what the generator emits, byte for byte.
#[test]
fn registry_json_matches_disk() {
    let generated = serde_json::to_string_pretty(&snapshot()).expect("serialize");
    let path = workspace_root().join("schemas/registry.json");
    let on_disk = std::fs::read_to_string(&path).expect("schemas/registry.json must exist");

    assert_eq!(
        on_disk.trim(),
        generated.trim(),
        "schemas/registry.json is stale. The published node reference is generated from this, \
         so it now describes a different application. Regenerate BOTH:\n\n  \
         cargo run -p solarxy-graph --example gen_registry -- json > schemas/registry.json\n  \
         cargo run -p solarxy-graph --example gen_registry -- markdown > \
         ../solarxy.wiki/Node-Reference.md\n"
    );
}

/// Every node in the public reference must document itself. `doc` is a `String`
/// that defaults to empty, so without this a node ships a blank section in the
/// wiki and nobody notices until a user reads it.
#[test]
fn every_node_type_is_documented() {
    let snap = snapshot();
    let undocumented: Vec<&str> = snap
        .nodes
        .iter()
        .filter(|n| n.doc.trim().is_empty())
        .map(|n| n.type_id.as_str())
        .collect();

    assert!(
        undocumented.is_empty(),
        "these node types have no `doc` and would render an empty section in the public \
         node reference: {undocumented:?}"
    );
}

/// The reference deep-links every node as `#<type_id>`, so the ids must be
/// unique and anchor-safe. The registry invariants already enforce the character
/// set; this pins the property the DOCS depend on.
#[test]
fn type_ids_are_unique_and_anchor_safe() {
    let snap = snapshot();
    let mut seen = std::collections::BTreeSet::new();
    for node in &snap.nodes {
        assert!(
            seen.insert(node.type_id.clone()),
            "duplicate type_id: {}",
            node.type_id
        );
        assert!(
            node.type_id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
            "type_id {:?} is not safe as an HTML anchor",
            node.type_id
        );
    }
}
