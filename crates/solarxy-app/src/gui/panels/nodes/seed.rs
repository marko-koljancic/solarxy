//! Seeding the canvas from the document, and reading positions back out.
//!
//! The canvas is a display mirror, not a second document. The engine owns
//! node positions, edges and selection; this module turns the current
//! graph into what the substrate draws, and turns the one mutation the
//! substrate performs without asking, moving a node, back into a
//! command.
//!
//! ## Why the seed is a rebuild rather than a reconcile
//!
//! The browser canvas reconciles, because its library writes measurement
//! bookkeeping onto the node objects it is handed and a wholesale replace
//! throws that away mid-gesture. This substrate measures every frame and
//! stores nothing on a node but its value, its position and whether it is
//! collapsed, so there is nothing to preserve and a rebuild is both
//! simpler and exactly as correct.
//!
//! ## Why the seed is gated on the revision
//!
//! Re-seeding on every frame would fight the substrate for the position of
//! a node under the pointer: the drag writes a position, the seed writes
//! the stale one back, and the node never moves. So the seed runs when the
//! engine's revision or the shown context has changed, and defers while a
//! pointer button is down, which is the whole of "an in-flight gesture is
//! not disturbed by an unrelated cook".

use std::collections::{BTreeMap, HashMap};

use egui_snarl::Snarl;
use solarxy_graph::cook::state::CookState;
use solarxy_graph::document::{Document, EdgeId, GraphContext, NodeId};
use solarxy_graph::registry::{Arity, Registry};

/// What a node's last cook says about it, which the document does not
/// carry and the canvas has to draw.
///
/// Assembled by the state layer for the shown context alone, because a
/// panel never sees the engine and these five answers come from five
/// different places on it. A context holds a handful of nodes, so a map
/// per frame costs nothing measurable and buys the panel a plain lookup.
#[derive(Debug, Clone, Default)]
pub(crate) struct NodeCook {
    pub state: CookState,
    /// Microseconds the last successful cook took. Zero when there has
    /// not been one.
    pub last_us: u64,
    /// Why the last cook failed, when it did.
    pub error: Option<String>,
    pub errors: u32,
    pub warnings: u32,
}

/// A cooked scene's document and everything drawn beside it that the
/// document does not carry.
///
/// Behind one reference rather than spread across the enum, because
/// [`PanelSources`] is `Copy` and passed by value into every interface
/// pass: seven fields inline would push that struct past the size a lint
/// is standing over, and the fix would be to move the entry point's
/// signature, which is the one thing the panel-source rule promises not
/// to do.
///
/// [`PanelSources`]: crate::gui::pass::PanelSources
pub(crate) struct CanvasScene<'a> {
    pub doc: &'a Document,
    pub registry: &'a Registry,
    /// The engine revision that produced this document. It rides here
    /// because a panel never sees the engine, and re-seeding correctly is
    /// the one thing on the canvas that has to know when the document
    /// moved.
    pub revision: u64,
    /// Per-node cook facts for the shown context.
    pub cook: &'a BTreeMap<NodeId, NodeCook>,
    /// Content hash to file name, so an import node's summary line names
    /// the file rather than its hash. The shells hold the manifest; the
    /// shared rule takes a lookup.
    pub assets: &'a BTreeMap<String, String>,
    /// Manual cook mode, where a dirty node is stale rather than about to
    /// be recooked. The distinction is the whole difference between the
    /// two badges a user reads.
    pub manual: bool,
    /// Whether the clock is running. A cook time that changes sixty times
    /// a second is unreadable, so it is suppressed rather than shown,
    /// which is also what keeps the label stack still.
    pub playing: bool,
}

/// What the canvas draws, or nothing.
#[derive(Clone, Copy)]
pub(crate) enum CanvasSource<'a> {
    /// Nothing is open at all.
    Empty,
    Scene(&'a CanvasScene<'a>),
}

/// One node on the canvas: its engine identifier, and nothing else.
///
/// Everything drawn about a node is read through this into the document on
/// the frame it is drawn. A field cached here would be a second copy of
/// document state, which is the thing the mirror rule exists to prevent.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CanvasNode {
    pub id: NodeId,
}

/// One input or output socket: which declared port it belongs to, and
/// which of that port's slots it is.
///
/// A single-arity port has exactly one slot. A variadic port has one slot
/// per connected edge plus one empty slot to grow into, computed from the
/// document every frame rather than stored, so the socket count cannot
/// disagree with the wiring.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Slot {
    pub port: String,
    /// Which occurrence on that port, always 0 for a single-arity port.
    pub occurrence: usize,
}

/// The input sockets a node shows, in draw order.
#[must_use]
pub(crate) fn input_slots(
    doc: &Document,
    registry: &Registry,
    ctx: GraphContext,
    id: NodeId,
) -> Vec<Slot> {
    let Some(node) = doc.graph(ctx).ok().and_then(|g| g.node(id)) else {
        return Vec::new();
    };
    let Some(desc) = registry.get(&node.type_id) else {
        return Vec::new();
    };
    let mut slots = Vec::with_capacity(desc.inputs.len());
    for spec in &desc.inputs {
        match spec.arity {
            Arity::Single { .. } => slots.push(Slot {
                port: spec.key.clone(),
                occurrence: 0,
            }),
            Arity::Variadic { min } => {
                let connected = node.port_order.get(&spec.key).map_or(0, Vec::len);
                // One past the last, always, so there is somewhere to drop
                // the next wire. `min` is the floor the descriptor asks for.
                let shown = connected.max(min) + 1;
                for occurrence in 0..shown {
                    slots.push(Slot {
                        port: spec.key.clone(),
                        occurrence,
                    });
                }
            }
        }
    }
    slots
}

/// The output sockets a node shows, in draw order. No registered type
/// declares a variadic output, but the rule is written once rather than
/// twice so that one arriving is not a second place to fix.
#[must_use]
pub(crate) fn output_slots(
    doc: &Document,
    registry: &Registry,
    ctx: GraphContext,
    id: NodeId,
) -> Vec<Slot> {
    let Some(node) = doc.graph(ctx).ok().and_then(|g| g.node(id)) else {
        return Vec::new();
    };
    let Some(desc) = registry.get(&node.type_id) else {
        return Vec::new();
    };
    desc.outputs
        .iter()
        .map(|spec| Slot {
            port: spec.key.clone(),
            occurrence: 0,
        })
        .collect()
}

/// What the canvas seeded from, so it can tell when it has to seed again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Seeded {
    revision: u64,
    ctx: GraphContext,
}

/// The canvas's own interface state. Holds no document state: the graph
/// below is the substrate's drawing model, rebuilt from the document, and
/// the maps beside it exist only to translate between the two identifier
/// spaces.
pub(crate) struct CanvasState {
    snarl: Snarl<CanvasNode>,
    /// Engine node to substrate node, rebuilt on every seed.
    to_snarl: HashMap<NodeId, egui_snarl::NodeId>,
    /// What the last seed was made from. `None` before the first one.
    seeded: Option<Seeded>,
    /// Positions as last seeded, which is what a read-back diffs against.
    seeded_pos: HashMap<NodeId, [f32; 2]>,
    /// Where every socket landed on the last frame, handed over once the
    /// canvas has drawn. The marker pass reads it, and a test reads it to
    /// assert that every declared socket got a position.
    sockets: HashMap<super::viewer::PinKey, egui::Pos2>,
    /// Every wire the seed put on the canvas, and the edge it stands
    /// for.
    ///
    /// Kept because the substrate takes a wire off its own graph without
    /// asking, on the one gesture that detaches an endpoint to move it.
    /// Comparing what is there against what was seeded is how that
    /// gesture is noticed at all, and noticing it is what lets a
    /// reconnect be one undo step and a wire dropped on nothing be a
    /// disconnect rather than a wire that silently reappears.
    seeded_wires: HashMap<(egui_snarl::OutPinId, egui_snarl::InPinId), EdgeId>,
    /// The substrate's own selected set as the previous frame left it.
    ///
    /// **Read as a change rather than as a value**, because the
    /// substrate's selection can be read and not written: its type is
    /// private, so a selection made in another panel cannot be pushed
    /// into it. Treating it as authoritative would fight the document
    /// every frame; treating a *change* in it as a gesture is what makes
    /// box selection work without that fight.
    substrate_selection: Vec<egui_snarl::NodeId>,
    /// Each node's layout box, as its own draw recorded it.
    ///
    /// **One frame behind, and it has to be**: the substrate draws a
    /// node's sockets before it draws the node, so a socket asking where
    /// its box is this frame gets no answer. The box is a fixed size and
    /// a node moves only by a drag the canvas is already watching, so a
    /// frame of latency is invisible; the pass a node first appears in
    /// falls through to the geometry recovered from what the socket is
    /// handed, which `the_fallback_recovers_the_box_from_the_edge_it_is_given`
    /// pins.
    boxes: HashMap<egui_snarl::NodeId, egui::Rect>,
}

impl Default for CanvasState {
    fn default() -> Self {
        Self {
            snarl: Snarl::new(),
            to_snarl: HashMap::new(),
            seeded: None,
            seeded_pos: HashMap::new(),
            sockets: HashMap::new(),
            seeded_wires: HashMap::new(),
            substrate_selection: Vec::new(),
            boxes: HashMap::new(),
        }
    }
}

impl CanvasState {
    /// Forget everything, so the next frame seeds from scratch. Called
    /// whenever the open document is replaced, since every identifier held
    /// here addresses nodes the new document need not contain.
    pub(crate) fn reset(&mut self) {
        self.snarl = Snarl::new();
        self.to_snarl.clear();
        self.seeded = None;
        self.seeded_pos.clear();
        self.sockets.clear();
        self.seeded_wires.clear();
        self.substrate_selection.clear();
        self.boxes.clear();
    }

    /// The substrate's graph, for the frame that draws it.
    pub(crate) fn snarl_mut(&mut self) -> &mut Snarl<CanvasNode> {
        &mut self.snarl
    }

    /// Rebuild from the document when the revision or the context has
    /// moved. `pointer_down` defers a seed that would land in the middle of
    /// a gesture; the seed is not lost, it happens on the frame after the
    /// gesture ends, because this is asked every frame.
    pub(crate) fn seed_if_stale(
        &mut self,
        doc: &Document,
        registry: &Registry,
        ctx: GraphContext,
        revision: u64,
        pointer_down: bool,
    ) {
        let want = Seeded { revision, ctx };
        if self.seeded == Some(want) {
            return;
        }
        if pointer_down && self.seeded.is_some_and(|s| s.ctx == ctx) {
            return;
        }
        self.seed(doc, registry, ctx);
        self.seeded = Some(want);
    }

    fn seed(&mut self, doc: &Document, registry: &Registry, ctx: GraphContext) {
        self.snarl = Snarl::new();
        self.to_snarl.clear();
        self.seeded_pos.clear();
        self.seeded_wires.clear();

        let Ok(graph) = doc.graph(ctx) else {
            return;
        };

        for node in graph.nodes() {
            let pos = egui::pos2(node.position[0], node.position[1]);
            let key = self.snarl.insert_node(pos, CanvasNode { id: node.id });
            self.to_snarl.insert(node.id, key);
            self.seeded_pos.insert(node.id, node.position);
        }

        for edge in graph.edges() {
            let (Some(&from), Some(&to)) =
                (self.to_snarl.get(&edge.from), self.to_snarl.get(&edge.to))
            else {
                continue;
            };
            let outputs = output_slots(doc, registry, ctx, edge.from);
            let inputs = input_slots(doc, registry, ctx, edge.to);
            let Some(out_index) = outputs.iter().position(|s| s.port == edge.from_port) else {
                continue;
            };
            let Some(in_index) = slot_index(&inputs, doc, ctx, edge) else {
                continue;
            };
            let out_pin = egui_snarl::OutPinId {
                node: from,
                output: out_index,
            };
            let in_pin = egui_snarl::InPinId {
                node: to,
                input: in_index,
            };
            self.snarl.connect(out_pin, in_pin);
            self.seeded_wires.insert((out_pin, in_pin), edge.id);
        }
    }

    /// The edges the substrate has taken off its own graph since the seed.
    ///
    /// Empty on every frame but the one where a user has grabbed a
    /// connected endpoint to move it. What comes back is the document's
    /// edges, not the substrate's wires, because the document is what a
    /// command has to name.
    pub(super) fn detached_edges(&self) -> Vec<EdgeId> {
        let present: std::collections::HashSet<_> = self.snarl.wires().collect();
        let mut out: Vec<EdgeId> = self
            .seeded_wires
            .iter()
            .filter(|(pins, _)| !present.contains(pins))
            .map(|(_, edge)| *edge)
            .collect();
        out.sort_unstable_by_key(|e| e.0);
        out
    }

    /// Put back whatever the substrate took, without going near the
    /// engine.
    ///
    /// The frame after a gesture that detached a wire and then connected
    /// nothing anywhere is the one case: the document never changed, so
    /// there is no new revision to re-seed on, and the canvas would sit
    /// there missing a wire the scene still has.
    pub(super) fn restore_detached(&mut self) {
        let present: std::collections::HashSet<_> = self.snarl.wires().collect();
        let missing: Vec<_> = self
            .seeded_wires
            .keys()
            .filter(|pins| !present.contains(*pins))
            .copied()
            .collect();
        for (out_pin, in_pin) in missing {
            self.snarl.connect(out_pin, in_pin);
        }
    }

    /// Which nodes the substrate has moved since the seed, and where to.
    /// Empty when nothing moved, which is every frame a user is not
    /// dragging.
    pub(crate) fn moved_nodes(&self) -> Vec<(NodeId, [f32; 2])> {
        let mut moves = Vec::new();
        for (_, pos, node) in self.snarl.nodes_pos_ids() {
            let now = [pos.x, pos.y];
            let was = self.seeded_pos.get(&node.id).copied();
            if was != Some(now) {
                moves.push((node.id, now));
            }
        }
        moves
    }

    /// The engine nodes the substrate has selected, and whether that set
    /// has moved since the last frame.
    ///
    /// Only a change is a gesture. The document is what a selection
    /// actually is, and this set is a buffer the substrate fills as a
    /// user boxes or shift-clicks; reading it every frame as though it
    /// were the truth would undo a selection made anywhere else.
    pub(super) fn substrate_selection_change(
        &mut self,
        selected: Vec<egui_snarl::NodeId>,
    ) -> Option<Vec<NodeId>> {
        if selected == self.substrate_selection {
            return None;
        }
        self.substrate_selection.clone_from(&selected);
        let mut ids: Vec<NodeId> = selected
            .iter()
            .filter_map(|key| self.snarl.get_node(*key).map(|node: &CanvasNode| node.id))
            .collect();
        ids.sort_unstable_by_key(|id| id.0);
        Some(ids)
    }

    /// Put every node back where the document has it, and forget the
    /// gesture that moved them.
    ///
    /// A cancelled drag must leave the document alone **and** add nothing
    /// to the history, which means it cannot be applied and undone: it
    /// has to not happen. The substrate has no cancel of its own, so the
    /// positions it wrote are simply overwritten with the seeded ones.
    pub(super) fn cancel_drag(&mut self) {
        for (_, info) in self.snarl.nodes_ids_data_mut() {
            if let Some(position) = self.seeded_pos.get(&info.value.id) {
                info.pos = egui::pos2(position[0], position[1]);
            }
        }
    }

    /// Take over what the frame just drew: where every socket landed,
    /// and where every node's box was.
    pub(super) fn accept_frame(
        &mut self,
        sockets: HashMap<super::viewer::PinKey, egui::Pos2>,
        boxes: HashMap<egui_snarl::NodeId, egui::Rect>,
    ) {
        self.sockets = sockets;
        self.boxes = boxes;
    }

    /// The boxes the previous frame recorded, for the sockets this one
    /// draws.
    pub(super) fn boxes(&self) -> HashMap<egui_snarl::NodeId, egui::Rect> {
        self.boxes.clone()
    }

    /// Where a socket landed, or `None` on the frame before it was first
    /// drawn.
    pub(super) fn socket_at(&self, key: &super::viewer::PinKey) -> Option<egui::Pos2> {
        self.sockets.get(key).copied()
    }

    /// How many sockets the last frame placed. Zero before the first
    /// frame, and after that the number the document declares.
    ///
    /// Nothing in the shipped path asks: the marker pass looks sockets up
    /// one at a time. It exists so a test can assert that the canvas
    /// placed every socket the document declares rather than none, which
    /// is the failure the box handover actually has.
    #[cfg(test)]
    pub(super) fn socket_count(&self) -> usize {
        self.sockets.len()
    }

    /// The socket one end of an edge lands on, as the marker pass keys
    /// them.
    ///
    /// The occurrence within a variadic port is what makes this more than
    /// a port lookup: a merge's third wire arrives at its third socket,
    /// and marking the first would put the tick on the wrong line.
    pub(super) fn socket_key(
        &self,
        node: NodeId,
        port: &str,
        input: bool,
        doc: &Document,
        registry: &Registry,
        ctx: GraphContext,
        occurrence: usize,
    ) -> Option<super::viewer::PinKey> {
        let slots = if input {
            input_slots(doc, registry, ctx, node)
        } else {
            output_slots(doc, registry, ctx, node)
        };
        let index = slots
            .iter()
            .position(|s| s.port == port && s.occurrence == occurrence)?;
        Some(super::viewer::PinKey {
            node: *self.to_snarl.get(&node)?,
            input,
            index,
        })
    }

    /// Accept a set of moves as the new baseline, so the frame after a
    /// commit does not raise the same command again while the engine's new
    /// revision is still on its way.
    pub(crate) fn accept_moves(&mut self, moves: &[(NodeId, [f32; 2])]) {
        for (id, pos) in moves {
            self.seeded_pos.insert(*id, *pos);
        }
    }
}

/// The input slot an edge lands on: its position in the port's edge order
/// for a variadic port, the port's single slot otherwise.
fn slot_index(
    slots: &[Slot],
    doc: &Document,
    ctx: GraphContext,
    edge: &solarxy_graph::document::Edge,
) -> Option<usize> {
    let occurrence = doc
        .graph(ctx)
        .ok()
        .and_then(|g| g.node(edge.to))
        .and_then(|n| n.port_order.get(&edge.to_port))
        .and_then(|order| order.iter().position(|e| *e == edge.id))
        .unwrap_or(0);
    slots
        .iter()
        .position(|s| s.port == edge.to_port && s.occurrence == occurrence)
}
