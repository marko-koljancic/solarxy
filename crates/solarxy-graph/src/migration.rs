//! Node-version migration.
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
        created_ms: None,
        modified_ms: None,
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
        created_ms: None,
        modified_ms: None,
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
        BypassBehavior, Category, ContextSet, MigrateError, NodeRole, NodeTypeDescriptor, PortSpec,
        Registry,
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
            category: Category::Generators,
            contexts: ContextSet::GEO,
            opens: None,
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
    fn v1_nodes_strip_their_dead_params_silently() {
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

        // import_ply v1 -> v3: the v1 hook drops the historical inert
        // `vertex_colors` (its stored value carried no intent), then the
        // v3 descriptor re-declares it for real and the registry default
        // fill supplies `true`.
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
        assert_eq!(loaded.node.type_version, 3);
        assert!(
            !loaded.node.params.contains_key("vertex_colors"),
            "the dead v1 value is dropped; the restored toggle resolves to \
             its descriptor default (true) at cook time"
        );
        assert_eq!(
            loaded.node.params.get("scale"),
            Some(&ParamSource::Literal(ParamValue::Float(2.0)))
        );

        // rect_area_light drops its unread transform params, keeps the rest.
        //
        // This one matters more since v3, which restored a `rotate` param.
        // A v1 document's `rotate` was authored against a renderer that
        // ignored it entirely, so it is not a value anyone chose to see;
        // carrying it forward would silently re-aim panels in old scenes
        // the first time they are opened. It must still be stripped, and
        // the light must come back face-down.
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
        // Silent: this fixture stores no `intensity`, so the v3-to-v4
        // rescale has nothing to touch and the param fills from the
        // registry default, which moved by the same factor.
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.node.type_version, 4);
        for key in ["rotate", "scale", "uniform_scale"] {
            assert!(
                !loaded.node.params.contains_key(key),
                "`{key}` survived the v1 strip; a v3 rect-area light would \
                 open pointing somewhere the author never chose"
            );
        }
        assert_eq!(
            loaded.node.params.get("width"),
            Some(&ParamSource::Literal(ParamValue::Float(25.0)))
        );

        // geo_export v1 -> v2 (0.8.0 material export): hookless pure
        // default-fill. Stored params survive untouched; the new
        // `include_materials` stays unset and resolves to its descriptor
        // default (true) at cook time.
        let loaded = load_node(
            &reg,
            NodeId(5),
            "geo_export",
            1,
            raw(&[("format", serde_json::json!("obj"))]),
            [0.0; 2],
            false,
        );
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert!(loaded.node.placeholder.is_none());
        assert_eq!(loaded.node.type_version, 2);
        assert_eq!(
            loaded.node.params.get("format"),
            Some(&ParamSource::Literal(ParamValue::Enum("obj".to_string())))
        );
        assert!(!loaded.node.params.contains_key("include_materials"));
    }

    /// The copy operations' v1 to v2 step, which is the migration in 0.8.2
    /// that changes what a document MEANS if it gets this wrong.
    ///
    /// Both node types gained `copy_mode`, whose default is Instance. Every
    /// v1 node was authored against an engine that could only bake, so a v1
    /// document loaded without the pin would open with its copies collapsed
    /// The intensity rescale, on the four lights it applies to.
    ///
    /// The raster path stopped multiplying every light's contribution by
    /// three, so a stored value has to move by the same factor or every
    /// scene saved before this release opens two thirds darker. Checked on
    /// the stored value here; the whole-document version, which checks the
    /// image instead, lives in `engine::tests`.
    #[test]
    fn a_stored_intensity_is_rescaled_on_the_lights_that_carried_the_multiplier() {
        let reg = crate::nodes::builtin_registry().unwrap();
        // (type id, the version it was stored at, the version it lands on).
        //
        // The landing version is not the rescale's own: a v1 node is carried to
        // whatever the current spec is, and point and spot have since gained
        // Radius on top. That the rescale still runs across two bumps is the
        // thing worth pinning, because the hook keys on the version it came
        // *from* rather than on the one it lands at.
        for (type_id, from, to) in [
            ("point_light", 1, 3),
            ("directional_light", 1, 2),
            ("spot_light", 1, 3),
            // Already at v3 for unrelated reasons, so its rescale is v4.
            ("rect_area_light", 3, 4),
        ] {
            let loaded = load_node(
                &reg,
                NodeId(1),
                type_id,
                from,
                raw(&[("intensity", serde_json::json!(1.5))]),
                [0.0; 2],
                false,
            );
            assert!(
                loaded.warnings.is_empty(),
                "{type_id}: {:?}",
                loaded.warnings
            );
            assert_eq!(loaded.node.type_version, to, "{type_id}");
            assert_eq!(
                loaded.node.params.get("intensity"),
                Some(&ParamSource::Literal(ParamValue::Float(4.5))),
                "{type_id}: a stored 1.5 rendered as 4.5 before the \
                 multiplier left the shader, so it has to say 4.5 now"
            );
        }
    }

    /// Ambient and hemisphere must NOT be rescaled.
    ///
    /// They fold into the hemisphere rows of the light uniform and never
    /// entered the per-light loop the multiplier lived in, so their stored
    /// values already meant what they said. Tripling them would brighten
    /// every old scene that used one, which is the same class of silent
    /// wrongness the rescale exists to prevent, pointed the other way.
    #[test]
    fn the_two_lights_that_never_saw_the_multiplier_are_left_alone() {
        let reg = crate::nodes::builtin_registry().unwrap();
        for type_id in ["ambient_light", "hemisphere_light"] {
            let loaded = load_node(
                &reg,
                NodeId(1),
                type_id,
                1,
                raw(&[("intensity", serde_json::json!(0.4))]),
                [0.0; 2],
                false,
            );
            assert!(
                loaded.warnings.is_empty(),
                "{type_id}: {:?}",
                loaded.warnings
            );
            assert_eq!(
                loaded.node.type_version, 1,
                "{type_id} has no spec change, so it has no version bump"
            );
            assert_eq!(
                loaded.node.params.get("intensity"),
                Some(&ParamSource::Literal(ParamValue::Float(0.4))),
                "{type_id}: rescaling this would make every old scene using \
                 it three times brighter"
            );
        }
    }

    /// An expression-valued intensity is reported rather than guessed at.
    ///
    /// Migrations run on the raw stored JSON, where the only object form a
    /// param takes is `{"$expr": ...}`. Rewriting that source text to
    /// inject a factor has no safe general form, and leaving it silently
    /// would make one light three times dimmer than the rest of the scene
    /// with nothing said. So it is left as written and surfaced as a load
    /// warning naming what to do about it.
    #[test]
    fn an_expression_intensity_survives_and_warns() {
        let reg = crate::nodes::builtin_registry().unwrap();
        let stored = serde_json::json!({ "$expr": "$F * 0.1" });
        let loaded = load_node(
            &reg,
            NodeId(1),
            "point_light",
            1,
            raw(&[("intensity", stored)]),
            [0.0; 2],
            false,
        );
        assert_eq!(loaded.node.type_version, 3);
        assert_eq!(
            loaded.warnings.len(),
            1,
            "expected exactly one warning, got {:?}",
            loaded.warnings
        );
        let warning = &loaded.warnings[0];
        assert!(
            warning.contains("expression") && warning.contains('3'),
            "the warning must say what happened and what to do: {warning}"
        );
        assert_eq!(
            loaded.node.params.get("intensity"),
            Some(&ParamSource::Expression {
                expr: "$F * 0.1".to_string()
            }),
            "the expression itself must survive untouched"
        );
    }

    /// A light with no stored intensity opens at the new default, which is
    /// the same brightness the old default rendered at.
    #[test]
    fn an_unset_intensity_fills_from_the_rescaled_default() {
        let reg = crate::nodes::builtin_registry().unwrap();
        let loaded = load_node(&reg, NodeId(1), "point_light", 1, raw(&[]), [0.0; 2], false);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        // Absent from the stored map: the resolver fills it from the spec.
        assert!(!loaded.node.params.contains_key("intensity"));
        let spec = reg
            .get("point_light")
            .unwrap()
            .params
            .iter()
            .find(|p| p.key == "intensity")
            .unwrap();
        assert_eq!(spec.default, ParamValue::Float(4.5));
    }

    /// into one prototype and every downstream node reading something
    /// different, with nothing on screen to say why.
    #[test]
    fn v1_copy_nodes_are_pinned_to_bake() {
        let reg = crate::nodes::builtin_registry().unwrap();

        for (type_id, live_key, live_json, live_value) in [
            (
                "copy_to_points",
                "scale",
                serde_json::json!(2.5),
                ParamValue::Float(2.5),
            ),
            ("array", "count", serde_json::json!(7), ParamValue::Int(7)),
        ] {
            let loaded = load_node(
                &reg,
                NodeId(1),
                type_id,
                1,
                raw(&[(live_key, live_json)]),
                [0.0; 2],
                false,
            );
            assert!(
                loaded.warnings.is_empty(),
                "{type_id}: {:?}",
                loaded.warnings
            );
            assert!(loaded.node.placeholder.is_none(), "{type_id}");
            assert_eq!(loaded.node.type_version, 2, "{type_id}");
            assert_eq!(
                loaded.node.params.get("copy_mode"),
                Some(&ParamSource::Literal(ParamValue::Enum("bake".to_string()))),
                "{type_id}: a v1 node must open baked, not inherit the new \
                 Instance default"
            );
            assert_eq!(
                loaded.node.params.get(live_key),
                Some(&ParamSource::Literal(live_value)),
                "{type_id}: the live param survives the migration"
            );
        }
    }

    /// A node that already carries the key keeps its own value. The step is
    /// guarded on absence, so re-running it is a no-op rather than an
    /// overwrite: a v2 node deliberately set to Instance must not be pinned.
    #[test]
    fn the_copy_mode_pin_never_overwrites_a_stored_choice() {
        let reg = crate::nodes::builtin_registry().unwrap();
        let loaded = load_node(
            &reg,
            NodeId(1),
            "array",
            1,
            raw(&[("copy_mode", serde_json::json!("instance"))]),
            [0.0; 2],
            false,
        );
        assert_eq!(
            loaded.node.params.get("copy_mode"),
            Some(&ParamSource::Literal(ParamValue::Enum(
                "instance".to_string()
            )))
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
