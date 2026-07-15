//! Node-version migration (node catalog part I, section 1).
//!
//! On document load, each stored node carries a `type_version`. This module
//! reconciles it with the registry's current descriptor version:
//!
//! - **equal**: type the params and load.
//! - **older**: run the descriptor's migration hook stepwise (one version
//!   at a time) over the raw JSON param map, then apply the registry-default
//!   migration (drop params the current descriptor no longer declares with a
//!   warning; fill missing params from defaults), then type and load.
//! - **newer, or unknown type id**: load as a non-cooking placeholder. The
//!   params and edges are preserved verbatim, the node badges the reason,
//!   and it refuses to cook, so the document is never destroyed.
//!
//! Migration hooks run on the raw `serde_json::Map` *before* schema typing,
//! so a hook can rename or re-shape a param key while the value is still a
//! plain JSON value.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};

use crate::document::{NodeData, NodeId};
use crate::params::ParamSource;
use crate::registry::Registry;
use crate::registry::resolve::param_source_from_json;

/// The outcome of loading one stored node.
#[derive(Debug, Clone)]
pub struct LoadedNode {
    pub node: NodeData,
    /// Non-fatal load warnings (dropped unknown params, defaulted invalid
    /// values, a migration note). Surfaced to the console, not fatal.
    pub warnings: Vec<String>,
}

/// Loads one stored node, migrating or placeholder-ing as needed. `raw`
/// is the node's stored params as a plain JSON map (schema-v1 shape).
#[must_use]
pub fn load_node(
    registry: &Registry,
    id: NodeId,
    type_id: &str,
    stored_version: u32,
    mut raw: Map<String, Json>,
    position: [f32; 2],
    bypassed: bool,
) -> LoadedNode {
    let mut warnings = Vec::new();

    // Unknown type id: placeholder, params preserved verbatim.
    let Some(desc) = registry.get(type_id) else {
        return placeholder(
            id,
            type_id,
            stored_version,
            &raw,
            position,
            bypassed,
            format!("unknown node type '{type_id}'"),
        );
    };

    // Made by a newer build: placeholder, params preserved verbatim.
    if stored_version > desc.version {
        return placeholder(
            id,
            type_id,
            stored_version,
            &raw,
            position,
            bypassed,
            format!(
                "made by a newer Solarxy (node version {stored_version} > {})",
                desc.version
            ),
        );
    }

    // Older: run the migration hook stepwise, then the registry default.
    // A clean migration is by-design behavior and produces no warning
    // (silent-strip drops must not toast); failures and the drop/default
    // paths below still do.
    if stored_version < desc.version
        && let Some(migrate) = desc.migrate
    {
        for from in stored_version..desc.version {
            if let Err(e) = migrate(from, &mut raw) {
                warnings.push(format!(
                    "migration of '{type_id}' from v{from} failed: {}; loading defaults",
                    e.reason
                ));
            }
        }
    }

    // Registry-default migration: drop params the descriptor no longer
    // declares (with a warning). Missing params fill from defaults during
    // typing below.
    let declared: std::collections::BTreeSet<&str> =
        desc.params.iter().map(|p| p.key.as_str()).collect();
    raw.retain(|key, _| {
        let keep = declared.contains(key.as_str());
        if !keep {
            warnings.push(format!("'{type_id}': dropped unknown param '{key}'"));
        }
        keep
    });

    // Type each declared param from the raw JSON (or its default), warning
    // on any value that fails to type.
    let mut params: BTreeMap<String, ParamSource> = BTreeMap::new();
    for spec in &desc.params {
        if let Some(json) = raw.get(&spec.key) {
            match param_source_from_json(json, &spec.ty) {
                Ok(source) => {
                    params.insert(spec.key.clone(), source);
                }
                Err(reason) => {
                    warnings.push(format!(
                        "'{type_id}': param '{}' did not type ({reason}); using default",
                        spec.key
                    ));
                    params.insert(spec.key.clone(), ParamSource::Literal(spec.default.clone()));
                }
            }
        }
        // A param absent from `raw` is simply left unset; the resolver fills
        // it from the descriptor default at cook time.
    }

    let node = NodeData {
        id,
        type_id: type_id.to_string(),
        type_version: desc.version,
        params,
        position,
        bypassed,
        port_order: BTreeMap::new(),
        placeholder: None,
    };
    LoadedNode { node, warnings }
}

/// Builds a placeholder node preserving the stored params as opaque
/// expression-free literals is impossible (they are untyped JSON), so the
/// params are dropped from the typed map but the node keeps its identity,
/// version, and error reason. Edges are preserved by the loader separately.
fn placeholder(
    id: NodeId,
    type_id: &str,
    stored_version: u32,
    _raw: &Map<String, Json>,
    position: [f32; 2],
    bypassed: bool,
    reason: String,
) -> LoadedNode {
    let node = NodeData {
        id,
        type_id: type_id.to_string(),
        type_version: stored_version,
        params: BTreeMap::new(),
        position,
        bypassed,
        port_order: BTreeMap::new(),
        placeholder: Some(reason.clone()),
    };
    LoadedNode {
        node,
        warnings: vec![format!("'{type_id}' loaded as a placeholder: {reason}")],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::{CookCtx, CookError, CookOutcome, Inputs, Outputs};
    use crate::params::ParamValue;
    use crate::registry::param_spec::{ParamSpec, ParamType};
    use crate::registry::resolve::ResolvedParams;
    use crate::registry::{
        BypassBehavior, Category, ContextMask, MigrateError, NodeRole, NodeTypeDescriptor,
        PortSpec, Registry,
    };
    use crate::registry::coerce::DataType;

    #[allow(clippy::unnecessary_wraps)]
    fn stub_cook(
        _p: &ResolvedParams,
        _in: &Inputs,
        _cx: &mut CookCtx,
    ) -> Result<CookOutcome, CookError> {
        Ok(CookOutcome::Done(Outputs::empty()))
    }

    /// A v2 test node: v1 stored `size` (Float); v2 renamed it to
    /// `dimension`. The migration hook renames the raw key. Signature
    /// matches `MigrateFn`.
    #[allow(clippy::unnecessary_wraps)]
    fn migrate_v1_to_v2(from: u32, params: &mut Map<String, Json>) -> Result<(), MigrateError> {
        if from == 1
            && let Some(v) = params.remove("size")
        {
            params.insert("dimension".to_string(), v);
        }
        Ok(())
    }

    fn v2_registry() -> Registry {
        let desc = NodeTypeDescriptor {
            type_id: "widget",
            version: 2,
            display_name: "Widget",
            category: Category::Primitives,
            contexts: ContextMask::SUBFLOW,
            inputs: vec![],
            outputs: vec![
                PortSpec::single("geometry", "Geometry", DataType::Geometry, false).default_port(),
            ],
            params: vec![
                ParamSpec::new(
                    "dimension",
                    "Dimension",
                    "geometry",
                    ParamType::Float,
                    ParamValue::Float(1.0),
                )
                .hard(0.0, 100.0),
            ],
            bypass: BypassBehavior::Mute,
            doc: "",
            search_aliases: &[],
            glyph: "widget",
            role: NodeRole::Standard,
            cook: stub_cook,
            migrate: Some(migrate_v1_to_v2),
        };
        Registry::with_descriptors(vec![desc]).unwrap()
    }

    fn raw(pairs: &[(&str, Json)]) -> Map<String, Json> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn v1_node_migrates_under_v2_descriptor() {
        let reg = v2_registry();
        // A v1 document stored `size = 3.5`.
        let loaded = load_node(
            &reg,
            NodeId(1),
            "widget",
            1,
            raw(&[("size", serde_json::json!(3.5))]),
            [0.0; 2],
            false,
        );
        assert!(loaded.node.placeholder.is_none());
        assert_eq!(loaded.node.type_version, 2);
        // The hook renamed size -> dimension; it typed as a Float literal.
        assert_eq!(
            loaded.node.params.get("dimension"),
            Some(&ParamSource::Literal(ParamValue::Float(3.5)))
        );
        assert!(!loaded.node.params.contains_key("size"));
        // A clean migration is silent (silent-strip drops must not toast).
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn newer_version_loads_as_placeholder() {
        let reg = v2_registry();
        let loaded = load_node(
            &reg,
            NodeId(1),
            "widget",
            5, // newer than the current v2
            raw(&[("dimension", serde_json::json!(2.0))]),
            [0.0; 2],
            false,
        );
        assert!(loaded.node.placeholder.is_some());
        assert!(loaded.node.placeholder.unwrap().contains("newer"));
        // The stored version is preserved so a re-save round-trips.
        assert_eq!(loaded.node.type_version, 5);
    }

    #[test]
    fn unknown_type_loads_as_placeholder() {
        let reg = v2_registry();
        let loaded = load_node(&reg, NodeId(1), "gizmo", 1, Map::new(), [0.0; 2], false);
        assert!(
            loaded
                .node
                .placeholder
                .unwrap()
                .contains("unknown node type")
        );
    }

    #[test]
    fn phase8_v1_nodes_strip_their_dead_params_silently() {
        // The real registry: v1 documents carrying values in the dropped
        // params (users may have toggled them expecting an effect) load
        // with zero warnings through the silent-strip migrations.
        let reg = crate::nodes::builtin_registry().unwrap();

        // A subflow geometry node drops its whole dead rendering group.
        let loaded = load_node(
            &reg,
            NodeId(1),
            "box",
            1,
            raw(&[
                ("visible", serde_json::json!(false)),
                ("cast_shadow", serde_json::json!(false)),
                ("receive_shadow", serde_json::json!(false)),
            ]),
            [0.0; 2],
            false,
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert!(loaded.node.placeholder.is_none());
        assert_eq!(loaded.node.type_version, 2);
        for key in ["visible", "cast_shadow", "receive_shadow"] {
            assert!(!loaded.node.params.contains_key(key));
        }

        // The geo container keeps its live flags; only receive_shadow goes.
        let loaded = load_node(
            &reg,
            NodeId(2),
            "geo",
            1,
            raw(&[
                ("visible", serde_json::json!(false)),
                ("receive_shadow", serde_json::json!(true)),
            ]),
            [0.0; 2],
            false,
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        // v1 walks BOTH steps: strip receive_shadow, then land on v3 (the
        // rotate-order unification). This geo has no `rotate`, so nothing is
        // stamped and it takes the new XYZ default.
        assert_eq!(loaded.node.type_version, 3);
        assert_eq!(
            loaded.node.params.get("visible"),
            Some(&ParamSource::Literal(ParamValue::Bool(false)))
        );
        assert!(!loaded.node.params.contains_key("receive_shadow"));
        assert!(!loaded.node.params.contains_key("rotate_order"));

        // subdivide v1 -> v2: `scheme` was a one-variant enum read by nothing (a
        // dropdown with a single option that did nothing), dropped by the Phase
        // 15H param audit before the schema freeze could set it in stone.
        let loaded = load_node(
            &reg,
            NodeId(3),
            "subdivide",
            1,
            raw(&[
                ("scheme", serde_json::json!("linear")),
                ("iterations", serde_json::json!(3)),
            ]),
            [0.0; 2],
            false,
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert!(loaded.node.placeholder.is_none());
        assert_eq!(loaded.node.type_version, 2);
        assert!(
            !loaded.node.params.contains_key("scheme"),
            "the dead param is gone"
        );
        assert_eq!(
            loaded.node.params.get("iterations"),
            Some(&ParamSource::Literal(ParamValue::Int(3))),
            "the live param survives"
        );

        // import_ply drops `vertex_colors` (declared in v1, never carried
        // into the parse path) along with its dead rendering group.
        let loaded = load_node(
            &reg,
            NodeId(4),
            "import_ply",
            1,
            raw(&[
                ("vertex_colors", serde_json::json!(false)),
                ("scale", serde_json::json!(2.0)),
            ]),
            [0.0; 2],
            false,
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.node.type_version, 2);
        assert!(!loaded.node.params.contains_key("vertex_colors"));
        assert_eq!(
            loaded.node.params.get("scale"),
            Some(&ParamSource::Literal(ParamValue::Float(2.0)))
        );

        // rect_area_light drops its unread transform params, keeps the rest
        // (`rotate` is stripped before typing, so its stored shape never
        // matters).
        let loaded = load_node(
            &reg,
            NodeId(3),
            "rect_area_light",
            1,
            raw(&[
                ("rotate", serde_json::json!([0.0, 45.0, 0.0])),
                ("scale", serde_json::json!([2.0, 2.0, 2.0])),
                ("uniform_scale", serde_json::json!(3.0)),
                ("width", serde_json::json!(25.0)),
            ]),
            [0.0; 2],
            false,
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.node.type_version, 2);
        for key in ["rotate", "scale", "uniform_scale"] {
            assert!(!loaded.node.params.contains_key(key));
        }
        assert_eq!(
            loaded.node.params.get("width"),
            Some(&ParamSource::Literal(ParamValue::Float(25.0)))
        );
    }

    #[test]
    fn unknown_params_are_dropped_with_a_warning() {
        let reg = v2_registry();
        let loaded = load_node(
            &reg,
            NodeId(1),
            "widget",
            2,
            raw(&[
                ("dimension", serde_json::json!(1.0)),
                ("obsolete", serde_json::json!(true)),
            ]),
            [0.0; 2],
            false,
        );
        assert!(loaded.node.params.contains_key("dimension"));
        assert!(!loaded.node.params.contains_key("obsolete"));
        assert!(loaded.warnings.iter().any(|w| w.contains("obsolete")));
    }
}
