//! The cook driver: the budgeted, resumable engine that turns a dirty set
//! into committed node outputs.
//!
//! State model (the key design choice): each node carries a persistent
//! [`CookState`] (`Clean` / `Dirty` / `Pending`), and the cook set is that
//! state plus the memoized topological order, never a consumable queue
//! with a cursor. Re-dirtying an already-passed node just re-marks it
//! `Dirty` for the next budget slice, so resuming across frames is correct
//! by construction: there is no cursor to invalidate.
//!
//! Ported Minimystix semantics: keep-last-good on renderable-empty output,
//! per-node async generation guards, dirty-union plus transitive
//! downstream recook in topological order, and cone gating to the active
//! display node. Deliberate catalog deltas: a required unconnected input
//! is a hard `InputRequired` error with keep-last-good NOT applied
//! (explicit disconnect is user intent), while a connected-but-empty
//! upstream flows through keep-last-good; and wire values coerce through
//! the enforced matrix at gather time.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use super::state::{CookState, CookStatus, NodeCookStats};
use super::{CookCtx, CookError, CookOutcome, InputSlot, Inputs, JobId, JobRequest, Outputs};
use crate::assets::AssetTable;
use crate::document::{Document, Graph, GraphContext, NodeId};
use crate::registry::coerce::{Value, coerce_value};
use crate::registry::resolve::resolve_params;
use crate::registry::{Arity, BypassBehavior, Registry};

/// One in-flight async job, tagged with the generation it was spawned at.
/// The request itself travels to the host in [`CookReport::jobs`]; the
/// engine only needs to relate the result back to its node and generation.
#[derive(Debug, Clone)]
struct PendingJob {
    node: NodeId,
    generation: u64,
}

/// What one cook pass did. `stats_changed` and `status_changed` are
/// coalesced against last-emitted state, so a caller can turn them into
/// `NodeStats` / `CookStatus` events without re-diffing.
#[derive(Debug, Default)]
pub struct CookReport {
    /// Nodes that ran a compute (or bypass resolve) this pass, in order.
    pub cooked: Vec<NodeId>,
    /// Still-dirty count after the pass (0 means the pass drained; > 0
    /// means the budget stopped it and another pass is needed).
    pub remaining_dirty: usize,
    /// Nodes whose geometry stats changed (duration ignored).
    pub stats_changed: Vec<(NodeId, NodeCookStats)>,
    /// Nodes whose badge status changed.
    pub status_changed: Vec<(NodeId, CookStatus)>,
    /// Async jobs to dispatch (host posts them to the worker; native
    /// callers resolve them synchronously and feed `submit_job_result`).
    pub jobs: Vec<(JobId, JobRequest)>,
}

/// Per-node cook bookkeeping and the keep-last-good output cache. Holds
/// state for every context (node ids are document-unique). Borrows the
/// document, registry, and asset table per cook; owns nothing structural.
#[derive(Debug, Default)]
pub struct CookEngine {
    state: BTreeMap<NodeId, CookState>,
    /// Keep-last-good output cache: a node's last committed outputs.
    outputs: BTreeMap<NodeId, Arc<Outputs>>,
    status: BTreeMap<NodeId, CookStatus>,
    stats: BTreeMap<NodeId, NodeCookStats>,
    /// Monotonic per-node generation, bumped on every (re)cook or
    /// (re)spawn; the async stale-drop compares against it.
    generation: BTreeMap<NodeId, u64>,
    jobs: BTreeMap<JobId, PendingJob>,
    next_job: u64,
    /// Whether cook bodies may offload to async jobs (true on web with an
    /// import worker; false natively, where imports parse inline).
    async_jobs: bool,
    /// Optional host wall-clock, in milliseconds. A `fn` pointer (not a
    /// closure) so the driver stays wasm-safe and the struct stays
    /// `Debug`/`Default`: the web host installs `performance.now`, native
    /// callers a monotonic source, and tests a deterministic tick. When
    /// unset, per-node cook durations stay `0` (the Phase-3 behavior).
    clock: Option<fn() -> f64>,
}

impl CookEngine {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables async job offloading (the web import worker). Off by
    /// default; native cooks parse imports inline.
    pub fn set_async_jobs(&mut self, enabled: bool) {
        self.async_jobs = enabled;
    }

    /// Installs a host wall-clock (milliseconds) so successful cooks report
    /// their real duration in `CookStatus::Ok { ms }` and `NodeCookStats`.
    /// Without one, durations stay `0`.
    pub fn set_clock(&mut self, clock: fn() -> f64) {
        self.clock = Some(clock);
    }

    /// The current host time in milliseconds, or `0.0` when no clock is
    /// installed.
    fn now(&self) -> f64 {
        self.clock.map_or(0.0, |f| f())
    }

    /// Clears all per-node cook bookkeeping (state, cached outputs, status,
    /// stats, generations, in-flight jobs) while preserving configuration
    /// (the async-jobs flag and the installed clock). Used when a whole new
    /// document is loaded, so the next cook rebuilds every node from clean.
    pub fn reset(&mut self) {
        self.state.clear();
        self.outputs.clear();
        self.status.clear();
        self.stats.clear();
        self.generation.clear();
        self.jobs.clear();
        self.next_job = 0;
    }

    /// Registers a freshly added node (starts `Dirty`).
    pub fn insert_node(&mut self, node: NodeId) {
        self.state.entry(node).or_default();
        self.generation.entry(node).or_insert(0);
    }

    /// Drops all bookkeeping for a removed node.
    pub fn forget_node(&mut self, node: NodeId) {
        self.state.remove(&node);
        self.outputs.remove(&node);
        self.status.remove(&node);
        self.stats.remove(&node);
        self.generation.remove(&node);
        self.jobs.retain(|_, j| j.node != node);
    }

    /// Marks a node and its transitive downstream `Dirty` (the dirty-union
    /// the cook pass then walks in topological order). Bumps the node's
    /// generation so any in-flight async result for it becomes stale.
    pub fn mark_dirty(&mut self, graph: &Graph, node: NodeId) {
        *self.generation.entry(node).or_insert(0) += 1;
        self.state.insert(node, CookState::Dirty);
        for down in graph.downstream(node) {
            self.state.insert(down, CookState::Dirty);
        }
    }

    /// The last committed outputs for a node, if any.
    #[must_use]
    pub fn outputs(&self, node: NodeId) -> Option<&Arc<Outputs>> {
        self.outputs.get(&node)
    }

    #[must_use]
    pub fn status(&self, node: NodeId) -> Option<&CookStatus> {
        self.status.get(&node)
    }

    #[must_use]
    pub fn stats(&self, node: NodeId) -> Option<&NodeCookStats> {
        self.stats.get(&node)
    }

    #[must_use]
    pub fn state(&self, node: NodeId) -> CookState {
        self.state.get(&node).copied().unwrap_or_default()
    }

    /// Cooks a context's dirty work set until `should_continue` returns
    /// false, always cooking at least one node per call (forward
    /// progress). The work set is the active display node's predecessor
    /// cone intersected with the dirty set for a subflow, or the whole
    /// dirty set for the root (lights cook display-independently). Nodes
    /// outside the displayed cone never consume budget.
    pub fn cook_until(
        &mut self,
        doc: &Document,
        registry: &Registry,
        assets: &AssetTable,
        ctx: GraphContext,
        should_continue: &mut dyn FnMut() -> bool,
    ) -> CookReport {
        let mut report = CookReport::default();
        let Ok(graph) = doc.graph(ctx) else {
            return report;
        };

        let work = self.work_set(graph);
        let ordered = {
            let mut g = graph.clone();
            g.topological_filter(&work)
        };

        for node in ordered {
            // Skip nodes that are not dirty (Pending nodes await their
            // async result; Clean nodes are current).
            if self.state(node) != CookState::Dirty {
                continue;
            }
            // Forward progress: always cook the first eligible node; only
            // then honor the budget predicate.
            if !report.cooked.is_empty() && !should_continue() {
                break;
            }
            self.cook_one(graph, registry, assets, node, &mut report);
        }

        report.remaining_dirty = self
            .state
            .iter()
            .filter(|(id, s)| **s == CookState::Dirty && work.contains(id))
            .count();
        report
    }

    /// The nodes eligible to cook in a context: a subflow gates to the
    /// active display cone; the root cooks everything dirty (additive
    /// display, lights cook independently).
    fn work_set(&self, graph: &Graph) -> BTreeSet<NodeId> {
        let dirty: BTreeSet<NodeId> = graph
            .nodes()
            .map(|n| n.id)
            .filter(|id| self.state(*id) == CookState::Dirty)
            .collect();
        match graph.active_output {
            Some(output) => graph
                .predecessor_cone(output)
                .intersection(&dirty)
                .copied()
                .collect(),
            None => dirty,
        }
    }

    /// Cooks (or bypass-resolves) one node and commits the result.
    fn cook_one(
        &mut self,
        graph: &Graph,
        registry: &Registry,
        assets: &AssetTable,
        node: NodeId,
        report: &mut CookReport,
    ) {
        let Some(data) = graph.node(node) else {
            return;
        };
        // A placeholder (too-new or unknown-type node) refuses to cook, so
        // the document is never destroyed by loading it.
        if let Some(reason) = &data.placeholder {
            let reason = reason.clone();
            report.cooked.push(node);
            self.set_status(node, CookStatus::Error { message: reason }, report);
            self.state.insert(node, CookState::Clean);
            return;
        }
        let Some(desc) = registry.get(&data.type_id) else {
            // Unknown / placeholder node: refuses to cook, stays as-is.
            self.set_status(
                node,
                CookStatus::Error {
                    message: format!("unknown node type '{}'", data.type_id),
                },
                report,
            );
            self.state.insert(node, CookState::Clean);
            return;
        };
        report.cooked.push(node);
        let generation = *self.generation.entry(node).or_insert(0);

        // Bypass short-circuits the compute entirely.
        if data.bypassed {
            let start = self.now();
            let outcome = self.resolve_bypass(graph, desc, node);
            let elapsed = self.now() - start;
            self.commit_outputs(node, outcome, elapsed, report);
            self.state.insert(node, CookState::Clean);
            return;
        }

        // Gather inputs (required-unconnected is the hard error).
        let inputs = match self.gather(graph, desc, node) {
            Ok(inputs) => inputs,
            Err(err) => {
                self.commit_error(node, &err, report);
                self.state.insert(node, CookState::Clean);
                return;
            }
        };

        // Resolve params (v1 refuses expressions).
        let resolved = match resolve_params(&data.params, &desc.params) {
            Ok(r) => r,
            Err(fail) => {
                self.commit_error(node, &CookError::Params(fail.to_string()), report);
                self.state.insert(node, CookState::Clean);
                return;
            }
        };

        // Compute (timed against the host clock, if installed).
        let mut cx = CookCtx::new(assets, self.async_jobs);
        let start = self.now();
        let outcome = (desc.cook)(&resolved, &inputs, &mut cx);
        let elapsed = self.now() - start;
        match outcome {
            Ok(CookOutcome::Done(outputs)) => {
                self.commit_outputs(node, outputs, elapsed, report);
                self.state.insert(node, CookState::Clean);
            }
            Ok(CookOutcome::Pending(request)) => {
                // Park the node; its result must match this generation.
                let job = JobId(self.next_job);
                self.next_job += 1;
                self.jobs.insert(job, PendingJob { node, generation });
                self.state.insert(node, CookState::Pending(generation));
                self.set_status(node, CookStatus::Pending, report);
                report.jobs.push((job, request));
            }
            Err(err) => {
                self.commit_error(node, &err, report);
                self.state.insert(node, CookState::Clean);
            }
        }
    }

    /// Resolves a bypassed node's output: pass-through copies the target
    /// input (first connected sub-input for a variadic port); mute emits
    /// empty. A pass-through node with nothing connected emits empty.
    fn resolve_bypass(
        &self,
        graph: &Graph,
        desc: &crate::registry::NodeTypeDescriptor,
        node: NodeId,
    ) -> Outputs {
        let out_key = desc.default_output().map_or("geometry", |p| p.key.as_str());
        match &desc.bypass {
            BypassBehavior::PassThrough { input } => {
                let value = self.first_input_value(graph, desc, node, input);
                value.map_or_else(Outputs::empty, |v| Outputs::single(out_key, v))
            }
            BypassBehavior::Mute | BypassBehavior::NotBypassable => Outputs::empty(),
        }
    }

    /// The first connected, coerced value on an input port (single or
    /// variadic), reading upstream cached outputs.
    fn first_input_value(
        &self,
        graph: &Graph,
        desc: &crate::registry::NodeTypeDescriptor,
        node: NodeId,
        port: &str,
    ) -> Option<Value> {
        let spec = desc.input(port)?;
        for edge in graph.incoming_to_port(node, port) {
            if let Some(value) = self.upstream_value(edge.from, &edge.from_port)
                && let Some(coerced) = coerce_value(&value, spec.data_type)
            {
                return Some(coerced);
            }
        }
        None
    }

    /// Gathers every input port into an [`Inputs`], applying wire
    /// coercion. A required single port with no edge is `InputRequired`
    /// (checked here against the graph, distinct from an edge to an empty
    /// upstream, which yields an `Absent` slot without erroring).
    fn gather(
        &self,
        graph: &Graph,
        desc: &crate::registry::NodeTypeDescriptor,
        node: NodeId,
    ) -> Result<Inputs, CookError> {
        let mut slots = BTreeMap::new();
        for port in &desc.inputs {
            let edges = graph.incoming_to_port(node, &port.key);
            match port.arity {
                Arity::Single { required } => {
                    if required && edges.is_empty() {
                        return Err(CookError::InputRequired {
                            port: port.key.clone(),
                        });
                    }
                    let value = edges.first().and_then(|e| {
                        self.upstream_value(e.from, &e.from_port)
                            .and_then(|v| coerce_value(&v, port.data_type))
                    });
                    slots.insert(
                        port.key.clone(),
                        value.map_or(InputSlot::Absent, InputSlot::Single),
                    );
                }
                Arity::Variadic { .. } => {
                    let values: Vec<Value> = edges
                        .iter()
                        .filter_map(|e| {
                            self.upstream_value(e.from, &e.from_port)
                                .and_then(|v| coerce_value(&v, port.data_type))
                        })
                        .collect();
                    slots.insert(port.key.clone(), InputSlot::Variadic(values));
                }
            }
        }
        Ok(Inputs::new(slots))
    }

    /// The cached value on an upstream node's output port, if the node has
    /// committed outputs.
    fn upstream_value(&self, from: NodeId, from_port: &str) -> Option<Value> {
        self.outputs.get(&from)?.get(from_port).cloned()
    }

    /// Commits a successful cook's outputs with keep-last-good: a
    /// renderable-empty result retains the previous output (and would
    /// badge a warning); a non-empty result replaces it. `elapsed_ms` is the
    /// host-clocked compute time (0.0 when no clock is installed).
    fn commit_outputs(
        &mut self,
        node: NodeId,
        outputs: Outputs,
        elapsed_ms: f64,
        report: &mut CookReport,
    ) {
        let keep_last_good = outputs.is_renderable_empty() && self.outputs.contains_key(&node);
        if !keep_last_good {
            self.outputs.insert(node, Arc::new(outputs));
        }
        self.emit_stats(node, elapsed_ms, report);
        self.set_status(node, CookStatus::Ok { ms: elapsed_ms }, report);
    }

    /// Commits a cook failure. `InputRequired` clears the cache (explicit
    /// disconnect is user intent, no keep-last-good); every other error
    /// keeps the last good geometry in the viewport (transient failure).
    fn commit_error(&mut self, node: NodeId, err: &CookError, report: &mut CookReport) {
        if matches!(err, CookError::InputRequired { .. }) {
            self.outputs.remove(&node);
        }
        self.set_status(
            node,
            CookStatus::Error {
                message: err.to_string(),
            },
            report,
        );
    }

    /// Submits an async job's result under the generation guard. A result
    /// whose token no longer matches the node's current generation (a
    /// newer cook or a re-dirty happened meanwhile) is dropped. On accept,
    /// commits the geometry, clears `Pending`, and marks downstream dirty.
    pub fn submit_job_result(
        &mut self,
        graph: &Graph,
        job: JobId,
        result: super::JobResult,
    ) -> CookReport {
        let mut report = CookReport::default();
        let Some(pending) = self.jobs.remove(&job) else {
            return report;
        };
        let node = pending.node;
        let current = self.generation.get(&node).copied().unwrap_or(0);
        // Stale: a newer generation superseded this job. Drop it.
        if pending.generation != current {
            return report;
        }
        // Only resurrect if still parked on this generation.
        if self.state(node) != CookState::Pending(pending.generation) {
            return report;
        }

        let super::JobResult::Model(model) = result;
        match model {
            Ok(set) => {
                // Async wall-time is not charged to a cook budget; the
                // commit itself is effectively instantaneous.
                self.commit_outputs(node, Outputs::geometry(set), 0.0, &mut report);
            }
            Err(message) => {
                self.commit_error(node, &CookError::Failed { message }, &mut report);
            }
        }
        self.state.insert(node, CookState::Clean);
        report.cooked.push(node);
        // Downstream consumers must recook against the new output.
        for down in graph.downstream(node) {
            self.state.insert(down, CookState::Dirty);
        }
        report
    }

    /// Emits a coalesced stats change (geometry shape only; duration is
    /// ignored by `same_shape` so an unchanged mesh does not spam events).
    /// `elapsed_ms` records the last cook's wall-time on the stored stats
    /// for the info popover; it is `0.0` when no clock is installed.
    fn emit_stats(&mut self, node: NodeId, elapsed_ms: f64, report: &mut CookReport) {
        let mut stats = self.outputs.get(&node).map_or(
            NodeCookStats {
                duration_us: 0,
                points: 0,
                prims: 0,
                meshes: 0,
                bounds: None,
            },
            |o| stats_from_outputs(o),
        );
        stats.duration_us = (elapsed_ms * 1000.0).max(0.0).round() as u64;
        let changed = self
            .stats
            .get(&node)
            .is_none_or(|prev| !prev.same_shape(&stats));
        self.stats.insert(node, stats);
        if changed {
            report.stats_changed.push((node, stats));
        }
    }

    fn set_status(&mut self, node: NodeId, status: CookStatus, report: &mut CookReport) {
        let changed = self.status.get(&node) != Some(&status);
        self.status.insert(node, status.clone());
        if changed {
            report.status_changed.push((node, status));
        }
    }
}

/// Derives cook statistics from a node's committed outputs.
fn stats_from_outputs(outputs: &Outputs) -> NodeCookStats {
    if let Some(Value::Geometry(set)) = outputs.get("geometry") {
        NodeCookStats {
            duration_us: 0,
            points: set.point_count(),
            prims: set.triangle_count(),
            meshes: set.mesh_count(),
            bounds: Some(set.bounds),
        }
    } else {
        NodeCookStats {
            duration_us: 0,
            points: 0,
            prims: 0,
            meshes: 0,
            bounds: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cook::state::CookState;
    use crate::document::{Edge, NodeData};
    use crate::params::{ParamSource, ParamValue};
    use crate::registry::coerce::DataType;
    use crate::registry::param_spec::{ParamSpec, ParamType};
    use crate::registry::resolve::ResolvedParams;
    use crate::registry::{
        BypassBehavior, Category, ContextMask, NodeTypeDescriptor, PortSpec, Registry,
    };
    use solarxy_kernel::primitives::generate_box;
    use solarxy_kernel::{GeometrySet, KernelMesh};

    // A generator whose `empty` param decides whether it emits a box or an
    // empty set (to drive keep-last-good). Signature matches CookFn.
    #[allow(clippy::unnecessary_wraps)]
    fn gen_cook(
        p: &ResolvedParams,
        _in: &Inputs,
        _cx: &mut CookCtx,
    ) -> Result<CookOutcome, CookError> {
        let set = if p.bool("empty") {
            GeometrySet::empty()
        } else {
            let size = p.f32("size");
            GeometrySet::from_mesh(generate_box(size, size, size, 1, 1, 1))
        };
        Ok(CookOutcome::Done(Outputs::geometry(set)))
    }

    // A required-input passthrough; empty input -> empty output.
    // Signature matches CookFn.
    #[allow(clippy::unnecessary_wraps)]
    fn pass_cook(
        _p: &ResolvedParams,
        inputs: &Inputs,
        _cx: &mut CookCtx,
    ) -> Result<CookOutcome, CookError> {
        match inputs.geometry("geometry") {
            Some(set) => Ok(CookOutcome::Done(Outputs::geometry((**set).clone()))),
            None => Ok(CookOutcome::Done(Outputs::geometry(GeometrySet::empty()))),
        }
    }

    fn registry() -> Registry {
        let gen_desc = NodeTypeDescriptor {
            type_id: "gen",
            version: 1,
            display_name: "Gen",
            category: Category::Primitives,
            contexts: ContextMask::SUBFLOW,
            inputs: vec![],
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
                .hard(0.01, 100.0),
                ParamSpec::new(
                    "empty",
                    "Empty",
                    "geometry",
                    ParamType::Bool,
                    ParamValue::Bool(false),
                ),
            ],
            bypass: BypassBehavior::Mute,
            doc: "",
            search_aliases: &[],
            cook: gen_cook,
            migrate: None,
        };
        let pass = NodeTypeDescriptor {
            type_id: "pass",
            version: 1,
            display_name: "Pass",
            category: Category::Modifiers,
            contexts: ContextMask::SUBFLOW,
            inputs: vec![
                PortSpec::single("geometry", "Geometry", DataType::Geometry, true).default_port(),
            ],
            outputs: vec![
                PortSpec::single("geometry", "Geometry", DataType::Geometry, false).default_port(),
            ],
            params: vec![],
            bypass: BypassBehavior::PassThrough {
                input: "geometry".to_string(),
            },
            doc: "",
            search_aliases: &[],
            cook: pass_cook,
            migrate: None,
        };
        Registry::with_descriptors(vec![gen_desc, pass]).unwrap()
    }

    struct Fixture {
        doc: Document,
        engine: CookEngine,
        registry: Registry,
        assets: AssetTable,
        ctx: GraphContext,
    }

    impl Fixture {
        fn new() -> Self {
            let mut doc = Document::new();
            let geo = doc.mint_node_id();
            doc.create_subflow(geo);
            Self {
                doc,
                engine: CookEngine::new(),
                registry: registry(),
                assets: AssetTable::new(),
                ctx: GraphContext::Subflow(geo),
            }
        }

        fn add(&mut self, type_id: &str) -> NodeId {
            let id = self.doc.mint_node_id();
            let g = self.doc.graph_mut(self.ctx).unwrap();
            g.add_node(NodeData::new(id, type_id, 1));
            self.engine.insert_node(id);
            self.engine
                .mark_dirty(self.doc.graph(self.ctx).unwrap(), id);
            id
        }

        fn set_param(&mut self, node: NodeId, key: &str, value: ParamValue) {
            let g = self.doc.graph_mut(self.ctx).unwrap();
            g.node_mut(node)
                .unwrap()
                .params
                .insert(key.to_string(), ParamSource::Literal(value));
            self.engine
                .mark_dirty(self.doc.graph(self.ctx).unwrap(), node);
        }

        fn connect(&mut self, from: NodeId, to: NodeId) {
            let eid = self.doc.mint_edge_id();
            let g = self.doc.graph_mut(self.ctx).unwrap();
            g.connect(
                Edge {
                    id: eid,
                    from,
                    from_port: "geometry".to_string(),
                    to,
                    to_port: "geometry".to_string(),
                },
                false,
            )
            .unwrap();
            self.engine
                .mark_dirty(self.doc.graph(self.ctx).unwrap(), to);
        }

        fn set_display(&mut self, node: NodeId) {
            self.doc.graph_mut(self.ctx).unwrap().active_output = Some(node);
        }

        fn cook_all(&mut self) -> CookReport {
            self.engine.cook_until(
                &self.doc,
                &self.registry,
                &self.assets,
                self.ctx,
                &mut || true,
            )
        }

        fn points(&self, node: NodeId) -> u64 {
            self.engine
                .outputs(node)
                .and_then(|o| o.get("geometry"))
                .and_then(Value::as_geometry)
                .map_or(0, |g| g.point_count())
        }
    }

    #[test]
    fn fresh_node_cooks_on_first_pass() {
        let mut f = Fixture::new();
        let g = f.add("gen");
        f.set_display(g);
        assert_eq!(f.engine.state(g), CookState::Dirty);
        f.cook_all();
        assert_eq!(f.engine.state(g), CookState::Clean);
        assert_eq!(f.points(g), 24); // a box
    }

    #[test]
    fn upstream_edit_propagates_downstream() {
        let mut f = Fixture::new();
        let g = f.add("gen");
        let p = f.add("pass");
        f.connect(g, p);
        f.set_display(p);
        f.cook_all();
        assert_eq!(f.points(p), 24);

        // Re-dirty the generator; the downstream pass must recook.
        f.set_param(g, "size", ParamValue::Float(2.0));
        assert_eq!(f.engine.state(p), CookState::Dirty);
        let report = f.cook_all();
        assert!(report.cooked.contains(&g));
        assert!(report.cooked.contains(&p));
        assert_eq!(f.points(p), 24); // still a box, just bigger
    }

    #[test]
    fn keep_last_good_retains_previous_on_transient_empty() {
        let mut f = Fixture::new();
        let g = f.add("gen");
        f.set_display(g);
        f.cook_all();
        assert_eq!(f.points(g), 24);

        // Flip to empty: keep-last-good retains the box.
        f.set_param(g, "empty", ParamValue::Bool(true));
        f.cook_all();
        assert_eq!(f.points(g), 24, "keep-last-good should retain the box");

        // Flip back to non-empty: the fresh box commits.
        f.set_param(g, "empty", ParamValue::Bool(false));
        f.cook_all();
        assert_eq!(f.points(g), 24);
    }

    #[test]
    fn required_input_unconnected_is_a_hard_error_with_no_keep_last_good() {
        let mut f = Fixture::new();
        let g = f.add("gen");
        let p = f.add("pass");
        f.connect(g, p);
        f.set_display(p);
        f.cook_all();
        assert_eq!(f.points(p), 24);

        // Disconnect the required input: p must error and NOT keep the box.
        let edge_id = f.doc.graph(f.ctx).unwrap().edges().next().unwrap().id;
        f.doc.graph_mut(f.ctx).unwrap().disconnect(edge_id).unwrap();
        f.engine.mark_dirty(f.doc.graph(f.ctx).unwrap(), p);
        f.cook_all();

        assert!(matches!(f.engine.status(p), Some(CookStatus::Error { .. })));
        assert_eq!(f.points(p), 0, "required-disconnect clears keep-last-good");
    }

    #[test]
    fn bypass_passthrough_copies_input_without_cooking() {
        let mut f = Fixture::new();
        let g = f.add("gen");
        let p = f.add("pass");
        f.connect(g, p);
        f.set_display(p);
        f.cook_all();

        // Bypass the pass node: it should still forward the box.
        f.doc
            .graph_mut(f.ctx)
            .unwrap()
            .node_mut(p)
            .unwrap()
            .bypassed = true;
        f.engine.mark_dirty(f.doc.graph(f.ctx).unwrap(), p);
        f.cook_all();
        assert_eq!(f.points(p), 24);
    }

    #[test]
    fn cone_gating_skips_nodes_outside_the_display_cone() {
        let mut f = Fixture::new();
        let shown = f.add("gen");
        let hidden = f.add("gen");
        f.set_display(shown);
        let report = f.cook_all();
        // Only the displayed generator cooked; the off-cone one did not.
        assert!(report.cooked.contains(&shown));
        assert!(!report.cooked.contains(&hidden));
        assert_eq!(f.engine.state(hidden), CookState::Dirty);
    }

    #[test]
    fn budget_is_resumable_across_passes() {
        let mut f = Fixture::new();
        // A chain gen -> pass -> pass -> pass so the cone has 4 nodes.
        let g = f.add("gen");
        let p1 = f.add("pass");
        let p2 = f.add("pass");
        let p3 = f.add("pass");
        f.connect(g, p1);
        f.connect(p1, p2);
        f.connect(p2, p3);
        f.set_display(p3);

        // Budget of one node per pass: forward progress cooks exactly one.
        let mut passes = 0;
        loop {
            let report = f.engine.cook_until(
                &f.doc,
                &f.registry,
                &f.assets,
                f.ctx,
                &mut || false, // stop after the forced first node
            );
            passes += 1;
            if report.cooked.is_empty() || report.remaining_dirty == 0 {
                break;
            }
            assert_eq!(report.cooked.len(), 1, "forward progress cooks one");
        }
        // Four nodes, one per pass.
        assert_eq!(passes, 4);
        assert_eq!(f.points(p3), 24);
    }

    #[test]
    fn stats_coalesce_and_report_only_on_change() {
        let mut f = Fixture::new();
        let g = f.add("gen");
        f.set_display(g);
        let report = f.cook_all();
        // First cook: stats change (from nothing).
        assert!(report.stats_changed.iter().any(|(id, _)| *id == g));

        // Re-cook with identical params: geometry shape unchanged, so no
        // stats event (duration is ignored by same_shape).
        f.engine.mark_dirty(f.doc.graph(f.ctx).unwrap(), g);
        let report = f.cook_all();
        assert!(
            !report.stats_changed.iter().any(|(id, _)| *id == g),
            "unchanged geometry must not emit a stats event"
        );

        // Change the size: stats change again.
        f.set_param(g, "size", ParamValue::Float(5.0));
        let report = f.cook_all();
        assert!(report.stats_changed.iter().any(|(id, _)| *id == g));
    }

    #[test]
    fn cooked_geometry_converts_to_scene_geometry() {
        // The output GeometrySet lowers to the renderer contract.
        let set = GeometrySet::from_mesh(KernelMesh::new(
            "m",
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        ));
        let cooked = set.to_cooked();
        assert_eq!(cooked.meshes.len(), 1);
    }
}
