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
mod seed;
mod vector;
mod viewer;

use solarxy_graph::document::{GraphContext, NodeId};

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
}

/// Render the node canvas into `ui` (the `egui_dock` tab supplies it).
pub(in crate::gui) fn draw_nodes_content(
    ui: &mut egui::Ui,
    source: CanvasSource<'_>,
    state: &mut CanvasState,
    ctx: &mut GraphContext,
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
    };
    state
        .snarl_mut()
        .show(&mut canvas_viewer, &style, "solarxy-node-canvas", ui);

    let released = ui.ctx().input(|i| i.pointer.any_released());
    read_back_positions(released, state, *ctx, intents);
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
