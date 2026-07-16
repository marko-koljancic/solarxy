//! The node registry: descriptors, ports, contexts, bypass, and the
//! registry invariants (node catalog part I, sections 1, 3, 6, 7).
//!
//! Adding a node is two touch points: one file in `nodes/` containing
//! `descriptor()`, the cook function, the optional migrate hook, and unit
//! tests; plus one registration line. The invariants below make the
//! contract's implicit rules explicit and machine-checked.

pub mod coerce;
pub mod param_spec;
pub mod resolve;
pub mod scalar;

use std::collections::{BTreeMap, BTreeSet};

use crate::GraphError;
use crate::cook::CookFn;
use crate::document::ContextKind;
use crate::params::ParamValue;
use crate::registry::coerce::DataType;
use crate::registry::param_spec::{ParamSpec, ParamType};
use crate::registry::resolve::conform_value;

/// Port arity. Outputs are always `Single`; at most one variadic input
/// per node (both enforced by the invariants).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arity {
    Single {
        required: bool,
    },
    /// Ordered, unlimited, reorderable (decision 25).
    Variadic {
        min: usize,
    },
}

/// One port declaration. The `key` is simultaneously the canvas handle id
/// and the compute body's input key (break that and a node silently
/// receives no input; the invariants make it impossible).
#[derive(Debug, Clone, PartialEq)]
pub struct PortSpec {
    pub key: String,
    pub label: String,
    pub data_type: DataType,
    pub arity: Arity,
    /// At most one default input and one default output per node: drives
    /// body-drop auto-connect, insert-on-wire, and bypass pass-through
    /// resolution.
    pub is_default: bool,
    pub doc: String,
}

impl PortSpec {
    /// A single-arity port.
    #[must_use]
    pub fn single(
        key: impl Into<String>,
        label: impl Into<String>,
        data_type: DataType,
        required: bool,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            data_type,
            arity: Arity::Single { required },
            is_default: false,
            doc: String::new(),
        }
    }

    /// A variadic input port.
    #[must_use]
    pub fn variadic(
        key: impl Into<String>,
        label: impl Into<String>,
        data_type: DataType,
        min: usize,
    ) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            data_type,
            arity: Arity::Variadic { min },
            is_default: false,
            doc: String::new(),
        }
    }

    #[must_use]
    pub fn default_port(mut self) -> Self {
        self.is_default = true;
        self
    }

    #[must_use]
    pub fn doc(mut self, doc: impl Into<String>) -> Self {
        self.doc = doc.into();
        self
    }

    #[must_use]
    pub fn is_variadic(&self) -> bool {
        matches!(self.arity, Arity::Variadic { .. })
    }
}

/// Palette category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Container,
    Primitives,
    Modifiers,
    Import,
    Lights,
    Utility,
}

impl Category {
    /// Title Case name for UI labels. The serde `snake_case` string stays
    /// the stable id (CSS classes, grouping keys, snapshot `category`);
    /// every label surface renders this instead.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Container => "Container",
            Self::Primitives => "Primitives",
            Self::Modifiers => "Modifiers",
            Self::Import => "Import",
            Self::Lights => "Lights",
            Self::Utility => "Utility",
        }
    }
}

/// The visual silhouette family a node renders with in the web canvas.
/// Orthogonal to [`Category`] (which picks the fill): a pure UI hint that
/// never affects cooking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NodeRole {
    Standard,
    Container,
    Gather,
    Branch,
    Terminal,
    Analyzer,
    ImageSource,
    Light,
    Note,
}

/// Which network kinds a node type may be placed in: a small bitset over
/// [`ContextKind`]. The phase-17 generalization of the old two-bool
/// root/subflow mask; legality is judged against the target graph's
/// `kind`, never against its address.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextSet(u8);

impl ContextSet {
    pub const OBJ: Self = Self::of(ContextKind::Obj);
    pub const GEO: Self = Self::of(ContextKind::Geo);
    pub const MAT: Self = Self::of(ContextKind::Mat);
    pub const TEX: Self = Self::of(ContextKind::Tex);
    /// Every kind (the note node's placement).
    pub const ALL: Self = Self::OBJ.or(Self::GEO).or(Self::MAT).or(Self::TEX);

    #[must_use]
    pub const fn of(kind: ContextKind) -> Self {
        Self(1 << kind as u8)
    }

    /// Set union, `const` so descriptor literals can compose
    /// (`ContextSet::GEO.or(ContextSet::TEX)`).
    #[must_use]
    pub const fn or(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, kind: ContextKind) -> bool {
        self.0 & (1 << kind as u8) != 0
    }

    /// The member kinds in [`ContextKind::ALL`] order (the snapshot's
    /// serialization).
    #[must_use]
    pub fn kinds(self) -> Vec<ContextKind> {
        ContextKind::ALL
            .into_iter()
            .filter(|k| self.contains(*k))
            .collect()
    }
}

/// Bypass semantics (node catalog part I, section 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BypassBehavior {
    /// Output = that input's gathered value; the node is not cooked. On a
    /// variadic port: the first connected sub-input.
    PassThrough {
        input: String,
    },
    /// Output = empty; the node contributes nothing while bypassed.
    Mute,
    NotBypassable,
}

/// A migration hook's failure (the node loads as a placeholder instead of
/// destroying the document).
#[derive(Debug, Clone, thiserror::Error)]
#[error("migration from version {from} failed: {reason}")]
pub struct MigrateError {
    pub from: u32,
    pub reason: String,
}

/// Stepwise migration hook: called once per version step on the raw JSON
/// param map, before schema typing. Only param renames or semantic
/// changes need one; dropped/added params migrate by registry default.
pub type MigrateFn = fn(
    from: u32,
    params: &mut serde_json::Map<String, serde_json::Value>,
) -> Result<(), MigrateError>;

/// One node type's complete declaration.
pub struct NodeTypeDescriptor {
    /// Plain `snake_case` id (`box`, `torus_knot`, `import_gltf`). Dots are
    /// rejected, reserved for a future `vendor.node` plugin namespace.
    pub type_id: &'static str,
    /// Starts at 1; bumped with a migration entry on any post-freeze spec
    /// change.
    pub version: u32,
    pub display_name: &'static str,
    pub category: Category,
    pub contexts: ContextSet,
    /// The network kind this node's child canvas is, for container nodes
    /// (`geo` opens `Geo`, `matnet` opens `Mat`, `texnet` opens `Tex`).
    /// `None` for everything else. The engine creates and kinds the child
    /// network from this, so no container type is ever special-cased by
    /// its `type_id`.
    pub opens: Option<ContextKind>,
    pub inputs: Vec<PortSpec>,
    pub outputs: Vec<PortSpec>,
    pub params: Vec<ParamSpec>,
    pub bypass: BypassBehavior,
    /// Markdown node help; also feeds the generated wiki reference.
    pub doc: &'static str,
    pub search_aliases: &'static [&'static str],
    /// Stable icon key the web frontend maps to vector art; by convention
    /// the type id (lights drop their `_light` suffix). An unknown key
    /// falls back to the category glyph client-side.
    pub glyph: &'static str,
    /// The silhouette family the node renders with.
    pub role: NodeRole,
    pub cook: CookFn,
    pub migrate: Option<MigrateFn>,
}

impl NodeTypeDescriptor {
    /// The default input port, if any.
    #[must_use]
    pub fn default_input(&self) -> Option<&PortSpec> {
        self.inputs.iter().find(|p| p.is_default)
    }

    /// The default output port, if any.
    #[must_use]
    pub fn default_output(&self) -> Option<&PortSpec> {
        self.outputs.iter().find(|p| p.is_default)
    }

    #[must_use]
    pub fn input(&self, key: &str) -> Option<&PortSpec> {
        self.inputs.iter().find(|p| p.key == key)
    }

    #[must_use]
    pub fn output(&self, key: &str) -> Option<&PortSpec> {
        self.outputs.iter().find(|p| p.key == key)
    }

    #[must_use]
    pub fn param(&self, key: &str) -> Option<&ParamSpec> {
        self.params.iter().find(|p| p.key == key)
    }
}

impl std::fmt::Debug for NodeTypeDescriptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NodeTypeDescriptor")
            .field("type_id", &self.type_id)
            .field("version", &self.version)
            .finish_non_exhaustive()
    }
}

/// The descriptor set, collected once at engine construction.
#[derive(Debug, Default)]
pub struct Registry {
    by_id: BTreeMap<&'static str, NodeTypeDescriptor>,
}

impl Registry {
    /// Builds and validates. Construction fails on any invariant
    /// violation, so a registry that exists is a valid one.
    pub fn with_descriptors(descriptors: Vec<NodeTypeDescriptor>) -> Result<Self, GraphError> {
        let mut by_id = BTreeMap::new();
        for desc in descriptors {
            if by_id.insert(desc.type_id, desc).is_some() {
                return Err(GraphError::InvalidRegistry(format!(
                    "duplicate type id '{}'",
                    by_id.keys().next_back().copied().unwrap_or_default()
                )));
            }
        }
        let registry = Self { by_id };
        let violations = registry.invariant_violations();
        if violations.is_empty() {
            Ok(registry)
        } else {
            Err(GraphError::InvalidRegistry(violations.join("; ")))
        }
    }

    #[must_use]
    pub fn get(&self, type_id: &str) -> Option<&NodeTypeDescriptor> {
        self.by_id.get(type_id)
    }

    pub fn descriptors(&self) -> impl Iterator<Item = &NodeTypeDescriptor> {
        self.by_id.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Every violated registry invariant, as human-readable findings. The
    /// `registry_invariants` test asserts this is empty over the full
    /// builtin set; [`Self::with_descriptors`] enforces it at runtime.
    #[must_use]
    pub fn invariant_violations(&self) -> Vec<String> {
        let mut violations = Vec::new();
        for desc in self.by_id.values() {
            let id = desc.type_id;
            // Type-id shape: non-empty snake_case, no dots (reserved).
            if id.is_empty()
                || !id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            {
                violations.push(format!(
                    "'{id}': type ids are plain snake_case (dots reserved for plugins)"
                ));
            }
            if desc.version == 0 {
                violations.push(format!("'{id}': versions start at 1"));
            }
            // At most one variadic input.
            let variadic = desc.inputs.iter().filter(|p| p.is_variadic()).count();
            if variadic > 1 {
                violations.push(format!("'{id}': more than one variadic input"));
            }
            // Outputs are always Single.
            if desc.outputs.iter().any(PortSpec::is_variadic) {
                violations.push(format!("'{id}': outputs must be single-arity"));
            }
            // At most one default port per direction.
            if desc.inputs.iter().filter(|p| p.is_default).count() > 1 {
                violations.push(format!("'{id}': more than one default input"));
            }
            if desc.outputs.iter().filter(|p| p.is_default).count() > 1 {
                violations.push(format!("'{id}': more than one default output"));
            }
            // Port keys unique per direction.
            for (ports, dir) in [(&desc.inputs, "input"), (&desc.outputs, "output")] {
                let mut seen = BTreeSet::new();
                for p in ports {
                    if !seen.insert(&p.key) {
                        violations.push(format!("'{id}': duplicate {dir} port key '{}'", p.key));
                    }
                }
            }
            // Bypass target exists among the inputs.
            if let BypassBehavior::PassThrough { input } = &desc.bypass
                && desc.input(input).is_none()
            {
                violations.push(format!(
                    "'{id}': bypass pass-through targets missing input '{input}'"
                ));
            }
            // Param keys unique across ALL groups (flat storage).
            let mut seen = BTreeSet::new();
            for p in &desc.params {
                if !seen.insert(&p.key) {
                    violations.push(format!("'{id}': duplicate param key '{}'", p.key));
                }
            }
            for p in &desc.params {
                // Defaults conform to their own declared type.
                if let Err(reason) = conform_value(&p.default, &p.ty) {
                    violations.push(format!(
                        "'{id}': param '{}' default does not conform: {reason}",
                        p.key
                    ));
                }
                // Enum variants are non-empty and the default is one of
                // them (conform_value already checks membership; check
                // emptiness explicitly).
                if let ParamType::Enum { variants } = &p.ty
                    && variants.is_empty()
                {
                    violations.push(format!("'{id}': param '{}' has no enum variants", p.key));
                }
                // Soft range must sit inside the hard range.
                if let Some(range) = &p.range {
                    if range.hard.0 > range.hard.1 {
                        violations
                            .push(format!("'{id}': param '{}' hard range is inverted", p.key));
                    }
                    if let Some(soft) = range.soft
                        && (soft.0 < range.hard.0 || soft.1 > range.hard.1)
                    {
                        violations.push(format!(
                            "'{id}': param '{}' soft range exceeds the hard range",
                            p.key
                        ));
                    }
                }
                // Every show_if target names an existing param, and never
                // itself.
                for cond in &p.show_if {
                    if cond.param == p.key {
                        violations.push(format!("'{id}': param '{}' shows-if on itself", p.key));
                    } else if desc.param(&cond.param).is_none() {
                        violations.push(format!(
                            "'{id}': param '{}' shows-if on missing param '{}'",
                            p.key, cond.param
                        ));
                    }
                }
                // Every driven_by_port names an existing input port (the
                // panel's dim-while-connected predicate must be real).
                if let Some(port) = &p.driven_by_port
                    && !desc.inputs.iter().any(|input| input.key == *port)
                {
                    violations.push(format!(
                        "'{id}': param '{}' driven by missing input port '{port}'",
                        p.key
                    ));
                }
            }
            // Context consistency: nodes placeable in the object network
            // are portless (lights and cameras cook at root portless;
            // containers and notes carry no ports). Cross-context data
            // flow is by path reference, never by wire, so the object
            // canvas never needs handles (context-expansion decision C-2).
            if desc.contexts.contains(ContextKind::Obj)
                && (!desc.inputs.is_empty() || !desc.outputs.is_empty())
            {
                violations.push(format!(
                    "'{id}': nodes placeable in the object network are portless"
                ));
            }
            // Every param default that is an Asset must be the unset
            // (empty) reference; concrete assets cannot be defaults. The
            // same rule binds NodePath defaults: a node id baked into a
            // descriptor could never resolve in a fresh document.
            for p in &desc.params {
                if let ParamValue::Asset(asset) = &p.default
                    && !asset.0.is_empty()
                {
                    violations.push(format!(
                        "'{id}': param '{}' defaults to a concrete asset",
                        p.key
                    ));
                }
                if let ParamValue::NodeRef(Some(_)) = &p.default {
                    violations.push(format!(
                        "'{id}': param '{}' defaults to a concrete node reference",
                        p.key
                    ));
                }
            }
        }
        violations
    }
}
