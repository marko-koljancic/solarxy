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
mod chrome;
mod glyphs;
mod layout;
mod list;
mod pins;
mod radial;
mod seed;
mod vector;
mod viewer;

use std::collections::HashMap;

use solarxy_graph::document::{EdgeId, GraphContext, NodeId};
use solarxy_graph::engine::PortRefDto;

pub(crate) use chrome::Toggle as CanvasToggle;
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
    /// What is selected now, as one command. Selection is document state
    /// rather than canvas state, which is what makes it agree across
    /// every panel without any panel telling another.
    SetSelection(GraphContext, Vec<NodeId>),
    /// Remove the selection, in one command and therefore one undo step.
    RemoveNodes(GraphContext, Vec<NodeId>),
    /// A canvas reading preference the toolbar toggled.
    ToggleChrome(chrome::Toggle),
    /// A node's name, which is an ordinary parameter and undoes like one.
    Rename(GraphContext, NodeId, String),
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
#[allow(clippy::too_many_arguments)]
pub(in crate::gui) fn draw_nodes_content(
    ui: &mut egui::Ui,
    source: CanvasSource<'_>,
    state: &mut CanvasState,
    ctx: &mut GraphContext,
    prefs: solarxy_core::preferences::CanvasPrefs,
    intents: &mut Intents,
    theme: Theme,
) {
    let routing = prefs.routing;
    let CanvasSource::Scene(scene) = source else {
        state.reset();
        return draw_placeholder(ui, "No document open");
    };
    let (doc, registry) = (scene.doc, scene.registry);

    // A dive whose container has gone falls back to the root rather than
    // leaving the panel blank with no way out, and the breadcrumb's own
    // lookup is where that happens. Checking the graph exists as well
    // would be a second fallback for the same case and a weaker one: a
    // context whose graph survives its owner passes that check and fails
    // this one.
    draw_breadcrumb(ui, doc, registry, ctx, state, theme);
    let request = chrome::toolbar(ui, prefs, state.list_view, state.last_scale(), theme);
    apply_chrome_request(request, doc, registry, *ctx, state, intents);

    // Rows rather than a graph: the same document, the same selection and
    // the same six operations, read as a list because finding one node
    // among sixty is a scan rather than a search of a plane.
    if state.list_view {
        let picked = list::draw(ui, doc, registry, *ctx, scene.cook, intents, theme);
        apply_row(picked, doc, registry, *ctx, state, ctx, intents);
        draw_rename(
            ui,
            doc,
            registry,
            *ctx,
            state,
            egui::emath::TSTransform::IDENTITY,
            intents,
            theme,
        );
        draw_info(ui, doc, registry, *ctx, state, theme);
        return;
    }

    let pointer_down = ui.ctx().input(|i| i.pointer.any_down());
    state.seed_if_stale(doc, registry, *ctx, scene.revision, pointer_down);

    let (zoom, fit) = state.exchange_view(state.last_scale());
    let extent = fit.then(|| state.graph_extent()).flatten();
    let viewport = ui.max_rect();
    let style = canvas_style(theme, prefs);
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
        dive: None,
        clicked: None,
        hovered: None,
        to_screen: egui::emath::TSTransform::IDENTITY,
        zoom,
        extent,
        viewport,
    };
    state
        .snarl_mut()
        .show(&mut canvas_viewer, &style, "solarxy-node-canvas", ui);
    // Taken apart rather than read field by field: the viewer holds the
    // intent queue mutably, and everything below raises into it.
    let viewer::CanvasViewer {
        sockets,
        boxes,
        pending,
        dive,
        clicked,
        hovered,
        to_screen,
        ..
    } = canvas_viewer;

    state.accept_frame(sockets, boxes);
    mark_coercions(ui, doc, registry, *ctx, state, theme);
    if prefs.minimap
        && let Ok(graph) = doc.graph(*ctx)
    {
        chrome::minimap(ui, viewport, graph, registry, viewport, to_screen, theme);
    }

    let released = ui.ctx().input(|i| i.pointer.any_released());
    resolve_rewiring(pending, state, *ctx, released, intents);

    let over = ui.rect_contains_pointer(ui.max_rect());
    let picked = drive_radial(ui, doc, registry, *ctx, state, hovered, to_screen, theme);
    if picked.is_some() {
        // A wedge took the press, so the click underneath it is the
        // ring's rather than the canvas's.
        apply_wedge(picked, doc, registry, *ctx, state, ctx, intents);
    } else {
        resolve_selection(ui, doc, *ctx, state, clicked, over, intents);
    }

    // Diving is the canvas's own, not the engine's: which graph is on
    // screen is session state that the tree beside it shares, so it is
    // written here rather than asked for through the queue.
    if let Some(node) = dive {
        *ctx = GraphContext::Subflow(node);
        state.reset();
    }

    // Cycling the routing is a canvas-scoped binding rather than a global
    // one: the same key types an `s` anywhere a field has focus, so it is
    // claimed only while the pointer is over this panel and nothing is
    // taking text.
    if over
        && ui
            .ctx()
            .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::S))
    {
        intents.panel(PanelIntent::Canvas(CanvasAction::CycleRouting));
    }

    draw_rename(ui, doc, registry, *ctx, state, to_screen, intents, theme);
    draw_info(ui, doc, registry, *ctx, state, theme);

    // Escape abandons a drag, and abandoning has to mean it never
    // happened: applying the move and undoing it would leave an entry in
    // the history a user did not make.
    if ui
        .ctx()
        .input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape))
    {
        state.cancel_drag();
    }
    read_back_positions(released, prefs.snap, state, *ctx, intents);
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

/// Apply whatever the toolbar asked for.
///
/// The toggles go through the queue so a preference is written and saved
/// in one place; the view swap and the zoom are the canvas's own, since
/// neither is a preference nor a document change.
#[allow(clippy::fn_params_excessive_bools)]
fn apply_chrome_request(
    request: chrome::ChromeRequest,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &mut CanvasState,
    intents: &mut Intents,
) {
    if request.layout
        && let Ok(graph) = doc.graph(ctx)
    {
        let moves = layout::layered(graph, registry);
        if !moves.is_empty() {
            // One command for the whole tidy, which is what makes it one
            // undo entry rather than one per node.
            intents.panel(PanelIntent::Canvas(CanvasAction::MoveNodes(ctx, moves)));
        }
    }
    if let Some(toggle) = request.toggled {
        intents.panel(PanelIntent::Canvas(CanvasAction::ToggleChrome(toggle)));
    }
    if request.view {
        state.list_view = !state.list_view;
    }
    state.request_view(request.zoom, request.fit);
}

/// Turn a list row's action into the same thing the ring would do.
#[allow(clippy::too_many_arguments)]
fn apply_row(
    picked: Option<list::RowAction>,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &mut CanvasState,
    ctx_out: &mut GraphContext,
    intents: &mut Intents,
) {
    let wedge = match picked {
        Some(list::RowAction::Select(node)) => {
            intents.panel(PanelIntent::Canvas(CanvasAction::SetSelection(
                ctx,
                vec![node],
            )));
            return;
        }
        Some(list::RowAction::Rename(node)) => Some((radial::Wedge::Rename, node)),
        Some(list::RowAction::Dive(node)) => Some((radial::Wedge::Dive, node)),
        Some(list::RowAction::Info(node)) => Some((radial::Wedge::Info, node)),
        Some(list::RowAction::Bypass(node, _)) => Some((radial::Wedge::Bypass, node)),
        Some(list::RowAction::Delete(node)) => Some((radial::Wedge::Delete, node)),
        None => None,
    };
    apply_wedge(wedge, doc, registry, ctx, state, ctx_out, intents);
}

/// The path into the network being shown, and the way back out.
///
/// **The same walk the scene tree descends**, in the shared crate, so a
/// breadcrumb here and a breadcrumb there cannot disagree about who owns
/// what. It is skipped at the root, where the path is one crumb long and
/// says nothing a user does not already know.
fn draw_breadcrumb(
    ui: &mut egui::Ui,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: &mut GraphContext,
    state: &mut CanvasState,
    theme: Theme,
) {
    if *ctx == GraphContext::Root {
        return;
    }
    let rows = solarxy_studio::tree::scene_tree(doc, registry);
    let Some((_, crumbs)) = solarxy_studio::tree::subtree(&rows, *ctx) else {
        *ctx = GraphContext::Root;
        state.reset();
        return;
    };
    egui::Frame::new()
        .fill(theme.bg_elevated)
        .inner_margin(egui::Margin::symmetric(6, 4))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let last = crumbs.len() - 1;
                for (i, crumb) in crumbs.iter().enumerate() {
                    if i > 0 {
                        ui.label(egui::RichText::new("/").color(theme.muted).size(10.0));
                    }
                    let text = egui::RichText::new(&crumb.label).size(10.0);
                    if i == last {
                        ui.label(text.color(theme.fg));
                    } else if ui
                        .add(egui::Label::new(text.color(theme.accent)).sense(egui::Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked()
                    {
                        *ctx = crumb.ctx;
                        state.reset();
                    }
                }
            });
        });
    ui.separator();
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

/// The inline rename field, over the node it renames.
///
/// A node's name is an ordinary parameter, so committing one is an
/// ordinary parameter write and undoes like any other edit. Escape
/// abandons without writing, which is what makes trying a name free.
#[allow(clippy::too_many_arguments)]
fn draw_rename(
    ui: &egui::Ui,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &mut CanvasState,
    to_screen: egui::emath::TSTransform,
    intents: &mut Intents,
    theme: Theme,
) {
    let Some((node, _)) = state.rename else {
        return;
    };
    // A node that has gone takes its rename with it.
    if doc.graph(ctx).ok().and_then(|g| g.node(node)).is_none() {
        state.rename = None;
        return;
    }
    let _ = registry;
    let Some(box_rect) = state.screen_box(node, to_screen) else {
        state.rename = None;
        return;
    };

    let mut commit = false;
    let mut cancel = false;
    egui::Area::new(ui.id().with(("rename", node)))
        .order(egui::Order::Foreground)
        .fixed_pos(box_rect.left_bottom() + egui::vec2(0.0, 6.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme.bg_elevated)
                .show(ui, |ui| {
                    let Some((_, text)) = state.rename.as_mut() else {
                        return;
                    };
                    let field = ui.add(
                        egui::TextEdit::singleline(text)
                            .desired_width(140.0)
                            .hint_text("Name"),
                    );
                    field.request_focus();
                    if field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        commit = true;
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel = true;
                    }
                });
        });

    if cancel {
        state.rename = None;
        return;
    }
    if commit && let Some((node, text)) = state.rename.take() {
        let trimmed = text.trim().to_string();
        // An empty name is a cancel rather than a write: a node with no
        // name answers to its display name, and expressions address it by
        // the one it has.
        if !trimmed.is_empty() {
            intents.panel(PanelIntent::Canvas(CanvasAction::Rename(
                ctx, node, trimmed,
            )));
        }
    }
}

/// The node info card: what this node is, what it did, and what it is
/// wired to.
///
/// Every line comes from the shared derivation, so the card says the same
/// thing the browser's does. Modeless and draggable, because it is read
/// beside the graph rather than instead of it.
fn draw_info(
    ui: &egui::Ui,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &mut CanvasState,
    theme: Theme,
) {
    let Some(node) = state.info else {
        return;
    };
    let Ok(graph) = doc.graph(ctx) else {
        state.info = None;
        return;
    };
    let Some(data) = graph.node(node) else {
        state.info = None;
        return;
    };
    let Some(desc) = registry.get(&data.type_id) else {
        state.info = None;
        return;
    };

    let title = solarxy_graph::naming::node_name(data, registry);
    let kind = solarxy_studio::node::describe_kind(desc);
    let summary = solarxy_studio::node::node_info_line(desc, &data.params, None);
    let wiring = solarxy_studio::node::connection_summary(graph, node, registry);

    let mut open = true;
    egui::Window::new(title)
        .id(ui.id().with(("node-info", node)))
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ui.ctx(), |ui| {
            ui.label(egui::RichText::new(kind).color(theme.muted).size(11.0));
            if let Some(line) = summary {
                ui.label(egui::RichText::new(line).color(theme.accent).size(11.0));
            }
            ui.separator();
            ui.label(
                egui::RichText::new(format!(
                    "{} upstream, {} downstream",
                    wiring.upstream, wiring.downstream
                ))
                .size(11.0),
            );
            for port in wiring.inputs.iter().chain(wiring.outputs.iter()) {
                ui.label(
                    egui::RichText::new(format!("{}: {}", port.port, port.nodes.join(", ")))
                        .color(theme.muted)
                        .size(10.0),
                );
            }
        });
    if !open {
        state.info = None;
    }
}

/// Run the hover clock, draw the ring, and answer what a press picked.
///
/// The ring is drawn into the panel's own painter rather than the
/// canvas's transform layer, so it is measured in pixels and reads the
/// same at every zoom. Only its centre comes from the graph.
#[allow(clippy::too_many_arguments)]
fn drive_radial(
    ui: &egui::Ui,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &mut CanvasState,
    hovered: Option<(NodeId, egui::Rect)>,
    to_screen: egui::emath::TSTransform,
    theme: Theme,
) -> Option<(radial::Wedge, NodeId)> {
    let (pointer, pointer_down, pressed, escaped) = ui.input_mut(|i| {
        (
            i.pointer.latest_pos(),
            i.pointer.any_down(),
            i.pointer.any_pressed(),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    state.tick_dwell(
        hovered,
        pointer_down,
        ui.input(|i| i.time) * 1000.0,
        |rect| to_screen * rect,
    );

    let open = state.radial()?;
    if escaped {
        state.close_radial();
        return None;
    }
    // Straying past the grace radius closes it, measured from the same
    // inner radius the band is drawn at so the two cannot disagree.
    if pointer.is_none_or(|at| (at - open.centre).length() > radial::stray_distance(open.radius)) {
        state.close_radial();
        return None;
    }

    let applies = wedge_applies(doc, registry, ctx, open.node);
    let hovered_wedge = radial::draw(ui.painter(), open, applies, pointer, theme);
    ui.ctx().request_repaint();

    if !pressed {
        return None;
    }
    let picked = hovered_wedge.map(|wedge| (wedge, open.node));
    // Any press closes the ring, whether it landed on a wedge or outside
    // it: a menu that survives a click somewhere else is a menu in the way.
    state.close_radial();
    picked
}

/// Which of the six operations apply to one node.
fn wedge_applies(
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    node: NodeId,
) -> radial::Applies {
    let Ok(graph) = doc.graph(ctx) else {
        return radial::Applies::default();
    };
    let Some(data) = graph.node(node) else {
        return radial::Applies::default();
    };
    let desc = registry.get(&data.type_id);
    let root = ctx == GraphContext::Root;
    let declares_visibility = desc.is_some_and(solarxy_studio::node::declares_visibility);
    radial::Applies {
        dive: viewer::opens_a_network(registry, &data.type_id),
        bypass: desc.is_some_and(|d| {
            !matches!(
                d.bypass,
                solarxy_graph::registry::BypassBehavior::NotBypassable
            )
        }),
        display_or_visibility: if root { declares_visibility } else { true },
        root,
        is_display: graph.active_output == Some(node),
        visible: solarxy_studio::node::is_visible(&data.params),
        bypassed: data.bypassed,
    }
}

/// Turn a picked wedge into whatever it means.
#[allow(clippy::too_many_arguments)]
fn apply_wedge(
    picked: Option<(radial::Wedge, NodeId)>,
    doc: &solarxy_graph::document::Document,
    registry: &solarxy_graph::registry::Registry,
    ctx: GraphContext,
    state: &mut CanvasState,
    ctx_out: &mut GraphContext,
    intents: &mut Intents,
) {
    let Some((wedge, node)) = picked else {
        return;
    };
    let applies = doc.graph(ctx).ok().and_then(|g| g.node(node)).map(|data| {
        (
            data.bypassed,
            solarxy_studio::node::is_visible(&data.params),
        )
    });
    match wedge {
        radial::Wedge::Rename => {
            // Seeded with the name the node answers to rather than with
            // nothing, so a rename is an edit rather than a retype.
            let name = doc
                .graph(ctx)
                .ok()
                .and_then(|g| g.node(node))
                .map(|data| solarxy_graph::naming::node_name(data, registry))
                .unwrap_or_default();
            state.rename = Some((node, name));
        }
        radial::Wedge::Info => state.info = Some(node),
        radial::Wedge::Dive => {
            *ctx_out = GraphContext::Subflow(node);
            state.reset();
        }
        radial::Wedge::Bypass => {
            if let Some((bypassed, _)) = applies {
                intents.panel(PanelIntent::Canvas(CanvasAction::SetBypass(
                    ctx, node, !bypassed,
                )));
            }
        }
        radial::Wedge::DisplayOrVisibility => {
            if ctx == GraphContext::Root {
                if let Some((_, visible)) = applies {
                    intents.panel(PanelIntent::Canvas(CanvasAction::SetVisible(
                        ctx, node, !visible,
                    )));
                }
            } else {
                intents.panel(PanelIntent::Canvas(CanvasAction::SetActiveOutput(
                    ctx, node,
                )));
            }
        }
        radial::Wedge::Delete => {
            intents.panel(PanelIntent::Canvas(CanvasAction::RemoveNodes(
                ctx,
                vec![node],
            )));
        }
    }
}

/// Everything a frame did to the selection, as at most one command.
///
/// Three gestures reach here and they compose in one place because the
/// document holds one selection: a press on a node, a press on empty
/// canvas, and a box the substrate drew.
///
/// **The substrate's own selected set is a buffer, not the truth.** Its
/// type is private, so a selection made in the scene tree cannot be
/// pushed into it, and reading it as authoritative every frame would undo
/// that selection immediately. Only a *change* in it is a gesture, which
/// is what lets box selection work without the two fighting. What a user
/// sees selected on the canvas is drawn from the document, so the two
/// surfaces agree whichever of them made the selection.
fn resolve_selection(
    ui: &egui::Ui,
    doc: &solarxy_graph::document::Document,
    ctx: GraphContext,
    state: &mut CanvasState,
    clicked: Option<NodeId>,
    over: bool,
    intents: &mut Intents,
) {
    let Ok(graph) = doc.graph(ctx) else {
        return;
    };
    let current = graph.selection.clone();

    // A box the substrate drew, or a modifier gesture it understood.
    let substrate =
        egui_snarl::ui::get_selected_nodes(ui.id().with("solarxy-node-canvas"), ui.ctx());
    if let Some(ids) = state.substrate_selection_change(substrate)
        && ids != current
    {
        intents.panel(PanelIntent::Canvas(CanvasAction::SetSelection(ctx, ids)));
        return;
    }

    if let Some(node) = clicked {
        // The platform's own modifier, which is what the browser canvas
        // uses too, so one habit serves both shells.
        let additive = ui.input(|i| i.modifiers.command);
        let mut ids = if additive {
            current.clone()
        } else {
            Vec::new()
        };
        if additive && ids.contains(&node) {
            ids.retain(|id| *id != node);
        } else if !ids.contains(&node) {
            ids.push(node);
        }
        if ids != current {
            intents.panel(PanelIntent::Canvas(CanvasAction::SetSelection(ctx, ids)));
        }
        return;
    }

    if !over {
        return;
    }

    // A press on the canvas itself clears the selection, which is how a
    // user says "nothing", and it is raised only when there is something
    // to clear so an idle click asks for nothing.
    let pressed_empty = ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Primary));
    if pressed_empty && !current.is_empty() {
        intents.panel(PanelIntent::Canvas(CanvasAction::SetSelection(
            ctx,
            Vec::new(),
        )));
        return;
    }

    if !current.is_empty()
        && ui.ctx().input_mut(|i| {
            i.consume_key(egui::Modifiers::NONE, egui::Key::Delete)
                || i.consume_key(egui::Modifiers::NONE, egui::Key::Backspace)
        })
    {
        intents.panel(PanelIntent::Canvas(CanvasAction::RemoveNodes(ctx, current)));
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
    snap: bool,
    state: &mut CanvasState,
    ctx: GraphContext,
    intents: &mut Intents,
) {
    if !released {
        return;
    }
    let mut moves = state.moved_nodes();
    if moves.is_empty() {
        return;
    }
    // Snapped at the commit rather than during the drag, which is where
    // the two shells differ and not in a way anybody sees: the browser
    // snaps the node under the pointer, this snaps what the document
    // records, and the document ends up holding the same numbers.
    if snap {
        for (_, position) in &mut moves {
            *position = chrome::snap(*position);
        }
    }
    state.accept_moves(&moves);
    intents.panel(PanelIntent::Canvas(CanvasAction::MoveNodes(ctx, moves)));
}

/// The substrate's own style. Colour comes from the shared palette through
/// the theme adapter; nothing here authors one.
fn canvas_style(
    theme: Theme,
    prefs: solarxy_core::preferences::CanvasPrefs,
) -> egui_snarl::ui::SnarlStyle {
    let mut style = egui_snarl::ui::SnarlStyle::new();
    style.bg_pattern = Some(if prefs.grid {
        egui_snarl::ui::BackgroundPattern::new()
    } else {
        egui_snarl::ui::BackgroundPattern::NoPattern
    });
    style.bg_pattern_stroke = Some(egui::Stroke::new(1.0_f32, theme.border));
    // Sockets on the box's edges, which is what the placement override
    // needs: an edge placement is the one that hands a pin the node's own
    // left or right edge rather than an inset from it.
    style.pin_placement = Some(egui_snarl::ui::PinPlacement::Edge);
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
                    solarxy_core::preferences::CanvasPrefs::default(),
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
        read_back_positions(false, false, &mut state, ctx, &mut intents);
        assert!(intents.take_ordered().is_empty());

        read_back_positions(true, false, &mut state, ctx, &mut intents);
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
        read_back_positions(true, false, &mut state, ctx, &mut intents);
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

    /// A box the substrate drew is one selection command, whatever it
    /// encloses, which is what makes it one undo step.
    #[test]
    fn a_box_selection_is_one_command_for_the_whole_set() {
        let (engine, geo, boxy, merge) = scene();
        let ctx = GraphContext::Subflow(geo);
        let mut state = seeded(&engine, ctx);
        let mut intents = Intents::default();

        let keys: Vec<_> = state
            .snarl_mut()
            .nodes_ids_data()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(keys.len(), 2, "the fixture holds a box and a merge");

        let selected = state
            .substrate_selection_change(keys)
            .expect("a set that was empty has changed");
        let mut expected = vec![boxy, merge];
        expected.sort_unstable_by_key(|id| id.0);
        assert_eq!(selected, expected);

        // And the same set again is no gesture at all, which is what
        // stops the canvas re-raising a selection every frame and undoing
        // one made anywhere else.
        let keys: Vec<_> = state
            .snarl_mut()
            .nodes_ids_data()
            .map(|(key, _)| key)
            .collect();
        assert!(state.substrate_selection_change(keys).is_none());
        assert!(raised(&mut intents).is_empty());
    }

    /// Escape during a drag has to mean the drag never happened. Applying
    /// the move and undoing it would leave an entry in the history the
    /// user did not make.
    #[test]
    fn a_cancelled_drag_leaves_the_document_and_the_history_alone() {
        let (engine, geo, boxy, _) = scene();
        let ctx = GraphContext::Subflow(geo);
        let mut state = seeded(&engine, ctx);
        let mut intents = Intents::default();

        move_in_snarl(&mut state, boxy, [400.0, 300.0]);
        state.cancel_drag();

        assert_eq!(
            position_in_snarl(&mut state, boxy),
            Some([0.0, 0.0]),
            "the node goes back where the document has it"
        );
        read_back_positions(true, false, &mut state, ctx, &mut intents);
        assert!(
            raised(&mut intents).is_empty(),
            "and nothing is asked for, so the history gains nothing"
        );
    }

    /// A dive that no longer resolves falls back to the root rather than
    /// leaving the canvas blank with no way out, which is what a second
    /// scene does to a context recorded against the first.
    #[test]
    fn a_stale_dive_falls_back_to_the_root() {
        let (engine, _, _, _) = scene();
        let mut state = CanvasState::default();
        let mut ctx = GraphContext::Subflow(NodeId(9_999));
        let mut intents = Intents::default();

        one_frame(
            &engine,
            &mut state,
            &mut ctx,
            &mut intents,
            egui::RawInput::default(),
        );

        assert_eq!(ctx, GraphContext::Root, "a dive with no home returns");
        assert!(
            raised(&mut intents).is_empty(),
            "and it asks the engine for nothing on the way"
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
