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
    Variadic(Vec<Value>),
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

    /// The geometries on a variadic port, in port order.
    #[must_use]
    pub fn geometry_list(&self, key: &str) -> Vec<&Arc<GeometrySet>> {
        match self.slot(key) {
            InputSlot::Variadic(values) => values.iter().filter_map(Value::as_geometry).collect(),
            InputSlot::Single(v) => v.as_geometry().into_iter().collect(),
            InputSlot::Absent => Vec::new(),
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
    /// keep-last-good test). Non-geometry and absent outputs count as
    /// empty.
    #[must_use]
    pub fn is_renderable_empty(&self) -> bool {
        match self.values.get("geometry") {
            Some(Value::Geometry(set)) => set.is_renderable_empty(),
            _ => true,
        }
    }
}

/// A cook failure: **data**, not a command error. It becomes the node's
/// error badge (`CookStatus::Error`); the topological pass continues
/// downstream, matching the Minimystix error-propagation model.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum CookError {
    /// A required input has no edge. Keep-last-good deliberately does not
    /// apply (catalog section 3).
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

/// One async work order (the import worker protocol; Phase 5 moves the
/// execution to a real web worker, the protocol is frozen here).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobRequest {
    /// Parse a staged model file into raw model data.
    ParseModel {
        asset: AssetId,
        /// Lowercase extension without the dot (`obj`, `gltf`, `glb`,
        /// `stl`, `ply`).
        format: String,
    },
}

/// One async work result, submitted back under the generation guard.
#[derive(Debug)]
pub enum JobResult {
    /// The parsed model as a geometry set, or the parse failure message.
    Model(Result<GeometrySet, String>),
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
    /// when false (native Phase 3), they parse inline.
    pub async_jobs: bool,
    warnings: Vec<String>,
}

impl<'a> CookCtx<'a> {
    #[must_use]
    pub fn new(assets: &'a AssetTable, async_jobs: bool) -> Self {
        Self {
            assets,
            async_jobs,
            warnings: Vec::new(),
        }
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
}

/// The per-cook signature every node implements.
pub type CookFn = fn(
    &crate::registry::resolve::ResolvedParams,
    &Inputs,
    &mut CookCtx,
) -> Result<CookOutcome, CookError>;
