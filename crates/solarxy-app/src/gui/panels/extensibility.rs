//! The zero-interface-change contract, as a test.
//!
//! A node type this shell has never seen, fabricated into a registry, must
//! be fully interpretable by the same registry-driven helpers the palette,
//! the canvas and the parameter panel use: it appears in the
//! context-filtered palette, its ports colour and coerce, it resolves
//! drawable art, and every one of its parameters gets a control. No
//! per-node code exists anywhere in this shell, and this is what says so.
//!
//! **The twin of `web/src/registry/extensibility.test.ts`**, case for
//! case, because the release's proof obligation is that a node type added
//! in Rust reaches *two* frontends with no interface change in *either*.
//! One shell proving it alone proves half of the claim.
//!
//! It lives beside the panels rather than inside one because the contract
//! spans the canvas and the parameter panel together, which is also why a
//! handful of items are visible to this module and no wider.
//!
//! ## Three places the two shells differ, and why each is not a gap
//!
//! **A parameter type with no control is a build failure here**, where the
//! browser prints the unknown type's name. So the browser needs a list of
//! what it draws, and a list can go stale: the browser's had drifted by
//! three entries before 0.10.0, with the only reader a fabricated probe
//! whose parameters all happened to be listed. Here the list *is* the
//! enum, so the case that catches drift on one shell is a compile error on
//! the other, and what remains worth asserting is the other direction:
//! that no control is claimed for a type nothing declares.
//!
//! **A category this build has not heard of cannot be fabricated.**
//! `Category` is a Rust enum, so an unknown category is a compile error
//! rather than a string that arrives over a boundary. The browser's
//! degradation case has no twin here for the same reason its ordering case
//! does: the order is the declaration order, shared, and pinned in
//! `solarxy-studio` rather than restated per shell.
//!
//! **A novel data type cannot be fabricated either**, and for the same
//! reason. What can be fabricated, and is, is a *glyph key* this shell has
//! no art for, which is the case that actually bites: art is host-bound
//! residue by design, so a node declaring a key neither shell has drawn
//! yet has to degrade rather than draw nothing.

use solarxy_graph::document::ContextKind;
use solarxy_graph::params::ParamValue;
use solarxy_graph::registry::param_spec::{EnumVariant, NodePathAccept, ParamSpec, ParamType, Unit};
use solarxy_graph::registry::coerce::DataType;
use solarxy_graph::registry::{
    BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor, PortSpec, Registry,
};
use solarxy_studio::types::{self, PortSide};

use super::nodes::{body_size, candidates, glyph_art, glyph_key, silhouette};
use super::params::{ControlKind, control_kind, driven, offers_toggle};

/// A node this shell has no knowledge of, using diverse existing types.
///
/// Deliberately the browser's probe: the same ports, the same eight
/// parameters, the same glyph key with no art, so a divergence shows up as
/// one of the two twins failing rather than as two fixtures that were
/// never comparable.
fn probe() -> NodeTypeDescriptor {
    NodeTypeDescriptor {
        type_id: "probe",
        version: 1,
        display_name: "Probe",
        category: Category::Generators,
        contexts: ContextSet::of(ContextKind::Sop),
        opens: None,
        inputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, false).default_port(),
            PortSpec::single("detail_map", "Detail Map", DataType::Image, false),
        ],
        outputs: vec![
            PortSpec::single("geometry", "Geometry", DataType::Geometry, false).default_port(),
        ],
        params: vec![
            ParamSpec::new(
                "size",
                "Size",
                "geometry",
                ParamType::Float,
                ParamValue::Float(1.0),
            )
            .hard(0.01, 100.0)
            .soft(0.1, 10.0)
            .step(0.1)
            .unit(Unit::Meters),
            ParamSpec::new(
                "segments",
                "Segments",
                "geometry",
                ParamType::Int,
                ParamValue::Int(3),
            )
            .hard(1.0, 64.0)
            .step(1.0),
            ParamSpec::new(
                "capped",
                "Capped",
                "geometry",
                ParamType::Bool,
                ParamValue::Bool(true),
            ),
            ParamSpec::new(
                "mode",
                "Mode",
                "shape",
                ParamType::Enum {
                    variants: vec![
                        EnumVariant::new("a", "Alpha"),
                        EnumVariant::new("b", "Beta"),
                    ],
                },
                ParamValue::Enum("a".to_string()),
            ),
            ParamSpec::new(
                "offset",
                "Offset",
                "shape",
                ParamType::Vec3,
                ParamValue::Vec3([0.0, 0.0, 0.0]),
            )
            .step(0.01),
            {
                let mut tint = ParamSpec::new(
                    "tint",
                    "Tint",
                    "shape",
                    ParamType::Color,
                    ParamValue::Color([1.0, 1.0, 1.0, 1.0]),
                );
                tint.driven_by_port = Some("detail_map".to_string());
                tint
            },
            ParamSpec::new(
                "material",
                "Material",
                "shape",
                ParamType::NodePath {
                    accept: NodePathAccept::Opens(ContextKind::Mat),
                },
                ParamValue::NodeRef(None),
            ),
            ParamSpec::new(
                "lane",
                "Lane",
                "shape",
                ParamType::AttributeName,
                ParamValue::Text("color".to_string()),
            ),
        ],
        bypass: BypassBehavior::Mute,
        doc: "A fabricated node this shell has never seen.",
        search_aliases: &["probe", "novel"],
        // A glyph key this shell has NO art for, so the category fallback
        // is what the cases below exercise.
        glyph: "probe",
        role: NodeRole::Standard,
        cook: |_, _, _| {
            Ok(solarxy_graph::cook::CookOutcome::Done(
                solarxy_graph::cook::Outputs::default(),
            ))
        },
        migrate: None,
    }
}

fn registry() -> Registry {
    Registry::with_descriptors(vec![probe()]).expect("one descriptor builds a registry")
}

fn shipped() -> Registry {
    solarxy_graph::nodes::builtin_registry().expect("builtin registry")
}

/// It is discoverable and context-filtered like any node.
#[test]
fn a_novel_node_is_offered_by_the_palette_in_its_own_contexts_only() {
    let registry = registry();
    assert_eq!(registry.get("probe").map(|d| d.display_name), Some("Probe"));

    let offered: Vec<&str> = candidates(&registry, ContextKind::Sop, None, "")
        .iter()
        .map(|d| d.type_id)
        .collect();
    assert_eq!(offered, vec!["probe"]);
    // And nowhere it did not declare. The contexts come from the typed
    // vocabulary; a node declaring a new kind is still a filter match
    // away.
    assert!(candidates(&registry, ContextKind::Obj, None, "").is_empty());
    assert!(candidates(&registry, ContextKind::Mat, None, "").is_empty());

    // Its search aliases answer too, which is the other half of being
    // discoverable: a reader who does not know the display name.
    assert_eq!(
        candidates(&registry, ContextKind::Sop, None, "novel").len(),
        1
    );
    assert!(candidates(&registry, ContextKind::Sop, None, "nothing").is_empty());
}

/// Its handles are typed, so the canvas can colour them and refuse a bad
/// wire without knowing what a probe is.
#[test]
fn a_novel_nodes_ports_colour_and_coerce_from_the_shared_rules() {
    let registry = registry();
    assert_eq!(
        types::port_data_type(&registry, "probe", "geometry", PortSide::Output),
        Some(DataType::Geometry)
    );
    assert_eq!(
        types::port_data_type(&registry, "probe", "detail_map", PortSide::Input),
        Some(DataType::Image)
    );

    // Probe to Probe geometry is a legal, same-type connection.
    let verdict = types::connection_verdict(&registry, "probe", "geometry", "probe", "geometry");
    assert!(verdict.legal);

    // An image never reaches a geometry input, and the refusal comes from
    // the shared matrix rather than from anything here.
    let refused = types::connection_verdict(&registry, "probe", "geometry", "probe", "detail_map");
    assert!(!refused.legal, "geometry must not wire into an image input");
    assert_eq!(refused.coercion, None);

    // A port neither side declares is refused rather than guessed at.
    assert!(!types::connection_verdict(&registry, "probe", "nope", "probe", "geometry").legal);
}

/// A port is drawn from the shared answer, not from a table in this shell.
#[test]
fn a_port_is_drawn_from_the_shared_rule_rather_than_from_a_local_copy() {
    let registry = registry();
    let map = types::port_data_type(&registry, "probe", "detail_map", PortSide::Input)
        .expect("the probe declares an image input");
    let geometry = types::port_data_type(&registry, "probe", "geometry", PortSide::Input)
        .expect("the probe declares a geometry input");

    // Two different data types are two different shapes, which is what
    // makes a wire readable without colour.
    assert_ne!(types::handle_shape(map), types::handle_shape(geometry));

    // A token, never a value: a shell that authored its own hue would put
    // a literal here and nothing would hold it to the palette.
    assert!(
        types::wire_token(map).starts_with("wire-"),
        "a wire's colour is a palette role, not a colour this shell picked"
    );
}

/// Categories sort in the engine's order, which both shells read rather
/// than curate.
#[test]
fn the_category_order_is_the_shared_one() {
    use std::cmp::Ordering;
    assert_eq!(
        types::compare_categories(Category::Container, Category::Generators),
        Ordering::Less
    );
    assert_eq!(
        types::compare_categories(Category::Lights, Category::Container),
        Ordering::Greater
    );
    assert_eq!(
        types::compare_categories(Category::Generators, Category::Generators),
        Ordering::Equal
    );
}

/// The map-overrides-factor link is plain declaration data.
#[test]
fn a_parameter_neutralized_by_a_wire_says_so_in_its_own_declaration() {
    let registry = registry();
    let desc = registry.get("probe").expect("the probe");
    let tint = desc
        .params
        .iter()
        .find(|p| p.key == "tint")
        .expect("the probe declares a tint");
    assert_eq!(tint.driven_by_port.as_deref(), Some("detail_map"));
    assert!(
        desc.inputs
            .iter()
            .any(|port| Some(&port.key) == tint.driven_by_port.as_ref()),
        "the port a parameter names must be one the node actually has"
    );
    // And the panel's dim predicate needs only that plus the node's edges,
    // never per-node code: with no document there is no wire, so nothing
    // is dimmed.
    let mut engine = solarxy_graph::Engine::new().expect("registry builds");
    let geo = add(
        &mut engine,
        solarxy_graph::document::GraphContext::Root,
        "sopnet",
    );
    let ctx = solarxy_graph::document::GraphContext::Subflow(geo);
    let node = add(&mut engine, ctx, "material");
    let graph = engine.document().graph(ctx).expect("the graph");
    let material = engine
        .registry()
        .get("material")
        .expect("the material type is registered");
    let linked = material
        .params
        .iter()
        .find(|p| p.driven_by_port.is_some())
        .expect("the material node links a parameter to a port");
    assert!(
        !driven(linked, graph, node),
        "an unwired port neutralizes nothing"
    );
}

fn add(
    engine: &mut solarxy_graph::Engine,
    ctx: solarxy_graph::document::GraphContext,
    ty: &str,
) -> solarxy_graph::document::NodeId {
    let before: Vec<_> = engine
        .document()
        .graph(ctx)
        .map(|g| g.nodes().map(|n| n.id).collect())
        .unwrap_or_default();
    engine
        .apply(solarxy_graph::Command::AddNode {
            ctx,
            node_type: ty.to_string(),
            position: [0.0, 0.0],
        })
        .expect("add a node");
    engine
        .document()
        .graph(ctx)
        .expect("the graph")
        .nodes()
        .map(|n| n.id)
        .find(|id| !before.contains(id))
        .expect("exactly one node was added")
}

/// Every one of its parameters gets a control, and the expression lane is
/// offered per type rather than per node.
#[test]
fn every_parameter_of_a_novel_node_gets_a_control() {
    let registry = registry();
    let desc = registry.get("probe").expect("the probe");
    assert_eq!(desc.params.len(), 8);

    // Four distinct families across eight parameters, so this is a real
    // dispatch rather than one arm answering everything.
    let kinds: Vec<ControlKind> = desc.params.iter().map(|p| control_kind(&p.ty)).collect();
    let distinct: std::collections::BTreeSet<String> =
        kinds.iter().map(|k| format!("{k:?}")).collect();
    assert!(
        distinct.len() >= 6,
        "eight parameters resolved to only {} families: {distinct:?}",
        distinct.len()
    );

    // The panel groups them by their declared group, in declaration
    // order, with no list here saying which groups exist.
    let mut groups: Vec<&str> = Vec::new();
    for spec in &desc.params {
        if !groups.contains(&spec.group.as_str()) {
            groups.push(&spec.group);
        }
    }
    assert_eq!(groups, vec!["geometry", "shape"]);

    // The expression lane is offered on the numeric ones and on nothing
    // else, and the probe declares both sides of that line.
    let offered: Vec<&str> = desc
        .params
        .iter()
        .filter(|p| offers_toggle(p))
        .map(|p| p.key.as_str())
        .collect();
    assert_eq!(
        offered,
        vec!["size", "segments", "capped", "offset", "tint"]
    );
}

/// It always resolves drawable art, even when this shell has never drawn
/// its glyph.
#[test]
fn a_novel_node_always_resolves_drawable_art() {
    let registry = registry();
    let desc = registry.get("probe").expect("the probe");

    // The premise: this shell has no art for the declared key. Without
    // this the fallback below would be untested.
    assert!(
        glyph_art(desc.glyph).is_none(),
        "the probe's glyph must be one this shell has never drawn"
    );
    // So the category's family answers instead, and that family has art.
    let resolved = glyph_key(Some(desc));
    assert_eq!(resolved, types::category_glyph(Category::Generators));
    assert!(
        glyph_art(&resolved).is_some(),
        "the fallback must produce real art rather than a blank"
    );

    // A declared glyph this shell does have wins over the fallback.
    let mut known = probe();
    known.glyph = "merge";
    assert_eq!(glyph_key(Some(&known)), "merge");

    // And a node with no descriptor at all still draws something.
    assert!(glyph_art(&glyph_key(None)).is_some());

    // Its role resolves to a body this shell can actually paint. A role is
    // a Rust enum here rather than a string over a boundary, so the
    // browser's unknown-role case is a compile error instead; what is
    // worth asserting is that every role has a body, since a zero-sized
    // one would draw nothing.
    for role in [
        NodeRole::Standard,
        NodeRole::Branch,
        NodeRole::Analyzer,
        NodeRole::ImageSource,
    ] {
        let size = body_size(role);
        assert!(
            size.x > 0.0 && size.y > 0.0,
            "{role:?} has no body to paint"
        );
        if let Some(outline) = silhouette(role) {
            assert!(
                outline.len() >= 3,
                "{role:?} declares a silhouette that is not a shape"
            );
        }
    }
}

/// **The proof, not a proxy for it.** A node type this shell has never
/// seen is drawn by both surfaces, in real interface passes, over a
/// document that holds one.
///
/// The cases above assert the rules the surfaces read. This asserts that
/// reading them is all the surfaces do: the probe's registry contains
/// nothing else, so a canvas or a panel with any per-node knowledge in it
/// would have to fall back, panic, or draw a blank, and there is nowhere
/// for such knowledge to hide. Two passes each, because egui runs a pass
/// twice on any frame a layout is still settling.
///
/// The browser's twin cannot make this claim: its fixture is a snapshot
/// rather than a running document, so it proves its helpers interpret a
/// novel node and stops there.
#[test]
fn a_novel_node_draws_in_both_surfaces_and_asks_for_nothing() {
    use crate::gui::intent::Intents;
    use crate::gui::theme::Theme;
    use solarxy_core::preferences::ThemeChoice;
    use solarxy_graph::document::GraphContext;

    let mut engine = solarxy_graph::Engine::with_registry(registry());
    let ctx = GraphContext::Root;
    // The probe declares only the geo-network context, and the root is an
    // object network, so placing one there is refused by the engine's own
    // placement rule rather than by anything here. That refusal is itself
    // part of the contract, so it is asserted rather than worked around.
    assert!(
        engine
            .apply(solarxy_graph::Command::AddNode {
                ctx,
                node_type: "probe".to_string(),
                position: [0.0, 0.0],
            })
            .is_err(),
        "a node declaring only one context must be refused in the others"
    );

    // A registry with a container in it, so there is a geo network to put
    // the probe in. The container is fabricated too: this shell knows
    // nothing about either type.
    let mut holder = probe();
    holder.type_id = "holder";
    holder.display_name = "Holder";
    holder.category = Category::Container;
    holder.contexts = ContextSet::of(ContextKind::Obj);
    holder.opens = Some(ContextKind::Sop);
    holder.params = Vec::new();
    holder.inputs = Vec::new();
    holder.outputs = Vec::new();
    let registry = Registry::with_descriptors(vec![probe(), holder])
        .expect("two distinct type ids build a registry");
    let mut engine = solarxy_graph::Engine::with_registry(registry);
    let container = add(&mut engine, ctx, "holder");
    let inside = GraphContext::Subflow(container);
    let node = add(&mut engine, inside, "probe");
    engine
        .apply(solarxy_graph::Command::SetSelection {
            ctx: inside,
            ids: vec![node],
        })
        .expect("select the probe");

    let theme = Theme::from_choice(ThemeChoice::default());
    let mut intents = Intents::default();
    let mut canvas = super::nodes::CanvasState::default();
    let mut panel = super::params::ParamPanelState::default();
    let mut current = inside;

    for _ in 0..2 {
        let egui_ctx = egui::Context::default();
        let _ = egui_ctx.run(egui::RawInput::default(), |c| {
            egui::CentralPanel::default().show(c, |ui| {
                super::nodes::draw_nodes_content(
                    ui,
                    super::nodes::CanvasSource::Scene(&super::nodes::CanvasScene {
                        doc: engine.document(),
                        registry: engine.registry(),
                        revision: engine.revision(),
                        cook: &std::collections::BTreeMap::new(),
                        assets: &std::collections::BTreeMap::new(),
                        manual: false,
                        playing: false,
                        info: None,
                    }),
                    &mut canvas,
                    &mut current,
                    solarxy_core::preferences::CanvasPrefs::default(),
                    &mut intents,
                    theme,
                );
                super::params::draw_params_content(
                    ui,
                    super::params::ParamPanelSource::Scene(&super::params::ParamScene {
                        doc: engine.document(),
                        registry: engine.registry(),
                        ctx: inside,
                        stats: None,
                        has_report: false,
                        report: None,
                        counts: (0, 0),
                        assets: &[],
                        lanes: &[],
                        error: None,
                        resolved: &super::params::ResolvedParams::new(),
                    }),
                    &mut panel,
                    &mut intents,
                    theme,
                );
            });
        });
    }

    assert!(
        intents.take_ordered().is_empty(),
        "a novel node nobody touched must ask for nothing in either surface"
    );
    // And the canvas really did mirror it, rather than drawing an empty
    // graph and passing for want of anything to get wrong.
    assert_eq!(
        engine
            .document()
            .graph(inside)
            .expect("the network")
            .nodes()
            .count(),
        1
    );
}

/// The control dispatch is not padded: every family it can answer with is
/// one some shipped node actually reaches.
///
/// The browser's twin needs two cases here, because its list of drawable
/// types is a separate array that had drifted by three entries. This
/// shell's dispatch is an exhaustive `match`, so the direction the
/// browser's first case covers is a compile error instead; this is the
/// other direction, which no compiler checks.
#[test]
fn no_control_is_claimed_for_a_type_nothing_declares() {
    let shipped = shipped();
    let reachable: std::collections::BTreeSet<String> = shipped
        .descriptors()
        .flat_map(|d| d.params.iter())
        .map(|spec| format!("{:?}", control_kind(&spec.ty)))
        .collect();

    // Every family the dispatch can produce, listed by producing it.
    let every: std::collections::BTreeSet<String> = [
        ParamType::Float,
        ParamType::Int,
        ParamType::Bool,
        ParamType::Text,
        ParamType::MultilineText,
        ParamType::AttributeName,
        ParamType::Snippet,
        ParamType::Vec2,
        ParamType::Vec3,
        ParamType::Vec4,
        ParamType::Color,
        ParamType::Enum {
            variants: Vec::new(),
        },
        ParamType::AssetRef { accept: Vec::new() },
        ParamType::Action,
        ParamType::NodePath {
            accept: NodePathAccept::TypeIs("camera".to_string()),
        },
    ]
    .iter()
    .map(|ty| format!("{:?}", control_kind(ty)))
    .collect();

    let orphaned: Vec<&String> = every.difference(&reachable).collect();
    assert!(
        orphaned.is_empty(),
        "these controls are drawn for no shipped node: {orphaned:?}"
    );
}
