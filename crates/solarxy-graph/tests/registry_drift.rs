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
        "schemas/registry.json is stale, so the palette and parameter panel it drives now \
         describe a different application. Regenerate BOTH committed copies:\n\n  \
         cargo run -p solarxy-graph --example gen_registry -- json > schemas/registry.json\n  \
         cargo run -p solarxy-graph --example gen_registry -- markdown > \
         schemas/node-reference.md\n\n\
         `node_reference_matches_disk` guards the second one, so a forgotten markdown \
         regeneration fails there rather than relying on anyone's memory.\n"
    );
}

/// The artefact users actually read is the wiki's `Node-Reference.md`, which
/// lives in a sibling repository that CI cannot see. The committed copy at
/// `schemas/node-reference.md` exists to move this assertion into the
/// repository where the generator and the tests are: it is the guard, not a
/// second reference, so do not delete it as redundant. Publication stays
/// manual and unchanged: after this check passes, a maintainer copies the
/// file to the wiki's `Node-Reference.md` on `develop` and merges to
/// `master`. When the documentation site lands, the committed copy becomes
/// the site's input and the wiki page a stub; this shape makes that a
/// deletion rather than a rewrite.
///
/// Compared as text for the same reason as the JSON guard above: the property
/// wanted is that the checked-in file is exactly what the generator emits.
#[test]
fn node_reference_matches_disk() {
    let generated = solarxy_graph::reference::render_markdown(&snapshot())
        .expect("every node documents itself; every_node_type_is_documented names offenders");
    let path = workspace_root().join("schemas/node-reference.md");
    let on_disk = std::fs::read_to_string(&path).expect("schemas/node-reference.md must exist");

    assert_eq!(
        on_disk.trim(),
        generated.trim(),
        "schemas/node-reference.md is stale, so the next wiki publication would describe a \
         different application. Regenerate:\n\n  \
         cargo run -p solarxy-graph --example gen_registry -- markdown > \
         schemas/node-reference.md\n\n\
         Then publish by copying that file to the wiki's Node-Reference.md on develop and \
         merging to master.\n"
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

/// The README's headline claims must match the code.
///
/// This is the same failure the `nodes/mod.rs` comment had, but public: the
/// README advertised "33 node types" and an "11 member" workspace long after
/// the registry reached 58 and `solarxy-imaging` landed as the 12th member.
/// Prose that recites a number the code owns goes stale silently, so the
/// number is checked rather than trusted.
///
/// Checks EVERY occurrence, not merely that a correct one exists somewhere:
/// the README states the node count in three places, so a `contains` check
/// passes happily while one of them lies.
#[test]
fn the_readme_states_the_real_counts() {
    let root = workspace_root();
    let readme = std::fs::read_to_string(root.join("README.md")).expect("README.md");
    let nodes = snapshot().nodes.len();

    let claims = count_claims(&readme, " node types");
    assert!(
        !claims.is_empty(),
        "the README no longer states the node count; it is a headline fact"
    );
    for claim in &claims {
        assert_eq!(
            *claim, nodes,
            "the README claims {claim} node types; the registry has {nodes}"
        );
    }

    // `.` (the root bin) plus one entry per crate.
    let manifest = std::fs::read_to_string(root.join("Cargo.toml")).expect("Cargo.toml");
    let members = manifest
        .lines()
        .skip_while(|l| !l.starts_with("members"))
        .take_while(|l| !l.starts_with(']'))
        .filter(|l| l.trim().starts_with('"'))
        .count();
    for claim in count_claims(&readme, " members") {
        assert_eq!(
            claim, members,
            "the README claims {claim} workspace members; Cargo.toml lists {members}"
        );
    }

    // Every crate must appear in the README's architecture table. The one
    // that was missing (`solarxy-imaging`) is exactly the one whose absence
    // made the member count wrong.
    for line in manifest
        .lines()
        .skip_while(|l| !l.starts_with("members"))
        .take_while(|l| !l.starts_with(']'))
    {
        let Some(name) = line
            .trim()
            .trim_matches(|c| c == '"' || c == ',')
            .strip_prefix("crates/")
        else {
            continue;
        };
        assert!(
            readme.contains(&format!("[`{name}`]")),
            "the README's crate table omits `{name}`"
        );
    }
}

/// Every integer immediately preceding `suffix` in `text`.
fn count_claims(text: &str, suffix: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, _) in text.match_indices(suffix) {
        let digits: String = text[..i]
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(n) = digits.chars().rev().collect::<String>().parse() {
            out.push(n);
        }
    }
    out
}

/// Every param must document itself.
///
/// `ParamSpec::doc` is a `String` that defaults to empty, so an undocumented
/// param is not an error anywhere: it renders a blank hover popover in the
/// parameter panel and a blank cell in the published node reference. Nobody
/// notices until a user hovers it and learns nothing.
#[test]
fn every_param_is_documented() {
    let snap = snapshot();
    let mut undocumented: Vec<String> = Vec::new();
    for node in &snap.nodes {
        for param in &node.params {
            if param.doc.trim().is_empty() {
                undocumented.push(format!("{}.{}", node.type_id, param.key));
            }
        }
    }
    assert!(
        undocumented.is_empty(),
        "{} param(s) have no `doc`, so their hover help and their row in the public node \
         reference are blank. Add `.doc(...)` to the ParamSpec:\n  {}",
        undocumented.len(),
        undocumented.join("\n  ")
    );
}

/// Every port must document itself.
///
/// A port's type and label carry a lot, but not what it EXPECTS: which of
/// several legal inputs it wants, what it does with an empty one, whether
/// order matters on a variadic. That belongs in `PortSpec::doc`.
#[test]
fn every_port_is_documented() {
    let snap = snapshot();
    let mut undocumented: Vec<String> = Vec::new();
    for node in &snap.nodes {
        for (dir, port) in node
            .inputs
            .iter()
            .map(|p| ("in", p))
            .chain(node.outputs.iter().map(|p| ("out", p)))
        {
            if port.doc.trim().is_empty() {
                undocumented.push(format!("{}.{dir}:{}", node.type_id, port.key));
            }
        }
    }
    assert!(
        undocumented.is_empty(),
        "{} port(s) have no `doc`:\n  {}",
        undocumented.len(),
        undocumented.join("\n  ")
    );
}

/// A node's `doc` must be a real explanation, not a restated title.
///
/// The registry-drift test has always required a non-empty `doc`, which 58
/// one-line stubs satisfied while telling a reader nothing: `import_gltf`
/// read "Load a glTF/GLB file." The floor is prose that says what the node
/// does and when to reach for it.
#[test]
fn node_docs_are_more_than_a_restated_title() {
    /// Two sentences of real explanation do not fit in less than this.
    const MIN_CHARS: usize = 120;

    let snap = snapshot();
    let thin: Vec<String> = snap
        .nodes
        .iter()
        .filter(|n| n.doc.trim().len() < MIN_CHARS)
        .map(|n| format!("{} ({} chars)", n.type_id, n.doc.trim().len()))
        .collect();

    assert!(
        thin.is_empty(),
        "{} node doc(s) are under {MIN_CHARS} chars, which is a restated title rather than \
         documentation. They are the public node reference AND the in-app info modal:\n  {}",
        thin.len(),
        thin.join("\n  ")
    );
}

/// `naming::node_name` and `web/src/flow/nodeLabel.ts` must agree.
///
/// The two implement one rule in two languages, and both files say so in
/// prose while nothing held them to it. The consequence of drift is
/// specific and nasty rather than cosmetic: expressions address nodes BY
/// NAME, so if Rust resolves `ch("box1/size")` against one name while the
/// canvas shows another, a user is typing a path against a label that is
/// not what the engine sees.
///
/// The Rust half is exercised directly. The frontend half is checked at the
/// source level, because vitest cannot be reached from here and the drift
/// direction that actually happens is someone tidying `nodeLabel.ts` (a
/// dropped `.trim()`, a reordered fallback) without knowing Rust mirrors it.
#[test]
fn node_name_matches_the_frontend_label() {
    use solarxy_graph::document::{ContextKind, Graph, NodeData, NodeId};
    use solarxy_graph::naming::node_name;
    use solarxy_graph::params::{ParamSource, ParamValue};

    let registry = builtin_registry().expect("builtin registry");

    // The Rust half, against the shared rule's three branches.
    let named = |name: Option<&str>| {
        let mut g = Graph::new(ContextKind::Geo);
        let mut node = NodeData::new(NodeId(1), "box", 1);
        if let Some(n) = name {
            node.params.insert(
                "name".to_string(),
                ParamSource::Literal(ParamValue::Text(n.to_string())),
            );
        }
        g.add_node(node);
        let node = g.nodes().next().expect("the node just added");
        node_name(node, &registry)
    };
    assert_eq!(named(Some("body")), "body", "a set name wins");
    assert_eq!(named(Some("  body  ")), "body", "a name is trimmed");
    assert_eq!(named(Some("   ")), "Box", "a blank name falls back");
    assert_eq!(named(None), "Box", "an unset name falls back");

    // The frontend half. Each needle is one load-bearing piece of the same
    // rule; the message names what breaks if it is gone.
    let path = workspace_root().join("web/src/flow/nodeLabel.ts");
    let src = std::fs::read_to_string(&path).expect("web/src/flow/nodeLabel.ts must exist");
    for (needle, why) in [
        ("params[\"name\"]", "the label must read the `name` param"),
        (
            "\"literal\"",
            "an expression-valued name must NOT be resolved here",
        ),
        ("\"text\"", "only a Text-typed name counts"),
        (
            ".trim()",
            "a whitespace-only name must fall back, not render as blank",
        ),
        (
            "displayName",
            "the first fallback is the descriptor's display name",
        ),
        ("typeId", "the last fallback is the type id"),
    ] {
        assert!(
            src.contains(needle),
            "web/src/flow/nodeLabel.ts no longer contains {needle:?}, so it may have diverged \
             from `naming::node_name` in crates/solarxy-graph/src/naming.rs: {why}. \
             Expressions resolve node paths by name, so a divergence means the engine and the \
             canvas disagree about what a node is called."
        );
    }
}
