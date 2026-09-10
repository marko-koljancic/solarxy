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

use egui::{Sense, Stroke, epaint::CornerRadiusF32};
use egui_snarl::ui::{PinInfo, SnarlPin, SnarlViewer};
use egui_snarl::{InPin, OutPin, Snarl};
use solarxy_graph::cook::state::CookState;
use solarxy_graph::document::{GraphContext, NodeData, NodeId};
use solarxy_graph::registry::{NodeRole, NodeTypeDescriptor};
use solarxy_studio::types;

use super::art::{self, NODE_BOX, NodeVisual};
use super::seed::{CanvasNode, CanvasScene, NodeCook, input_slots, output_slots};
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
        self.scale = to_global.scaling;
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
        _pin: &InPin,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) -> impl SnarlPin + 'static {
        PinInfo::circle()
    }

    fn show_output(
        &mut self,
        _pin: &OutPin,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) -> impl SnarlPin + 'static {
        PinInfo::circle()
    }

    fn has_body(&mut self, _node: &CanvasNode) -> bool {
        false
    }

    fn has_footer(&mut self, _node: &CanvasNode) -> bool {
        false
    }

    // The four mutation points. Empty bodies rather than absent ones: the
    // default implementations mutate the substrate's own wire set, and
    // inheriting even one of them would put a wire on the canvas that no
    // command ever created.
    fn connect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<CanvasNode>) {}

    fn disconnect(&mut self, _from: &OutPin, _to: &InPin, _snarl: &mut Snarl<CanvasNode>) {}

    fn drop_outputs(&mut self, _pin: &OutPin, _snarl: &mut Snarl<CanvasNode>) {}

    fn drop_inputs(&mut self, _pin: &InPin, _snarl: &mut Snarl<CanvasNode>) {}
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
