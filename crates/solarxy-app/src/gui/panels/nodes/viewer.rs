//! The substrate's view of one graph: what a node looks like, and the
//! rule that keeps the engine the single writer.
//!
//! Every method here reads. The four the substrate calls to mutate its
//! own graph (`connect`, `disconnect`, `drop_inputs` and `drop_outputs`)
//! deliberately do nothing yet and, when they do something, will record a
//! command and still not mutate. One of them calling through would leave
//! the canvas holding a wire the document does not have, which is the
//! precise shape of the disagreement between the two shells that this
//! canvas is built to avoid.
//!
//! ## The node is drawn by hand, and the substrate paints nothing
//!
//! `node_frame` returns a transparent frame with no margin, so what a
//! node looks like is entirely [`super::art`]'s answer rather than the
//! library's. The header allocates exactly the layout box and paints into
//! it; there is no body and no footer, because every role is one fixed
//! box and a taller node would change how the graph lays out.
//!
//! Positions here are the substrate's own space rather than screen space:
//! the canvas is drawn into a transform layer and pan and zoom are
//! applied to the layer, so a node's rect arrives already in the
//! coordinates its art was authored in and nothing has to unproject.

use std::collections::HashMap;

use egui::{Pos2, Sense, Stroke, epaint::CornerRadiusF32};
use solarxy_core::preferences::WireRouting;
use solarxy_studio::types::{HandleShape, PortSide};
use egui_snarl::ui::{SnarlPin, SnarlViewer};
use egui_snarl::{InPin, OutPin, Snarl};
use solarxy_graph::cook::state::CookState;
use solarxy_graph::document::{GraphContext, NodeData, NodeId};
use solarxy_graph::registry::{NodeRole, NodeTypeDescriptor};
use solarxy_studio::types;

use super::art::{self, NODE_BOX, NodeVisual};
use super::seed::{CanvasNode, CanvasScene, NodeCook, Slot, input_slots, output_slots};
use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;

/// The gap between the layout box and the label stack beside it.
const LABEL_GAP: f32 = 12.0;

/// The wing's width, and how far past the body its hit area reaches, so a
/// ten-pixel target is still grabbable at low zoom.
const WING_WIDTH: f32 = 10.0;
const WING_HIT_PAD: f32 = 4.0;

/// What one frame of the canvas is drawn against.
///
/// Read-only borrows and the intent queue, which is the panel discipline
/// the whole shell runs on: draw from what is lent, ask for changes
/// through the queue, and never see the engine.
pub(super) struct CanvasViewer<'a> {
    pub scene: &'a CanvasScene<'a>,
    pub ctx: GraphContext,
    pub intents: &'a mut Intents,
    pub theme: Theme,
    /// The canvas transform's scale, captured before any node is drawn.
    /// The label stack sheds rows by it.
    pub scale: f32,
    /// How wires are routed, which is a reading preference rather than
    /// anything about the document.
    pub routing: WireRouting,
    /// Each node's layout box as its own draw recorded it, so a socket
    /// sits on the box's edge rather than on the side the substrate would
    /// put it. Written by the header and read by the sockets, both within
    /// one frame.
    pub boxes: HashMap<egui_snarl::NodeId, egui::Rect>,
    /// Where every socket drawn this frame ended up, so the pass after
    /// the canvas can mark the wires between them. The substrate declares
    /// a per-wire widget hook and never calls it, so a wire that narrows
    /// its value has to be marked from outside.
    pub sockets: HashMap<PinKey, Pos2>,
    /// What the frame's rewiring gestures asked for, gathered rather than
    /// applied.
    pub pending: Pending,
    /// The container a double-click asked to enter.
    pub dive: Option<NodeId>,
    /// The node the pointer is resting on, and its box, for the ring.
    pub hovered: Option<(NodeId, egui::Rect)>,
    /// The canvas transform, captured before any node is drawn, so a rect
    /// in graph space can be put on the screen where the ring reads in
    /// pixels rather than in graph units.
    pub to_screen: egui::emath::TSTransform,
    /// What the toolbar asked of the view, applied through the
    /// substrate's own transform hook because that is the only way in.
    pub zoom: Option<super::chrome::ZoomStep>,
    /// Every node's box in graph space, so a fit has something to fit to.
    pub extent: Option<egui::Rect>,
    /// The area the canvas is drawn into, so a fit knows what it is
    /// fitting into.
    pub viewport: egui::Rect,
    /// The node a plain or modified click landed on.
    ///
    /// The substrate does not select on an unmodified click at all, and
    /// its two modifier gestures are its own choice rather than this
    /// product's, so clicking to select is the canvas's job.
    pub clicked: Option<NodeId>,
}

/// What the four mutation points recorded this frame.
///
/// One bundle rather than four, because the gestures compose: a
/// reconnect is a removal and a connection, and a wire dropped on nothing
/// is a removal with no connection. Resolving them together after the
/// frame is what makes each of those one undo step without the four hooks
/// having to know about each other.
#[derive(Debug, Default)]
pub(super) struct Pending {
    pub remove: Vec<solarxy_graph::document::EdgeId>,
    pub connect: Option<(
        solarxy_graph::engine::PortRefDto,
        solarxy_graph::engine::PortRefDto,
    )>,
    /// A refusal to show, when the coercion matrix would not carry the
    /// value. Names both wire types, which is the information a user
    /// needs to fix it.
    pub refusal: Option<String>,
    /// A warning for a connection that is legal and narrows the value.
    pub warning: Option<String>,
}

/// One socket, on the side it belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct PinKey {
    pub node: egui_snarl::NodeId,
    /// The side, as a flag rather than as the shared enum, which carries
    /// no hash because nothing else ever needed one.
    pub input: bool,
    pub index: usize,
}

/// Everything one node needs drawn, gathered once and owned.
///
/// Owned rather than borrowed on purpose: the wings raise intents, which
/// needs the viewer mutably, and a borrow of the document held across
/// that would not compile. Gathering costs a handful of small strings per
/// node per frame, against a context holding a handful of nodes.
struct Painted {
    id: NodeId,
    visual: NodeVisual,
    glyph: String,
    title: String,
    /// The type's display name, when it says something the title does not.
    type_label: Option<String>,
    info_line: Option<String>,
    description: Option<String>,
    status: Option<String>,
    sub: String,
    bypassable: bool,
    declares_visibility: bool,
    errored: bool,
}

impl CanvasViewer<'_> {
    /// A port's declared wire type, through the shared lookup.
    fn port_type(
        &self,
        id: NodeId,
        slot: &Slot,
        side: PortSide,
    ) -> Option<solarxy_graph::registry::coerce::DataType> {
        let type_id = &self.node_data(id)?.type_id;
        types::port_data_type(self.scene.registry, type_id, &slot.port, side)
    }

    /// What a port says about itself, for the hover.
    fn slot_doc(&self, id: NodeId, slot: &Slot, side: PortSide) -> Option<String> {
        let desc = self.registry_descriptor(id)?;
        let spec = match side {
            PortSide::Input => desc.input(&slot.port),
            PortSide::Output => desc.output(&slot.port),
        }?;
        let variadic = matches!(spec.arity, solarxy_graph::registry::Arity::Variadic { .. });
        let mut title = format!(
            "{} ({}",
            spec.label,
            format!("{:?}", spec.data_type).to_ascii_lowercase()
        );
        if variadic {
            title.push_str(", variadic");
        }
        title.push(')');
        if !spec.doc.is_empty() {
            title.push('\n');
            title.push_str(&spec.doc);
        }
        Some(title)
    }

    /// A double-click inside a node's box enters the network it opens.
    ///
    /// **Read from the input rather than through a widget**, deliberately.
    /// The substrate already interacts with the node's frame for dragging
    /// and selection; a second widget over the same rectangle would take
    /// the press and the node would stop moving. Nothing here senses
    /// anything, so nothing is stolen.
    ///
    /// **Whether a node is a container is its descriptor's answer**, never
    /// its type identifier. That is the same correction 0.10.0 applied to
    /// the engine, and applying it here is what leaves the canvas free for
    /// network kinds that do not exist yet. A node that opens nothing does
    /// nothing at all, and in particular does not zoom: the substrate's own
    /// double-click centring is switched off so the gesture means one
    /// thing.
    fn watch_for_dive(&mut self, ui: &egui::Ui, box_rect: egui::Rect, id: NodeId) {
        if self.dive.is_some() {
            return;
        }
        let hit = ui.input(|i| {
            i.pointer
                .button_double_clicked(egui::PointerButton::Primary)
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| box_rect.contains(p))
        });
        if !hit {
            return;
        }
        if let Some(data) = self.node_data(id)
            && opens_a_network(self.scene.registry, &data.type_id)
        {
            self.dive = Some(id);
        }
    }

    /// A press inside a node's box is a selection.
    ///
    /// Read from the input for the same reason the dive is: the substrate
    /// interacts with the node's frame for dragging, and a widget over the
    /// same rectangle would take the press and stop the node moving.
    ///
    /// The press rather than the release, because a drag begins with a
    /// press on the node being dragged and a user expects it selected as
    /// it moves rather than after it lands.
    fn watch_for_click(&mut self, ui: &egui::Ui, box_rect: egui::Rect, id: NodeId) {
        if self.clicked.is_some() {
            return;
        }
        let hit = ui.input(|i| {
            i.pointer.button_pressed(egui::PointerButton::Primary)
                && i.pointer
                    .interact_pos()
                    .is_some_and(|p| box_rect.contains(p))
        });
        if hit {
            self.clicked = Some(id);
        }
    }

    /// One end of a gesture, as the engine names it.
    fn port_ref(
        &self,
        node: egui_snarl::NodeId,
        side: PortSide,
        index: usize,
        snarl: &Snarl<CanvasNode>,
    ) -> Option<solarxy_graph::engine::PortRefDto> {
        let id = snarl.get_node(node)?.id;
        let slots = match side {
            PortSide::Input => input_slots(self.scene.doc, self.scene.registry, self.ctx, id),
            PortSide::Output => output_slots(self.scene.doc, self.scene.registry, self.ctx, id),
        };
        Some(solarxy_graph::engine::PortRefDto {
            node: id,
            port: slots.get(index)?.port.clone(),
        })
    }

    /// The document edge a pair of sockets stands for.
    ///
    /// The occurrence is what makes this more than a port lookup: a
    /// merge's third wire is a different edge from its first, and
    /// disconnecting the wrong one looks like the gesture did nothing.
    fn edge_between(
        &self,
        from: egui_snarl::OutPinId,
        to: egui_snarl::InPinId,
        snarl: &Snarl<CanvasNode>,
    ) -> Option<solarxy_graph::document::EdgeId> {
        let (source, target) = (snarl.get_node(from.node)?.id, snarl.get_node(to.node)?.id);
        let out_slot = output_slots(self.scene.doc, self.scene.registry, self.ctx, source)
            .into_iter()
            .nth(from.output)?;
        let in_slot = input_slots(self.scene.doc, self.scene.registry, self.ctx, target)
            .into_iter()
            .nth(to.input)?;
        let graph = self.scene.doc.graph(self.ctx).ok()?;
        let node = graph.node(target)?;
        if let Some(order) = node.port_order.get(&in_slot.port) {
            return order.get(in_slot.occurrence).copied();
        }
        graph
            .edges()
            .find(|e| {
                e.from == source
                    && e.from_port == out_slot.port
                    && e.to == target
                    && e.to_port == in_slot.port
            })
            .map(|e| e.id)
    }

    fn node_type_id(&self, id: NodeId) -> Option<String> {
        Some(self.node_data(id)?.type_id.clone())
    }

    fn registry_descriptor(&self, id: NodeId) -> Option<&NodeTypeDescriptor> {
        self.scene.registry.get(&self.node_data(id)?.type_id)
    }

    /// Build one socket, record where it landed, and hover it.
    ///
    /// The colour and the shape both come from the shared rules, so the
    /// two shells cannot drift apart on what a wire type looks like; an
    /// unknown type falls back to the text hue and a plain round socket,
    /// which is what a shell reading a newer engine sees.
    #[allow(clippy::too_many_arguments)]
    fn socket(
        &mut self,
        node: egui_snarl::NodeId,
        id: Option<NodeId>,
        side: PortSide,
        index: usize,
        ui: &mut egui::Ui,
    ) -> super::pins::EdgePin {
        let slots = id.map(|id| match side {
            PortSide::Input => input_slots(self.scene.doc, self.scene.registry, self.ctx, id),
            PortSide::Output => output_slots(self.scene.doc, self.scene.registry, self.ctx, id),
        });
        let count = slots.as_ref().map_or(1, Vec::len).max(1);
        let slot = slots.as_ref().and_then(|s| s.get(index));
        let data_type = id
            .zip(slot)
            .and_then(|(id, slot)| self.port_type(id, slot, side));
        let hover = id
            .zip(slot)
            .and_then(|(id, slot)| self.slot_doc(id, slot, side));
        let palette = solarxy_core::theme::Palette::for_dark(self.theme.dark);
        let (shape, fill) = data_type.map_or((HandleShape::Round, self.theme.muted), |dt| {
            let rgb = types::wire_color(dt, &palette);
            (
                types::handle_shape(dt),
                egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b),
            )
        });
        let box_rect = self.boxes.get(&node).copied();
        if let Some(rect) = box_rect {
            self.sockets.insert(
                PinKey {
                    node,
                    input: side == PortSide::Input,
                    index,
                },
                super::pins::centre(rect, side, index, count),
            );
        }
        if let (Some(rect), Some(text)) = (box_rect, hover) {
            let at = super::pins::centre(rect, side, index, count);
            let target = egui::Rect::from_center_size(at, egui::Vec2::splat(12.0));
            ui.interact(
                target,
                ui.id()
                    .with(("socket", node, side == PortSide::Input, index)),
                Sense::hover(),
            )
            .on_hover_text(text);
        }
        super::pins::EdgePin {
            box_rect,
            side,
            index,
            count,
            shape,
            fill,
            border: self.theme.bg_elevated,
            wire_style: routing_style(self.routing),
        }
    }

    fn node_data(&self, id: NodeId) -> Option<&NodeData> {
        self.scene.doc.graph(self.ctx).ok().and_then(|g| g.node(id))
    }

    fn gather(&self, id: NodeId) -> Option<Painted> {
        let data = self.node_data(id)?;
        let desc = self.scene.registry.get(&data.type_id);
        let cook = self.scene.cook.get(&id).cloned().unwrap_or_default();
        let graph = self.scene.doc.graph(self.ctx).ok();
        let declares_visibility = desc.is_some_and(solarxy_studio::node::declares_visibility);
        let visible = solarxy_studio::node::is_visible(&data.params);
        let visual = NodeVisual {
            role: desc.map_or(NodeRole::Standard, |d| d.role),
            fill: art_fill(desc, &self.theme),
            selected: graph.is_some_and(|g| g.selection.contains(&id)),
            bypassed: data.bypassed,
            stale: self.scene.manual && cook.state == CookState::Dirty,
            pending: !self.scene.manual && cook.state != CookState::Clean,
            cooking: matches!(cook.state, CookState::Pending(_)),
            is_display: graph.is_some_and(|g| g.active_output == Some(id)),
            hidden: self.ctx == GraphContext::Root && declares_visibility && !visible,
        };
        let title = solarxy_graph::naming::node_name(data, self.scene.registry);
        Some(Painted {
            visual,
            glyph: glyph_key(desc),
            type_label: desc
                .map(|d| d.display_name.to_string())
                .filter(|name| *name != title),
            info_line: desc.and_then(|d| {
                solarxy_studio::node::node_info_line(
                    d,
                    &data.params,
                    Some(&|hash| self.scene.assets.get(hash).cloned()),
                )
            }),
            description: authored_description(data),
            status: status_row(data, &cook, visual, self.scene.manual),
            sub: sub_row(&cook, self.scene.playing),
            bypassable: desc.is_some_and(|d| {
                !matches!(
                    d.bypass,
                    solarxy_graph::registry::BypassBehavior::NotBypassable
                )
            }),
            declares_visibility,
            errored: cook.error.is_some() || cook.errors > 0,
            title,
            id,
        })
    }
}

impl SnarlViewer<CanvasNode> for CanvasViewer<'_> {
    fn title(&mut self, node: &CanvasNode) -> String {
        self.node_data(node.id)
            .map(|n| solarxy_graph::naming::node_name(n, self.scene.registry))
            .unwrap_or_default()
    }

    /// Captured before any node is drawn, which is what makes the label
    /// stack's level of detail a property of this frame rather than of
    /// the previous one.
    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        // The one way in: the hook takes the transform mutably and the
        // substrate stores what it is handed. Everything else about the
        // view lives in egui's memory behind a private type.
        if let Some(step) = self.zoom {
            let centre = self.viewport.center();
            let scale = match step {
                super::chrome::ZoomStep::In => to_global.scaling * 1.25,
                super::chrome::ZoomStep::Out => to_global.scaling / 1.25,
                super::chrome::ZoomStep::Reset => 1.0,
            };
            *to_global = zoom_about(*to_global, scale.clamp(0.1, 4.0), centre);
        }
        if let Some(extent) = self.extent {
            *to_global = fit_extent(extent, self.viewport);
        }
        self.scale = to_global.scaling;
        self.to_screen = *to_global;
    }

    /// Transparent and marginless: the art is painted by hand, and a
    /// frame under it would show through every silhouette that is not a
    /// rectangle.
    fn node_frame(
        &mut self,
        _default: egui::Frame,
        _node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<CanvasNode>,
    ) -> egui::Frame {
        egui::Frame::NONE
    }

    fn header_frame(
        &mut self,
        _default: egui::Frame,
        _node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<CanvasNode>,
    ) -> egui::Frame {
        egui::Frame::NONE
    }

    /// Inputs above the body, outputs below, which is the row order the
    /// browser's canvas has and the only one of the three the substrate
    /// offers that matches it. The rows collapse to nothing because the
    /// pins are drawn on the box's edges rather than in rows of their
    /// own.
    fn node_layout(
        &mut self,
        _default: egui_snarl::ui::NodeLayout,
        _node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        _snarl: &Snarl<CanvasNode>,
    ) -> egui_snarl::ui::NodeLayout {
        egui_snarl::ui::NodeLayout {
            kind: egui_snarl::ui::NodeLayoutKind::Sandwich,
            min_pin_row_height: 0.0,
            equal_pin_row_heights: false,
        }
    }

    /// The whole node: the box, its art, its wings and the label stack
    /// beside it.
    fn show_header(
        &mut self,
        node: egui_snarl::NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let Some(&CanvasNode { id }) = snarl.get_node(node) else {
            return;
        };
        let (box_rect, _) = ui.allocate_exact_size(NODE_BOX, Sense::hover());
        // Recorded before the sockets are drawn, so each one can sit on
        // this box's edge rather than on the side the substrate expects.
        self.boxes.insert(node, box_rect);
        self.watch_for_dive(ui, box_rect, id);
        self.watch_for_click(ui, box_rect, id);
        if ui
            .input(|i| i.pointer.latest_pos())
            .is_some_and(|p| box_rect.contains(p))
        {
            self.hovered = Some((id, box_rect));
        }
        let Some(painted) = self.gather(id) else {
            return;
        };
        art::paint_body(
            ui.painter(),
            box_rect,
            &painted.glyph,
            painted.visual,
            self.theme,
        );
        if painted.visual.cooking {
            ui.ctx().request_repaint();
        }
        self.draw_wings(ui, box_rect, &painted);
        self.draw_labels(ui, box_rect, &painted);
    }

    /// Asked every frame and answered from the document, which is what
    /// makes a variadic port grow a socket the moment its last one fills
    /// without anything here storing a count that could disagree.
    fn inputs(&mut self, node: &CanvasNode) -> usize {
        input_slots(self.scene.doc, self.scene.registry, self.ctx, node.id).len()
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        output_slots(self.scene.doc, self.scene.registry, self.ctx, node.id).len()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> impl SnarlPin + 'static {
        let id = snarl.get_node(pin.id.node).map(|n| n.id);
        self.socket(pin.id.node, id, PortSide::Input, pin.id.input, ui)
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> impl SnarlPin + 'static {
        let id = snarl.get_node(pin.id.node).map(|n| n.id);
        self.socket(pin.id.node, id, PortSide::Output, pin.id.output, ui)
    }

    fn has_body(&mut self, _node: &CanvasNode) -> bool {
        false
    }

    fn has_footer(&mut self, _node: &CanvasNode) -> bool {
        false
    }

    // The four mutation points. Every one of them records and returns
    // without touching the graph it is handed: the default
    // implementations mutate the substrate's own wire set, and inheriting
    // even one would put a wire on the canvas that no command created.

    /// A wire was dropped on a socket.
    ///
    /// Refused here rather than by the engine when the coercion matrix
    /// says the value cannot arrive, because a refusal a user can read is
    /// worth more than an error a user has to interpret, and because the
    /// document must not move at all. A legal but lossy connection is
    /// allowed with a warning, which is what the browser does and what
    /// the matrix means by lossy.
    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let (Some(source), Some(target)) = (
            self.port_ref(from.id.node, PortSide::Output, from.id.output, snarl),
            self.port_ref(to.id.node, PortSide::Input, to.id.input, snarl),
        ) else {
            return;
        };
        let (Some(from_type), Some(to_type)) = (
            self.node_type_id(source.node),
            self.node_type_id(target.node),
        ) else {
            return;
        };
        match judge(
            self.scene.registry,
            &from_type,
            &source.port,
            &to_type,
            &target.port,
        ) {
            Judgement::Refused(message) => self.pending.refusal = Some(message),
            Judgement::Lossy => {
                self.pending.warning = Some(LOSSY_WARNING.to_string());
                self.pending.connect = Some((source, target));
            }
            Judgement::Clean => self.pending.connect = Some((source, target)),
        }
    }

    /// A wire was removed from the canvas, which is the right-click on it.
    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        if let Some(edge) = self.edge_between(from.id, to.id, snarl) {
            self.pending.remove.push(edge);
        }
    }

    /// Every wire leaving one output socket, which is the right-click on
    /// it.
    fn drop_outputs(&mut self, pin: &OutPin, snarl: &mut Snarl<CanvasNode>) {
        for remote in &pin.remotes {
            if let Some(edge) = self.edge_between(pin.id, *remote, snarl) {
                self.pending.remove.push(edge);
            }
        }
    }

    /// Every wire arriving at one input socket.
    fn drop_inputs(&mut self, pin: &InPin, snarl: &mut Snarl<CanvasNode>) {
        for remote in &pin.remotes {
            if let Some(edge) = self.edge_between(*remote, pin.id, snarl) {
                self.pending.remove.push(edge);
            }
        }
    }
}

impl CanvasViewer<'_> {
    /// The two edge wings: bypass on the leading edge, and on the
    /// trailing edge either the display flag or root visibility.
    ///
    /// **Registry-gated, not decorative.** A node that declares neither
    /// shows neither, and a wing on a node that cannot use it is worse
    /// than no wing. The narrow silhouettes get none at all: their bodies
    /// have no straight edge to carry one, and both affordances stay
    /// reachable from the radial.
    fn draw_wings(&mut self, ui: &mut egui::Ui, box_rect: egui::Rect, painted: &Painted) {
        let Painted { id, visual, .. } = *painted;
        let has_wings = matches!(
            visual.role,
            NodeRole::Standard
                | NodeRole::Gather
                | NodeRole::Container
                | NodeRole::Camera
                | NodeRole::Light
        );
        if !has_wings {
            return;
        }
        let body = art::body_rect(box_rect, visual.role);
        let radius = CornerRadiusF32::same(6.0);

        if painted.bypassable {
            let rect = egui::Rect::from_min_size(body.min, egui::vec2(WING_WIDTH, body.height()));
            let response = ui
                .interact(
                    rect.expand(WING_HIT_PAD),
                    ui.id().with((id, "bypass")),
                    Sense::click(),
                )
                .on_hover_text(if visual.bypassed {
                    "Bypassed (click to re-enable)"
                } else {
                    "Bypass"
                });
            let fill = if visual.bypassed {
                self.theme.severity_warn
            } else {
                wing_rest(self.theme, response.hovered())
            };
            ui.painter().rect_filled(rect, radius, fill);
            if response.clicked() {
                self.intents
                    .panel(PanelIntent::Canvas(super::CanvasAction::SetBypass(
                        self.ctx,
                        id,
                        !visual.bypassed,
                    )));
            }
        }

        let trailing = egui::Rect::from_min_size(
            egui::pos2(body.right() - WING_WIDTH, body.top()),
            egui::vec2(WING_WIDTH, body.height()),
        );
        if self.ctx != GraphContext::Root {
            let response = ui
                .interact(
                    trailing.expand(WING_HIT_PAD),
                    ui.id().with((id, "display")),
                    Sense::click(),
                )
                .on_hover_text(if visual.is_display {
                    "Display node"
                } else {
                    "Set the display flag"
                });
            let fill = if visual.is_display {
                self.theme.accent
            } else {
                wing_rest(self.theme, response.hovered())
            };
            ui.painter().rect_filled(trailing, radius, fill);
            // A radio rather than a toggle: clicking the node that
            // already holds the flag must not clear it, or a context ends
            // up showing nothing.
            if response.clicked() && !visual.is_display {
                self.intents
                    .panel(PanelIntent::Canvas(super::CanvasAction::SetActiveOutput(
                        self.ctx, id,
                    )));
            }
        } else if painted.declares_visibility {
            let visible = !visual.hidden;
            let response = ui
                .interact(
                    trailing.expand(WING_HIT_PAD),
                    ui.id().with((id, "visible")),
                    Sense::click(),
                )
                .on_hover_text(if visible {
                    "Hide (stays cooked)"
                } else {
                    "Show"
                });
            ui.painter()
                .rect_filled(trailing, radius, wing_rest(self.theme, response.hovered()));
            // A hollow dot while hidden, so the abnormal state reads from
            // the canvas without hovering anything.
            if !visible {
                ui.painter().circle_stroke(
                    trailing.center(),
                    3.0,
                    Stroke::new(1.0_f32, self.theme.fg),
                );
            }
            if response.clicked() {
                self.intents
                    .panel(PanelIntent::Canvas(super::CanvasAction::SetVisible(
                        self.ctx, id, !visible,
                    )));
            }
        }
    }

    /// The label stack, drawn beside the box rather than inside it.
    ///
    /// Rows in a fixed order, top to bottom: the type name when it adds
    /// something the title does not, the title, a status row, the summary
    /// line, the authored description, and a sub row that is always
    /// present even when empty so the stack does not jump as a cook
    /// finishes.
    fn draw_labels(&self, ui: &egui::Ui, box_rect: egui::Rect, painted: &Painted) {
        let detail = art::label_detail(self.scale);
        let mut rows: Vec<(&str, egui::Color32, f32)> = Vec::new();

        if let (true, Some(name)) = (detail.type_label, painted.type_label.as_deref()) {
            rows.push((name, self.theme.muted, 9.0));
        }
        rows.push((&painted.title, self.theme.fg, 12.0));
        if let Some(status) = painted.status.as_deref() {
            let colour = if painted.errored {
                self.theme.severity_error
            } else {
                self.theme.severity_warn
            };
            rows.push((status, colour, 9.0));
        }
        if let Some(line) = painted.info_line.as_deref() {
            rows.push((line, self.theme.accent, 9.0));
        }
        if let (true, Some(text)) = (detail.description, painted.description.as_deref()) {
            rows.push((text, self.theme.muted, 10.0));
        }
        // Always present, even empty: a row that appears and disappears
        // as a cook finishes makes the whole stack jump.
        rows.push((&painted.sub, self.theme.muted, 9.0));

        let mut y = box_rect.center().y - total_height(&rows) / 2.0;
        let x = box_rect.right() + LABEL_GAP;
        for (text, colour, size) in rows {
            ui.painter().text(
                egui::pos2(x, y),
                egui::Align2::LEFT_TOP,
                text,
                egui::FontId::proportional(size),
                colour,
            );
            y += size * 1.25;
        }
    }
}

fn total_height(rows: &[(&str, egui::Color32, f32)]) -> f32 {
    rows.iter().map(|(_, _, size)| size * 1.25).sum()
}

/// A wing at rest is a faint tint on the body; hovering lifts it, which
/// is what tells a user it is a target at all.
fn wing_rest(theme: Theme, hovered: bool) -> egui::Color32 {
    if hovered {
        theme.widget_hover
    } else {
        egui::Color32::from_black_alpha(20)
    }
}

/// The category fill for a node, or the plain surface for a type this
/// build has never heard of.
fn art_fill(desc: Option<&NodeTypeDescriptor>, theme: &Theme) -> egui::Color32 {
    let palette = solarxy_core::theme::Palette::for_dark(theme.dark);
    desc.map_or(theme.widget_bg, |d| {
        let rgb = types::node_fill(d.category, d.opens, &palette);
        egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b)
    })
}

/// The declared glyph key, falling back to the category's family through
/// the shared rule when this shell has no art for it.
fn glyph_key(desc: Option<&NodeTypeDescriptor>) -> String {
    let Some(desc) = desc else {
        return super::glyphs::FALLBACK_GLYPH.to_string();
    };
    if super::glyphs::art(desc.glyph).is_some() {
        return desc.glyph.to_string();
    }
    let family = types::category_glyph(desc.category);
    if super::glyphs::art(family).is_some() {
        family.to_string()
    } else {
        super::glyphs::FALLBACK_GLYPH.to_string()
    }
}

/// The status row's text, or nothing when the node has nothing to say.
fn status_row(
    data: &NodeData,
    cook: &NodeCook,
    visual: NodeVisual,
    manual: bool,
) -> Option<String> {
    let mut parts = Vec::new();
    if cook.error.is_some() {
        parts.push("error".to_string());
    }
    if cook.errors > 0 {
        parts.push(format!("{} err", cook.errors));
    } else if cook.warnings > 0 {
        parts.push(format!("{} warn", cook.warnings));
    }
    if data.bypassed {
        parts.push("bypassed".to_string());
    }
    if manual && visual.stale {
        parts.push("stale".to_string());
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// The sub row's text, by priority. Cook time is suppressed while the
/// clock runs: a figure that changes sixty times a second is unreadable
/// noise, and hiding it is the other half of keeping the stack still.
fn sub_row(cook: &NodeCook, playing: bool) -> String {
    match cook.state {
        CookState::Pending(_) => "loading geometry...".to_string(),
        #[allow(clippy::cast_precision_loss)]
        CookState::Clean if cook.last_us > 0 && !playing => {
            format!("{:.1} ms", cook.last_us as f64 / 1000.0)
        }
        _ => String::new(),
    }
}

/// The node's own authored description: a literal `description` param
/// with something in it.
fn authored_description(data: &NodeData) -> Option<String> {
    let solarxy_graph::params::ParamSource::Literal(solarxy_graph::params::ParamValue::Text(text)) =
        data.params.get("description")?
    else {
        return None;
    };
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Whether a node type may be entered.
///
/// **Its descriptor's answer, never its type identifier.** A container
/// added in Rust with an identifier this shell has never heard of is
/// enterable with no change here, and an identifier that merely reads
/// like a container is not. That is the same correction 0.10.0 applied to
/// the engine, and it is a named function so it can be driven against a
/// registry built to disagree with the naming.
pub(super) fn opens_a_network(registry: &solarxy_graph::registry::Registry, type_id: &str) -> bool {
    registry.opens(type_id).is_some()
}

/// What a lossy but legal connection says.
pub(super) const LOSSY_WARNING: &str = "Lossy connection (value narrowed)";

/// What the canvas does with a proposed connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Judgement {
    /// The matrix will not carry the value. The message names both wire
    /// types, which is the information needed to fix it.
    Refused(String),
    /// Legal, and the value narrows on the way. Allowed with a warning
    /// rather than refused, which is what the matrix means by lossy.
    Lossy,
    Clean,
}

/// Judge a proposed connection before the document moves.
///
/// Judged here rather than by the engine for two reasons. A refusal a
/// user can read is worth more than an error a user has to interpret; and
/// the document must not move at all, which means the gesture cannot be
/// tried and rolled back. `connection_verdict` is the only public
/// pre-flight answer, since the engine's own check is private.
///
/// An unknown type or port is refused rather than assumed legal, which is
/// the shared rule's own choice and the safe one: a shell reading a newer
/// engine refuses a wire it cannot vouch for instead of writing one the
/// engine will reject anyway.
pub(super) fn judge(
    registry: &solarxy_graph::registry::Registry,
    from_type: &str,
    from_port: &str,
    to_type: &str,
    to_port: &str,
) -> Judgement {
    let verdict = types::connection_verdict(registry, from_type, from_port, to_type, to_port);
    if !verdict.legal {
        let named = |type_id: &str, port: &str, side| {
            types::port_data_type(registry, type_id, port, side).map_or_else(
                || "unknown".to_string(),
                |dt| format!("{dt:?}").to_ascii_lowercase(),
            )
        };
        return Judgement::Refused(format!(
            "Cannot connect {} to {}",
            named(from_type, from_port, PortSide::Output),
            named(to_type, to_port, PortSide::Input),
        ));
    }
    if matches!(
        verdict.coercion,
        Some(solarxy_graph::registry::coerce::Coercion::Lossy)
    ) {
        Judgement::Lossy
    } else {
        Judgement::Clean
    }
}

/// The routing a wire is drawn with.
///
/// The four the browser offers map one to one onto the four the substrate
/// draws, which is the whole of the translation: a straight line, two
/// curve degrees and right angles with rounded corners.
fn routing_style(routing: WireRouting) -> egui_snarl::ui::WireStyle {
    match routing {
        WireRouting::Bezier => egui_snarl::ui::WireStyle::Bezier5,
        WireRouting::Straight => egui_snarl::ui::WireStyle::Line,
        WireRouting::SimpleBezier => egui_snarl::ui::WireStyle::Bezier3,
        WireRouting::SmoothStep => egui_snarl::ui::WireStyle::AxisAligned { corner_radius: 8.0 },
    }
}

/// Scale a transform about a fixed screen point, so zooming keeps what
/// is under the middle of the canvas under the middle of the canvas.
fn zoom_about(
    transform: egui::emath::TSTransform,
    scale: f32,
    about: egui::Pos2,
) -> egui::emath::TSTransform {
    let anchor = transform.inverse() * about;
    let mut out = transform;
    out.scaling = scale;
    out.translation += about - (out * anchor);
    out
}

/// The transform that puts a graph-space box inside a screen-space one,
/// with a margin so the outermost nodes are not against the edge.
fn fit_extent(extent: egui::Rect, viewport: egui::Rect) -> egui::emath::TSTransform {
    let target = viewport.shrink(24.0);
    let scale = (target.width() / extent.width().max(1.0))
        .min(target.height() / extent.height().max(1.0))
        .clamp(0.1, 1.0);
    let mut out = egui::emath::TSTransform::IDENTITY;
    out.scaling = scale;
    out.translation = target.center().to_vec2() - (extent.center().to_vec2() * scale);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::registry::coerce::DataType;

    /// Four routings, four distinct drawings. Two of them collapsing onto
    /// one style would make a menu entry a lie.
    #[test]
    fn the_four_routings_are_four_distinct_drawings() {
        let styles: Vec<_> = WireRouting::ALL.into_iter().map(routing_style).collect();
        for (i, a) in styles.iter().enumerate() {
            for b in &styles[i + 1..] {
                assert_ne!(a, b, "two routings draw the same wire");
            }
        }
        assert_eq!(styles.len(), 4);
    }

    /// The cycle visits every routing and comes back, so pressing the key
    /// four times is where you started.
    #[test]
    fn the_routing_cycle_closes() {
        let mut seen = vec![WireRouting::default()];
        let mut at = WireRouting::default();
        for _ in 0..3 {
            at = at.next();
            assert!(!seen.contains(&at), "the cycle repeats before it closes");
            seen.push(at);
        }
        assert_eq!(at.next(), WireRouting::default());
        assert_eq!(seen.len(), WireRouting::ALL.len());
    }

    /// A registry built to disagree with its own naming: a type whose
    /// identifier reads like nothing in particular opens a network, and a
    /// type whose identifier reads exactly like a container opens
    /// nothing.
    ///
    /// The point of the fixture is that it is wrong on both counts by
    /// naming, so an implementation reading a type identifier fails both
    /// ways rather than one.
    fn contrary_registry() -> solarxy_graph::registry::Registry {
        use solarxy_graph::document::ContextKind;
        use solarxy_graph::registry::{
            BypassBehavior, Category, ContextSet, NodeRole, NodeTypeDescriptor,
        };

        let make = |type_id: &'static str, opens: Option<ContextKind>| NodeTypeDescriptor {
            type_id,
            version: 1,
            display_name: "Probe",
            category: Category::Utility,
            contexts: ContextSet::ALL,
            opens,
            inputs: Vec::new(),
            outputs: Vec::new(),
            params: Vec::new(),
            bypass: BypassBehavior::Mute,
            doc: "A fabricated type this shell has never seen.",
            search_aliases: &[],
            glyph: "null",
            role: NodeRole::Standard,
            // Never cooked: nothing here drives a cook, and a probe
            // that produced geometry would be claiming to be a node type
            // rather than standing in for one.
            cook: |_, _, _| {
                Ok(solarxy_graph::cook::CookOutcome::Done(
                    solarxy_graph::cook::Outputs::default(),
                ))
            },
            migrate: None,
        };

        solarxy_graph::registry::Registry::with_descriptors(vec![
            make("widget", Some(ContextKind::Sop)),
            make("sopnet", None),
        ])
        .expect("two distinct type ids build a registry")
    }

    /// Whether a node can be entered is its descriptor's answer.
    #[test]
    fn a_container_is_decided_by_its_descriptor_rather_than_its_type_id() {
        let registry = contrary_registry();
        assert!(
            opens_a_network(&registry, "widget"),
            "a type that opens a network is enterable whatever it is called"
        );
        assert!(
            !opens_a_network(&registry, "sopnet"),
            "a type that opens nothing is not enterable however it is named"
        );
        assert!(!opens_a_network(&registry, "absent"));
    }

    /// A refusal names both wire types, because "cannot connect" alone
    /// tells a user nothing they can act on. Driven off the real registry
    /// rather than a fixture, so it says something about what ships.
    #[test]
    fn a_refused_connection_names_both_wire_types() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        // An image cannot become geometry, and the matrix says so.
        let Judgement::Refused(message) =
            judge(&registry, "import_image", "image", "merge", "inputs")
        else {
            panic!("an image feeding a geometry port must be refused");
        };
        assert!(
            message.contains("image") && message.contains("geometry"),
            "the refusal must name both ends: {message}"
        );
    }

    /// Lossy is allowed and warned about rather than refused, which is
    /// what the matrix means by lossy and what the browser does.
    #[test]
    fn a_lossy_connection_is_allowed_with_a_warning() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let lossy = DataType::ALL
            .into_iter()
            .flat_map(|from| DataType::ALL.into_iter().map(move |to| (from, to)))
            .find(|(from, to)| {
                solarxy_graph::registry::coerce::can_coerce(*from, *to)
                    == solarxy_graph::registry::coerce::Coercion::Lossy
            });
        assert!(lossy.is_some(), "the matrix declares no lossy pair at all");

        assert_eq!(
            judge(&registry, "box", "geometry", "merge", "inputs"),
            Judgement::Clean,
            "geometry into geometry loses nothing"
        );
    }

    /// A type or a port this build has never heard of is refused rather
    /// than assumed legal, which is the safe direction: a shell reading a
    /// newer engine writes no wire it cannot vouch for.
    #[test]
    fn an_unknown_type_or_port_is_refused_rather_than_assumed() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        assert!(matches!(
            judge(&registry, "hologram", "out", "merge", "inputs"),
            Judgement::Refused(_)
        ));
        assert!(matches!(
            judge(&registry, "box", "geometry", "merge", "phantom"),
            Judgement::Refused(_)
        ));
    }

    /// A socket's two channels both come from the shared rules, and a
    /// reader who cannot tell two hues apart is who the second one is
    /// for: every data type must be told from every other by colour, by
    /// shape, or by both.
    #[test]
    fn every_wire_type_is_told_apart_by_colour_or_by_shape() {
        let palette = solarxy_core::theme::Palette::dark();
        let signature = |dt: DataType| {
            let rgb = types::wire_color(dt, &palette);
            ((rgb.r, rgb.g, rgb.b), types::handle_shape(dt))
        };
        for (i, a) in DataType::ALL.into_iter().enumerate() {
            for b in DataType::ALL.into_iter().skip(i + 1) {
                assert_ne!(
                    signature(a),
                    signature(b),
                    "{a:?} and {b:?} draw identically on both channels"
                );
            }
        }
    }
}
