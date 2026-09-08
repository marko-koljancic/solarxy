//! The dependency layering, asserted rather than described.
//!
//! `docs/architecture/05-boundaries-and-contracts.md` carries a table of which
//! crate may depend on which. Until this test existed, nothing read it: the
//! most load-bearing entry, that the shared rendering host must not see the
//! engine, lived in a comment on a manifest, and the whole table was enforced
//! by nobody having typed the wrong import yet.
//!
//! A SOURCE-level rule, like its neighbours in `tokens_drift.rs`: it reads the
//! document and the manifests, runs on every `cargo test` with no build, and
//! names the offending edge rather than a count that would drift. It
//! deliberately does not shell out to `cargo metadata`, which would contend
//! for the package lock cargo already holds while running this.
//!
//! **Every dependency kind counts.** A boundary crossed only in a test, only
//! in a build script, or only on one platform is still crossed, and the first
//! two are exactly where a boundary gets crossed by someone who believes it
//! does not count.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .map(PathBuf::from)
        .expect("workspace root")
}

fn contracts_doc() -> String {
    let path = workspace_root().join("docs/architecture/05-boundaries-and-contracts.md");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("the boundaries document is the source of the rule: {path:?}: {e}")
    })
}

/// The backticked tokens in a table cell, in order. The document writes crate
/// names in backticks throughout, which is what makes the table machine
/// readable without asking anyone to maintain a second copy of it.
fn ticked(cell: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = cell;
    while let Some(open) = rest.find('`') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('`') else { break };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

fn row_cells(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// The allow-matrix: crate to the set it may depend on. Parsed from the
/// document rather than restated here, so the document stays the single
/// statement of the rule and the two cannot disagree.
fn allow_matrix(doc: &str) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    let mut in_table = false;
    for line in doc.lines() {
        if line.starts_with("| Crate | May depend on |") {
            in_table = true;
            continue;
        }
        if in_table {
            if !line.trim_start().starts_with('|') {
                break;
            }
            if line.contains("---") {
                continue;
            }
            let cells = row_cells(line);
            if cells.len() < 2 {
                continue;
            }
            let Some(crate_name) = ticked(&cells[0]).into_iter().next() else {
                continue;
            };
            let permitted: BTreeSet<String> = ticked(&cells[1]).into_iter().collect();
            out.insert(crate_name, permitted);
        }
    }
    assert!(
        out.len() >= 10,
        "the allow-matrix parsed to {} rows, which means the table moved and this parser did not",
        out.len()
    );
    out
}

/// A dated exception. The date and the closing release are what separate an
/// exception from a rule nobody holds: without them the choice is between a
/// test that fails on every run and no test at all, and both end the same way.
#[derive(Debug)]
struct Exception {
    from: String,
    to: String,
}

/// The test-only edges: permitted, and listed so they are permitted
/// deliberately rather than by nobody looking. A dependency that exists only
/// for a test or an example cannot reach a user, but it can still say that a
/// crate's abstraction is in the wrong place, which is why it is written down.
fn test_only_edges(doc: &str) -> BTreeSet<(String, String)> {
    let mut out = BTreeSet::new();
    let mut in_table = false;
    for line in doc.lines() {
        if line.starts_with("| Edge | For |") {
            in_table = true;
            continue;
        }
        if in_table {
            if !line.trim_start().starts_with('|') {
                break;
            }
            if line.contains("---") {
                continue;
            }
            let cells = row_cells(line);
            if cells.len() < 2 {
                continue;
            }
            let edge = ticked(&cells[0]);
            if edge.len() == 2 {
                out.insert((edge[0].clone(), edge[1].clone()));
            }
        }
    }
    out
}

fn exceptions(doc: &str) -> Vec<Exception> {
    let mut out = Vec::new();
    let mut in_table = false;
    for line in doc.lines() {
        if line.starts_with("| Edge | Why it stands | Opened | Closes |") {
            in_table = true;
            continue;
        }
        if in_table {
            if !line.trim_start().starts_with('|') {
                break;
            }
            if line.contains("---") {
                continue;
            }
            let cells = row_cells(line);
            if cells.len() < 4 {
                continue;
            }
            let edge = ticked(&cells[0]);
            if edge.len() != 2 {
                continue;
            }
            // An entry missing a date or a closing release is not an
            // exception, and is refused rather than honoured.
            assert!(
                !cells[2].is_empty(),
                "exception {} to {} carries no opened date",
                edge[0],
                edge[1]
            );
            assert!(
                !cells[3].is_empty(),
                "exception {} to {} names no closing release",
                edge[0],
                edge[1]
            );
            out.push(Exception {
                from: edge[0].clone(),
                to: edge[1].clone(),
            });
        }
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Kind {
    Normal,
    Dev,
    Build,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Normal => "dependencies",
            Kind::Dev => "dev-dependencies",
            Kind::Build => "build-dependencies",
        }
    }
}

/// Reads one manifest and returns the workspace-internal crates it names, in
/// every dependency kind including target-conditional ones.
fn edges_of(manifest: &Path, members: &BTreeSet<String>) -> BTreeSet<(String, Kind)> {
    let text = std::fs::read_to_string(manifest).unwrap_or_default();
    let mut out = BTreeSet::new();
    let mut kind: Option<Kind> = None;
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            // `[target.'cfg(...)'.dependencies]` counts as the kind it names.
            // A platform-conditional edge is still an edge.
            kind = if line.ends_with("dev-dependencies]") {
                Some(Kind::Dev)
            } else if line.ends_with("build-dependencies]") {
                Some(Kind::Build)
            } else if line.ends_with("dependencies]") {
                Some(Kind::Normal)
            } else {
                None
            };
            continue;
        }
        let Some(kind) = kind else { continue };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // `name = ...`, `name.workspace = true`, `name = { path = ... }`.
        let Some(eq) = line.find('=') else { continue };
        let key = line[..eq].trim().trim_matches('"');
        let name = key.split('.').next().unwrap_or(key).trim();
        if members.contains(name) {
            out.insert((name.to_string(), kind));
        }
    }
    out
}

fn members() -> BTreeMap<String, PathBuf> {
    let root = workspace_root();
    let mut out = BTreeMap::new();
    out.insert("solarxy".to_string(), root.join("Cargo.toml"));
    let dir = root.join("crates");
    for entry in std::fs::read_dir(&dir).expect("crates directory").flatten() {
        let manifest = entry.path().join("Cargo.toml");
        if manifest.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            out.insert(name, manifest);
        }
    }
    out
}

/// The whole rule, in one assertion.
///
/// A failure here is one of three things and the message says which: an edge
/// nobody argued for, an exception that has lost its date, or the table having
/// moved out from under the parser. The third is a defect in this test and is
/// caught by the row-count assertion above rather than reported as a violation.
#[test]
fn every_workspace_edge_is_permitted_or_excepted() {
    let doc = contracts_doc();
    let matrix = allow_matrix(&doc);
    let excepted = exceptions(&doc);
    let test_only = test_only_edges(&doc);
    let members = members();
    let names: BTreeSet<String> = members.keys().cloned().collect();

    let mut violations: Vec<String> = Vec::new();

    for (name, manifest) in &members {
        // A crate the table does not mention is not silently permitted
        // anything: the table is the whole rule, so an unlisted crate is a
        // gap in the document and is reported as one.
        let Some(permitted) = matrix.get(name) else {
            violations.push(format!(
                "{name} is a workspace member and the allow-matrix does not list it, so nothing \
                 states what it may depend on"
            ));
            continue;
        };
        for (dep, kind) in edges_of(manifest, &names) {
            if dep == *name || permitted.contains(&dep) {
                continue;
            }
            if excepted.iter().any(|e| e.from == *name && e.to == dep) {
                continue;
            }
            // A development edge is a different class: it cannot reach a
            // shipped artifact. It is still listed, because an unlisted one is
            // an edge nobody examined.
            if kind == Kind::Dev && test_only.contains(&(name.clone(), dep.clone())) {
                continue;
            }
            let hint = if kind == Kind::Dev {
                "the test-only table, which is where a development edge belongs when it is \
                 deliberate"
            } else {
                "the dated-exceptions table, with a date and the release that closes it"
            };
            violations.push(format!(
                "{name} depends on {dep} in [{}], which the allow-matrix does not permit. If the \
                 edge is wanted it belongs in {hint}",
                kind.label()
            ));
        }
    }

    assert!(
        violations.is_empty(),
        "the dependency layering in docs/architecture/05-boundaries-and-contracts.md is not \
         held:\n  {}\n\nAdding an edge to the allow-matrix instead of to one of the two lists \
         changes the target architecture, which is a decision that wants an ADR rather than a \
         line in a table.",
        violations.join("\n  ")
    );
}

/// The three non-edges the architecture set calls load-bearing, named
/// individually so a reader of a failure knows which one broke.
///
/// They are already covered by the assertion above. They are here as well
/// because a general rule failing tells you the layering moved, and this tells
/// you which of the three things the layering exists to protect stopped being
/// protected.
#[test]
fn the_three_load_bearing_non_edges_hold() {
    let members = members();
    let names: BTreeSet<String> = members.keys().cloned().collect();
    let forbidden = [
        ("solarxy-host", "solarxy-graph"),
        ("solarxy-graph", "solarxy-renderer"),
        ("solarxy-renderer", "solarxy-graph"),
    ];
    for (from, to) in forbidden {
        let Some(manifest) = members.get(from) else {
            continue;
        };
        let edges = edges_of(manifest, &names);
        let found: Vec<_> = edges.iter().filter(|(d, _)| d == to).collect();
        assert!(
            found.is_empty(),
            "{from} depends on {to} in {found:?}. This is one of the three non-edges the \
             architecture set calls load-bearing: the engine compiles without a GPU, the import \
             worker runs GPU-free, and the cook is testable with no device, all because these \
             edges do not exist. Cargo is currently the only thing enforcing that, by their \
             absence."
        );
    }
}
