//! The engine facade: the single entry point a shell (native or web)
//! drives. It owns the document, registry, cook engine, and asset table,
//! turns [`Command`]s into document mutations plus [`EngineEvent`]
//! batches, cooks under a budget, and lowers displayed geometry to a
//! `solarxy_core::scene::SceneDelta`.
//!
//! Rust owns all document state; a frontend mirrors it via events and
//! mutates only by dispatching commands (the mirror-and-command model).
//! The monotonic `revision` on every batch lets a desynced mirror recover
//! by taking a full [`snapshot`](Engine::snapshot).

pub mod snapshot;

use std::collections::BTreeMap;

use cgmath::{Matrix3, Matrix4, Point3, SquareMatrix, Transform, Vector3};
use serde::{Deserialize, Serialize};
use solarxy_core::scene::SceneDelta;
use solarxy_kernel::transform::{RotateOrder, rotation_matrix};

use crate::GraphError;
use crate::assets::AssetTable;
use crate::nodes::common::rotate_order_from_key;
use crate::registry::resolve::ResolvedParams;
use crate::cook::state::{CookState, CookStatus};
use crate::cook::{CookEngine, JobId, JobRequest, JobResult};
use crate::document::{Document, DocumentData, Edge, EdgeId, GraphContext, NodeData, NodeId, PortRef};
use crate::params::{AssetId, ParamSource, ParamValue};
use crate::registry::resolve::param_source_from_json;
use crate::registry::{Arity, Registry};

pub use snapshot::{AnnotationSnapshot, DocumentSnapshot, RegistrySnapshot};

mod scene;
mod scenefile;
mod undo;

pub use scenefile::{LoadedScene, SceneSidecar};

use undo::{Transaction, UndoOp, UndoStack};

/// Cook scheduling mode (node catalog / boundary design).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CookMode {
    /// Recook dirtied nodes on the next frame automatically.
    #[default]
    Auto,
    /// Accumulate a stale set; recook only on `CookNow`.
    Manual,
}

/// A command from the shell. The serde form is the wasm-boundary contract
/// (variant tags and all fields are camelCase for idiomatic JS); it
/// round-trips losslessly for clipboard and undo tests.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Command {
    AddNode {
        ctx: GraphContext,
        node_type: String,
        position: [f32; 2],
    },
    RemoveNodes {
        ctx: GraphContext,
        ids: Vec<NodeId>,
    },
    Connect {
        ctx: GraphContext,
        from: PortRefDto,
        to: PortRefDto,
    },
    Disconnect {
        ctx: GraphContext,
        edge: EdgeId,
    },
    SetParam {
        ctx: GraphContext,
        node: NodeId,
        key: String,
        value: ParamSource,
    },
    MoveNodes {
        ctx: GraphContext,
        moves: Vec<(NodeId, [f32; 2])>,
    },
    SetActiveOutput {
        ctx: GraphContext,
        node: Option<NodeId>,
    },
    SetSelection {
        ctx: GraphContext,
        ids: Vec<NodeId>,
    },
    SetBypass {
        ctx: GraphContext,
        node: NodeId,
        bypassed: bool,
    },
    ReorderVariadicInput {
        ctx: GraphContext,
        node: NodeId,
        port: String,
        order: Vec<EdgeId>,
    },
    SetCookMode {
        mode: CookMode,
    },
    CookNow,
    /// Reinserts a copied fragment with fresh ids at `position` (offset
    /// from the fragment's own layout). Context-illegal nodes are skipped.
    PasteNodes {
        ctx: GraphContext,
        fragment: crate::document::GraphFragment,
        position: [f32; 2],
    },
    /// Copies then pastes the given nodes in place with a small offset.
    DuplicateNodes {
        ctx: GraphContext,
        ids: Vec<NodeId>,
    },
    /// Adds a review annotation. With `reply_to` set it is a reply: the
    /// engine validates the parent (must exist, must not itself be a
    /// reply) and inherits its anchor, ignoring the host-sent one; a
    /// top-level 3D anchor gets its `geometry_hash` filled engine-side.
    /// `author`/`created_at` are host-provided (see the review module doc).
    AddAnnotation {
        anchor: crate::review::ReviewAnchor,
        text: String,
        category: crate::review::ReviewCategory,
        #[serde(default)]
        author: Option<String>,
        #[serde(default)]
        created_at: String,
        #[serde(default)]
        reply_to: Option<crate::review::AnnotationId>,
    },
    /// Edits an annotation's text and/or category.
    EditAnnotation {
        id: crate::review::AnnotationId,
        text: String,
        category: crate::review::ReviewCategory,
        #[serde(default)]
        updated_at: String,
    },
    /// Toggles an annotation's resolved flag.
    ResolveAnnotation {
        id: crate::review::AnnotationId,
        resolved: bool,
        #[serde(default)]
        updated_at: String,
    },
    /// Deletes an annotation and (for a top-level note) its direct replies.
    DeleteAnnotation {
        id: crate::review::AnnotationId,
    },
    /// Re-places an annotation's pin: replaces the anchor (the engine
    /// refills `geometry_hash`), propagates it to replies, and clears the
    /// runtime stale flag.
    ReanchorAnnotation {
        id: crate::review::AnnotationId,
        anchor: crate::review::ReviewAnchor,
        #[serde(default)]
        updated_at: String,
    },
    /// Resolves the node a viewport gizmo should write to inside `geo`'s
    /// subflow, creating it if necessary (the ratified reuse-tail-transform
    /// policy): if the subflow's display node is already a non-bypassed
    /// `transform`, that node is the target; otherwise a fresh `transform` is
    /// appended after the display node and the display flag moves to it.
    ///
    /// Either way the answer arrives as [`EngineEvent::TransformTargetReady`] --
    /// which is why the event exists at all, since the reuse path mints nothing
    /// and would otherwise emit no events to read the id from.
    ///
    /// Issued inside the drag's transaction, so appending undoes together with
    /// the drag in one step.
    EnsureTransformTarget {
        geo: NodeId,
    },
    /// Groups following commands into one undo step until `EndTransaction`
    /// (drags, marquee moves).
    BeginTransaction {
        label: String,
    },
    EndTransaction,
    /// Rolls the open transaction back to where it began and discards it, so a
    /// cancelled drag (Escape) leaves no document mutation AND no redo entry.
    /// Commit-then-undo was rejected precisely because it pollutes the redo
    /// stack with a step the user never asked for.
    CancelTransaction,
    Undo,
    Redo,
}

/// Serde-friendly [`PortRef`] (the internal type is not serialized).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortRefDto {
    pub node: NodeId,
    pub port: String,
}

impl From<PortRefDto> for PortRef {
    fn from(d: PortRefDto) -> Self {
        PortRef {
            node: d.node,
            port: d.port,
        }
    }
}

/// One engine-emitted event. Output-only: the frontend deserializes these
/// in JavaScript, so only `Serialize` is derived. Batched with a monotonic
/// revision so a mirror can detect desync and resnapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum EngineEvent {
    NodeAdded {
        ctx: GraphContext,
        node: snapshot::NodeMirror,
    },
    NodeRemoved {
        ctx: GraphContext,
        id: NodeId,
    },
    ParamChanged {
        ctx: GraphContext,
        node: NodeId,
        key: String,
        value: ParamSource,
    },
    EdgeAdded {
        ctx: GraphContext,
        edge: snapshot::EdgeMirror,
    },
    EdgeRemoved {
        ctx: GraphContext,
        id: EdgeId,
    },
    NodesMoved {
        ctx: GraphContext,
        moves: Vec<(NodeId, [f32; 2])>,
    },
    ActiveOutputChanged {
        ctx: GraphContext,
        node: Option<NodeId>,
    },
    SelectionChanged {
        ctx: GraphContext,
        ids: Vec<NodeId>,
    },
    BypassChanged {
        ctx: GraphContext,
        node: NodeId,
        bypassed: bool,
    },
    VariadicReordered {
        ctx: GraphContext,
        node: NodeId,
        port: String,
        order: Vec<EdgeId>,
    },
    CookStatus {
        node: NodeId,
        status: CookStatus,
    },
    NodeStats {
        node: NodeId,
        points: u64,
        prims: u64,
        meshes: u32,
    },
    /// A node's validation counts changed (validate node cook, import load
    /// validation). Zero counts on a clean result AND on a cleared one
    /// (bypass, lost input), so the badge lifecycle is one event.
    ValidationSummary {
        node: NodeId,
        errors: usize,
        warnings: usize,
    },
    /// The full issue list behind a fresh [`EngineEvent::ValidationSummary`],
    /// capped at [`REPORT_EVENT_ISSUE_CAP`] rows (`truncated` flags the
    /// cut; the summary carries the uncapped counts).
    ValidationReport {
        node: NodeId,
        errors: usize,
        warnings: usize,
        truncated: bool,
        issues: Vec<solarxy_core::validation::ValidationIssue>,
    },
    CookModeChanged {
        mode: CookMode,
    },
    /// The node a gizmo drag will write to (see [`Command::EnsureTransformTarget`]).
    /// Emitted on BOTH policy paths, because the reuse path creates nothing and
    /// so has no `NodeAdded` to carry the id.
    TransformTargetReady {
        ctx: GraphContext,
        node: NodeId,
    },
    /// The review annotation set changed; the mirror re-reads it from the
    /// snapshot (annotations are few, so a coarse signal is cheap).
    ReviewChanged,
    /// Full-resnapshot signal (structural undo, scene load).
    DocumentReplaced,
}

/// A coalesced batch of events plus the monotonic document revision.
#[derive(Debug, Clone, Serialize)]
pub struct EventBatch {
    pub revision: u64,
    pub events: Vec<EngineEvent>,
}

/// The most issues a single [`EngineEvent::ValidationReport`] carries; a
/// pathological mesh truncates the list (flagged) while the summary keeps
/// the real counts.
pub const REPORT_EVENT_ISSUE_CAP: usize = 2000;

/// A detailed pick over the displayed scene: the review workflow's anchor
/// source ([`Engine::pick_detailed`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickDetail {
    /// The root `geo` container hit (anchor semantics: geometry resolves
    /// through its subflow's display output).
    pub node: NodeId,
    /// Mesh index within the displayed `GeometrySet`.
    pub mesh: u32,
    /// Triangle index within that mesh.
    pub face: u32,
    /// Barycentric `[w, u, v]` on the face (Moller-Trumbore convention).
    pub barycentric: [f32; 3],
    /// World-space hit point.
    pub world_pos: [f32; 3],
    /// Distance along the ray.
    pub distance: f32,
}

/// One top-level annotation's marker, world-resolved
/// ([`Engine::review_markers_world`]); the host projects it per pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReviewMarkerWorld {
    pub id: crate::review::AnnotationId,
    /// Pin position: the anchored point, else the stored fallback, else
    /// `None` (node-only annotations render in the panel, not the scene).
    pub world: Option<[f32; 3]>,
    pub category: crate::review::ReviewCategory,
    pub resolved: bool,
    pub needs_reanchor: bool,
}

/// Bumps `updated_at` from a host-provided timestamp; an empty string (an
/// omitted boundary field, older callers) leaves the existing stamp alone.
fn touch(a: &mut crate::review::Annotation, updated_at: String) {
    if !updated_at.is_empty() {
        a.updated_at = updated_at;
    }
}

/// Turns one validation-cache change into its boundary events: a fresh
/// result emits the summary plus the capped issue list; a cleared one
/// emits a zeroed summary (one badge lifecycle, no tombstone event).
fn push_validation_events(
    events: &mut Vec<EngineEvent>,
    node: NodeId,
    validation: Option<&solarxy_core::validation::ValidationResult>,
) {
    match validation {
        Some(v) => {
            let errors = v.report.error_count();
            let warnings = v.report.warning_count();
            events.push(EngineEvent::ValidationSummary {
                node,
                errors,
                warnings,
            });
            events.push(EngineEvent::ValidationReport {
                node,
                errors,
                warnings,
                truncated: v.report.issues.len() > REPORT_EVENT_ISSUE_CAP,
                issues: v
                    .report
                    .issues
                    .iter()
                    .take(REPORT_EVENT_ISSUE_CAP)
                    .cloned()
                    .collect(),
            });
        }
        None => events.push(EngineEvent::ValidationSummary {
            node,
            errors: 0,
            warnings: 0,
        }),
    }
}

/// A whole-document save file: the graph data plus the editor's cook mode.
/// The Phase-4 web host serializes this to JSON for OPFS autosave and the
/// explicit save/load path; Phase 5's `.slxy` ZIP embeds the same
/// `DocumentData` as its `document.json`, wrapping asset payloads around it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentFile {
    /// The save-format version (1 in Phase 4), for forward migration.
    #[serde(default = "one")]
    pub format_version: u32,
    pub document: DocumentData,
    #[serde(default)]
    pub cook_mode: CookMode,
}

fn one() -> u32 {
    1
}

/// A command failure (a structurally illegal request). Cook failures are
/// data, delivered as `CookStatus::Error` events, not errors here.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("unknown node type '{0}'")]
    UnknownNodeType(String),
    #[error("port '{port}' does not exist on node type '{type_id}'")]
    UnknownPort { type_id: String, port: String },
    #[error("node type '{type_id}' is not allowed in this context")]
    ContextIllegal { type_id: String },
    #[error("param '{key}' rejected: {reason}")]
    InvalidParam { key: String, reason: String },
    #[error("geo {geo:?} has no display node, so there is nothing to transform")]
    NoDisplayNode { geo: NodeId },
}

/// What a viewport gizmo drives, resolved by [`Engine::gizmo_target`].
///
/// The host reads this once per frame to place the manipulator, and again at
/// drag start to know what to write; all of the POLICY (which node, which space,
/// whether a node must be appended first) is decided engine-side.
///
/// Carries the target's whole transform, not just its translate: a rotate drag
/// has to decompose back into the target's own `rotate_order`, and a scale drag
/// writes two different params depending on the handle.
///
/// Not `Serialize`: this never crosses the wasm boundary. The drag loop runs
/// entirely in the host and streams into the preview lane, so the only thing JS
/// ever sees of a gizmo is the `EventBatch` on commit.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GizmoTarget {
    /// Where a `SetParam` for this drag must be addressed.
    pub ctx: GraphContext,
    /// The node the drag writes. On the append path this is still the DISPLAY
    /// node: the real target is minted by [`Command::EnsureTransformTarget`]
    /// when the drag actually starts, so a mere hover never mutates the
    /// document.
    pub node: NodeId,
    /// The target's current translate, previews included (so the handle tracks
    /// the object mid-drag).
    pub translate: [f32; 3],
    /// The current rotate, in **degrees**. Deliberately not radians: this is
    /// what a `SetParam` writes, and `resolve_params` owns the conversion in
    /// the other direction. Handing back radians here would put a silent
    /// 57x error one careless assignment away.
    pub rotate: [f32; 3],
    /// The order the target composes its rotation in, so a rotate drag can
    /// decompose its result back into the angles this node actually means.
    pub rotate_order: RotateOrder,
    /// The current per-axis scale.
    pub scale: [f32; 3],
    /// The current uniform-scale factor (the centre handle's param).
    pub uniform_scale: f32,
    /// The target's local pivot. Zero on a `geo` (its pivot is its origin).
    pub pivot: [f32; 3],
    /// World matrix placing the manipulator, **pivot included**: rotation and
    /// scale happen about `translate + pivot`, so that is where the rings and
    /// cubes have to sit or they would spin about a point they are not drawn
    /// around.
    pub anchor: [[f32; 4]; 4],
    /// The target's own orthonormal orientation basis, for local-space handles.
    /// Identity-equivalent when nothing upstream is rotated.
    pub basis: [[f32; 3]; 3],
    /// The PARENT's orthonormal basis alone (identity at root, the container's
    /// rotation in a subflow). A rotate drag turns about a world axis but writes
    /// a param expressed in the parent's frame, so it needs this to conjugate
    /// the delta into that frame; `basis` (which already has the node's own
    /// rotation folded in) cannot do that job.
    pub parent_basis: [[f32; 3]; 3],
    /// Maps a world-space delta into the target's own space. Identity at root.
    pub parent: [[f32; 4]; 4],
    /// True when the subflow's tail is not a usable `transform`, so the drag
    /// must append one before it can preview anything.
    pub append_pending: bool,
}

/// A node's transform params, preview-resolved, in **document units**.
///
/// The unit trap this type exists to close: `resolve_params` hands back
/// RADIANS (it owns the degrees conversion, since every consumer downstream of
/// it wants radians), but a `SetParam` writes DEGREES. A gizmo both reads and
/// writes, so it straddles the conversion, and getting it backwards is a silent
/// 57x error. `rotate_deg` is named for the unit it carries.
#[derive(Debug, Clone, Copy)]
struct NodeTransform {
    translate: [f32; 3],
    rotate_deg: [f32; 3],
    order: RotateOrder,
    scale: [f32; 3],
    uniform_scale: f32,
    /// Zero on a `geo`, which has no pivot param and rotates about its origin.
    pivot: [f32; 3],
}

impl NodeTransform {
    /// `has_pivot` is the caller's job because `ResolvedParams` debug-asserts on
    /// a key the descriptor never declared, and only `transform` declares one.
    fn read(p: &ResolvedParams, has_pivot: bool) -> Self {
        Self {
            translate: p.vec3_f32("translate"),
            rotate_deg: p.vec3_f32("rotate").map(f32::to_degrees),
            order: rotate_order_from_key(p.enum_key("rotate_order")),
            scale: p.vec3_f32("scale"),
            uniform_scale: p.f32("uniform_scale"),
            pivot: if has_pivot {
                p.vec3_f32("pivot")
            } else {
                [0.0; 3]
            },
        }
    }

    /// The orthonormal orientation basis: the rotation with the scale left out,
    /// so local-space handles land on the object's axes without stretching with
    /// its scale.
    fn basis(self) -> Matrix3<f32> {
        rotation_matrix(self.rotate_deg.map(f32::to_radians), self.order)
    }

    /// The point rotation and scale actually happen about, in the node's own
    /// parent space. For `compose_trs` that is `translate + pivot`: feed the
    /// pivot through the matrix and the P and P-inverse cancel.
    fn pivot_point(self) -> Point3<f32> {
        Point3::from([
            self.translate[0] + self.pivot[0],
            self.translate[1] + self.pivot[1],
            self.translate[2] + self.pivot[2],
        ])
    }
}

/// The manipulator's placement frame: positioned at `center`, oriented by
/// `basis`, and deliberately carrying **no scale**. A scaled frame would
/// stretch the handles with the object, which is exactly what a screen-constant
/// gizmo must not do.
fn gizmo_frame(center: Point3<f32>, basis: Matrix3<f32>) -> Matrix4<f32> {
    Matrix4::from_translation(Vector3::new(center.x, center.y, center.z)) * Matrix4::from(basis)
}

fn mat3_to_array(m: Matrix3<f32>) -> [[f32; 3]; 3] {
    [m.x.into(), m.y.into(), m.z.into()]
}

/// The engine.
pub struct Engine {
    doc: Document,
    registry: Registry,
    cook: CookEngine,
    assets: AssetTable,
    cook_mode: CookMode,
    /// In `Manual` mode, cooking is suppressed until a `CookNow` arms this;
    /// it disarms once the stale set drains. Ignored in `Auto`.
    manual_cook_requested: bool,
    revision: u64,
    /// Transient preview overlays (param drags): consulted by the resolver
    /// path, never written to the document or the undo stack.
    previews: BTreeMap<(NodeId, String), ParamSource>,
    scene: SceneDelta,
    undo: UndoStack,
    /// Async jobs spawned by the last cook, awaiting dispatch by the host
    /// (each tagged with the context it was spawned in, for `submit`).
    pending_jobs: Vec<(GraphContext, JobId, JobRequest)>,
    /// Runtime review staleness (never persisted): per top-level 3D-anchored
    /// annotation, whether the anchored output's current [`geometry_hash`]
    /// no longer matches the anchor's stored hash. Refreshed after every
    /// apply/cook/job-commit; surfaced through the snapshot's
    /// `needs_reanchor`.
    review_stale: BTreeMap<crate::review::AnnotationId, bool>,
    /// Current-hash memo behind the staleness refresh, keyed by geo node
    /// with the displayed `Arc`'s pointer as the validity stamp (the `Arc`
    /// is stable until the node recooks, so hashes recompute only for
    /// geometry that actually changed).
    review_hash_cache: BTreeMap<NodeId, (usize, u64)>,
}

impl Engine {
    /// Builds an engine over the builtin registry.
    pub fn new() -> Result<Self, GraphError> {
        Ok(Self::with_registry(crate::builtin_registry()?))
    }

    /// Builds an engine over a supplied registry (tests).
    #[must_use]
    pub fn with_registry(registry: Registry) -> Self {
        Self {
            doc: Document::new(),
            registry,
            cook: CookEngine::new(),
            assets: AssetTable::new(),
            cook_mode: CookMode::Auto,
            manual_cook_requested: false,
            revision: 0,
            previews: BTreeMap::new(),
            scene: SceneDelta::default(),
            undo: UndoStack::default(),
            pending_jobs: Vec::new(),
            review_stale: BTreeMap::new(),
            review_hash_cache: BTreeMap::new(),
        }
    }

    /// Enables async import offloading (the web host has an import worker).
    /// Off by default: native cooks parse imports inline.
    pub fn set_async_jobs(&mut self, enabled: bool) {
        self.cook.set_async_jobs(enabled);
    }

    /// Installs a host wall-clock (milliseconds), so successful cooks report
    /// their real duration in `CookStatus::Ok { ms }` (the badge tooltip and
    /// info-popover cook time). The web host passes `performance.now`;
    /// without one, durations stay `0` (the native/test default).
    pub fn set_clock(&mut self, clock: fn() -> f64) {
        self.cook.set_clock(clock);
    }

    #[must_use]
    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    #[must_use]
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// Stages asset bytes, returning the content id an import node's
    /// `file` param should reference. `mime` is recorded for the `.slxy`
    /// manifest (empty is acceptable when the source provided none).
    pub fn stage_asset(
        &mut self,
        name: impl Into<String>,
        mime: impl Into<String>,
        bytes: Vec<u8>,
    ) -> AssetId {
        self.assets.stage(name, mime, bytes)
    }

    /// The staged bytes behind an asset id, if present. Introspection for
    /// the boundary (OPFS cache checks) and tests; geometry never crosses,
    /// but asset bytes do.
    #[must_use]
    pub fn asset_bytes(&self, id: &AssetId) -> Option<&[u8]> {
        self.assets.get(id).map(|e| e.bytes.as_slice())
    }

    /// The number of staged assets (introspection / tests).
    #[must_use]
    pub fn asset_count(&self) -> usize {
        self.assets.len()
    }

    /// `(content-hash, name)` for every staged asset (the boundary uses it to
    /// hand the import worker a job's sidecar candidates, and the frontend's
    /// missing-sidecar preflight treats it as the authoritative staged set).
    ///
    /// One row per NAME, not per entry: bytes staged under several names are a
    /// single content-addressed entry, and every one of those names has to look
    /// staged, or the preflight reports a companion missing whose bytes it is
    /// already holding.
    #[must_use]
    pub fn asset_manifest(&self) -> Vec<(String, String)> {
        self.assets
            .entries()
            .flat_map(|(id, entry)| {
                entry
                    .names()
                    .map(|name| (id.0.clone(), name.to_string()))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    /// Applies one command, returning the events it produced. Each apply is
    /// an implicit transaction (unless one is explicitly open); the
    /// revision advances once per call.
    pub fn apply(&mut self, cmd: Command) -> Result<EventBatch, EngineError> {
        // Undo-stack control commands are handled here, not in dispatch.
        match cmd {
            Command::Undo => return self.run_undo(),
            Command::Redo => return self.run_redo(),
            Command::BeginTransaction { label } => {
                self.undo.begin(label);
                self.revision += 1;
                return Ok(self.batch(Vec::new()));
            }
            Command::EndTransaction => {
                self.undo.end();
                self.revision += 1;
                return Ok(self.batch(Vec::new()));
            }
            Command::CancelTransaction => return self.run_cancel(),
            _ => {}
        }
        let mut events = Vec::new();
        let mut inv = Vec::new();
        self.dispatch(cmd, &mut events, &mut inv)?;
        // Non-cook commands can still change what an anchor sees (display
        // flag moves, node/annotation edits), so staleness refreshes on
        // every mutation, not just after cooks. Cheap: no-op without
        // 3D-anchored annotations.
        self.refresh_review_staleness(&mut events);
        self.undo.push_command("edit", inv);
        self.revision += 1;
        Ok(self.batch(events))
    }

    fn batch(&self, events: Vec<EngineEvent>) -> EventBatch {
        EventBatch {
            revision: self.revision,
            events,
        }
    }

    fn dispatch(
        &mut self,
        cmd: Command,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        match cmd {
            Command::AddNode {
                ctx,
                node_type,
                position,
            } => self
                .add_node(ctx, &node_type, position, events, inv)
                .map(|_| ()),
            Command::RemoveNodes { ctx, ids } => self.remove_nodes(ctx, &ids, events, inv),
            Command::Connect { ctx, from, to } => {
                self.connect(ctx, from.into(), to.into(), events, inv)
            }
            Command::Disconnect { ctx, edge } => self.disconnect(ctx, edge, events, inv),
            Command::SetParam {
                ctx,
                node,
                key,
                value,
            } => self.set_param(ctx, node, &key, value, events, inv),
            Command::MoveNodes { ctx, moves } => self.move_nodes(ctx, moves, events, inv),
            Command::SetActiveOutput { ctx, node } => {
                self.set_active_output(ctx, node, events, inv)
            }
            Command::SetSelection { ctx, ids } => self.set_selection(ctx, ids, events, inv),
            Command::EnsureTransformTarget { geo } => {
                self.ensure_transform_target(geo, events, inv).map(|_| ())
            }
            Command::SetBypass {
                ctx,
                node,
                bypassed,
            } => self.set_bypass(ctx, node, bypassed, events, inv),
            Command::ReorderVariadicInput {
                ctx,
                node,
                port,
                order,
            } => self.reorder_variadic(ctx, node, &port, order, events, inv),
            Command::SetCookMode { mode } => {
                self.cook_mode = mode;
                // Leaving Manual re-cooks the stale set automatically (Auto
                // ignores the flag); entering Manual freezes until CookNow.
                self.manual_cook_requested = false;
                events.push(EngineEvent::CookModeChanged { mode });
                Ok(())
            }
            Command::PasteNodes {
                ctx,
                fragment,
                position,
            } => self.paste_nodes(ctx, &fragment, position, events, inv),
            Command::DuplicateNodes { ctx, ids } => self.duplicate_nodes(ctx, &ids, events, inv),
            Command::AddAnnotation {
                anchor,
                text,
                category,
                author,
                created_at,
                reply_to,
            } => {
                // A reply inherits its parent's anchor (validated first); a
                // top-level 3D anchor gets its hash filled from the
                // currently displayed output. Both resolve before the
                // mutation closure so it stays a pure document edit.
                let anchor = if let Some(parent_id) = reply_to {
                    let parent = self
                        .doc
                        .review()
                        .get(parent_id)
                        .ok_or(GraphError::UnknownAnnotation(parent_id))?;
                    if parent.reply_to.is_some() {
                        return Err(GraphError::InvalidReply(
                            "cannot reply to a reply (threading is flat)",
                        )
                        .into());
                    }
                    parent.anchor.clone()
                } else {
                    let mut anchor = anchor;
                    if anchor.face.is_some() {
                        anchor.geometry_hash = self.anchor_hash(&anchor);
                    }
                    anchor
                };
                self.review_mutate(events, inv, |doc| {
                    let id = doc.mint_annotation_id();
                    doc.review_mut().insert(crate::review::Annotation {
                        id,
                        anchor,
                        text,
                        category,
                        resolved: false,
                        author,
                        updated_at: created_at.clone(),
                        created_at,
                        reply_to,
                    });
                    Ok(())
                })
            }
            Command::EditAnnotation {
                id,
                text,
                category,
                updated_at,
            } => self.review_mutate(events, inv, |doc| {
                let a = doc
                    .review_mut()
                    .get_mut(id)
                    .ok_or(GraphError::UnknownAnnotation(id))?;
                a.text = text;
                a.category = category;
                touch(a, updated_at);
                Ok(())
            }),
            Command::ResolveAnnotation {
                id,
                resolved,
                updated_at,
            } => self.review_mutate(events, inv, |doc| {
                let a = doc
                    .review_mut()
                    .get_mut(id)
                    .ok_or(GraphError::UnknownAnnotation(id))?;
                a.resolved = resolved;
                touch(a, updated_at);
                Ok(())
            }),
            Command::DeleteAnnotation { id } => self.review_mutate(events, inv, |doc| {
                if doc.review_mut().remove_cascade(id) == 0 {
                    return Err(GraphError::UnknownAnnotation(id));
                }
                Ok(())
            }),
            Command::ReanchorAnnotation {
                id,
                anchor,
                updated_at,
            } => {
                let target = self
                    .doc
                    .review()
                    .get(id)
                    .ok_or(GraphError::UnknownAnnotation(id))?;
                if target.reply_to.is_some() {
                    return Err(GraphError::InvalidReply(
                        "replies share their parent's anchor; re-anchor the parent",
                    )
                    .into());
                }
                let mut anchor = anchor;
                if anchor.face.is_some() {
                    anchor.geometry_hash = self.anchor_hash(&anchor);
                }
                self.review_mutate(events, inv, |doc| {
                    let review = doc.review_mut();
                    let a = review
                        .get_mut(id)
                        .ok_or(GraphError::UnknownAnnotation(id))?;
                    a.anchor = anchor.clone();
                    touch(a, updated_at.clone());
                    // Replies mirror the parent's pin.
                    for reply in review.iter_mut().filter(|r| r.reply_to == Some(id)) {
                        reply.anchor = anchor.clone();
                    }
                    Ok(())
                })
            }
            // CookNow arms a manual-mode cook (drains on the next frames).
            Command::CookNow => {
                self.manual_cook_requested = true;
                Ok(())
            }
            // The transaction/undo commands are intercepted in `apply`.
            Command::BeginTransaction { .. }
            | Command::EndTransaction
            | Command::CancelTransaction
            | Command::Undo
            | Command::Redo => Ok(()),
        }
    }

    fn add_node(
        &mut self,
        ctx: GraphContext,
        node_type: &str,
        position: [f32; 2],
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<NodeId, EngineError> {
        let desc = self
            .registry
            .get(node_type)
            .ok_or_else(|| EngineError::UnknownNodeType(node_type.to_string()))?;
        if !desc.contexts.allows(ctx) {
            return Err(EngineError::ContextIllegal {
                type_id: node_type.to_string(),
            });
        }
        let id = self.doc.mint_node_id();
        let mut node = NodeData::new(id, node_type, desc.version);
        node.position = position;
        // Subflow-owning container nodes open their own canvas.
        if node_type == "geo" {
            self.doc.create_subflow(id);
        }
        let mirror = snapshot::NodeMirror::from_public(&node);
        let graph = self.doc.graph_mut(ctx)?;
        // First subflow node claims the display flag.
        let claim_display = matches!(ctx, GraphContext::Subflow(_))
            && graph.active_output.is_none()
            && desc.default_output().is_some();
        let prev_active = graph.active_output;
        graph.add_node(node);
        if claim_display {
            graph.active_output = Some(id);
            events.push(EngineEvent::ActiveOutputChanged {
                ctx,
                node: Some(id),
            });
        }
        self.cook.insert_node(id);
        self.mark_dirty(ctx, id);
        events.push(EngineEvent::NodeAdded { ctx, node: mirror });
        // Inverse: restore the prior display flag (if we claimed it), then
        // remove the node.
        if claim_display {
            inv.push(UndoOp::SetActiveOutput {
                ctx,
                node: prev_active,
            });
        }
        inv.push(UndoOp::RemoveNodes { ctx, ids: vec![id] });
        Ok(id)
    }

    /// The ratified reuse-tail-transform policy, executed atomically.
    ///
    /// Composed from the ordinary `add_node` / `connect` / `set_active_output`
    /// helpers, each of which appends to the same `inv`. `apply` pushes that
    /// whole `inv` as ONE undo step, so appending a transform and then dragging
    /// it undoes together, in one press of Cmd+Z -- the same trick the exclusive
    /// shadow-caster cascade uses.
    fn ensure_transform_target(
        &mut self,
        geo: NodeId,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<NodeId, EngineError> {
        let ctx = GraphContext::Subflow(geo);
        let graph = self.doc.graph(ctx)?;
        let Some(display) = graph.active_output else {
            // Nothing is displayed, so there is nothing to transform. The host
            // does not offer a gizmo in this state; treat it as a hard error
            // rather than silently inventing geometry.
            return Err(EngineError::NoDisplayNode { geo });
        };

        // Reuse: the tail already IS a transform. A BYPASSED transform is
        // treated as absent -- dragging must move the object the user can see,
        // and a bypassed node moves nothing.
        let tail = graph
            .node(display)
            .ok_or(GraphError::UnknownNode(display))?;
        if tail.type_id == "transform" && !tail.bypassed {
            events.push(EngineEvent::TransformTargetReady { ctx, node: display });
            return Ok(display);
        }

        // Append: a fresh transform downstream of the display node, which then
        // becomes the displayed output.
        let position = [tail.position[0] + 180.0, tail.position[1]];
        let new = self.add_node(ctx, "transform", position, events, inv)?;
        self.connect(
            ctx,
            PortRef {
                node: display,
                port: "geometry".to_string(),
            },
            PortRef {
                node: new,
                port: "geometry".to_string(),
            },
            events,
            inv,
        )?;
        // Without this the appended transform is invisible: `active_output` IS
        // the displayed node, there is no implicit "tail".
        self.set_active_output(ctx, Some(new), events, inv)?;
        events.push(EngineEvent::TransformTargetReady { ctx, node: new });
        Ok(new)
    }

    fn remove_nodes(
        &mut self,
        ctx: GraphContext,
        ids: &[NodeId],
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        inv.push(self.remove_nodes_core(ctx, ids, events)?);
        Ok(())
    }

    /// Removes nodes with their edges and subflows, returning the
    /// `RestoreFragment` inverse. Shared by the command and undo/redo.
    fn remove_nodes_core(
        &mut self,
        ctx: GraphContext,
        ids: &[NodeId],
        events: &mut Vec<EngineEvent>,
    ) -> Result<UndoOp, EngineError> {
        // Verify all ids exist before mutating, so the whole op is
        // all-or-nothing.
        {
            let graph = self.doc.graph(ctx)?;
            for &id in ids {
                if graph.node(id).is_none() {
                    return Err(GraphError::UnknownNode(id).into());
                }
            }
        }
        let set: std::collections::BTreeSet<NodeId> = ids.iter().copied().collect();
        // Capture the removed slice, its boundary edges, and the prior
        // display flag for one RestoreFragment inverse.
        let fragment = crate::document::GraphFragment::capture(&self.doc, ctx, ids);
        let (boundary, prev_active) = {
            let graph = self.doc.graph(ctx)?;
            let boundary: Vec<(Edge, bool)> = graph
                .edges()
                .filter(|e| set.contains(&e.from) ^ set.contains(&e.to))
                .map(|e| {
                    let variadic = graph
                        .node(e.to)
                        .is_some_and(|n| n.port_order.contains_key(&e.to_port));
                    (e.clone(), variadic)
                })
                .collect();
            (boundary, graph.active_output)
        };

        for &id in ids {
            let graph = self.doc.graph_mut(ctx)?;
            // Mark the removed node's downstream dirty before it vanishes.
            let downstream = graph.downstream(id);
            let (_node, removed_edges) = graph.remove_node(id)?;
            // A removed geo container drops its whole subflow.
            self.doc.remove_subflow(id);
            for edge in &removed_edges {
                events.push(EngineEvent::EdgeRemoved { ctx, id: edge.id });
            }
            self.cook.forget_node(id);
            for down in downstream {
                self.mark_dirty(ctx, down);
            }
            self.previews.retain(|(n, _), _| *n != id);
            events.push(EngineEvent::NodeRemoved { ctx, id });
        }
        Ok(UndoOp::RestoreFragment {
            ctx,
            fragment,
            boundary_edges: boundary,
            active_output: prev_active,
        })
    }

    fn connect(
        &mut self,
        ctx: GraphContext,
        from: PortRef,
        to: PortRef,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        // Type-check against the descriptors before mutating the graph.
        let to_variadic = self.validate_connection(&from, &to)?;
        let id = self.doc.mint_edge_id();
        let to_node = to.node;
        let edge = Edge {
            id,
            from: from.node,
            from_port: from.port,
            to: to.node,
            to_port: to.port,
        };
        let mirror = snapshot::EdgeMirror::from(&edge);
        self.doc.graph_mut(ctx)?.connect(edge, to_variadic)?;
        self.mark_dirty(ctx, to_node);
        events.push(EngineEvent::EdgeAdded { ctx, edge: mirror });
        inv.push(UndoOp::RemoveEdge { ctx, edge: id });
        Ok(())
    }

    /// Validates a prospective connection against the port types and the
    /// coercion matrix; returns whether the target port is variadic.
    fn validate_connection(&self, from: &PortRef, to: &PortRef) -> Result<bool, EngineError> {
        let from_node = self.node_type(from.node)?;
        let to_node = self.node_type(to.node)?;
        let from_desc = self
            .registry
            .get(&from_node)
            .ok_or_else(|| EngineError::UnknownNodeType(from_node.clone()))?;
        let to_desc = self
            .registry
            .get(&to_node)
            .ok_or_else(|| EngineError::UnknownNodeType(to_node.clone()))?;
        let out = from_desc
            .output(&from.port)
            .ok_or_else(|| EngineError::UnknownPort {
                type_id: from_node.clone(),
                port: from.port.clone(),
            })?;
        let inp = to_desc
            .input(&to.port)
            .ok_or_else(|| EngineError::UnknownPort {
                type_id: to_node.clone(),
                port: to.port.clone(),
            })?;
        if !crate::registry::coerce::can_coerce(out.data_type, inp.data_type).is_legal() {
            return Err(GraphError::TypeMismatch {
                from: format!("{:?}", out.data_type),
                to: format!("{:?}", inp.data_type),
            }
            .into());
        }
        Ok(matches!(inp.arity, Arity::Variadic { .. }))
    }

    fn disconnect(
        &mut self,
        ctx: GraphContext,
        edge: EdgeId,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        inv.push(self.disconnect_core(ctx, edge, events)?);
        Ok(())
    }

    /// Removes one edge, returning the `RestoreEdge` inverse (with its
    /// original id, which variadic `port_order` references). Shared by the
    /// command and undo/redo.
    fn disconnect_core(
        &mut self,
        ctx: GraphContext,
        edge: EdgeId,
        events: &mut Vec<EngineEvent>,
    ) -> Result<UndoOp, EngineError> {
        // The target's variadic-ness AND the edge's position in the port order,
        // captured before removal: `disconnect` drops the id out of the order
        // and `connect` would append it back on the end, silently reordering
        // the wires (see `UndoOp::RestoreEdge`).
        let (to_variadic, slot) = {
            let graph = self.doc.graph(ctx)?;
            let e = graph.edge(edge).ok_or(GraphError::UnknownEdge(edge))?;
            let order = graph.node(e.to).and_then(|n| n.port_order.get(&e.to_port));
            (
                order.is_some(),
                order.and_then(|o| o.iter().position(|&id| id == edge)),
            )
        };
        let removed = self.doc.graph_mut(ctx)?.disconnect(edge)?;
        self.mark_dirty(ctx, removed.to);
        events.push(EngineEvent::EdgeRemoved { ctx, id: edge });
        Ok(UndoOp::RestoreEdge {
            ctx,
            edge: removed,
            to_variadic,
            slot,
        })
    }

    fn set_param(
        &mut self,
        ctx: GraphContext,
        node: NodeId,
        key: &str,
        value: ParamSource,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let type_id = self.node_type_in(ctx, node)?;
        let desc = self
            .registry
            .get(&type_id)
            .ok_or_else(|| EngineError::UnknownNodeType(type_id.clone()))?;
        let spec = desc.param(key).ok_or_else(|| EngineError::InvalidParam {
            key: key.to_string(),
            reason: "no such param on this node type".to_string(),
        })?;
        // Conform a literal to the spec type on write (the authoritative
        // value; a mistyped write is a command error, not silent).
        let conformed = match &value {
            ParamSource::Literal(v) => {
                let c = crate::registry::resolve::conform_value(v, &spec.ty).map_err(|reason| {
                    EngineError::InvalidParam {
                        key: key.to_string(),
                        reason,
                    }
                })?;
                ParamSource::Literal(c)
            }
            ParamSource::Expression { .. } => value.clone(),
        };
        // The authoritative value clears any transient preview overlay.
        self.previews.remove(&(node, key.to_string()));
        let graph = self.doc.graph_mut(ctx)?;
        let node_data = graph.node_mut(node).ok_or(GraphError::UnknownNode(node))?;
        let prev = node_data.params.insert(key.to_string(), conformed.clone());
        self.mark_dirty(ctx, node);
        events.push(EngineEvent::ParamChanged {
            ctx,
            node,
            key: key.to_string(),
            value: conformed,
        });
        inv.push(UndoOp::RestoreParam {
            ctx,
            node,
            key: key.to_string(),
            prev,
        });
        // The exclusive-shadow-caster rule (UX spec J3): at most one root
        // light carries the shadow map. Granting it to one shadow-capable
        // light clears the flag on every other one INSIDE the same command,
        // so the whole handoff is a single undo step and the batch carries
        // a ParamChanged per released light (the frontend toasts the name).
        if ctx == GraphContext::Root
            && key == "cast_shadow"
            && scene::is_light(&type_id)
            && matches!(
                events.last(),
                Some(EngineEvent::ParamChanged {
                    value: ParamSource::Literal(ParamValue::Bool(true)),
                    ..
                })
            )
        {
            let others: Vec<NodeId> = {
                let graph = self.doc.graph(ctx)?;
                graph
                    .nodes()
                    .filter(|n| n.id != node && scene::is_light(&n.type_id))
                    .filter(|n| {
                        // Currently casting (resolved through the schema:
                        // shadow-capable lights default the flag true).
                        self.registry.get(&n.type_id).is_some_and(|d| {
                            d.param("cast_shadow").is_some()
                                && crate::registry::resolve::resolve_params(&n.params, &d.params)
                                    .is_ok_and(|p| p.bool("cast_shadow"))
                        })
                    })
                    .map(|n| n.id)
                    .collect()
            };
            for other in others {
                self.set_param(
                    ctx,
                    other,
                    "cast_shadow",
                    ParamSource::Literal(ParamValue::Bool(false)),
                    events,
                    inv,
                )?;
            }
        }
        Ok(())
    }

    fn move_nodes(
        &mut self,
        ctx: GraphContext,
        moves: Vec<(NodeId, [f32; 2])>,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let graph = self.doc.graph_mut(ctx)?;
        let mut prev = Vec::with_capacity(moves.len());
        for &(node, pos) in &moves {
            let node_data = graph.node_mut(node).ok_or(GraphError::UnknownNode(node))?;
            prev.push((node, node_data.position));
            node_data.position = pos;
        }
        events.push(EngineEvent::NodesMoved { ctx, moves });
        inv.push(UndoOp::MoveNodes { ctx, moves: prev });
        Ok(())
    }

    fn set_active_output(
        &mut self,
        ctx: GraphContext,
        node: Option<NodeId>,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let graph = self.doc.graph_mut(ctx)?;
        if let Some(n) = node
            && graph.node(n).is_none()
        {
            return Err(GraphError::UnknownNode(n).into());
        }
        let prev = graph.active_output;
        graph.active_output = node;
        // Changing the display node changes which cone must cook.
        if let Some(n) = node {
            self.mark_dirty(ctx, n);
        }
        events.push(EngineEvent::ActiveOutputChanged { ctx, node });
        inv.push(UndoOp::SetActiveOutput { ctx, node: prev });
        Ok(())
    }

    fn set_selection(
        &mut self,
        ctx: GraphContext,
        ids: Vec<NodeId>,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let graph = self.doc.graph_mut(ctx)?;
        let prev = graph.selection.clone();
        graph.selection.clone_from(&ids);
        events.push(EngineEvent::SelectionChanged { ctx, ids });
        inv.push(UndoOp::SetSelection { ctx, ids: prev });
        Ok(())
    }

    fn set_bypass(
        &mut self,
        ctx: GraphContext,
        node: NodeId,
        bypassed: bool,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let graph = self.doc.graph_mut(ctx)?;
        let node_data = graph.node_mut(node).ok_or(GraphError::UnknownNode(node))?;
        let prev = node_data.bypassed;
        node_data.bypassed = bypassed;
        self.mark_dirty(ctx, node);
        events.push(EngineEvent::BypassChanged {
            ctx,
            node,
            bypassed,
        });
        inv.push(UndoOp::SetBypass {
            ctx,
            node,
            bypassed: prev,
        });
        Ok(())
    }

    fn reorder_variadic(
        &mut self,
        ctx: GraphContext,
        node: NodeId,
        port: &str,
        order: Vec<EdgeId>,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let prev = self
            .doc
            .graph_mut(ctx)?
            .reorder_variadic(node, port, order.clone())?;
        self.mark_dirty(ctx, node);
        events.push(EngineEvent::VariadicReordered {
            ctx,
            node,
            port: port.to_string(),
            order,
        });
        inv.push(UndoOp::ReorderVariadic {
            ctx,
            node,
            port: port.to_string(),
            order: prev,
        });
        Ok(())
    }

    // Review annotations.

    /// Applies a review-store mutation with a whole-store snapshot inverse
    /// (annotations are few, so this is cheap and exact), emitting
    /// `ReviewChanged`.
    fn review_mutate(
        &mut self,
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
        f: impl FnOnce(&mut Document) -> Result<(), GraphError>,
    ) -> Result<(), EngineError> {
        let before = self.doc.review().clone();
        f(&mut self.doc)?;
        inv.push(UndoOp::RestoreReview { store: before });
        events.push(EngineEvent::ReviewChanged);
        Ok(())
    }

    /// The current [`crate::review::geometry_hash`] of the output an anchor
    /// pins to (the geo container's displayed geometry), memoized by the
    /// displayed `Arc`'s pointer. `None` when nothing is displayed/cooked.
    fn anchor_hash(&mut self, anchor: &crate::review::ReviewAnchor) -> Option<u64> {
        let set = scene::display_output(&self.doc, &self.cook, anchor.node)?;
        let stamp = std::sync::Arc::as_ptr(set).cast::<()>() as usize;
        if let Some(&(cached_stamp, hash)) = self.review_hash_cache.get(&anchor.node)
            && cached_stamp == stamp
        {
            return Some(hash);
        }
        let hash = crate::review::geometry_hash(set);
        self.review_hash_cache.insert(anchor.node, (stamp, hash));
        Some(hash)
    }

    /// Recomputes the runtime `needs_reanchor` flags: a top-level
    /// 3D-anchored annotation is stale when its anchored output is gone or
    /// its current hash no longer matches the anchor's stored one. Pushes a
    /// single `ReviewChanged` when any flag flips. Cheap: early-outs with
    /// no annotations, and current hashes are memoized per displayed `Arc`.
    fn refresh_review_staleness(&mut self, events: &mut Vec<EngineEvent>) {
        if self.doc.review().is_empty() {
            if !self.review_stale.is_empty() {
                self.review_stale.clear();
                self.review_hash_cache.clear();
                events.push(EngineEvent::ReviewChanged);
            }
            return;
        }
        // Collect first: hashing borrows self mutably (the memo).
        let anchored: Vec<(crate::review::AnnotationId, crate::review::ReviewAnchor)> = self
            .doc
            .review()
            .iter()
            .filter(|a| a.reply_to.is_none() && a.anchor.face.is_some())
            .map(|a| (a.id, a.anchor.clone()))
            .collect();
        let mut fresh = BTreeMap::new();
        let mut live_nodes = std::collections::BTreeSet::new();
        for (id, anchor) in anchored {
            live_nodes.insert(anchor.node);
            let stale = match (self.anchor_hash(&anchor), anchor.geometry_hash) {
                (None, _) => true,
                (Some(current), Some(stored)) => current != stored,
                // No reference hash (pre-Phase-7 annotation): nothing to
                // compare against, so never flag it.
                (Some(_), None) => false,
            };
            fresh.insert(id, stale);
        }
        self.review_hash_cache
            .retain(|node, _| live_nodes.contains(node));
        if fresh != self.review_stale {
            self.review_stale = fresh;
            events.push(EngineEvent::ReviewChanged);
        }
    }

    /// Whether an annotation's anchor is currently stale (runtime-derived: the
    /// engine refreshes review staleness after each cook that changes geometry).
    #[must_use]
    pub fn annotation_stale(&self, id: crate::review::AnnotationId) -> bool {
        self.review_stale.get(&id).copied().unwrap_or(false)
    }

    /// World-space marker data for every top-level annotation: the pin
    /// position resolved through the anchored face's barycentric point and
    /// the geo world matrix, or the stored world fallback when the anchor
    /// is stale or unresolvable. Replies carry no pin and are skipped, as
    /// are annotations anchored to a hidden geo (the pins hide with the
    /// object and return on re-show; the review panel still lists them).
    #[must_use]
    pub fn review_markers_world(&self) -> Vec<ReviewMarkerWorld> {
        self.doc
            .review()
            .iter()
            .filter(|a| a.reply_to.is_none())
            .filter(|a| {
                scene::geo_visible(&self.doc, &self.registry, &self.previews, a.anchor.node)
            })
            .map(|a| {
                let stale = self.annotation_stale(a.id);
                let world = if stale {
                    a.anchor.world_fallback
                } else {
                    self.resolve_anchor_world(&a.anchor)
                        .or(a.anchor.world_fallback)
                };
                ReviewMarkerWorld {
                    id: a.id,
                    world,
                    category: a.category,
                    resolved: a.resolved,
                    needs_reanchor: stale,
                }
            })
            .collect()
    }

    /// The bary-weighted world position of a 3D anchor over the currently
    /// displayed geometry (`None` for node-only anchors or out-of-range
    /// pins).
    fn resolve_anchor_world(&self, anchor: &crate::review::ReviewAnchor) -> Option<[f32; 3]> {
        use cgmath::Transform as _;
        let (mesh_idx, face, bary) = (anchor.mesh?, anchor.face?, anchor.barycentric?);
        let set = scene::display_output(&self.doc, &self.cook, anchor.node)?;
        let mesh = set.meshes.get(mesh_idx as usize)?;
        let base = (face as usize).checked_mul(3)?;
        let tri = mesh.indices.get(base..base + 3)?;
        let mut p = [0.0f32; 3];
        for (k, &vi) in tri.iter().enumerate() {
            let v = mesh.positions.get(vi as usize)?;
            for c in 0..3 {
                p[c] += v[c] * bary[k];
            }
        }
        let m = scene::geo_world_matrix(&self.doc, &self.registry, &self.previews, anchor.node);
        let world = m.transform_point(cgmath::Point3::new(p[0], p[1], p[2]));
        Some([world.x, world.y, world.z])
    }

    /// Every root geo container's displayed geometry with its world matrix,
    /// ascending geo id (the renderer's `SceneObjects` draw order). Feeds
    /// the host-side normals/bounds visualization aggregation, so hidden
    /// geos are excluded (an invisible object draws no overlays either).
    #[must_use]
    pub fn display_geometries(
        &self,
    ) -> Vec<(
        NodeId,
        std::sync::Arc<solarxy_kernel::GeometrySet>,
        [[f32; 4]; 4],
    )> {
        let Ok(root) = self.doc.graph(GraphContext::Root) else {
            return Vec::new();
        };
        let mut out: Vec<_> = root
            .nodes()
            .filter(|n| n.type_id == "geo")
            .filter(|n| scene::geo_visible(&self.doc, &self.registry, &self.previews, n.id))
            .filter_map(|n| {
                let set = scene::display_output(&self.doc, &self.cook, n.id)?;
                let m = scene::geo_world_matrix(&self.doc, &self.registry, &self.previews, n.id);
                Some((n.id, std::sync::Arc::clone(set), m.into()))
            })
            .collect();
        out.sort_by_key(|(id, _, _)| *id);
        out
    }

    // Clipboard.

    /// Captures a fragment of the given nodes for the clipboard (the
    /// frontend serializes it to `application/x-solarxy-nodes`). Not a
    /// command: it produces data, not a mutation.
    #[must_use]
    pub fn copy_nodes(&self, ctx: GraphContext, ids: &[NodeId]) -> crate::document::GraphFragment {
        crate::document::GraphFragment::capture(&self.doc, ctx, ids)
    }

    // Infallible today, but Result-typed to match the handler shape and to
    // leave room for a hard "nothing legal to paste" error later.
    #[allow(clippy::unnecessary_wraps)]
    fn paste_nodes(
        &mut self,
        ctx: GraphContext,
        fragment: &crate::document::GraphFragment,
        position: [f32; 2],
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        // Context legality is checked per node against the registry.
        let registry = &self.registry;
        let ctx_ok = |type_id: &str| {
            registry
                .get(type_id)
                .is_some_and(|d| d.contexts.allows(ctx))
        };
        let result = fragment.insert_into(
            &mut self.doc,
            ctx,
            crate::document::InsertMode::Remap,
            &ctx_ok,
        );
        // Offset the pasted nodes so they do not sit exactly on the source,
        // and register + dirty them.
        for &id in &result.inserted {
            if let Ok(graph) = self.doc.graph_mut(ctx)
                && let Some(node) = graph.node_mut(id)
            {
                node.position[0] += position[0];
                node.position[1] += position[1];
            }
            self.cook.insert_node(id);
            self.mark_dirty(ctx, id);
            if let Some(mirror) = self
                .doc
                .graph(ctx)
                .ok()
                .and_then(|g| g.node(id))
                .map(snapshot::NodeMirror::from_public)
            {
                events.push(EngineEvent::NodeAdded { ctx, node: mirror });
            }
        }
        for edge in self.pasted_edge_mirrors(ctx, &result) {
            events.push(EngineEvent::EdgeAdded { ctx, edge });
        }
        // Inverse: remove exactly the pasted nodes.
        if !result.inserted.is_empty() {
            inv.push(UndoOp::RemoveNodes {
                ctx,
                ids: result.inserted,
            });
        }
        Ok(())
    }

    /// Edge mirrors for the freshly pasted internal edges.
    fn pasted_edge_mirrors(
        &self,
        ctx: GraphContext,
        result: &crate::document::InsertResult,
    ) -> Vec<snapshot::EdgeMirror> {
        let Ok(graph) = self.doc.graph(ctx) else {
            return Vec::new();
        };
        result
            .edge_map
            .values()
            .filter_map(|new_id| graph.edge(*new_id).map(snapshot::EdgeMirror::from))
            .collect()
    }

    fn duplicate_nodes(
        &mut self,
        ctx: GraphContext,
        ids: &[NodeId],
        events: &mut Vec<EngineEvent>,
        inv: &mut Vec<UndoOp>,
    ) -> Result<(), EngineError> {
        let fragment = self.copy_nodes(ctx, ids);
        // A small in-place offset so the duplicate is visible.
        self.paste_nodes(ctx, &fragment, [24.0, 24.0], events, inv)
    }

    // Undo / redo.

    fn run_undo(&mut self) -> Result<EventBatch, EngineError> {
        self.revision += 1;
        let Some(txn) = self.undo.pop_undo() else {
            return Ok(self.batch(Vec::new()));
        };
        let (redo, mut events) = self.apply_transaction(txn)?;
        self.undo.push_redo(redo);
        // Undoing a re-anchor (or any display-affecting edit) must flip the
        // stale flags in the same batch.
        self.refresh_review_staleness(&mut events);
        Ok(self.batch(events))
    }

    /// Rolls the open transaction back and throws it away: the document returns
    /// to where the drag started, and NOTHING lands on either stack -- no undo
    /// entry (the user cancelled), and no redo entry (they never asked to
    /// re-apply a cancelled drag). Reuses the same `apply_transaction` machinery
    /// as undo, so an appended node is removed exactly as an undo would remove
    /// it.
    fn run_cancel(&mut self) -> Result<EventBatch, EngineError> {
        self.revision += 1;
        let Some(txn) = self.undo.take_open() else {
            return Ok(self.batch(Vec::new()));
        };
        let (_discarded, mut events) = self.apply_transaction(txn)?;
        self.refresh_review_staleness(&mut events);
        Ok(self.batch(events))
    }

    fn run_redo(&mut self) -> Result<EventBatch, EngineError> {
        self.revision += 1;
        let Some(txn) = self.undo.pop_redo() else {
            return Ok(self.batch(Vec::new()));
        };
        let (undo, mut events) = self.apply_transaction(txn)?;
        self.undo.push_undo(undo);
        self.refresh_review_staleness(&mut events);
        Ok(self.batch(events))
    }

    /// Applies a transaction's inverse ops in reverse order, collecting the
    /// opposite transaction (for the redo/undo stack). A structural
    /// transaction emits one `DocumentReplaced`; a scalar-only one emits
    /// precise inverse events.
    fn apply_transaction(
        &mut self,
        txn: Transaction,
    ) -> Result<(Transaction, Vec<EngineEvent>), EngineError> {
        let mut precise = Vec::new();
        let mut opposite = Transaction::new(txn.label.clone());
        let structural = txn.structural;
        for op in txn.ops.into_iter().rev() {
            let inverse = self.apply_undo_op(op, &mut precise)?;
            opposite.structural |= inverse.is_structural();
            opposite.ops.push(inverse);
        }
        let events = if structural {
            vec![EngineEvent::DocumentReplaced]
        } else {
            precise
        };
        Ok((opposite, events))
    }

    /// Applies one inverse op, returning the op that reverses it (so undo
    /// and redo are symmetric). Emits precise events into `events`.
    fn apply_undo_op(
        &mut self,
        op: UndoOp,
        events: &mut Vec<EngineEvent>,
    ) -> Result<UndoOp, EngineError> {
        match op {
            UndoOp::RestoreParam {
                ctx,
                node,
                key,
                prev,
            } => {
                let graph = self.doc.graph_mut(ctx)?;
                let node_data = graph.node_mut(node).ok_or(GraphError::UnknownNode(node))?;
                let current = match &prev {
                    Some(v) => node_data.params.insert(key.clone(), v.clone()),
                    None => node_data.params.remove(&key),
                };
                self.mark_dirty(ctx, node);
                // Precise event: the restored value, or the default when
                // the param returned to unset.
                let value = prev.clone().unwrap_or_else(|| {
                    self.registry
                        .get(&self.node_type_in(ctx, node).unwrap_or_default())
                        .and_then(|d| d.param(&key))
                        .map_or(
                            ParamSource::Literal(crate::params::ParamValue::Bool(false)),
                            |s| ParamSource::Literal(s.default.clone()),
                        )
                });
                events.push(EngineEvent::ParamChanged {
                    ctx,
                    node,
                    key: key.clone(),
                    value,
                });
                Ok(UndoOp::RestoreParam {
                    ctx,
                    node,
                    key,
                    prev: current,
                })
            }
            UndoOp::MoveNodes { ctx, moves } => {
                let graph = self.doc.graph_mut(ctx)?;
                let mut prev = Vec::with_capacity(moves.len());
                for (node, pos) in &moves {
                    if let Some(n) = graph.node_mut(*node) {
                        prev.push((*node, n.position));
                        n.position = *pos;
                    }
                }
                events.push(EngineEvent::NodesMoved {
                    ctx,
                    moves: moves.clone(),
                });
                Ok(UndoOp::MoveNodes { ctx, moves: prev })
            }
            UndoOp::SetBypass {
                ctx,
                node,
                bypassed,
            } => {
                let graph = self.doc.graph_mut(ctx)?;
                let n = graph.node_mut(node).ok_or(GraphError::UnknownNode(node))?;
                let prev = n.bypassed;
                n.bypassed = bypassed;
                self.mark_dirty(ctx, node);
                events.push(EngineEvent::BypassChanged {
                    ctx,
                    node,
                    bypassed,
                });
                Ok(UndoOp::SetBypass {
                    ctx,
                    node,
                    bypassed: prev,
                })
            }
            UndoOp::SetActiveOutput { ctx, node } => {
                let graph = self.doc.graph_mut(ctx)?;
                let prev = graph.active_output;
                graph.active_output = node;
                if let Some(n) = node {
                    self.mark_dirty(ctx, n);
                }
                events.push(EngineEvent::ActiveOutputChanged { ctx, node });
                Ok(UndoOp::SetActiveOutput { ctx, node: prev })
            }
            UndoOp::SetSelection { ctx, ids } => {
                let graph = self.doc.graph_mut(ctx)?;
                let prev = graph.selection.clone();
                graph.selection.clone_from(&ids);
                events.push(EngineEvent::SelectionChanged {
                    ctx,
                    ids: ids.clone(),
                });
                Ok(UndoOp::SetSelection { ctx, ids: prev })
            }
            UndoOp::ReorderVariadic {
                ctx,
                node,
                port,
                order,
            } => {
                let prev = self
                    .doc
                    .graph_mut(ctx)?
                    .reorder_variadic(node, &port, order.clone())?;
                self.mark_dirty(ctx, node);
                events.push(EngineEvent::VariadicReordered {
                    ctx,
                    node,
                    port: port.clone(),
                    order,
                });
                Ok(UndoOp::ReorderVariadic {
                    ctx,
                    node,
                    port,
                    order: prev,
                })
            }
            UndoOp::RemoveNodes { ctx, ids } => self.remove_nodes_core(ctx, &ids, events),
            UndoOp::RemoveEdge { ctx, edge } => self.disconnect_core(ctx, edge, events),
            UndoOp::RestoreEdge {
                ctx,
                edge,
                to_variadic,
                slot,
            } => {
                let id = edge.id;
                let to = edge.to;
                // `connect_at`, not `connect`: the wire must go back where it
                // was in the variadic order, or an index-selecting consumer
                // silently reads a different branch.
                self.doc
                    .graph_mut(ctx)?
                    .connect_at(edge.clone(), to_variadic, slot)?;
                self.mark_dirty(ctx, to);
                events.push(EngineEvent::EdgeAdded {
                    ctx,
                    edge: snapshot::EdgeMirror::from(&edge),
                });
                Ok(UndoOp::RemoveEdge { ctx, edge: id })
            }
            UndoOp::RestoreFragment {
                ctx,
                fragment,
                boundary_edges,
                active_output,
            } => {
                let ids: Vec<NodeId> = fragment.nodes.iter().map(|n| n.id).collect();
                // Restore nodes + internal edges + owned subflows verbatim.
                fragment.insert_into(&mut self.doc, ctx, undo::UNDO_INSERT, &|_| true);
                // Re-add boundary edges (to surviving outside nodes).
                for (edge, variadic) in &boundary_edges {
                    let _ = self.doc.graph_mut(ctx)?.connect(edge.clone(), *variadic);
                }
                // Re-register cook state and dirty the restored cone.
                for &id in &ids {
                    self.cook.insert_node(id);
                    self.mark_dirty(ctx, id);
                }
                if let Ok(graph) = self.doc.graph_mut(ctx) {
                    graph.active_output = active_output;
                }
                Ok(UndoOp::RemoveNodes { ctx, ids })
            }
            UndoOp::RestoreReview { store } => {
                let current = self.doc.review().clone();
                *self.doc.review_mut() = store;
                events.push(EngineEvent::ReviewChanged);
                Ok(UndoOp::RestoreReview { store: current })
            }
        }
    }

    /// A transient preview value for a param drag: no event, no undo entry,
    /// no document write. It only dirty-marks so the next cook previews it,
    /// and the resolver path consults it until the authoritative `SetParam`
    /// clears it.
    pub fn preview_param(
        &mut self,
        ctx: GraphContext,
        node: NodeId,
        key: &str,
        value: ParamSource,
    ) {
        self.previews.insert((node, key.to_string()), value);
        self.mark_dirty(ctx, node);
    }

    /// Drops an in-flight preview for one param, so a CANCELLED drag does not
    /// strand the object where the pointer left it.
    ///
    /// The symmetric counterpart to [`Engine::preview_param`]. Without it a
    /// cancel would roll the DOCUMENT back while the preview overlay kept
    /// asserting the dragged value, and the viewport would disagree with the
    /// parameter panel indefinitely (the overlay is only otherwise cleared by an
    /// authoritative `SetParam`).
    pub fn clear_preview(&mut self, ctx: GraphContext, node: NodeId, key: &str) {
        if self.previews.remove(&(node, key.to_string())).is_some() {
            self.mark_dirty(ctx, node);
        }
    }

    /// The geo container's world matrix, as the renderer and picking see it
    /// (previews included, so it follows a gizmo drag).
    #[must_use]
    pub fn geo_world_matrix(&self, geo: NodeId) -> Option<[[f32; 4]; 4]> {
        let root = self.doc.graph(GraphContext::Root).ok()?;
        let node = root.node(geo)?;
        if node.type_id != "geo" {
            return None;
        }
        Some(scene::geo_world_matrix(&self.doc, &self.registry, &self.previews, geo).into())
    }

    /// What a viewport gizmo should drive, given where the node canvas is.
    ///
    /// This is the whole of the ratified context-sensitive policy, kept engine
    /// side so it is testable and platform-neutral; the host does routing and
    /// arithmetic only.
    ///
    /// - **Root**: the selected `geo`'s OWN transform. The renderer applies it as
    ///   the object transform, so a drag costs one small buffer write -- no cook,
    ///   no re-upload, and it works on any geo including a heavy import.
    /// - **Subflow**: the tail `transform` inside that geo (reuse-or-append, see
    ///   [`Command::EnsureTransformTarget`]), which BAKES into the points. That
    ///   is the SOP-level transform, and it is what a modeller diving into the
    ///   subflow is asking for.
    ///
    /// `parent` maps a world-space drag delta into the target's own space: it is
    /// identity at root (the geo's translate IS world), and the geo's world
    /// matrix inside a subflow (where the transform node's translate is local to
    /// the container).
    /// Reads a transform-carrying node's params, previews included so the gizmo
    /// tracks the object mid-drag. Only `transform` declares a `pivot`.
    fn node_transform(
        &self,
        node: NodeId,
        params: &BTreeMap<String, ParamSource>,
        type_id: &str,
    ) -> Option<NodeTransform> {
        let desc = self.registry.get(type_id)?;
        let effective = crate::previews::effective_params(&self.previews, node, params);
        let resolved = crate::registry::resolve::resolve_params(&effective, &desc.params).ok()?;
        Some(NodeTransform::read(&resolved, type_id == "transform"))
    }

    #[must_use]
    pub fn gizmo_target(&self, ctx: GraphContext) -> Option<GizmoTarget> {
        match ctx {
            GraphContext::Root => {
                let root = self.doc.graph(GraphContext::Root).ok()?;
                // Exactly one selected node, and it must be a geo.
                let &[selected] = root.selection.as_slice() else {
                    return None;
                };
                let node = root.node(selected)?;
                if node.type_id != "geo" {
                    return None;
                }
                let xf = self.node_transform(selected, &node.params, "geo")?;

                // A geo IS its own parent frame at root, so a world drag delta
                // lands 1:1 on its translate.
                Some(GizmoTarget {
                    ctx: GraphContext::Root,
                    node: selected,
                    translate: xf.translate,
                    rotate: xf.rotate_deg,
                    rotate_order: xf.order,
                    scale: xf.scale,
                    uniform_scale: xf.uniform_scale,
                    pivot: xf.pivot,
                    anchor: gizmo_frame(xf.pivot_point(), xf.basis()).into(),
                    basis: mat3_to_array(xf.basis()),
                    // A geo has no parent frame: world IS its parent.
                    parent_basis: mat3_to_array(Matrix3::identity()),
                    parent: Matrix4::identity().into(),
                    append_pending: false,
                })
            }
            GraphContext::Subflow(geo) => {
                let sub = self.doc.graph(ctx).ok()?;
                let display = sub.active_output?;
                let tail = sub.node(display)?;

                let geo_node = self.doc.graph(GraphContext::Root).ok()?.node(geo)?;
                let geo_xf = self.node_transform(geo, &geo_node.params, "geo")?;
                let geo_matrix =
                    scene::geo_world_matrix(&self.doc, &self.registry, &self.previews, geo);

                // A bypassed transform passes geometry straight through, so it
                // moves nothing: treat it as absent and append a live one.
                let reusable = tail.type_id == "transform" && !tail.bypassed;
                let xf = if reusable {
                    self.node_transform(display, &tail.params, "transform")?
                } else {
                    // Nothing to read yet: the node is minted at drag start.
                    NodeTransform {
                        translate: [0.0; 3],
                        rotate_deg: [0.0; 3],
                        order: RotateOrder::default(),
                        scale: [1.0; 3],
                        uniform_scale: 1.0,
                        pivot: [0.0; 3],
                    }
                };

                // The gizmo sits on the point the transform actually rotates and
                // scales about, carried out through the container into the world.
                let center = geo_matrix.transform_point(xf.pivot_point());
                // The transform's rotation is expressed inside the container's
                // frame, so the world basis is the two composed.
                let basis = geo_xf.basis() * xf.basis();

                Some(GizmoTarget {
                    ctx,
                    node: display,
                    translate: xf.translate,
                    rotate: xf.rotate_deg,
                    rotate_order: xf.order,
                    scale: xf.scale,
                    uniform_scale: xf.uniform_scale,
                    pivot: xf.pivot,
                    anchor: gizmo_frame(center, basis).into(),
                    basis: mat3_to_array(basis),
                    parent_basis: mat3_to_array(geo_xf.basis()),
                    parent: geo_matrix.into(),
                    append_pending: !reusable,
                })
            }
        }
    }

    /// Cooks every context that has dirty work, under `should_continue`
    /// (native callers pass a wall-clock deadline; the web host a frame
    /// budget). Returns the cook events (status + coalesced stats).
    ///
    /// In `Manual` cook mode the stale set accumulates untouched (the
    /// viewport keeps its last cooked scene) until a `CookNow` arms a cook;
    /// the arm clears once the stale set drains.
    pub fn cook(&mut self, should_continue: &mut dyn FnMut() -> bool) -> Vec<EngineEvent> {
        if self.cook_mode == CookMode::Manual && !self.manual_cook_requested {
            return Vec::new();
        }
        let mut events = Vec::new();
        let mut remaining = 0usize;
        // Root first, then each subflow in id order (deterministic).
        let mut contexts = vec![GraphContext::Root];
        contexts.extend(self.doc.subflow_owners().map(GraphContext::Subflow));
        for ctx in contexts {
            let report = self.cook.cook_until(
                &self.doc,
                &self.registry,
                &self.assets,
                &self.previews,
                ctx,
                should_continue,
            );
            remaining += report.remaining_dirty;
            for (node, status) in report.status_changed {
                events.push(EngineEvent::CookStatus { node, status });
            }
            for (node, stats) in report.stats_changed {
                events.push(EngineEvent::NodeStats {
                    node,
                    points: stats.points,
                    prims: stats.prims,
                    meshes: stats.meshes,
                });
            }
            for (node, validation) in report.validation_changed {
                push_validation_events(&mut events, node, validation.as_deref());
            }
            // Async jobs spawned this pass are queued for the host to
            // dispatch (tagged with their context for `submit_job_result`).
            for (job, request) in report.jobs {
                self.pending_jobs.push((ctx, job, request));
            }
        }
        // A manual cook stays armed until the stale set fully drains.
        if remaining == 0 {
            self.manual_cook_requested = false;
        }
        // Recooked outputs may no longer match anchored geometry hashes.
        self.refresh_review_staleness(&mut events);
        events
    }

    /// The nodes currently dirty (stale) across all contexts, for the
    /// manual-mode stale badges and header count.
    #[must_use]
    pub fn dirty_nodes(&self) -> Vec<NodeId> {
        let mut ids = Vec::new();
        let mut contexts = vec![GraphContext::Root];
        contexts.extend(self.doc.subflow_owners().map(GraphContext::Subflow));
        for ctx in contexts {
            if let Ok(graph) = self.doc.graph(ctx) {
                for node in graph.nodes() {
                    if self.cook.state(node.id) == CookState::Dirty {
                        ids.push(node.id);
                    }
                }
            }
        }
        ids
    }

    /// The cached validation result for a node (a validate node's cook, an
    /// import's load validation), if any. The `Arc` is stable until the
    /// node recooks.
    #[must_use]
    pub fn validation(
        &self,
        node: NodeId,
    ) -> Option<&std::sync::Arc<solarxy_core::validation::ValidationResult>> {
        self.cook.validation(node)
    }

    /// A node's cooked `geometry` output, if committed. The `Arc` is
    /// stable until the node recooks, so callers may dedupe uploads by
    /// pointer identity (the UV pane's selected-node preview).
    #[must_use]
    pub fn geometry_output(
        &self,
        node: NodeId,
    ) -> Option<&std::sync::Arc<solarxy_kernel::GeometrySet>> {
        self.cook.outputs(node)?.get("geometry")?.as_geometry()
    }

    /// The UV pane's source in a context: the first selected node's
    /// committed geometry (subflow contexts only; the root's selection is
    /// geo containers, whose display objects the caller already holds).
    #[must_use]
    pub fn selected_geometry(
        &self,
        ctx: GraphContext,
    ) -> Option<(NodeId, &std::sync::Arc<solarxy_kernel::GeometrySet>)> {
        if ctx == GraphContext::Root {
            return None;
        }
        let node = *self.doc.graph(ctx).ok()?.selection.first()?;
        Some((node, self.geometry_output(node)?))
    }

    /// Drains the async jobs the last cook spawned. The host dispatches each
    /// (to the import worker on web, or resolves it inline via
    /// [`Engine::resolve_job`] natively) and feeds the result back through
    /// [`Engine::submit_job_result`] with the same context.
    pub fn take_jobs(&mut self) -> Vec<(GraphContext, JobId, JobRequest)> {
        std::mem::take(&mut self.pending_jobs)
    }

    /// Fulfills a job synchronously from the staged asset table (the native
    /// path, and the deterministic test harness). On web the import worker
    /// does this off-thread instead.
    #[must_use]
    pub fn resolve_job(&self, request: &JobRequest) -> JobResult {
        match request {
            JobRequest::ParseModel {
                asset,
                format,
                options,
            } => {
                let Some(entry) = self.assets.get(asset) else {
                    return JobResult::Model(Err("asset not staged".to_string()));
                };
                let parsed = crate::nodes::parse_model_validated(
                    format,
                    &entry.bytes,
                    &entry.name,
                    &self.assets,
                    options,
                )
                .map(|(set, validation)| crate::cook::ParsedModel {
                    set,
                    validation: Some(validation),
                });
                JobResult::Model(parsed)
            }
            JobRequest::ValidateGeometry {
                geometry,
                config,
                budget,
            } => {
                let raw = geometry.to_raw();
                let result = solarxy_core::validation::validate_raw_model_with_config(
                    &raw,
                    "",
                    config,
                    &solarxy_core::validation::ValidationThresholds::default(),
                    *budget,
                );
                JobResult::Report(Ok(result))
            }
            JobRequest::DecodeImage { asset } => {
                let Some(entry) = self.assets.get(asset) else {
                    return JobResult::Image(Err("asset not staged".to_string()));
                };
                JobResult::Image(
                    solarxy_formats::decode_image_bytes(&entry.bytes)
                        .map(std::sync::Arc::new)
                        .map_err(|e| e.to_string()),
                )
            }
        }
    }

    /// Feeds an async job result back under the generation guard, cooking
    /// its downstream and returning the resulting events.
    pub fn submit_job_result(
        &mut self,
        ctx: GraphContext,
        job: JobId,
        result: JobResult,
    ) -> Vec<EngineEvent> {
        let mut events = Vec::new();
        let Ok(graph) = self.doc.graph(ctx) else {
            return events;
        };
        let report = self.cook.submit_job_result(&graph.clone(), job, result);
        for (node, status) in report.status_changed {
            events.push(EngineEvent::CookStatus { node, status });
        }
        for (node, stats) in report.stats_changed {
            events.push(EngineEvent::NodeStats {
                node,
                points: stats.points,
                prims: stats.prims,
                meshes: stats.meshes,
            });
        }
        for (node, validation) in report.validation_changed {
            push_validation_events(&mut events, node, validation.as_deref());
        }
        // An async job commit is a geometry change like any cook.
        self.refresh_review_staleness(&mut events);
        events
    }

    /// Drains the accumulated scene delta for the renderer, rebuilding it
    /// from the current committed display outputs and light nodes.
    pub fn take_scene_delta(&mut self) -> SceneDelta {
        self.scene =
            scene::build_scene_delta(&self.doc, &self.registry, &self.cook, &self.previews);
        std::mem::take(&mut self.scene)
    }

    /// Picks the root `geo` container the ray hits nearest over the
    /// committed, world-transformed display geometry (single-pane picking;
    /// pane-awareness is Phase 6). Runs in Rust over CPU-retained geometry,
    /// so nothing crosses into JavaScript. The host builds the ray from the
    /// cursor via `solarxy_core::raycast::screen_to_world_ray`.
    #[must_use]
    pub fn pick(&self, origin: [f32; 3], direction: [f32; 3]) -> Option<NodeId> {
        scene::pick_node(
            &self.doc,
            &self.registry,
            &self.cook,
            &self.previews,
            origin,
            direction,
        )
    }

    /// [`Engine::pick`] with the full hit detail (mesh, face, barycentric,
    /// world point): the anchor source for creating and re-placing review
    /// annotations.
    #[must_use]
    pub fn pick_detailed(&self, origin: [f32; 3], direction: [f32; 3]) -> Option<PickDetail> {
        scene::pick_node_detailed(
            &self.doc,
            &self.registry,
            &self.cook,
            &self.previews,
            origin,
            direction,
        )
    }

    /// Serializes the whole document plus the editor cook mode (the autosave
    /// / explicit-save path). Geometry is not included; the frontend cooks
    /// after a load.
    #[must_use]
    pub fn save_document(&self) -> DocumentFile {
        DocumentFile {
            format_version: 1,
            document: self.doc.to_data(),
            cook_mode: self.cook_mode,
        }
    }

    /// Replaces the whole document from a save file (autosave recovery,
    /// scene open). Resets cook state (preserving the async flag and clock),
    /// re-registers and dirties every node so the next cook rebuilds the
    /// scene, clears undo/previews/pending jobs, advances the revision, and
    /// emits a single `DocumentReplaced` for a full mirror resnapshot.
    pub fn load_document(&mut self, file: &DocumentFile) -> EventBatch {
        self.doc = Document::from_data(&file.document);
        self.cook_mode = file.cook_mode;
        // Arm a cook so the loaded scene populates the viewport once even in
        // Manual mode (there is no last-cooked scene to keep after a load).
        self.manual_cook_requested = true;
        self.cook.reset();
        let mut contexts = vec![GraphContext::Root];
        contexts.extend(self.doc.subflow_owners().map(GraphContext::Subflow));
        for ctx in contexts {
            let ids: Vec<NodeId> = self
                .doc
                .graph(ctx)
                .map(|g| g.nodes().map(|n| n.id).collect())
                .unwrap_or_default();
            for id in ids {
                self.cook.insert_node(id);
                self.mark_dirty(ctx, id);
            }
        }
        self.undo = UndoStack::default();
        self.previews.clear();
        self.pending_jobs.clear();
        self.scene = SceneDelta::default();
        // Fresh document: staleness re-derives after the first cook (until
        // then nothing is displayed, so no annotation is flagged).
        self.review_stale.clear();
        self.review_hash_cache.clear();
        self.revision += 1;
        self.batch(vec![EngineEvent::DocumentReplaced])
    }

    /// The full UI mirror (recovery after desync / structural undo).
    #[must_use]
    pub fn snapshot(&self) -> DocumentSnapshot {
        DocumentSnapshot::capture(&self.doc, &self.review_stale)
    }

    /// The static registry snapshot (fetched once at startup; drives the
    /// palette + parameter panel).
    #[must_use]
    pub fn registry_snapshot(&self) -> RegistrySnapshot {
        RegistrySnapshot::capture(&self.registry)
    }

    #[must_use]
    pub fn cook_mode(&self) -> CookMode {
        self.cook_mode
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// The cook state of a node (test/inspection helper).
    #[must_use]
    pub fn cook_state(&self, node: NodeId) -> CookState {
        self.cook.state(node)
    }

    /// The committed vertex count on a node's default geometry output
    /// (0 if the node has no committed geometry). Inspection helper.
    #[must_use]
    pub fn node_geometry_points(&self, node: NodeId) -> u64 {
        self.cook
            .outputs(node)
            .and_then(|o| o.get("geometry"))
            .and_then(crate::registry::coerce::Value::as_geometry)
            .map_or(0, |g| g.point_count())
    }

    /// Parses a schema-v1 JSON param source under a node's declared type
    /// (the load / SetParam-from-JSON path).
    pub fn param_source_from_json(
        &self,
        ctx: GraphContext,
        node: NodeId,
        key: &str,
        json: &serde_json::Value,
    ) -> Result<ParamSource, EngineError> {
        let type_id = self.node_type_in(ctx, node)?;
        let desc = self
            .registry
            .get(&type_id)
            .ok_or_else(|| EngineError::UnknownNodeType(type_id.clone()))?;
        let spec = desc.param(key).ok_or_else(|| EngineError::InvalidParam {
            key: key.to_string(),
            reason: "no such param".to_string(),
        })?;
        param_source_from_json(json, &spec.ty).map_err(|reason| EngineError::InvalidParam {
            key: key.to_string(),
            reason,
        })
    }

    // Helpers.

    fn mark_dirty(&mut self, ctx: GraphContext, node: NodeId) {
        if let Ok(graph) = self.doc.graph(ctx) {
            self.cook.mark_dirty(graph, node);
        }
    }

    /// The type id of a node found in any context (used by connect, whose
    /// `PortRef` carries only the node id).
    fn node_type(&self, node: NodeId) -> Result<String, EngineError> {
        // Search root then subflows.
        if let Ok(root) = self.doc.graph(GraphContext::Root)
            && let Some(n) = root.node(node)
        {
            return Ok(n.type_id.clone());
        }
        for owner in self.doc.subflow_owners() {
            if let Ok(g) = self.doc.graph(GraphContext::Subflow(owner))
                && let Some(n) = g.node(node)
            {
                return Ok(n.type_id.clone());
            }
        }
        Err(GraphError::UnknownNode(node).into())
    }

    fn node_type_in(&self, ctx: GraphContext, node: NodeId) -> Result<String, EngineError> {
        let graph = self.doc.graph(ctx)?;
        graph
            .node(node)
            .map(|n| n.type_id.clone())
            .ok_or_else(|| GraphError::UnknownNode(node).into())
    }
}

#[cfg(test)]
mod tests;
