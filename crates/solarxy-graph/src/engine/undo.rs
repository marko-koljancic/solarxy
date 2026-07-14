//! The hybrid transactional undo stack.
//!
//! Most commands invert to a single scalar inverse op (restore a param,
//! move back, restore the previous bypass/active-output/order). Destructive
//! structural ops need a fragment snapshot: removing a node (especially a
//! `geo` container that owns a whole subflow) inverts to restoring a
//! [`GraphFragment`] under its **original** ids (`PreserveIds`), because
//! variadic `port_order` references edge ids and a fresh id would corrupt
//! it. `Disconnect` inverts by re-adding the edge with its original id.
//!
//! Undo and redo are symmetric: applying an inverse op returns *its*
//! inverse, so undoing records the redo transaction and vice versa. Within
//! a transaction, consecutive param edits on the same (node, key) coalesce
//! to the earliest "before" value, so one drag is one undo step.

use crate::document::{Edge, EdgeId, GraphContext, GraphFragment, InsertMode, NodeId};
use crate::params::ParamSource;

/// One reversible mutation. Applying it mutates the document and returns
/// the op that would reverse the application (used to build the opposite
/// stack).
#[derive(Debug, Clone)]
pub(super) enum UndoOp {
    /// Restore a param to its previous source (`None` = it was unset).
    RestoreParam {
        ctx: GraphContext,
        node: NodeId,
        key: String,
        prev: Option<ParamSource>,
    },
    MoveNodes {
        ctx: GraphContext,
        moves: Vec<(NodeId, [f32; 2])>,
    },
    SetBypass {
        ctx: GraphContext,
        node: NodeId,
        bypassed: bool,
    },
    SetActiveOutput {
        ctx: GraphContext,
        node: Option<NodeId>,
    },
    SetSelection {
        ctx: GraphContext,
        ids: Vec<NodeId>,
    },
    ReorderVariadic {
        ctx: GraphContext,
        node: NodeId,
        port: String,
        order: Vec<EdgeId>,
    },
    /// Undo of an add / paste / duplicate: remove the created nodes.
    RemoveNodes {
        ctx: GraphContext,
        ids: Vec<NodeId>,
    },
    /// Undo of a connect: remove that one edge.
    RemoveEdge {
        ctx: GraphContext,
        edge: EdgeId,
    },
    /// Undo of a disconnect: re-add the edge with its original id and its
    /// original position in the target's variadic `port_order`.
    ///
    /// `slot` is load-bearing, not cosmetic. `Graph::connect` appends, so
    /// restoring without it puts the wire back at the END of the port order:
    /// `merge` would then concatenate in a different order, and `switch`,
    /// which selects BY INDEX, would silently read a different branch.
    /// `None` for a single-arity target (no order to preserve).
    RestoreEdge {
        ctx: GraphContext,
        edge: Edge,
        to_variadic: bool,
        slot: Option<usize>,
    },
    /// Undo of a remove: restore the captured fragment with original ids,
    /// then re-add any boundary edges (to surviving outside nodes).
    RestoreFragment {
        ctx: GraphContext,
        fragment: GraphFragment,
        boundary_edges: Vec<(Edge, bool)>,
        active_output: Option<NodeId>,
    },
    /// Undo of any annotation edit: restore the whole review store
    /// (annotations are few, so a snapshot is cheap and exact).
    RestoreReview {
        store: crate::review::ReviewStore,
    },
}

impl UndoOp {
    /// Whether this op changes graph structure (nodes/edges), which forces
    /// a `DocumentReplaced` event rather than precise inverse events.
    pub(super) fn is_structural(&self) -> bool {
        matches!(
            self,
            UndoOp::RemoveNodes { .. }
                | UndoOp::RemoveEdge { .. }
                | UndoOp::RestoreEdge { .. }
                | UndoOp::RestoreFragment { .. }
        )
    }

    /// The (node, key) a param op targets, for coalescing.
    pub(super) fn param_target(&self) -> Option<(NodeId, &str)> {
        match self {
            UndoOp::RestoreParam { node, key, .. } => Some((*node, key.as_str())),
            _ => None,
        }
    }
}

/// One undo step: an ordered list of inverse ops. Undoing applies them in
/// reverse; the label groups a drag or marquee move for the UI.
#[derive(Debug, Clone, Default)]
pub(super) struct Transaction {
    pub label: String,
    pub ops: Vec<UndoOp>,
    /// Any op structural forces a coarse `DocumentReplaced` on undo.
    pub structural: bool,
}

impl Transaction {
    pub(super) fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            ops: Vec::new(),
            structural: false,
        }
    }

    /// Records an inverse op, coalescing consecutive param edits on the
    /// same target to the earliest "before" value.
    pub(super) fn record(&mut self, op: UndoOp) {
        if let Some((node, key)) = op.param_target()
            && self
                .ops
                .iter()
                .any(|existing| existing.param_target() == Some((node, key)))
        {
            // Keep the earliest before-value; drop this later one.
            return;
        }
        self.structural |= op.is_structural();
        self.ops.push(op);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }
}

/// The paired undo/redo stacks plus the open explicit transaction, if any.
#[derive(Debug, Default)]
pub(super) struct UndoStack {
    undo: Vec<Transaction>,
    redo: Vec<Transaction>,
    /// An open `BeginTransaction`; commands accumulate into it until
    /// `EndTransaction`.
    open: Option<Transaction>,
}

impl UndoStack {
    /// Begins an explicit transaction (drags, marquee moves).
    pub(super) fn begin(&mut self, label: impl Into<String>) {
        // A nested begin flushes the prior open transaction first.
        self.flush_open();
        self.open = Some(Transaction::new(label));
    }

    /// Ends the open explicit transaction, committing it if non-empty.
    pub(super) fn end(&mut self) {
        self.flush_open();
    }

    /// Takes the open transaction WITHOUT committing it, for a cancelled drag.
    /// The caller applies its inverse ops and discards the result, so neither
    /// stack is touched.
    pub(super) fn take_open(&mut self) -> Option<Transaction> {
        self.open.take()
    }

    fn flush_open(&mut self) {
        if let Some(txn) = self.open.take()
            && !txn.is_empty()
        {
            self.undo.push(txn);
            self.redo.clear();
        }
    }

    /// Records one command's inverse ops. If a transaction is open they
    /// accumulate into it; otherwise they form their own one-shot step.
    pub(super) fn push_command(&mut self, label: impl Into<String>, ops: Vec<UndoOp>) {
        if ops.is_empty() {
            return;
        }
        if let Some(open) = &mut self.open {
            for op in ops {
                open.record(op);
            }
        } else {
            let mut txn = Transaction::new(label);
            for op in ops {
                txn.record(op);
            }
            if !txn.is_empty() {
                self.undo.push(txn);
                self.redo.clear();
            }
        }
    }

    /// Pops the next transaction to undo (the caller applies it and pushes
    /// the resulting redo transaction via [`Self::push_redo`]).
    pub(super) fn pop_undo(&mut self) -> Option<Transaction> {
        self.flush_open();
        self.undo.pop()
    }

    pub(super) fn push_redo(&mut self, txn: Transaction) {
        if !txn.is_empty() {
            self.redo.push(txn);
        }
    }

    pub(super) fn pop_redo(&mut self) -> Option<Transaction> {
        self.flush_open();
        self.redo.pop()
    }

    pub(super) fn push_undo(&mut self, txn: Transaction) {
        if !txn.is_empty() {
            self.undo.push(txn);
        }
    }
}

/// The `PreserveIds` insert mode is what undo always uses.
pub(super) const UNDO_INSERT: InsertMode = InsertMode::PreserveIds;
