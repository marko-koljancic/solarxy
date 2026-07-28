//! Asserts the editor's wrangle syntax highlighting knows every name the
//! Rust grammar accepts.
//!
//! The cost of drift here is silent and one-directional. A builtin added in
//! Rust and missed in `wrangleLang.ts` renders as a plain identifier, which
//! is exactly how the editor paints a name that does NOT exist -- so the
//! feature ships looking like a bug in the language. Nothing else catches
//! that: the program still parses and cooks correctly, so no test fails and
//! no error appears.
//!
//! A source-level rule, like the player-import guard in
//! `solarxy-core/tests/tokens_drift.rs`: it runs on every `cargo test`, needs
//! no frontend build, and names the missing word rather than a count.

use std::path::PathBuf;

use solarxy_graph::expr::ast::Var;
use solarxy_graph::expr::builtins::{BUILTIN_NAMES, LOCAL_TYPE_NAMES, QUERY_NAMES};

fn lang_source() -> String {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let path = manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("web/src/components/inputs/wrangleLang.ts");
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "read {}: {e}. The wrangle highlighter must exist beside the grammar it mirrors.",
            path.display()
        )
    })
}

/// The string literals inside the named exported array, e.g. `BUILTINS`.
fn ts_array(src: &str, name: &str) -> Vec<String> {
    let at = src
        .find(&format!("export const {name} = ["))
        .unwrap_or_else(|| panic!("wrangleLang.ts must export a `{name}` array"));
    let rest = &src[at..];
    let end = rest.find(']').expect("the array must be closed");
    rest[..end]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

/// Compares as SETS, not as ordered lists: the editor groups its names for
/// readability, and holding it to the Rust declaration order would be a
/// formatting rule wearing a correctness rule's clothes.
fn assert_same(kind: &str, rust: &[&str], ts: &[String], export: &str) {
    let rust_set: std::collections::BTreeSet<&str> = rust.iter().copied().collect();
    let ts_set: std::collections::BTreeSet<&str> = ts.iter().map(String::as_str).collect();

    let missing: Vec<&&str> = rust_set.difference(&ts_set).collect();
    assert!(
        missing.is_empty(),
        "{kind} the editor does not highlight: {missing:?}\n\
         Add them to `{export}` in web/src/components/inputs/wrangleLang.ts. \
         Until then they render like undefined names to anyone writing a wrangle."
    );

    let extra: Vec<&&str> = ts_set.difference(&rust_set).collect();
    assert!(
        extra.is_empty(),
        "{kind} the editor highlights that the grammar does not accept: {extra:?}\n\
         Remove them from `{export}`; highlighting a name the parser rejects is \
         worse than not highlighting it at all."
    );
}

#[test]
fn the_editor_highlights_every_builtin() {
    assert_same(
        "builtins",
        BUILTIN_NAMES,
        &ts_array(&lang_source(), "BUILTINS"),
        "BUILTINS",
    );
}

#[test]
fn the_editor_highlights_every_context_query() {
    assert_same(
        "queries",
        QUERY_NAMES,
        &ts_array(&lang_source(), "QUERIES"),
        "QUERIES",
    );
}

#[test]
fn the_editor_highlights_every_local_type() {
    assert_same(
        "type keywords",
        LOCAL_TYPE_NAMES,
        &ts_array(&lang_source(), "LOCAL_TYPES"),
        "LOCAL_TYPES",
    );
}

#[test]
fn the_editor_highlights_every_clock_variable() {
    let names: Vec<&str> = Var::ALL.iter().map(|v| v.name()).collect();
    assert_same(
        "variables",
        &names,
        &ts_array(&lang_source(), "VARS"),
        "VARS",
    );
}

/// The lists above are only a guard if they describe what the parser really
/// accepts. `QUERY_NAMES` and `LOCAL_TYPE_NAMES` are hand-maintained beside
/// their `match` arms, so this pins them to observable behaviour rather than
/// to a second copy of the same list.
#[test]
fn the_query_and_type_lists_describe_the_real_parser() {
    use solarxy_graph::expr::parse;

    for name in QUERY_NAMES {
        // A parse failure naming an UNKNOWN FUNCTION would mean the list has
        // a name the parser never had. Any other outcome (parses fine, or
        // fails on arity/arguments) means the name itself is recognized.
        let src = format!("{name}()");
        if let Err(e) = parse(&src) {
            let msg = e.to_string().to_lowercase();
            assert!(
                !msg.contains("unknown"),
                "QUERY_NAMES lists `{name}`, but the parser does not know it: {e}"
            );
        }
    }

    for ty in LOCAL_TYPE_NAMES {
        let src = format!("{ty} x = 1;");
        assert!(
            solarxy_graph::expr::stmt::parse_program(&src).is_ok(),
            "LOCAL_TYPE_NAMES lists `{ty}`, but `{src}` does not parse"
        );
    }
}
