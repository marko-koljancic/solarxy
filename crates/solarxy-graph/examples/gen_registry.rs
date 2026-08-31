//! Emits the node registry, either as JSON or as the wiki's Node Reference page.
//!
//! The registry is the single source of truth for every node: its ports, its
//! params, their ranges, units and defaults, and the prose that describes them.
//! The palette, the parameter panel and the typed handles are already pure
//! interpreters of it. The documentation should be too -- a node reference
//! written by hand goes stale the first time somebody adds a param.
//!
//! ```text
//! cargo run -p solarxy-graph --example gen_registry -- json     > registry.json
//! cargo run -p solarxy-graph --example gen_registry -- markdown > ../solarxy.wiki/Node-Reference.md
//! ```
//!
//! `tests/registry_drift.rs` asserts the checked-in `schemas/registry.json`
//! matches this output, so a node added without regenerating fails CI.
//!
//! Unlike the `solarxy-scenefile` schema emitter this needs no feature gate and
//! no extra dependency: `RegistrySnapshot` derives `Serialize` and `serde_json`
//! is already an unconditional dependency of this crate.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use solarxy_graph::builtin_registry;
use solarxy_graph::document::ContextKind;
use solarxy_graph::engine::RegistrySnapshot;
use solarxy_graph::engine::snapshot::{ShowIfPredSnapshot, ShowIfSnapshot};
use solarxy_graph::registry::NodeRole;

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

/// Renders the whole reference. Returns the ids of undocumented nodes instead of
/// quietly emitting empty sections: `doc` is a `String` that defaults to empty,
/// so a node with no prose would otherwise ship a blank heading and nobody would
/// notice until a user did.
fn render_markdown(snap: &RegistrySnapshot) -> Result<String, Vec<String>> {
    let undocumented: Vec<String> = snap
        .nodes
        .iter()
        .filter(|n| n.doc.trim().is_empty())
        .map(|n| n.type_id.clone())
        .collect();
    if !undocumented.is_empty() {
        return Err(undocumented);
    }

    // Group by category, then sort by display name inside each.
    let mut by_category: BTreeMap<String, Vec<_>> = BTreeMap::new();
    for node in &snap.nodes {
        by_category
            .entry(node.category_label.clone())
            .or_default()
            .push(node);
    }
    for nodes in by_category.values_mut() {
        nodes.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    }

    let mut out = String::new();
    out.push_str("← [Home](Home)\n\n# Node Reference\n\n");
    out.push_str(
        "Every node type in Solarxy Web, with its ports, parameters and defaults.\n\n\
         Every node also carries a `name` and a `description` parameter. They are common to \
         all types, so the per-node tables below do not repeat them.\n\n\
         **This page is generated** from the node registry itself \
         (`cargo run -p solarxy-graph --example gen_registry -- markdown`), so it cannot drift \
         from the application. Do not edit it by hand: change the node's descriptor in \
         `crates/solarxy-graph/src/nodes/` and regenerate.\n\n",
    );

    let _ = writeln!(
        out,
        "{} node types across {} categories.\n",
        snap.nodes.len(),
        by_category.len()
    );

    // Contents.
    out.push_str("## Contents\n\n");
    for (category, nodes) in &by_category {
        let _ = writeln!(out, "**{category}**  ");
        let links: Vec<String> = nodes
            .iter()
            .map(|n| format!("[{}](#{})", n.display_name, n.type_id))
            .collect();
        let _ = writeln!(out, "{}\n", links.join(" · "));
    }

    for (category, nodes) in &by_category {
        let _ = writeln!(out, "## {category}\n");
        for node in nodes {
            render_node(&mut out, node);
        }
    }

    Ok(out)
}

fn render_node(out: &mut String, node: &solarxy_graph::engine::snapshot::NodeTypeSnapshot) {
    // An explicit anchor gives stable `#array`-style deep links (the wiki's own
    // convention, used by FAQ.md), independent of how GitHub slugifies headings.
    let _ = writeln!(
        out,
        "### {} <a id=\"{}\"></a>\n",
        node.display_name, node.type_id
    );

    let context_names: Vec<&str> = node
        .contexts
        .iter()
        .map(|k| match k {
            ContextKind::Obj => "scene",
            ContextKind::Geo => "inside a geo",
            ContextKind::Mat => "inside a material network",
            ContextKind::Tex => "inside a texture network",
        })
        .collect();
    // A container's silhouette phrase would restate the sentence below it.
    let silhouette = match role_words(node.role) {
        Some(words) if node.opens.is_none() => format!(" · {words} silhouette"),
        _ => String::new(),
    };
    let _ = writeln!(
        out,
        "`{}` · v{} · {} · placed {}{}\n",
        node.type_id,
        node.version,
        node.category_label,
        context_names.join(" or "),
        silhouette
    );
    if let Some(kind) = node.opens {
        let network = match kind {
            ContextKind::Obj => "scene",
            ContextKind::Geo => "geo",
            ContextKind::Mat => "material",
            ContextKind::Tex => "texture",
        };
        let _ = writeln!(
            out,
            "*A container: diving in opens its {network} network.*\n"
        );
    }
    if !node.search_aliases.is_empty() {
        let _ = writeln!(
            out,
            "Palette search also matches: {}.\n",
            node.search_aliases.join(", ")
        );
    }
    let _ = writeln!(out, "{}\n", node.doc.trim());

    if !node.inputs.is_empty() || !node.outputs.is_empty() {
        out.push_str("| Port | Direction | Type | Notes |\n|---|---|---|---|\n");
        for p in &node.inputs {
            let mut notes = Vec::new();
            if p.variadic {
                notes.push("accepts many".to_string());
            }
            if p.min > 0 {
                notes.push(format!("needs at least {}", p.min));
            }
            if p.required {
                notes.push("required".to_string());
            }
            // `is_default` names the port a wire dropped on the node body
            // lands on; on a single-input node that is the only candidate,
            // so the mark is rendered only where there is a choice.
            if p.is_default && node.inputs.len() > 1 {
                notes.push("the default input".to_string());
            }
            if !p.doc.trim().is_empty() {
                notes.push(p.doc.trim().to_string());
            }
            let _ = writeln!(
                out,
                "| `{}` | in | {:?} | {} |",
                p.key,
                p.data_type,
                notes.join("; ")
            );
        }
        for p in &node.outputs {
            let mut notes = Vec::new();
            if p.is_default && node.outputs.len() > 1 {
                notes.push("the default output".to_string());
            }
            if !p.doc.trim().is_empty() {
                notes.push(p.doc.trim().to_string());
            }
            let _ = writeln!(
                out,
                "| `{}` | out | {:?} | {} |",
                p.key,
                p.data_type,
                notes.join("; ")
            );
        }
        out.push('\n');
    }

    // The implicit `general` group (name / description) is on every node and
    // carries no information; the preamble states it once, and listing it
    // once per node type would be noise.
    let params: Vec<_> = node
        .params
        .iter()
        .filter(|p| p.group != "general")
        .collect();
    if !params.is_empty() {
        out.push_str("| Parameter | Type | Default | Range | Notes |\n|---|---|---|---|---|\n");
        for p in params {
            let ty = if p.enum_variants.is_empty() {
                p.param_type.clone()
            } else {
                let variants: Vec<&str> = p.enum_variants.iter().map(|(k, _)| k.as_str()).collect();
                format!("enum ({})", variants.join(" / "))
            };
            let default = compact_json(&p.default);
            let range = match (p.hard, p.soft) {
                (Some((lo, hi)), _) => format!("{lo} to {hi}"),
                (None, Some((lo, hi))) => format!("{lo} to {hi} (soft)"),
                _ => String::new(),
            };
            let mut notes = Vec::new();
            if let Some(subgroup) = &p.subgroup {
                notes.push(format!("under \"{subgroup}\""));
            }
            let unit = format!("{:?}", p.unit);
            if unit != "None" {
                notes.push(unit.to_lowercase());
            }
            if let Some(port) = &p.driven_by_port {
                notes.push(format!("overridden when `{port}` is connected"));
            }
            if let Some(condition) = show_if_prose(&p.show_if) {
                notes.push(condition);
            }
            if !p.doc.trim().is_empty() {
                notes.push(p.doc.trim().to_string());
            }
            let _ = writeln!(
                out,
                "| `{}` | {} | `{}` | {} | {} |",
                p.key,
                ty,
                default,
                range,
                notes.join("; ")
            );
        }
        out.push('\n');
    }

    let bypass = match &node.bypass {
        solarxy_graph::engine::snapshot::BypassSnapshot::PassThrough { input } => {
            format!("passes `{input}` straight through")
        }
        solarxy_graph::engine::snapshot::BypassSnapshot::Mute => "emits nothing".to_string(),
        solarxy_graph::engine::snapshot::BypassSnapshot::NotBypassable => {
            "cannot be bypassed".to_string()
        }
    };
    let _ = writeln!(out, "*Bypassed: {bypass}.*\n");
}

/// The silhouette family, in reader's words; `None` for the standard body,
/// which is the default and carries no information worth a table's width.
fn role_words(role: NodeRole) -> Option<&'static str> {
    match role {
        NodeRole::Standard => None,
        NodeRole::Container => Some("container"),
        NodeRole::Gather => Some("gather"),
        NodeRole::Branch => Some("branch"),
        NodeRole::Terminal => Some("terminal"),
        NodeRole::Analyzer => Some("analyzer"),
        NodeRole::ImageSource => Some("image source"),
        NodeRole::Light => Some("light"),
        NodeRole::Camera => Some("camera"),
        NodeRole::Text => Some("text"),
        NodeRole::Note => Some("note"),
    }
}

/// A param's visibility conditions as prose ("shown only while `mode` is
/// `inline`"), or `None` when it is always visible. The predicate is a data
/// structure the parameter panel evaluates; published verbatim it would
/// document the engine's encoding rather than what the reader sees.
fn show_if_prose(clauses: &[ShowIfSnapshot]) -> Option<String> {
    if clauses.is_empty() {
        return None;
    }
    let parts: Vec<String> = clauses
        .iter()
        .map(|clause| match &clause.pred {
            ShowIfPredSnapshot::Truthy => format!("`{}` is on", clause.param),
            ShowIfPredSnapshot::Eq { value } => {
                format!("`{}` is `{}`", clause.param, compact_json(value))
            }
            ShowIfPredSnapshot::Neq { value } => {
                format!("`{}` is not `{}`", clause.param, compact_json(value))
            }
            ShowIfPredSnapshot::In { values } => {
                let rendered: Vec<String> = values
                    .iter()
                    .map(|v| format!("`{}`", compact_json(v)))
                    .collect();
                format!("`{}` is one of {}", clause.param, rendered.join(", "))
            }
        })
        .collect();
    Some(format!("shown only while {}", parts.join(" and ")))
}

/// A parameter default, rendered for a table cell (no newlines, no quotes around
/// plain strings).
fn compact_json(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "?".to_string()),
    }
}
