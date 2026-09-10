//! The substrate's view of one graph, and the rule that keeps the engine
//! the single writer.
//!
//! Every method here reads. The four the substrate calls to mutate its own
//! graph (`connect`, `disconnect`, `drop_inputs` and `drop_outputs`)
//! deliberately do nothing yet and, when they do something, will record a
//! command and still not mutate. One of them calling through would leave
//! the canvas holding a wire the document does not have, which is the
//! precise shape of the disagreement between the two shells that this
//! canvas is built to avoid.

use egui_snarl::ui::{PinInfo, SnarlPin, SnarlViewer};
use egui_snarl::{InPin, OutPin, Snarl};
use solarxy_graph::document::{Document, GraphContext};
use solarxy_graph::registry::Registry;

use super::seed::{CanvasNode, input_slots, output_slots};
use crate::gui::theme::Theme;

/// What one frame of the canvas is drawn against.
///
/// The document and the registry by shared reference, and nothing else,
/// which is the panel discipline the whole shell runs on: draw from a
/// read-only borrow and never see the engine. The intent queue joins it
/// when the mutation points below have something to raise.
pub(super) struct CanvasViewer<'a> {
    pub doc: &'a Document,
    pub registry: &'a Registry,
    pub ctx: GraphContext,
    pub theme: Theme,
}

impl SnarlViewer<CanvasNode> for CanvasViewer<'_> {
    fn title(&mut self, node: &CanvasNode) -> String {
        self.doc
            .graph(self.ctx)
            .ok()
            .and_then(|g| g.node(node.id))
            .map(|n| solarxy_graph::naming::node_name(n, self.registry))
            .unwrap_or_default()
    }

    /// Asked every frame and answered from the document, which is what
    /// makes a variadic port grow a socket the moment its last one fills
    /// without anything here storing a count that could disagree.
    fn inputs(&mut self, node: &CanvasNode) -> usize {
        input_slots(self.doc, self.registry, self.ctx, node.id).len()
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        output_slots(self.doc, self.registry, self.ctx, node.id).len()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> impl SnarlPin + 'static {
        let label = snarl
            .get_node(pin.id.node)
            .map(|node| input_slots(self.doc, self.registry, self.ctx, node.id))
            .and_then(|slots| slots.get(pin.id.input).map(|s| s.port.clone()))
            .unwrap_or_default();
        ui.label(
            egui::RichText::new(label)
                .size(10.0)
                .color(self.theme.muted),
        );
        PinInfo::circle()
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> impl SnarlPin + 'static {
        let label = snarl
            .get_node(pin.id.node)
            .map(|node| output_slots(self.doc, self.registry, self.ctx, node.id))
            .and_then(|slots| slots.get(pin.id.output).map(|s| s.port.clone()))
            .unwrap_or_default();
        ui.label(
            egui::RichText::new(label)
                .size(10.0)
                .color(self.theme.muted),
        );
        PinInfo::circle()
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
