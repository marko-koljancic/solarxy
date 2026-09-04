//! Emits the node registry, either as JSON or as the node reference page.
//!
//! The registry is the single source of truth for every node: its ports, its
//! params, their ranges, units and defaults, and the prose that describes them.
//! The palette, the parameter panel and the typed handles are already pure
//! interpreters of it. The documentation should be too -- a node reference
//! written by hand goes stale the first time somebody adds a param.
//!
//! ```text
//! cargo run -p solarxy-graph --example gen_registry -- json     > schemas/registry.json
//! cargo run -p solarxy-graph --example gen_registry -- markdown > schemas/node-reference.md
//! ```
//!
//! `tests/registry_drift.rs` asserts both checked-in copies match this output,
//! so a node added without regenerating fails CI. The wiki's published
//! `Node-Reference.md` is a copy of `schemas/node-reference.md`, made after
//! the check passes; the rendering itself lives in
//! `solarxy_graph::reference` so the drift test and this example share one
//! generator.
//!
//! Unlike the `solarxy-scenefile` schema emitter this needs no feature gate and
//! no extra dependency: `RegistrySnapshot` derives `Serialize` and `serde_json`
//! is already an unconditional dependency of this crate.

use solarxy_graph::builtin_registry;
use solarxy_graph::engine::RegistrySnapshot;
use solarxy_graph::reference::render_markdown;

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "json".to_string());
    let registry = builtin_registry().expect("the builtin registry must satisfy its invariants");
    let snap = RegistrySnapshot::capture(&registry);

    match mode.as_str() {
        "json" => {
            println!(
                "{}",
                serde_json::to_string_pretty(&snap).expect("serialize the registry snapshot")
            );
        }
        "markdown" => match render_markdown(&snap) {
            Ok(md) => print!("{md}"),
            Err(undocumented) => {
                eprintln!(
                    "error: {} node type(s) have an empty `doc` and would render a blank section:",
                    undocumented.len()
                );
                for id in &undocumented {
                    eprintln!("  - {id}");
                }
                eprintln!(
                    "\nEvery node in the public reference must document itself. Add a `doc` to \
                     its descriptor."
                );
                std::process::exit(1);
            }
        },
        other => {
            eprintln!("usage: gen_registry [json|markdown]   (got {other:?})");
            std::process::exit(2);
        }
    }
}
