//! Serializable mirrors: the per-node [`NodeMirror`] and whole-document
//! [`DocumentSnapshot`] the frontend rebuilds its store from, and the
//! [`RegistrySnapshot`] that drives the palette and parameter panel (the
//! zero-frontend-change contract, node catalog part I section 8).
//!
//! Geometry never appears here: the mirror holds only node, edge, and
//! param metadata, so a full resnapshot after a desync or structural undo
//! is cheap.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::document::{Document, Edge, EdgeId, Graph, GraphContext, NodeId};
use crate::params::ParamSource;
use crate::registry::param_spec::{ParamSpec, ParamType, Unit};
use crate::registry::{Arity, BypassBehavior, NodeTypeDescriptor, PortSpec, Registry};

/// One node as the UI mirror sees it (no geometry).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeMirror {
    pub id: NodeId,
    pub type_id: String,
    pub type_version: u32,
    pub params: BTreeMap<String, ParamSource>,
    pub position: [f32; 2],
    pub bypassed: bool,
}

impl NodeMirror {
    /// Mirrors one node's UI-visible state (no geometry).
    #[must_use]
    pub fn from_public(node: &crate::document::NodeData) -> Self {
        Self {
            id: node.id,
            type_id: node.type_id.clone(),
            type_version: node.type_version,
            params: node.params.clone(),
            position: node.position,
            bypassed: node.bypassed,
        }
    }
}

/// One edge as the mirror sees it.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EdgeMirror {
    pub id: EdgeId,
    pub from: NodeId,
    pub from_port: String,
    pub to: NodeId,
    pub to_port: String,
}

impl From<&Edge> for EdgeMirror {
    fn from(e: &Edge) -> Self {
        Self {
            id: e.id,
            from: e.from,
            from_port: e.from_port.clone(),
            to: e.to,
            to_port: e.to_port.clone(),
        }
    }
}

/// One graph context's contents.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphMirror {
    pub nodes: Vec<NodeMirror>,
    pub edges: Vec<EdgeMirror>,
    pub active_output: Option<NodeId>,
    pub selection: Vec<NodeId>,
}

impl GraphMirror {
    fn from_graph(graph: &Graph) -> Self {
        Self {
            nodes: graph.nodes().map(NodeMirror::from_public).collect(),
            edges: graph.edges().map(EdgeMirror::from).collect(),
            active_output: graph.active_output,
            selection: graph.selection.clone(),
        }
    }
}

/// One annotation as the UI mirror sees it: the document annotation plus
/// the engine's runtime `needs_reanchor` flag (derived, never persisted).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationSnapshot {
    #[serde(flatten)]
    pub annotation: crate::review::Annotation,
    pub needs_reanchor: bool,
}

/// The full UI mirror: the root graph plus every subflow (keyed by owning
/// geo-node id, since JSON object keys are strings) plus the review
/// annotations with their runtime staleness.
#[derive(Debug, Clone, Serialize)]
pub struct DocumentSnapshot {
    pub root: GraphMirror,
    pub subflows: BTreeMap<String, GraphMirror>,
    pub annotations: Vec<AnnotationSnapshot>,
}

impl DocumentSnapshot {
    #[must_use]
    pub fn capture(doc: &Document, stale: &BTreeMap<crate::review::AnnotationId, bool>) -> Self {
        let root =
            GraphMirror::from_graph(doc.graph(GraphContext::Root).expect("root always exists"));
        let subflows = doc
            .subflow_owners()
            .filter_map(|owner| {
                doc.graph(GraphContext::Subflow(owner))
                    .ok()
                    .map(|g| (owner.0.to_string(), GraphMirror::from_graph(g)))
            })
            .collect();
        let annotations = doc
            .review()
            .iter()
            .map(|a| AnnotationSnapshot {
                needs_reanchor: stale.get(&a.id).copied().unwrap_or(false),
                annotation: a.clone(),
            })
            .collect();
        Self {
            root,
            subflows,
            annotations,
        }
    }
}

// The registry snapshot: descriptors minus function pointers.

#[derive(Debug, Clone, Serialize)]
pub struct RegistrySnapshot {
    pub nodes: Vec<NodeTypeSnapshot>,
    /// Every legal wire coercion (`Same`/`Lossless`/`Lossy`); pairs absent
    /// from this list are forbidden. The frontend reads it to preview a
    /// connection's ring (plain vs warning) or its rejection without a
    /// round-trip; the engine's `validate_connection` remains the single
    /// source of legality.
    pub coercions: Vec<CoercionEntry>,
}

/// One legal cell of the wire-coercion matrix.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CoercionEntry {
    pub from: crate::registry::coerce::DataType,
    pub to: crate::registry::coerce::DataType,
    pub kind: CoercionKind,
}

/// How a legal wire coercion presents in the UI.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CoercionKind {
    /// Identical endpoint types (no ring).
    Same,
    /// Allowed, no information lost (plain ring).
    Lossless,
    /// Allowed, information lost (filled warning ring).
    Lossy,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeTypeSnapshot {
    pub type_id: String,
    pub version: u32,
    pub display_name: String,
    pub category: crate::registry::Category,
    /// Title Case label for the category; `category` stays the stable id.
    pub category_label: String,
    pub root_context: bool,
    pub subflow_context: bool,
    pub inputs: Vec<PortSnapshot>,
    pub outputs: Vec<PortSnapshot>,
    pub params: Vec<ParamSnapshot>,
    pub bypass: BypassSnapshot,
    pub doc: String,
    pub search_aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortSnapshot {
    pub key: String,
    pub label: String,
    pub data_type: crate::registry::coerce::DataType,
    pub variadic: bool,
    pub required: bool,
    pub min: usize,
    pub is_default: bool,
    pub doc: String,
}

impl From<&PortSpec> for PortSnapshot {
    fn from(p: &PortSpec) -> Self {
        let (variadic, required, min) = match p.arity {
            Arity::Single { required } => (false, required, 0),
            Arity::Variadic { min } => (true, false, min),
        };
        Self {
            key: p.key.clone(),
            label: p.label.clone(),
            data_type: p.data_type,
            variadic,
            required,
            min,
            is_default: p.is_default,
            doc: p.doc.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ParamSnapshot {
    pub key: String,
    pub label: String,
    pub group: String,
    pub param_type: String,
    pub enum_variants: Vec<(String, String)>,
    pub accept: Vec<String>,
    /// The default value in schema-v1 plain JSON form.
    pub default: serde_json::Value,
    pub hard: Option<(f64, f64)>,
    pub soft: Option<(f64, f64)>,
    pub step: Option<f64>,
    pub unit: UnitSnapshot,
    /// The input-port key whose connection neutralizes this param (the
    /// panel dims the row while that port is connected).
    pub driven_by_port: Option<String>,
    pub doc: String,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UnitSnapshot {
    None,
    Degrees,
    Meters,
    Normalized,
}

impl From<Unit> for UnitSnapshot {
    fn from(u: Unit) -> Self {
        match u {
            Unit::None => UnitSnapshot::None,
            Unit::Degrees => UnitSnapshot::Degrees,
            Unit::Meters => UnitSnapshot::Meters,
            Unit::Normalized => UnitSnapshot::Normalized,
        }
    }
}

impl From<&ParamSpec> for ParamSnapshot {
    fn from(p: &ParamSpec) -> Self {
        let (param_type, enum_variants, accept) = match &p.ty {
            ParamType::Float => ("float", vec![], vec![]),
            ParamType::Int => ("int", vec![], vec![]),
            ParamType::Bool => ("bool", vec![], vec![]),
            ParamType::Text => ("text", vec![], vec![]),
            ParamType::Vec2 => ("vec2", vec![], vec![]),
            ParamType::Vec3 => ("vec3", vec![], vec![]),
            ParamType::Vec4 => ("vec4", vec![], vec![]),
            ParamType::Color => ("color", vec![], vec![]),
            ParamType::Enum { variants } => (
                "enum",
                variants
                    .iter()
                    .map(|v| (v.key.clone(), v.label.clone()))
                    .collect(),
                vec![],
            ),
            ParamType::AssetRef { accept } => ("assetRef", vec![], accept.clone()),
        };
        Self {
            key: p.key.clone(),
            label: p.label.clone(),
            group: p.group.clone(),
            param_type: param_type.to_string(),
            enum_variants,
            accept,
            default: crate::registry::resolve::param_value_to_json(&p.default),
            hard: p.range.map(|r| r.hard),
            soft: p.range.and_then(|r| r.soft),
            driven_by_port: p.driven_by_port.clone(),
            step: p.step,
            unit: p.unit.into(),
            doc: p.doc.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "mode")]
pub enum BypassSnapshot {
    PassThrough { input: String },
    Mute,
    NotBypassable,
}

impl From<&BypassBehavior> for BypassSnapshot {
    fn from(b: &BypassBehavior) -> Self {
        match b {
            BypassBehavior::PassThrough { input } => BypassSnapshot::PassThrough {
                input: input.clone(),
            },
            BypassBehavior::Mute => BypassSnapshot::Mute,
            BypassBehavior::NotBypassable => BypassSnapshot::NotBypassable,
        }
    }
}

impl NodeTypeSnapshot {
    fn from_descriptor(desc: &NodeTypeDescriptor) -> Self {
        Self {
            type_id: desc.type_id.to_string(),
            version: desc.version,
            display_name: desc.display_name.to_string(),
            category: desc.category,
            category_label: desc.category.display_name().to_string(),
            root_context: desc.contexts.root,
            subflow_context: desc.contexts.subflow,
            inputs: desc.inputs.iter().map(PortSnapshot::from).collect(),
            outputs: desc.outputs.iter().map(PortSnapshot::from).collect(),
            params: desc.params.iter().map(ParamSnapshot::from).collect(),
            bypass: BypassSnapshot::from(&desc.bypass),
            doc: desc.doc.to_string(),
            search_aliases: desc
                .search_aliases
                .iter()
                .map(ToString::to_string)
                .collect(),
        }
    }
}

impl RegistrySnapshot {
    #[must_use]
    pub fn capture(registry: &Registry) -> Self {
        use crate::registry::coerce::{Coercion, DataType, can_coerce};
        let mut coercions = Vec::new();
        for from in DataType::ALL {
            for to in DataType::ALL {
                let kind = match can_coerce(from, to) {
                    Coercion::Same => CoercionKind::Same,
                    Coercion::Lossless => CoercionKind::Lossless,
                    Coercion::Lossy => CoercionKind::Lossy,
                    Coercion::Forbidden => continue,
                };
                coercions.push(CoercionEntry { from, to, kind });
            }
        }
        Self {
            nodes: registry
                .descriptors()
                .map(NodeTypeSnapshot::from_descriptor)
                .collect(),
            coercions,
        }
    }
}
