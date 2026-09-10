//! How a node describes itself: the one-line summary under its label, the
//! report the info card reads, and who is wired to it.
//!
//! All of it is a pure reading of the declaration and the document. None
//! of it decides a colour, a size or a widget.
//!
//! One thing deliberately stays with the shells: rendering an absolute
//! timestamp. That needs a locale and a timezone, which is host knowledge
//! rather than document knowledge, and a crate that took an
//! internationalization stack to print one line in a card would be paying
//! a large bill for a small answer. [`relative_time`] is the half that is
//! a rule, and the shells compose it with whatever their platform gives
//! them for the absolute date.

use std::collections::BTreeMap;

use solarxy_graph::document::{Graph, NodeId};
use solarxy_graph::params::{ParamSource, ParamValue};
use solarxy_graph::registry::param_spec::{ParamSpec, ParamType};
use solarxy_graph::registry::{Category, NodeTypeDescriptor, Registry};

/// Well-known dimension parameter keys, in display priority order, with
/// the short prefix shown before the value.
///
/// The keys are the registry's own, which is worth stating because they
/// were not always: this table was written in the browser against
/// camelCase names while the registry declares `snake_case`, so
/// `radius_top` and `radius_bottom` never matched and the cylinder's
/// summary quietly showed only its height. Two further entries named
/// parameters that exist in neither spelling and are gone.
const DIMENSION_ABBREV: &[(&str, &str)] = &[
    ("size", "s"),
    ("width", "w"),
    ("height", "h"),
    ("depth", "d"),
    ("radius", "r"),
    ("radius_top", "rt"),
    ("radius_bottom", "rb"),
];

/// How many dimensions a summary shows before it stops.
const MAX_DIMENSIONS: usize = 3;

/// Resolves a staged asset's content hash to the file name it was staged
/// from. The shells hold the manifest; this crate only needs to ask.
pub type AssetNameLookup<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The effective value of a parameter: the stored literal, else the
/// declared default. An expression yields nothing, because a summary
/// showing the default while the expression says otherwise would mislead.
fn effective<'a>(
    params: &'a BTreeMap<String, ParamSource>,
    spec: &'a ParamSpec,
) -> Option<&'a ParamValue> {
    match params.get(&spec.key) {
        Some(ParamSource::Literal(value)) => Some(value),
        Some(ParamSource::Expression { .. }) => None,
        None => Some(&spec.default),
    }
}

/// A value the summary can print as a number. Both integer and float
/// parameters qualify, which is what the browser's `typeof v === "number"`
/// amounted to once the value had crossed as JSON.
fn numeric(value: &ParamValue) -> Option<f64> {
    match value {
        ParamValue::Float(v) => Some(*v),
        ParamValue::Int(v) => Some(*v as f64),
        _ => None,
    }
}

/// A float trimmed for a summary line: at most three decimals, no
/// trailing zeros.
#[must_use]
pub fn fmt_number(v: f64) -> String {
    if v.is_nan() {
        return "NaN".to_string();
    }
    if v.is_infinite() {
        return if v > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    trim_zeros(&format!("{v:.3}"))
}

/// Drops a trailing run of zeros, and the decimal point with them when
/// nothing is left after it.
fn trim_zeros(s: &str) -> String {
    if !s.contains('.') {
        return s.to_string();
    }
    let trimmed = s.trim_end_matches('0');
    trimmed.strip_suffix('.').unwrap_or(trimmed).to_string()
}

/// The one-line node summary, or nothing when the node has no parameter
/// worth showing.
///
/// The heuristic is per category: lights show intensity, generators show
/// their dimensions, imports show the staged asset, and everything else
/// shows its first numeric or enumerated parameter, preferring one outside
/// the general group because that group tends to hold housekeeping.
///
/// `asset_name` resolves a staged asset's content hash to a file name; a
/// caller with no manifest to hand passes `None` and gets the truncated
/// hash, which is what the summary showed before names were available.
#[must_use]
pub fn node_info_line(
    desc: &NodeTypeDescriptor,
    params: &BTreeMap<String, ParamSource>,
    asset_name: Option<AssetNameLookup<'_>>,
) -> Option<String> {
    let by_key = |key: &str| desc.params.iter().find(|p| p.key == key);

    if desc.category == Category::Lights
        && let Some(spec) = by_key("intensity")
        && let Some(v) = effective(params, spec).and_then(numeric)
    {
        return Some(format!("intensity {}", fmt_number(v)));
    }

    if desc.category == Category::Import
        && let Some(spec) = desc
            .params
            .iter()
            .find(|p| matches!(p.ty, ParamType::AssetRef { .. }))
    {
        let hash = match effective(params, spec) {
            Some(ParamValue::Asset(id)) => id.0.as_str(),
            _ => "",
        };
        if hash.is_empty() {
            return Some("no file".to_string());
        }
        return Some(
            asset_name.and_then(|f| f(hash)).unwrap_or_else(|| {
                format!("{}\u{2026}", hash.chars().take(10).collect::<String>())
            }),
        );
    }

    if desc.category == Category::Generators {
        let mut parts: Vec<String> = Vec::new();
        for (key, abbrev) in DIMENSION_ABBREV {
            let Some(spec) = desc
                .params
                .iter()
                .find(|p| p.key == *key && matches!(p.ty, ParamType::Float | ParamType::Int))
            else {
                continue;
            };
            if let Some(v) = effective(params, spec).and_then(numeric) {
                parts.push(format!("{abbrev} {}", fmt_number(v)));
            }
            if parts.len() == MAX_DIMENSIONS {
                break;
            }
        }
        if !parts.is_empty() {
            return Some(parts.join("  "));
        }
    }

    // The fallback: the first numeric or enumerated parameter, preferring
    // one outside the general group.
    let candidates: Vec<&ParamSpec> = desc
        .params
        .iter()
        .filter(|p| {
            matches!(
                p.ty,
                ParamType::Float | ParamType::Int | ParamType::Enum { .. }
            )
        })
        .collect();
    let pick = candidates
        .iter()
        .find(|p| !p.group.eq_ignore_ascii_case("general"))
        .or_else(|| candidates.first())?;
    let value = effective(params, pick)?;
    if let ParamType::Enum { variants } = &pick.ty
        && let ParamValue::Enum(key) = value
    {
        return Some(
            variants
                .iter()
                .find(|v| v.key == *key)
                .map_or_else(|| key.clone(), |v| v.label.clone()),
        );
    }
    let v = numeric(value)?;
    Some(format!("{} {}", pick.label.to_lowercase(), fmt_number(v)))
}

/// Whether a node type declares the root visibility parameter, and so
/// whether the affordance exists for it.
///
/// Forwarded to [`solarxy_graph::registry::visibility`] rather than
/// restated. The mirror derives the same field for every node it sends,
/// and this crate sits above the engine, so a copy here would be the
/// second implementation of a rule a reader compares across the shells.
#[must_use]
pub fn declares_visibility(desc: &NodeTypeDescriptor) -> bool {
    solarxy_graph::registry::visibility::declares_node_visibility(desc)
}

/// Whether a node is currently shown.
///
/// Forwarded for the same reason as [`declares_visibility`]: the mirror
/// already answers this for the browser, and the desktop asks it here.
#[must_use]
pub fn is_visible(params: &BTreeMap<String, ParamSource>) -> bool {
    solarxy_graph::registry::visibility::node_visible(params)
}

/// A duration in microseconds, at a scale a human reads.
///
/// Microseconds below a millisecond because that is where most nodes live
/// and `0.0 ms` says nothing; seconds above a thousand milliseconds
/// because `4200.0 ms` is worse than `4.2 s`. The argument is a float
/// rather than the engine's integer because the info card divides a total
/// by a count to show an average.
#[must_use]
pub fn format_duration(us: f64) -> String {
    if !us.is_finite() || us < 0.0 {
        return "unknown".to_string();
    }
    if us == 0.0 {
        return "0".to_string();
    }
    if us < 1000.0 {
        return format!("{} us", us.round());
    }
    let ms = us / 1000.0;
    if ms < 1000.0 {
        let decimals = if ms < 10.0 { 2 } else { 1 };
        return format!("{ms:.decimals$} ms");
    }
    format!("{:.2} s", ms / 1000.0)
}

/// A short "5 minutes ago" for a recent stamp, or an empty string once the
/// absolute date carries it on its own.
///
/// A stamp in the future returns nothing rather than a negative interval,
/// because a system-clock change produces one and "-3 seconds ago" reads
/// as a defect.
#[must_use]
pub fn relative_time(ms: f64, now: f64) -> String {
    let delta = now - ms;
    if !delta.is_finite() || delta < 0.0 {
        return String::new();
    }
    let secs = (delta / 1000.0).floor() as i64;
    if secs < 10 {
        return "just now".to_string();
    }
    if secs < 60 {
        return format!("{secs} seconds ago");
    }
    let mins = secs / 60;
    if mins < 60 {
        return plural(mins, "minute");
    }
    let hours = mins / 60;
    if hours < 24 {
        return plural(hours, "hour");
    }
    let days = hours / 24;
    if days < 30 {
        return plural(days, "day");
    }
    String::new()
}

fn plural(n: i64, unit: &str) -> String {
    let s = if n == 1 { "" } else { "s" };
    format!("{n} {unit}{s} ago")
}

/// A bounds box as size then centre, which is what someone actually wants
/// to know, or nothing when there is no finite box to describe.
#[must_use]
pub fn format_bounds(bounds: Option<[f32; 6]>) -> Option<String> {
    let b = bounds?;
    if !b.iter().all(|v| v.is_finite()) {
        return None;
    }
    let size = [b[3] - b[0], b[4] - b[1], b[5] - b[2]];
    let centre = [
        f32::midpoint(b[0], b[3]),
        f32::midpoint(b[1], b[4]),
        f32::midpoint(b[2], b[5]),
    ];
    let n = |v: f32| {
        if v.abs() < 1e-4 {
            "0".to_string()
        } else {
            trim_zeros(&format!("{v:.3}"))
        }
    };
    Some(format!(
        "{} at {}",
        size.map(n).join(" x "),
        centre.map(n).join(", ")
    ))
}

/// One port and the nodes on the other side of it, in edge order.
///
/// `Serialize` because the browser reads this across the WebAssembly
/// boundary, on the same terms as [`crate::tree::TreeRow`]: there is no
/// `Deserialize`, because nothing sends one back.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortNeighbours {
    pub port: String,
    pub nodes: Vec<String>,
}

/// Who is wired to a node, by name.
#[derive(Debug, Clone, PartialEq, Eq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionSummary {
    /// Each input port and the node names feeding it.
    pub inputs: Vec<PortNeighbours>,
    /// Each output port and the node names it feeds.
    pub outputs: Vec<PortNeighbours>,
    /// How many distinct nodes feed this one.
    pub upstream: usize,
    /// How many distinct nodes it feeds.
    pub downstream: usize,
}

/// Reads the wiring around one node.
///
/// The counts are of distinct neighbours rather than of edges, because two
/// edges from one node into a variadic port is one relationship. An edge
/// whose far end is not in the graph is named by its id rather than
/// dropped, so a snapshot taken mid-edit describes itself instead of
/// silently losing a row.
#[must_use]
pub fn connection_summary(graph: &Graph, node: NodeId, registry: &Registry) -> ConnectionSummary {
    let name = |id: NodeId| {
        graph.node(id).map_or_else(
            || format!("node {}", id.0),
            |n| solarxy_graph::naming::node_name(n, registry),
        )
    };
    let push = |lists: &mut Vec<PortNeighbours>, port: &str, who: String| match lists
        .iter_mut()
        .find(|entry| entry.port == port)
    {
        Some(entry) => entry.nodes.push(who),
        None => lists.push(PortNeighbours {
            port: port.to_string(),
            nodes: vec![who],
        }),
    };

    let mut summary = ConnectionSummary::default();
    let mut upstream: Vec<NodeId> = Vec::new();
    let mut downstream: Vec<NodeId> = Vec::new();
    for edge in graph.edges() {
        if edge.to == node {
            push(&mut summary.inputs, &edge.to_port, name(edge.from));
            if !upstream.contains(&edge.from) {
                upstream.push(edge.from);
            }
        }
        if edge.from == node {
            push(&mut summary.outputs, &edge.from_port, name(edge.to));
            if !downstream.contains(&edge.to) {
                downstream.push(edge.to);
            }
        }
    }
    summary.upstream = upstream.len();
    summary.downstream = downstream.len();
    summary
}

/// What kind of node this is, in one line: its category, the networks it
/// may sit in, and the network it opens when it is a container.
#[must_use]
pub fn describe_kind(desc: &NodeTypeDescriptor) -> String {
    let mut bits = vec![desc.category.display_name().to_string()];
    let contexts = desc.contexts.kinds();
    if !contexts.is_empty() {
        let names: Vec<&str> = contexts.iter().map(|k| k.as_str()).collect();
        bits.push(format!("in {}", names.join(", ")));
    }
    if let Some(opens) = desc.opens {
        bits.push(format!("opens a {} network", opens.as_str()));
    }
    bits.join(" \u{b7} ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::document::{ContextKind, Edge, EdgeId, NodeData};
    use solarxy_graph::registry::param_spec::EnumVariant;

    fn lit(value: ParamValue) -> ParamSource {
        ParamSource::Literal(value)
    }

    fn stored(pairs: Vec<(&str, ParamSource)>) -> BTreeMap<String, ParamSource> {
        pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
    }

    fn param(key: &str, group: &str, ty: ParamType, default: ParamValue) -> ParamSpec {
        ParamSpec::new(key, key, group, ty, default)
    }

    fn float(key: &str, group: &str, default: f64) -> ParamSpec {
        param(key, group, ParamType::Float, ParamValue::Float(default))
    }

    /// A descriptor is a big literal, so the test builds one from the two
    /// fields these rules read and leaves the rest at something inert.
    fn desc(category: Category, params: Vec<ParamSpec>) -> NodeTypeDescriptor {
        NodeTypeDescriptor {
            type_id: "t",
            version: 1,
            display_name: "T",
            category,
            contexts: solarxy_graph::registry::ContextSet::OBJ
                .or(solarxy_graph::registry::ContextSet::SOP),
            opens: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            params,
            bypass: solarxy_graph::registry::BypassBehavior::Mute,
            doc: "",
            search_aliases: &[],
            glyph: "t",
            role: solarxy_graph::registry::NodeRole::Standard,
            cook: |_, _, _| unreachable!("these rules never cook"),
            migrate: None,
        }
    }

    #[test]
    fn shows_light_intensity_from_an_explicit_literal() {
        let d = desc(Category::Lights, vec![float("intensity", "general", 0.0)]);
        let n = stored(vec![("intensity", lit(ParamValue::Float(1.5)))]);
        assert_eq!(
            node_info_line(&d, &n, None).as_deref(),
            Some("intensity 1.5")
        );
    }

    #[test]
    fn falls_back_to_the_registry_default_when_params_are_sparse() {
        let d = desc(Category::Lights, vec![float("intensity", "light", 0.5)]);
        assert_eq!(
            node_info_line(&d, &BTreeMap::new(), None).as_deref(),
            Some("intensity 0.5")
        );
    }

    #[test]
    fn shows_primitive_dimensions_with_abbreviations_capped_at_three() {
        let d = desc(
            Category::Generators,
            vec![
                float("width", "geometry", 1.0),
                float("height", "geometry", 2.0),
                float("depth", "geometry", 1.0),
                float("radius", "geometry", 9.0),
            ],
        );
        let n = stored(vec![("depth", lit(ParamValue::Float(0.25)))]);
        assert_eq!(
            node_info_line(&d, &n, None).as_deref(),
            Some("w 1  h 2  d 0.25")
        );
    }

    #[test]
    fn a_cylinder_shows_both_radii_beside_its_height() {
        // The defect the port closes. The keys were written in camelCase
        // against a registry that declares snake_case, so these two never
        // matched and the summary showed the height alone.
        let d = desc(
            Category::Generators,
            vec![
                float("radius_top", "geometry", 1.0),
                float("radius_bottom", "geometry", 1.0),
                float("height", "geometry", 2.0),
            ],
        );
        assert_eq!(
            node_info_line(&d, &BTreeMap::new(), None).as_deref(),
            Some("h 2  rt 1  rb 1")
        );
    }

    #[test]
    fn shows_the_staged_asset_name_for_imports_and_the_hash_without_one() {
        let d = desc(
            Category::Import,
            vec![param(
                "source",
                "general",
                ParamType::AssetRef { accept: Vec::new() },
                ParamValue::Asset(solarxy_graph::params::AssetId(String::new())),
            )],
        );
        let hash = "abcdef0123456789";
        let n = stored(vec![(
            "source",
            lit(ParamValue::Asset(solarxy_graph::params::AssetId(
                hash.to_string(),
            ))),
        )]);
        let name = |_: &str| Some("dragon.obj".to_string());
        assert_eq!(
            node_info_line(&d, &n, Some(&name)).as_deref(),
            Some("dragon.obj")
        );
        assert_eq!(
            node_info_line(&d, &n, None).as_deref(),
            Some("abcdef0123\u{2026}")
        );
        assert_eq!(
            node_info_line(&d, &BTreeMap::new(), None).as_deref(),
            Some("no file")
        );
    }

    #[test]
    fn falls_back_to_the_first_non_general_numeric_param() {
        let d = desc(
            Category::Topology,
            vec![
                param("seed", "general", ParamType::Int, ParamValue::Int(4)),
                float("angle", "transform", 45.0),
            ],
        );
        assert_eq!(
            node_info_line(&d, &BTreeMap::new(), None).as_deref(),
            Some("angle 45")
        );
    }

    #[test]
    fn labels_enums_with_the_variant_display_name() {
        let d = desc(
            Category::Utility,
            vec![param(
                "mode",
                "options",
                ParamType::Enum {
                    variants: vec![EnumVariant {
                        key: "x".into(),
                        label: "Exact".into(),
                    }],
                },
                ParamValue::Enum("x".into()),
            )],
        );
        assert_eq!(
            node_info_line(&d, &BTreeMap::new(), None).as_deref(),
            Some("Exact")
        );
    }

    #[test]
    fn returns_nothing_with_no_matching_params_and_skips_expressions() {
        assert_eq!(
            node_info_line(&desc(Category::Utility, Vec::new()), &BTreeMap::new(), None),
            None
        );
        let d = desc(Category::Lights, vec![float("intensity", "general", 0.0)]);
        let n = stored(vec![(
            "intensity",
            ParamSource::Expression {
                expr: "1+1".to_string(),
            },
        )]);
        assert_eq!(node_info_line(&d, &n, None), None);
    }

    #[test]
    fn trims_to_three_decimals_without_trailing_zeros() {
        assert_eq!(fmt_number(1.0), "1");
        assert_eq!(fmt_number(0.25), "0.25");
        assert_eq!(fmt_number(1.23456), "1.235");
    }

    #[test]
    fn a_type_declaring_the_visibility_param_gets_the_affordance() {
        let with = desc(
            Category::Generators,
            vec![ParamSpec::new(
                "visible",
                "Visible",
                "general",
                ParamType::Bool,
                ParamValue::Bool(true),
            )],
        );
        assert!(declares_visibility(&with));
        assert!(!declares_visibility(&desc(Category::Utility, Vec::new())));
    }

    #[test]
    fn only_an_explicit_false_hides_a_node() {
        // Parameters are override-only, so a fresh node carries no entry
        // and must read as shown. An expression reads as shown too: the
        // reserve refuses to evaluate it, and hiding on an unevaluated
        // expression would make nodes disappear at random.
        assert!(is_visible(&BTreeMap::new()));
        assert!(is_visible(&stored(vec![(
            "visible",
            lit(ParamValue::Bool(true))
        )])));
        assert!(!is_visible(&stored(vec![(
            "visible",
            lit(ParamValue::Bool(false))
        )])));
        assert!(is_visible(&stored(vec![(
            "visible",
            ParamSource::Expression {
                expr: "0 > 1".to_string()
            }
        )])));
    }

    #[test]
    fn keeps_sub_millisecond_cooks_in_microseconds() {
        // The whole reason the rule exists: the badge says "0.0 ms" for
        // these.
        assert_eq!(format_duration(340.0), "340 us");
        assert_eq!(format_duration(999.0), "999 us");
    }

    #[test]
    fn switches_to_milliseconds_and_then_seconds_as_the_scale_grows() {
        assert_eq!(format_duration(1500.0), "1.50 ms");
        assert_eq!(format_duration(42_000.0), "42.0 ms");
        assert_eq!(format_duration(4_200_000.0), "4.20 s");
    }

    #[test]
    fn refuses_to_invent_a_figure_for_junk() {
        // Still reachable despite the engine's own count being unsigned:
        // the info card divides a total by a count to show an average.
        assert_eq!(format_duration(f64::NAN), "unknown");
        assert_eq!(format_duration(-1.0), "unknown");
        assert_eq!(format_duration(0.0), "0");
    }

    #[test]
    fn relative_time_scales_through_the_units_and_singularizes() {
        let now = 1_000_000_000.0;
        assert_eq!(relative_time(now - 3_000.0, now), "just now");
        assert_eq!(relative_time(now - 30_000.0, now), "30 seconds ago");
        assert_eq!(relative_time(now - 60_000.0, now), "1 minute ago");
        assert_eq!(relative_time(now - 7_200_000.0, now), "2 hours ago");
        assert_eq!(relative_time(now - 2.0 * 86_400_000.0, now), "2 days ago");
    }

    #[test]
    fn relative_time_says_nothing_for_a_stamp_in_the_future() {
        // A system-clock change produces one; it must not read as
        // "-3 seconds ago".
        let now = 1_000_000_000.0;
        assert_eq!(relative_time(now + 5_000.0, now), "");
        // And nothing once the absolute date carries it alone.
        assert_eq!(relative_time(now - 200.0 * 86_400_000.0, now), "");
    }

    #[test]
    fn reports_bounds_as_size_then_centre() {
        assert_eq!(
            format_bounds(Some([-1.0, -1.0, -1.0, 1.0, 1.0, 1.0])).as_deref(),
            Some("2 x 2 x 2 at 0, 0, 0")
        );
    }

    #[test]
    fn handles_a_degenerate_box_without_producing_nothing() {
        assert_eq!(
            format_bounds(Some([3.0, 4.0, 5.0, 3.0, 4.0, 5.0])).as_deref(),
            Some("0 x 0 x 0 at 3, 4, 5")
        );
    }

    #[test]
    fn returns_nothing_when_there_are_no_bounds_to_show() {
        assert_eq!(format_bounds(None), None);
        assert_eq!(
            format_bounds(Some([0.0, 0.0, 0.0, f32::NAN, 1.0, 1.0])),
            None
        );
    }

    /// Two generators into one merge, and the merge into a null. The node
    /// types are real ones with real ports, because `Graph::connect`
    /// validates against the registry and a graph built from types that do
    /// not accept the wiring is a graph with no edges in it.
    fn wired() -> (Graph, Registry) {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let mut g = Graph::new(ContextKind::Sop);
        for (id, ty) in [(1, "box"), (2, "box"), (3, "merge"), (4, "null")] {
            g.add_node(NodeData::new(NodeId(id), ty, 1));
        }
        let mut wire = |id, from, from_port: &str, to, to_port: &str, variadic| {
            g.connect(
                Edge {
                    id: EdgeId(id),
                    from: NodeId(from),
                    from_port: from_port.to_string(),
                    to: NodeId(to),
                    to_port: to_port.to_string(),
                },
                variadic,
            )
            .expect("the wiring the ports accept");
        };
        wire(10, 1, "geometry", 3, "inputs", true);
        wire(11, 2, "geometry", 3, "inputs", true);
        wire(12, 3, "geometry", 4, "geometry", false);
        (g, registry)
    }

    #[test]
    fn groups_sources_by_the_port_they_feed_in_edge_order() {
        let (g, r) = wired();
        let s = connection_summary(&g, NodeId(3), &r);
        assert_eq!(s.inputs.len(), 1);
        assert_eq!(s.inputs[0].port, "inputs");
        assert_eq!(s.inputs[0].nodes.len(), 2);
    }

    #[test]
    fn counts_distinct_neighbours_not_edges() {
        let (g, r) = wired();
        let s = connection_summary(&g, NodeId(3), &r);
        assert_eq!(s.upstream, 2);
        assert_eq!(s.downstream, 1);
    }

    #[test]
    fn reports_an_isolated_node_as_empty_rather_than_failing() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let mut g = Graph::new(ContextKind::Sop);
        g.add_node(NodeData::new(NodeId(3), "merge", 1));
        let s = connection_summary(&g, NodeId(3), &registry);
        assert_eq!(s, ConnectionSummary::default());
    }

    // The browser guarded against an edge whose far end had gone, which a
    // mirror mid-update really can carry. A `Graph` cannot: `remove_node`
    // returns the incident edges and takes them with it, so the state has
    // no constructor here and no test. The `node {id}` fallback stays
    // because `Graph::node` answers with an `Option` and something has to
    // be said, but it is unreachable from a document, and that difference
    // is one of the reasons the rule is better off on this side.

    #[test]
    fn describe_kind_names_the_category_the_contexts_and_what_it_opens() {
        let plain = desc(Category::Generators, Vec::new());
        assert_eq!(describe_kind(&plain), "Generators \u{b7} in obj, sop");
        let mut container = desc(Category::Container, Vec::new());
        container.opens = Some(ContextKind::Sop);
        assert_eq!(
            describe_kind(&container),
            "Container \u{b7} in obj, sop \u{b7} opens a sop network"
        );
    }
}

#[cfg(test)]
mod registry_tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The summary every generator in the shipped catalog produces at its
    /// own defaults, pinned so a table edit or a renamed parameter shows up
    /// as a diff rather than as a line that quietly went missing.
    #[test]
    fn every_generator_summarises_itself_at_its_defaults() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let empty = BTreeMap::new();
        let mut lines: Vec<String> = registry
            .descriptors()
            .filter(|d| d.category == Category::Generators)
            .map(|d| {
                format!(
                    "{}: {}",
                    d.type_id,
                    node_info_line(d, &empty, None).unwrap_or_else(|| "(none)".to_string())
                )
            })
            .collect();
        lines.sort();
        assert_eq!(
            lines,
            [
                "box: w 1  h 1  d 1",
                "circle: r 0.5",
                "cone: h 1  r 0.5",
                // Both radii, which is the line the browser could not draw:
                // its table spelled these two in camelCase and the registry
                // declares them in snake case, so neither ever matched and
                // the cylinder showed its height alone.
                "cylinder: h 1  rt 0.5  rb 0.5",
                "line: points 2",
                "plane: w 1  h 1",
                "sphere: r 0.5",
                "torus: r 0.5",
                "torus_knot: r 0.5",
            ]
        );
    }
}
