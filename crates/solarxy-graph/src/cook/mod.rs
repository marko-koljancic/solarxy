//! The cook contract and engine: what a compute body receives
//! ([`Inputs`], [`crate::registry::resolve::ResolvedParams`],
//! [`CookCtx`]), what it returns ([`CookOutcome`], [`CookError`]), and the
//! budgeted resumable loop that drives it (`state` submodule).
//!
//! Semantics ported from the Minimystix executable spec: keep-last-good on
//! renderable-empty output, per-node async generation guards, dirty-union
//! plus transitive-downstream recook in topological order, and cone
//! gating to the active display node. Deliberate deltas per the catalog:
//! a **required, unconnected** input is a hard cook error (keep-last-good
//! deliberately NOT applied; an explicit disconnect is user intent),
//! whereas a connected-but-transiently-empty upstream flows through
//! keep-last-good; and wire values coerce through the enforced matrix at
//! gather time.
//!
//! One deliberate simplification vs the Minimystix `out[port] ||
//! out.default` gather fallback: that fallback papered over a keying
//! mismatch (ports named one thing, outputs keyed another) which the
//! registry invariants make impossible here, so a gather miss is a
//! node-authoring bug (debug-asserted), never silently absorbed.

pub mod driver;
pub mod state;

pub use driver::{CookEngine, CookReport};

use std::collections::BTreeMap;
use std::sync::Arc;

use solarxy_core::validation::{ValidationConfig, ValidationResult};
use solarxy_kernel::GeometrySet;

use crate::assets::AssetTable;
use crate::params::AssetId;
use crate::registry::coerce::Value;

/// One input port's gathered content.
#[derive(Debug, Clone)]
pub enum InputSlot {
    /// No edge is connected (distinct from "connected but empty": a
    /// required port with an `Absent` slot is a hard cook error).
    Absent,
    /// The single edge's value, already coerced to the port type.
    Single(Value),
    /// A variadic port's values in `port_order`, each coerced. May be
    /// empty (variadic `min: 0` with nothing connected).
    ///
    /// One entry per connected edge, positionally aligned with `port_order`.
    /// A `None` is a connected wire whose upstream has no committed value
    /// (it errored, is bypassed to empty, or has not cooked yet). Holes are
    /// preserved rather than compacted away, because a node that selects an
    /// input BY INDEX (`switch`) would otherwise silently pick the wrong
    /// wire. Consumers that only concatenate (`merge`) skip the holes via
    /// [`Inputs::geometry_list`]; consumers that index use
    /// [`Inputs::geometry_slots`].
    Variadic(Vec<Option<Value>>),
}

/// Everything gathered for one cook, keyed by input-port key.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    slots: BTreeMap<String, InputSlot>,
}

impl Inputs {
    #[must_use]
    pub fn new(slots: BTreeMap<String, InputSlot>) -> Self {
        Self { slots }
    }

    #[must_use]
    pub fn slot(&self, key: &str) -> &InputSlot {
        self.slots.get(key).unwrap_or(&InputSlot::Absent)
    }

    /// The geometry on a single-arity port, if connected.
    #[must_use]
    pub fn geometry(&self, key: &str) -> Option<&Arc<GeometrySet>> {
        match self.slot(key) {
            InputSlot::Single(v) => v.as_geometry(),
            _ => None,
        }
    }

    /// The geometries on a variadic port, in port order, with holes skipped.
    /// The right accessor for concatenating consumers like `merge`.
    #[must_use]
    pub fn geometry_list(&self, key: &str) -> Vec<&Arc<GeometrySet>> {
        match self.slot(key) {
            InputSlot::Variadic(values) => values
                .iter()
                .filter_map(|v| v.as_ref().and_then(Value::as_geometry))
                .collect(),
            InputSlot::Single(v) => v.as_geometry().into_iter().collect(),
            InputSlot::Absent => Vec::new(),
        }
    }

    /// The geometries on a variadic port, in port order, **with holes kept**.
    /// One entry per connected wire, so `slots[i]` is the *i*th wire whatever
    /// happened upstream of the others. The right accessor for consumers that
    /// select by index (`switch`); see [`InputSlot::Variadic`].
    #[must_use]
    pub fn geometry_slots(&self, key: &str) -> Vec<Option<&Arc<GeometrySet>>> {
        match self.slot(key) {
            InputSlot::Variadic(values) => values
                .iter()
                .map(|v| v.as_ref().and_then(Value::as_geometry))
                .collect(),
            InputSlot::Single(v) => vec![v.as_geometry()],
            InputSlot::Absent => Vec::new(),
        }
    }

    /// The image on a single-arity port, if connected.
    #[must_use]
    pub fn image(&self, key: &str) -> Option<&Arc<solarxy_core::RawImageData>> {
        match self.slot(key) {
            InputSlot::Single(v) => v.as_image(),
            _ => None,
        }
    }

    /// The material on a single-arity port, if connected.
    #[must_use]
    pub fn material(&self, key: &str) -> Option<&Arc<solarxy_core::RawMaterialData>> {
        match self.slot(key) {
            InputSlot::Single(v) => v.as_material(),
            _ => None,
        }
    }
}

/// A compute body's outputs, keyed by output-port key.
#[derive(Debug, Clone, Default)]
pub struct Outputs {
    values: BTreeMap<String, Value>,
}

impl Outputs {
    /// The one-geometry-output convenience (key `geometry`, the catalog's
    /// default output on every geometry node).
    #[must_use]
    pub fn geometry(set: GeometrySet) -> Self {
        Self::single("geometry", Value::Geometry(Arc::new(set)))
    }

    #[must_use]
    pub fn single(key: impl Into<String>, value: Value) -> Self {
        let mut values = BTreeMap::new();
        values.insert(key.into(), value);
        Self { values }
    }

    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, key: impl Into<String>, value: Value) {
        self.values.insert(key.into(), value);
    }

    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// Whether the default geometry output is renderable-empty (the
    /// keep-last-good test). Keep-last-good is a geometry-only semantic
    /// (the transiently-empty-input case), so outputs that carry no
    /// geometry value never qualify: an image-only commit must replace
    /// the cache (a re-decode would otherwise be silently swallowed by
    /// the stale first commit), and a fully empty commit (mute bypass,
    /// pass-through with nothing connected) must replace so the node
    /// actually stops contributing downstream. The `geometry` key is the
    /// registry-invariant-pinned default-output name (see
    /// `every_geometry_producing_node_has_a_default_geometry_output`),
    /// so keying on it is keying on the descriptor's declaration.
    #[must_use]
    pub fn is_renderable_empty(&self) -> bool {
        match self.values.get("geometry") {
            Some(Value::Geometry(set)) => set.is_renderable_empty(),
            _ => false,
        }
    }
}

/// A cook failure: **data**, not a command error. It becomes the node's
/// error badge (`CookStatus::Error`); the topological pass continues
/// downstream, matching the Minimystix error-propagation model.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CookError {
    /// A required input has no edge. Keep-last-good deliberately does not
    /// apply.
    #[error("input '{port}' is required")]
    InputRequired { port: String },

    /// Param resolution refused (v1 expression refusal).
    #[error("{0}")]
    Params(String),

    /// Node-specific failure (parse error, kernel error, missing asset).
    #[error("{message}")]
    Failed { message: String },
}

/// What a cook produced: finished outputs, or a pending async job whose
/// result arrives later through the generation guard.
#[derive(Debug)]
pub enum CookOutcome {
    Done(Outputs),
    /// The node is cooking asynchronously; the engine parks it as
    /// `Pending` and resumes on `submit_job_result` (stale results are
    /// dropped by the per-node generation token).
    Pending(JobRequest),
}

/// The resolved per-import finishing options that ride along with a parse
/// job so the fulfiller (native inline, or the web import worker) returns
/// already-finished geometry: the common uniform scale and recenter, plus
/// the format-specific toggles. A format-specific field is `Some` only for
/// the format that declares it (`recompute_normals` on STL,
/// `preserve_materials` on glTF). PLY's `vertex_colors` is reserved until
/// the renderer grows a per-vertex color channel, so it is not carried yet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOptions {
    pub scale: f32,
    pub center_to_origin: bool,
    pub recompute_normals: Option<bool>,
    pub preserve_materials: Option<bool>,
}

/// One async work order (the import worker protocol; runs the
/// execution in a real web worker). The `options` travel with the request
/// so the worker's parsed result is finished (scaled, recentered, toggles
/// applied) before it transfers back, keeping the native and web paths
/// bit-identical.
#[derive(Debug, Clone)]
pub enum JobRequest {
    /// Parse a staged model file into a finished geometry set.
    ParseModel {
        asset: AssetId,
        /// Lowercase extension without the dot (`obj`, `gltf`, `glb`,
        /// `stl`, `ply`).
        format: String,
        /// The resolved finishing options for this import.
        options: ImportOptions,
    },
    /// Validate cooked geometry off-thread (the validate node above its
    /// inline triangle threshold). The geometry rides as an `Arc` so the
    /// request costs a pointer; the web host packs it into a transfer
    /// blob for the worker, the native path validates it directly.
    ValidateGeometry {
        geometry: Arc<GeometrySet>,
        config: ValidationConfig,
        /// The resolved triangle budget, when the budget check is on.
        budget: Option<u32>,
    },
    /// Decode a staged encoded image (`import_image`). The web
    /// worker decodes via `createImageBitmap`; the native path decodes
    /// inline through the shared formats decoder.
    DecodeImage { asset: AssetId },
}

impl PartialEq for JobRequest {
    /// Structural equality; the `ValidateGeometry` geometry compares by
    /// `Arc` identity (a `GeometrySet` has no cheap content equality).
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                Self::ParseModel {
                    asset: a,
                    format: f,
                    options: o,
                },
                Self::ParseModel {
                    asset: a2,
                    format: f2,
                    options: o2,
                },
            ) => a == a2 && f == f2 && o == o2,
            (
                Self::ValidateGeometry {
                    geometry: g,
                    config: c,
                    budget: b,
                },
                Self::ValidateGeometry {
                    geometry: g2,
                    config: c2,
                    budget: b2,
                },
            ) => Arc::ptr_eq(g, g2) && c == c2 && b == b2,
            (Self::DecodeImage { asset: a }, Self::DecodeImage { asset: a2 }) => a == a2,
            _ => false,
        }
    }
}

/// A fulfilled parse: the finished geometry plus the implicit load-time
/// validation run beside the parse (same code path as the desktop viewer's
/// load validation), so import nodes badge issues with zero extra plumbing.
#[derive(Debug)]
pub struct ParsedModel {
    pub set: GeometrySet,
    pub validation: Option<ValidationResult>,
}

/// One async work result, submitted back under the generation guard.
#[derive(Debug)]
pub enum JobResult {
    /// The parsed model (plus its implicit validation), or the parse
    /// failure message.
    Model(Result<ParsedModel, String>),
    /// A fulfilled `ValidateGeometry`: the full validation result (the
    /// parked validate node's passthrough geometry is retained engine-side,
    /// so only the result crosses back).
    Report(Result<ValidationResult, String>),
    /// A fulfilled `DecodeImage`: decoded RGBA8 pixels (hash stamped at
    /// construction), or the decode failure message.
    Image(Result<Arc<solarxy_core::RawImageData>, String>),
}

/// Job handle relating a spawned request to its later result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct JobId(pub u64);

/// What a compute body may reach during a cook. Cook statistics are
/// derived by the engine from the committed outputs (duration, points,
/// prims, meshes, bounds), so no manual stats sink is needed for the MVP
/// catalog.
pub struct CookCtx<'a> {
    /// The staged-asset table (import nodes read file bytes here).
    pub assets: &'a AssetTable,
    /// Whether async cooking is available. When true, import-style nodes
    /// should return [`CookOutcome::Pending`] with a [`JobRequest`];
    /// when false (native), they parse inline.
    pub async_jobs: bool,
    warnings: Vec<String>,
    /// Side-channel for a cook that ran validation (the validate node's
    /// inline path, an import's implicit load validation): the driver
    /// drains it on commit into its per-node validation cache, which
    /// feeds node badges and the per-object overlay lowering.
    validation: Option<ValidationResult>,
    /// The published values of networks this node references through its
    /// `NodePath` params, pre-resolved by the driver before the compute
    /// runs (keyed by the referenced container's id). The engine cooks
    /// referenced networks first (`ordered_contexts`), so these are fresh
    /// within the pass. Empty for nodes without references.
    referenced: BTreeMap<crate::document::NodeId, Value>,
}

impl<'a> CookCtx<'a> {
    #[must_use]
    pub fn new(assets: &'a AssetTable, async_jobs: bool) -> Self {
        Self {
            assets,
            async_jobs,
            warnings: Vec::new(),
            validation: None,
            referenced: BTreeMap::new(),
        }
    }

    /// Installs the pre-resolved referenced values (driver-side).
    pub fn set_referenced(&mut self, referenced: BTreeMap<crate::document::NodeId, Value>) {
        self.referenced = referenced;
    }

    /// The published value of a referenced network, if the reference
    /// resolves (the target exists, its network designates a display node,
    /// and that node has a committed value). A cook that requires the
    /// reference treats `None` as a dangling-reference [`CookError`].
    #[must_use]
    pub fn referenced(&self, target: crate::document::NodeId) -> Option<&Value> {
        self.referenced.get(&target)
    }

    /// Records a non-fatal warning (badges the node without failing the
    /// cook; e.g. merge's empty-output warning).
    pub fn warn(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    /// Drains the warnings gathered during one cook.
    #[must_use]
    pub fn take_warnings(&mut self) -> Vec<String> {
        std::mem::take(&mut self.warnings)
    }

    /// Records the validation result this cook produced (validate node,
    /// import load validation). The driver caches it per node on commit.
    pub fn set_validation(&mut self, validation: ValidationResult) {
        self.validation = Some(validation);
    }

    /// Drains the validation result recorded during one cook.
    #[must_use]
    pub fn take_validation(&mut self) -> Option<ValidationResult> {
        self.validation.take()
    }
}

/// The per-cook signature every node implements.
pub type CookFn = fn(
    &crate::registry::resolve::ResolvedParams,
    &Inputs,
    &mut CookCtx,
) -> Result<CookOutcome, CookError>;
