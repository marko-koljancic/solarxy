//! The node canvas: the desktop's editing surface over an open graph.
//!
//! **A display mirror, not a second document.** The engine owns node
//! positions, edges and selection. This panel seeds the substrate's
//! drawing model from them, turns gestures into [`Command`]s through the
//! shell's intent queue, and never writes document state itself. A canvas
//! that kept its own wire state and reconciled later is how two shells
//! begin to disagree, which is the failure this release exists to close
//! rather than to open a second front on.
//!
//! [`Command`]: solarxy_graph::Command
//!
//! ## The one mutation with no hook
//!
//! The substrate moves a node under the pointer without asking, because
//! that is what a drag is. The resolution is seed-and-read-back: positions
//! are compared against what was seeded and raised as one move command per
//! gesture rather than per frame. The browser canvas accepts the same
//! bargain for the same reason.
//!
//! No transaction wraps that drag, and the reason is worth stating because
//! the specification asks for one. [`Command::MoveNodes`] carries every
//! move in a single command, so it is already one undo step; and a node's
//! position reaches no renderer, so there is no live feedback that
//! streaming intermediate positions would preserve. A transaction here
//! would be machinery around a thing that is already atomic.
//!
//! [`Command::MoveNodes`]: solarxy_graph::Command::MoveNodes

mod art;
mod glyphs;
mod pins;
mod seed;
mod vector;
mod viewer;

use std::collections::HashMap;

use solarxy_core::preferences::WireRouting;
use solarxy_graph::document::{EdgeId, GraphContext, NodeId};
use solarxy_graph::engine::PortRefDto;

pub(crate) use seed::{CanvasScene, CanvasSource, CanvasState, NodeCook};

use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;

/// One canvas gesture, raised during an egui pass and drained by
/// `state/intents.rs` after it.
#[derive(Debug, Clone)]
pub(crate) enum CanvasAction {
    /// A drag finished. Every node it moved travels in one command, which
    /// is what makes the whole gesture one undo step.
    MoveNodes(GraphContext, Vec<(NodeId, [f32; 2])>),
    /// The leading wing: switch a node off without removing it.
    SetBypass(GraphContext, NodeId, bool),
    /// The trailing wing inside a container: which node's output the
    /// context shows. A radio, so it is only ever raised to set.
    SetActiveOutput(GraphContext, NodeId),
    /// The trailing wing at the root: an object's additive `visible`
    /// param, which is an ordinary parameter edit and undoes like one.
    SetVisible(GraphContext, NodeId, bool),
    /// The next wire routing. A reading preference, so it changes no
    /// document state and adds nothing to the undo history.
    CycleRouting,
    /// A gesture the coercion matrix would not carry. Named rather than
    /// silently dropped, and with both wire types in it, because that is
    /// the information needed to fix it.
    Refuse(String),
    /// A gesture that happened and is worth saying out loud: a lossy
    /// connection, or a wire dropped on nothing.
    Warn(String),
    /// Every rewiring gesture, in one shape.
    ///
    /// Connect, disconnect, reconnect and drop-to-void differ only in
    /// which halves are present, so they travel as one variant and the
    /// drain wraps a pair in a transaction. Writing them as four
    /// variants would put the one-gesture-one-undo-step rule in four
    /// places, and it is the kind of rule that holds in three of them.
    Rewire {
        ctx: GraphContext,
        /// Edges to remove first, in the order the document holds them.
        remove: Vec<EdgeId>,
        /// The connection to make afterwards.
        add: Option<(PortRefDto, PortRefDto)>,
    },
}

/// Render the node canvas into `ui` (the `egui_dock` tab supplies it).
pub(in crate::gui) fn draw_nodes_content(
    ui: &mut egui::Ui,
    source: CanvasSource<'_>,
    state: &mut CanvasState,
    ctx: &mut GraphContext,
    routing: WireRouting,
    intents: &mut Intents,
    theme: Theme,
) {
    let CanvasSource::Scene(scene) = source else {
        state.reset();
        return draw_placeholder(ui, "No document open");
    };
    let (doc, registry) = (scene.doc, scene.registry);

    // A dive whose container has gone falls back to the root rather than
    // leaving the panel blank with no way out. The same rule the Node Tree
    // applies, on the context the two of them now share.
    if doc.graph(*ctx).is_err() {
        *ctx = GraphContext::Root;
    }

    let pointer_down = ui.ctx().input(|i| i.pointer.any_down());
    state.seed_if_stale(doc, registry, *ctx, scene.revision, pointer_down);

    let style = canvas_style(theme);
    let mut canvas_viewer = viewer::CanvasViewer {
        scene,
        ctx: *ctx,
        intents: &mut *intents,
        theme,
        // Replaced before any node is drawn, by the substrate's own
        // transform hook.
        scale: 1.0,
        routing,
        boxes: state.boxes(),
        sockets: HashMap::new(),
        pending: viewer::Pending::default(),
    };
    state
        .snarl_mut()
        .show(&mut canvas_viewer, &style, "solarxy-node-canvas", ui);
    state.accept_frame(canvas_viewer.sockets, canvas_viewer.boxes);
    mark_coercions(ui, doc, registry, *ctx, state, theme);

    let released = ui.ctx().input(|i| i.pointer.any_released());
    resolve_rewiring(canvas_viewer.pending, state, *ctx, released, intents);

    // Cycling the routing is a canvas-scoped binding rather than a global
    // one: the same key types an `s` anywhere a field has focus, so it is
    // claimed only while the pointer is over this panel and nothing is
    // taking text.
    let over = ui.rect_contains_pointer(ui.max_rect());
    if over
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::S))
    {
        intents.panel(PanelIntent::Canvas(CanvasAction::CycleRouting));
    }

    read_back_positions(released, state, *ctx, intents);
}

/// Turn the frame's rewiring gestures into at most one command batch.
///
/// **Four gestures, one shape, and the substrate is why there are four.**
/// A fresh drag onto a socket arrives as a connection. Right-clicking a
/// wire or a socket arrives as a removal. But grabbing a connected
/// endpoint to move it arrives as neither: the library takes that wire off
/// its own graph without asking, so it is noticed by comparing what is on
/// the canvas against what was seeded. Pairing that with the connection
/// that follows is what makes a reconnect one undo step, and pairing it
/// with nothing is what makes a wire dropped on empty space a disconnect
/// rather than a wire that quietly comes back.
fn resolve_rewiring(
    pending: viewer::Pending,
    state: &mut CanvasState,
    ctx: GraphContext,
    released: bool,
    intents: &mut Intents,
) {
    if let Some(message) = pending.refusal {
        intents.panel(PanelIntent::Canvas(CanvasAction::Refuse(message)));
    }
    if let Some(message) = pending.warning {
        intents.panel(PanelIntent::Canvas(CanvasAction::Warn(message)));
    }

    let detached = state.detached_edges();
    // Still dragging a detached endpoint: leave the canvas showing the
    // wire in flight and decide when the gesture ends.
    if !detached.is_empty() && !released && pending.connect.is_none() {
        return;
    }

    let mut remove = pending.remove;
    let dropped_to_void = pending.connect.is_none() && !detached.is_empty();
    remove.extend(detached);
    remove.sort_unstable_by_key(|e| e.0);
    remove.dedup();

    if remove.is_empty() && pending.connect.is_none() {
        return;
    }
    if dropped_to_void {
        intents.panel(PanelIntent::Canvas(CanvasAction::Warn(
            "Disconnected".to_string(),
        )));
    }
    // The canvas is put back the way the document has it, because the
    // command has not been applied yet and the frame after this one seeds
    // from a document that has moved. Without this a refused or failed
    // rewiring would leave the canvas short a wire the scene still has.
    state.restore_detached();
    intents.panel(PanelIntent::Canvas(CanvasAction::Rewire {
        ctx,
        remove,
        add: pending.connect,
    }));
}

/// Mark every wire that does not carry its value across unchanged.
///
/// **A pass of its own, and the substrate is why.** Colour already says
/// what a wire carries, so a second channel is needed for what happens to
/// the value on the way; the library declares a per-wire widget hook for
/// exactly that and never calls it. It draws each wire itself in one
/// colour and one routing, neither of which is free. So the marks are
/// painted afterwards, at the midpoint of the two sockets the canvas
/// recorded as it drew them, which is the same arithmetic the sockets
/// themselves used and therefore lands on the wire rather than beside it.
///
/// Two marks, and neither is a colour: a coerced wire gets one tick
/// across it, a lossy one gets two. A clean wire gets nothing, which is
/// the common case and should stay quiet.
fn mark_coercions(
    ui: &egui::Ui,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &CanvasState,
    theme: Theme,
) {
    let Ok(graph) = doc.graph(ctx) else {
        return;
    };
    for edge in graph.edges() {
        let (Some(from), Some(to)) = (graph.node(edge.from), graph.node(edge.to)) else {
            continue;
        };
        let verdict = solarxy_studio::types::connection_verdict(
            registry,
            &from.type_id,
            &edge.from_port,
            &to.type_id,
            &edge.to_port,
        );
        let ticks = match verdict.coercion {
            Some(solarxy_graph::registry::coerce::Coercion::Lossy) => 2,
            Some(solarxy_graph::registry::coerce::Coercion::Lossless) => 1,
            _ => continue,
        };
        // A wire into a variadic port lands on the socket its edge
        // order gives it, so the tick goes on the line it belongs to.
        let occurrence = to
            .port_order
            .get(&edge.to_port)
            .and_then(|order| order.iter().position(|e| *e == edge.id))
            .unwrap_or(0);
        let (Some(out), Some(inp)) = (
            state.socket_key(edge.from, &edge.from_port, false, doc, registry, ctx, 0),
            state.socket_key(edge.to, &edge.to_port, true, doc, registry, ctx, occurrence),
        ) else {
            continue;
        };
        let (Some(a), Some(b)) = (state.socket_at(&out), state.socket_at(&inp)) else {
            continue;
        };
        paint_ticks(ui.painter(), a, b, ticks, theme);
    }
}

/// One or two short strokes across a wire at its midpoint.
fn paint_ticks(painter: &egui::Painter, a: egui::Pos2, b: egui::Pos2, ticks: usize, theme: Theme) {
    let along = (b - a).normalized();
    let across = egui::vec2(-along.y, along.x) * 4.0;
    let mid = a + (b - a) * 0.5;
    let stroke = egui::Stroke::new(1.5_f32, theme.fg);
    for tick in 0..ticks {
        #[allow(clippy::cast_precision_loss)]
        let offset = along * ((tick as f32) - (ticks as f32 - 1.0) / 2.0) * 4.0;
        painter.line_segment([mid + offset - across, mid + offset + across], stroke);
    }
}

/// Turn whatever the substrate moved into one command, once the gesture
/// that moved it is over.
///
/// The baseline is accepted **before** the intent is raised, so the second
/// run of a twice-run frame finds nothing to raise. The queue is not
/// cleared inside a pass, and an intent raised twice would be applied
/// twice.
fn read_back_positions(
    released: bool,
    state: &mut CanvasState,
    ctx: GraphContext,
    intents: &mut Intents,
) {
    if !released {
        return;
    }
    let moves = state.moved_nodes();
    if moves.is_empty() {
        return;
    }
    state.accept_moves(&moves);
    intents.panel(PanelIntent::Canvas(CanvasAction::MoveNodes(ctx, moves)));
}

/// The substrate's own style. Colour comes from the shared palette through
/// the theme adapter; nothing here authors one.
fn canvas_style(theme: Theme) -> egui_snarl::ui::SnarlStyle {
    let mut style = egui_snarl::ui::SnarlStyle::new();
    style.bg_pattern_stroke = Some(egui::Stroke::new(1.0_f32, theme.border));
    style.select_stoke = Some(egui::Stroke::new(1.0_f32, theme.accent));
    style.select_fill = Some(theme.accent.linear_multiply(0.12));
    // Double-click is the dive gesture, so it must not also be a zoom.
    style.centering = Some(false);
    style
}

fn draw_placeholder(ui: &mut egui::Ui, headline: &str) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(headline).weak());
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gui::theme::Theme;
    use std::collections::BTreeMap;
    use solarxy_core::preferences::ThemeChoice;
    use solarxy_graph::{Command, Engine};

    /// A geo container holding a box and a merge, wired together, plus a
    /// light at the root: two contexts, an edge, and a variadic port.
    fn scene() -> (Engine, NodeId, NodeId, NodeId) {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let _light = added(&mut engine, GraphContext::Root, "point_light");
        let sop = GraphContext::Subflow(geo);
        let boxy = added(&mut engine, sop, "box");
        let merge = added(&mut engine, sop, "merge");
        engine
            .apply(Command::Connect {
                ctx: sop,
                from: solarxy_graph::engine::PortRefDto {
                    node: boxy,
                    port: "geometry".to_string(),
                },
                to: solarxy_graph::engine::PortRefDto {
                    node: merge,
                    port: "inputs".to_string(),
                },
            })
            .expect("a box output feeds a merge input");
        (engine, geo, boxy, merge)
    }

    fn added(engine: &mut Engine, ctx: GraphContext, type_id: &str) -> NodeId {
        let before: Vec<NodeId> = engine
            .document()
            .graph(ctx)
            .expect("context exists")
            .nodes()
            .map(|n| n.id)
            .collect();
        engine
            .apply(Command::AddNode {
                ctx,
                node_type: type_id.to_string(),
                position: [0.0, 0.0],
            })
            .expect("node type is registered and legal here");
        engine
            .document()
            .graph(ctx)
            .expect("context exists")
            .nodes()
            .map(|n| n.id)
            .find(|id| !before.contains(id))
            .expect("exactly one node was added")
    }

    fn theme() -> Theme {
        Theme::from_choice(ThemeChoice::default())
    }

    /// Draw one real interface pass over a real document.
    fn one_frame(
        engine: &Engine,
        state: &mut CanvasState,
        ctx: &mut GraphContext,
        intents: &mut Intents,
        input: egui::RawInput,
    ) {
        let egui_ctx = egui::Context::default();
        let _ = egui_ctx.run(input, |c| {
            egui::CentralPanel::default().show(c, |ui| {
                draw_nodes_content(
                    ui,
                    CanvasSource::Scene(&CanvasScene {
                        doc: engine.document(),
                        registry: engine.registry(),
                        revision: engine.revision(),
                        cook: &BTreeMap::new(),
                        assets: &BTreeMap::new(),
                        manual: false,
                        playing: false,
                    }),
                    state,
                    ctx,
                    WireRouting::default(),
                    intents,
                    theme(),
                );
            });
        });
    }

    /// The criterion in one line: looking at a graph must not change it.
    ///
    /// Drawn twice deliberately. A queue fed from a condition that is
    /// merely true while a panel is open passes the first frame and fails
    /// the second, and egui runs a pass twice on any frame a layout is
    /// still settling, which is the case this is really about.
    #[test]
    fn an_idle_frame_raises_no_commands() {
        let (engine, geo, _, _) = scene();
        let mut state = CanvasState::default();
        let mut ctx = GraphContext::Subflow(geo);
        let mut intents = Intents::default();

        one_frame(
            &engine,
            &mut state,
            &mut ctx,
            &mut intents,
            egui::RawInput::default(),
        );
        one_frame(
            &engine,
            &mut state,
            &mut ctx,
            &mut intents,
            egui::RawInput::default(),
        );

        assert!(
            intents.take_ordered().is_empty(),
            "a canvas nobody touched must ask for nothing"
        );
    }

    /// The seed is the mirror, so what the substrate holds is what the
    /// document holds, wires included.
    #[test]
    fn the_seed_mirrors_the_document() {
        let (engine, geo, _, _) = scene();
        let mut state = CanvasState::default();
        let ctx = GraphContext::Subflow(geo);

        state.seed_if_stale(
            engine.document(),
            engine.registry(),
            ctx,
            engine.revision(),
            false,
        );

        let graph = engine
            .document()
            .graph(ctx)
            .expect("the geo network exists");
        let snarl = state.snarl_mut();
        assert_eq!(snarl.nodes().count(), graph.nodes().count());
        assert_eq!(snarl.wires().count(), graph.edges().count());
    }

    /// A drag in flight must survive an unrelated cook. Re-seeding on
    /// every change rather than on a revision change with the pointer up
    /// is what would clobber it: the seed writes the old position back and
    /// the node never moves.
    #[test]
    fn a_seed_defers_while_the_pointer_is_down() {
        let (mut engine, geo, boxy, _) = scene();
        let mut state = CanvasState::default();
        let ctx = GraphContext::Subflow(geo);
        state.seed_if_stale(
            engine.document(),
            engine.registry(),
            ctx,
            engine.revision(),
            false,
        );

        // The user drags the box somewhere.
        move_in_snarl(&mut state, boxy, [500.0, 500.0]);

        // Something else changes the document underneath the gesture.
        engine
            .apply(Command::AddNode {
                ctx,
                node_type: "sphere".to_string(),
                position: [0.0, 0.0],
            })
            .expect("a sphere is legal in a sop network");

        state.seed_if_stale(
            engine.document(),
            engine.registry(),
            ctx,
            engine.revision(),
            true,
        );
        assert_eq!(
            position_in_snarl(&mut state, boxy),
            Some([500.0, 500.0]),
            "a seed must not land in the middle of a gesture"
        );

        // And the seed is deferred rather than lost.
        state.seed_if_stale(
            engine.document(),
            engine.registry(),
            ctx,
            engine.revision(),
            false,
        );
        assert_eq!(
            position_in_snarl(&mut state, boxy),
            Some([0.0, 0.0]),
            "the frame after the gesture seeds what was deferred"
        );
    }

    /// One gesture, one command, carrying every node it moved.
    #[test]
    fn a_finished_drag_raises_one_move_carrying_every_node() {
        let (engine, geo, boxy, merge) = scene();
        let mut state = CanvasState::default();
        let ctx = GraphContext::Subflow(geo);
        let mut intents = Intents::default();
        state.seed_if_stale(
            engine.document(),
            engine.registry(),
            ctx,
            engine.revision(),
            false,
        );

        move_in_snarl(&mut state, boxy, [10.0, 20.0]);
        move_in_snarl(&mut state, merge, [30.0, 40.0]);

        // Still dragging: nothing is asked for.
        read_back_positions(false, &mut state, ctx, &mut intents);
        assert!(intents.take_ordered().is_empty());

        read_back_positions(true, &mut state, ctx, &mut intents);
        let raised = intents.take_ordered();
        assert_eq!(raised.len(), 1, "one gesture is one command");
        let Some(crate::gui::Intent::Panel(PanelIntent::Canvas(CanvasAction::MoveNodes(
            raised_ctx,
            mut moves,
        )))) = raised.into_iter().next()
        else {
            panic!("the canvas raises a move");
        };
        assert_eq!(raised_ctx, ctx);
        moves.sort_by_key(|(id, _)| id.0);
        let mut expected = vec![(boxy, [10.0, 20.0]), (merge, [30.0, 40.0])];
        expected.sort_by_key(|(id, _)| id.0);
        assert_eq!(moves, expected);

        // The baseline moved with it, so the next pass of the same frame
        // does not raise the same command again.
        read_back_positions(true, &mut state, ctx, &mut intents);
        assert!(
            intents.take_ordered().is_empty(),
            "a twice-run frame must not move a node twice"
        );
    }

    /// A variadic port shows one socket per wire plus one to grow into,
    /// asked for every frame and answered from the document. A stored
    /// count is what would let the canvas disagree with the wiring.
    #[test]
    fn a_variadic_port_grows_a_socket_as_its_last_one_fills() {
        let (mut engine, geo, boxy, merge) = scene();
        let ctx = GraphContext::Subflow(geo);
        let slots =
            |engine: &Engine| seed::input_slots(engine.document(), engine.registry(), ctx, merge);

        // The scene already wired one box into the merge.
        assert_eq!(slots(&engine).len(), 2, "one wire in, one socket spare");

        let second = added(&mut engine, ctx, "sphere");
        engine
            .apply(Command::Connect {
                ctx,
                from: solarxy_graph::engine::PortRefDto {
                    node: second,
                    port: "geometry".to_string(),
                },
                to: solarxy_graph::engine::PortRefDto {
                    node: merge,
                    port: "inputs".to_string(),
                },
            })
            .expect("a merge takes many inputs");
        assert_eq!(slots(&engine).len(), 3, "the spare socket grew a new spare");

        engine.apply(Command::Undo).expect("undo is available");
        assert_eq!(slots(&engine).len(), 2, "and it shrinks back");

        // A single-arity port never grows, however the graph changes.
        assert_eq!(
            seed::input_slots(engine.document(), engine.registry(), ctx, boxy).len(),
            0,
            "a box takes no geometry input"
        );
    }

    /// Every socket the document declares gets a position, which is what
    /// the wire marks are drawn between.
    ///
    /// **Two frames, deliberately.** The substrate draws a node's sockets
    /// before it draws the node, so the box a socket sits on is one frame
    /// behind. Asserting after a single frame passed while this was being
    /// written, because egui had run that frame's closure twice for its
    /// own reasons, which is not something to build on.
    #[test]
    fn every_declared_socket_is_placed_by_the_frame_after_the_first() {
        let (engine, geo, _, _) = scene();
        let mut state = CanvasState::default();
        let mut ctx = GraphContext::Subflow(geo);
        let mut intents = Intents::default();

        for _ in 0..2 {
            one_frame(
                &engine,
                &mut state,
                &mut ctx,
                &mut intents,
                egui::RawInput::default(),
            );
        }

        let graph = engine.document().graph(ctx).expect("the network exists");
        let declared: usize = graph
            .nodes()
            .map(|n| {
                seed::input_slots(engine.document(), engine.registry(), ctx, n.id).len()
                    + seed::output_slots(engine.document(), engine.registry(), ctx, n.id).len()
            })
            .sum();
        assert!(declared > 0, "the fixture declares no sockets at all");
        assert_eq!(
            state.socket_count(),
            declared,
            "the canvas placed a different number of sockets than the document declares"
        );
    }

    /// Every rewiring assertion reads the same way: run the resolution
    /// against a set of gestures and see what it asked the engine for.
    fn raised(intents: &mut Intents) -> Vec<CanvasAction> {
        intents
            .take_ordered()
            .into_iter()
            .filter_map(|intent| match intent {
                crate::gui::Intent::Panel(PanelIntent::Canvas(action)) => Some(action),
                _ => None,
            })
            .collect()
    }

    fn port(node: NodeId, port: &str) -> solarxy_graph::engine::PortRefDto {
        solarxy_graph::engine::PortRefDto {
            node,
            port: port.to_string(),
        }
    }

    fn seeded(engine: &Engine, ctx: GraphContext) -> CanvasState {
        let mut state = CanvasState::default();
        state.seed_if_stale(
            engine.document(),
            engine.registry(),
            ctx,
            engine.revision(),
            false,
        );
        state
    }

    /// One connect gesture, one command, and no transaction: a lone
    /// connection is already atomic and wrapping it would put an empty
    /// pair of markers in the history.
    #[test]
    fn a_connect_gesture_raises_exactly_one_rewiring() {
        let (engine, geo, boxy, merge) = scene();
        let ctx = GraphContext::Subflow(geo);
        let mut state = seeded(&engine, ctx);
        let mut intents = Intents::default();

        let pending = viewer::Pending {
            connect: Some((port(boxy, "geometry"), port(merge, "inputs"))),
            ..viewer::Pending::default()
        };
        resolve_rewiring(pending, &mut state, ctx, true, &mut intents);

        let actions = raised(&mut intents);
        assert_eq!(actions.len(), 1);
        let CanvasAction::Rewire { remove, add, .. } = &actions[0] else {
            panic!("a connect asks for a rewiring");
        };
        assert!(remove.is_empty(), "a connect removes nothing");
        assert!(add.is_some());
    }

    /// A disconnect is one command too, and it names the edge rather than
    /// the ports, so a variadic port's third wire is the one that goes.
    #[test]
    fn a_disconnect_gesture_raises_exactly_one_rewiring() {
        let (engine, geo, _, _) = scene();
        let ctx = GraphContext::Subflow(geo);
        let edge = engine
            .document()
            .graph(ctx)
            .expect("the network exists")
            .edges()
            .next()
            .expect("the fixture is wired")
            .id;
        let mut state = seeded(&engine, ctx);
        let mut intents = Intents::default();

        let pending = viewer::Pending {
            remove: vec![edge],
            ..viewer::Pending::default()
        };
        resolve_rewiring(pending, &mut state, ctx, true, &mut intents);

        let actions = raised(&mut intents);
        assert_eq!(actions.len(), 1);
        let CanvasAction::Rewire { remove, add, .. } = &actions[0] else {
            panic!("a disconnect asks for a rewiring");
        };
        assert_eq!(remove, &vec![edge]);
        assert!(add.is_none());
    }

    /// Moving a connected endpoint is a removal and a connection in one
    /// gesture, and it must be one entry in the history rather than two.
    /// The substrate takes the wire off its own graph without asking, so
    /// the removal is noticed rather than reported.
    #[test]
    fn a_reconnect_is_one_rewiring_carrying_both_halves() {
        let (engine, geo, boxy, merge) = scene();
        let ctx = GraphContext::Subflow(geo);
        let edge = engine
            .document()
            .graph(ctx)
            .expect("the network exists")
            .edges()
            .next()
            .expect("the fixture is wired")
            .id;
        let mut state = seeded(&engine, ctx);
        detach_every_wire(&mut state);
        let mut intents = Intents::default();

        let pending = viewer::Pending {
            connect: Some((port(boxy, "geometry"), port(merge, "inputs"))),
            ..viewer::Pending::default()
        };
        resolve_rewiring(pending, &mut state, ctx, true, &mut intents);

        let actions = raised(&mut intents);
        assert_eq!(actions.len(), 1, "one gesture is one entry");
        let CanvasAction::Rewire { remove, add, .. } = &actions[0] else {
            panic!("a reconnect asks for a rewiring");
        };
        assert_eq!(remove, &vec![edge], "the detached wire is what goes");
        assert!(add.is_some(), "and the new one is what arrives");
    }

    /// A wire dragged off and dropped on nothing disconnects, and says so.
    #[test]
    fn a_wire_dropped_on_nothing_disconnects_and_says_so() {
        let (engine, geo, _, _) = scene();
        let ctx = GraphContext::Subflow(geo);
        let mut state = seeded(&engine, ctx);
        detach_every_wire(&mut state);
        let mut intents = Intents::default();

        resolve_rewiring(
            viewer::Pending::default(),
            &mut state,
            ctx,
            true,
            &mut intents,
        );

        let actions = raised(&mut intents);
        assert_eq!(actions.len(), 2, "the disconnect, and the word for it");
        assert!(
            matches!(&actions[0], CanvasAction::Warn(m) if m == "Disconnected"),
            "a dropped wire must say what happened: {:?}",
            actions[0]
        );
        assert!(matches!(
            &actions[1],
            CanvasAction::Rewire { add: None, remove, .. } if !remove.is_empty()
        ));
    }

    /// A gesture still in flight asks for nothing. Deciding at the moment
    /// the wire leaves its socket would disconnect it before the user had
    /// chosen where to put it.
    #[test]
    fn a_detached_wire_asks_for_nothing_until_the_gesture_ends() {
        let (engine, geo, _, _) = scene();
        let ctx = GraphContext::Subflow(geo);
        let mut state = seeded(&engine, ctx);
        detach_every_wire(&mut state);
        let mut intents = Intents::default();

        resolve_rewiring(
            viewer::Pending::default(),
            &mut state,
            ctx,
            false,
            &mut intents,
        );
        assert!(raised(&mut intents).is_empty());
    }

    /// Whatever the substrate took off its own graph is put back, because
    /// the command has not been applied yet and a refused one never will
    /// be. Without this the canvas sits missing a wire the scene has.
    #[test]
    fn the_canvas_is_put_back_the_way_the_document_has_it() {
        let (engine, geo, _, _) = scene();
        let ctx = GraphContext::Subflow(geo);
        let mut state = seeded(&engine, ctx);
        let wired = state.snarl_mut().wires().count();
        assert_eq!(wired, 1, "the fixture is wired");

        detach_every_wire(&mut state);
        assert_eq!(state.snarl_mut().wires().count(), 0);

        let mut intents = Intents::default();
        resolve_rewiring(
            viewer::Pending::default(),
            &mut state,
            ctx,
            true,
            &mut intents,
        );
        assert_eq!(
            state.snarl_mut().wires().count(),
            wired,
            "the canvas must show what the document holds, not what a gesture left"
        );
    }

    /// The substrate's own detach, reproduced: it drops the wire from its
    /// graph and never tells the viewer.
    fn detach_every_wire(state: &mut CanvasState) {
        let wires: Vec<_> = state.snarl_mut().wires().collect();
        for (out_pin, in_pin) in wires {
            state.snarl_mut().disconnect(out_pin, in_pin);
        }
    }

    fn move_in_snarl(state: &mut CanvasState, node: NodeId, to: [f32; 2]) {
        for (_, info) in state.snarl_mut().nodes_ids_data_mut() {
            if info.value.id == node {
                info.pos = egui::pos2(to[0], to[1]);
                return;
            }
        }
        panic!("node {node:?} is not on the canvas");
    }

    fn position_in_snarl(state: &mut CanvasState, node: NodeId) -> Option<[f32; 2]> {
        state
            .snarl_mut()
            .nodes_pos_ids()
            .find(|(_, _, value)| value.id == node)
            .map(|(_, pos, _)| [pos.x, pos.y])
    }
}
