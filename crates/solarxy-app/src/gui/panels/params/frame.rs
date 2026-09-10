//! The panel's frame: which node, what it says about itself, which tabs
//! it has, and which rows are in them.

use egui::Ui;
use solarxy_graph::cook::state::NodeCookStats;
use solarxy_graph::document::{Document, GraphContext, NodeId};
use solarxy_graph::registry::Registry;
use solarxy_graph::registry::param_spec::ParamSpec;
use solarxy_graph::registry::visibility::param_visible;
use solarxy_studio::params::{self, VALIDATION_TAB};

use super::controls::{self, ControlEdit, ControlEnv};
use crate::gui::intent::Intents;
use crate::gui::panels::nodes::CanvasAction;
use crate::gui::theme::Theme;

/// The document and everything the panel draws beside it.
///
/// Behind one reference rather than inline, for the same reason the
/// canvas's scene is: [`PanelSources`] is `Copy` and passed by value into
/// every interface pass, and these six fields push it past the size a
/// lint stands over. The fix on the other side of that lint is to move
/// the entry point's signature, which is the one thing the panel-source
/// rule promises not to do.
///
/// [`PanelSources`]: crate::gui::pass::PanelSources
pub(crate) struct ParamScene<'a> {
    pub doc: &'a Document,
    pub registry: &'a Registry,
    /// The graph both surfaces are showing, so a pin that is not in
    /// it is recognisably out of context rather than silently
    /// editing something else.
    pub ctx: GraphContext,
    /// The node's last cook, when it has had one.
    pub stats: Option<NodeCookStats>,
    /// Whether the node has a validation report at all.
    ///
    /// **Presence, not cleanliness.** A clean validate cook still
    /// stores a report, and gating the tab on the report being dirty
    /// would hide it exactly when someone wants to confirm a model is
    /// clean.
    pub has_report: bool,
    /// Errors and warnings from the whole report, which is a
    /// different number from the rows a list can show.
    pub counts: (u32, u32),
    /// Every staged asset, as its hash and the name it was staged under,
    /// so a file reference reads as a file name. The engine holds the
    /// names and a panel cannot reach the engine.
    pub assets: &'a [(String, String)],
    /// The attribute lanes on the subject's upstream geometry, as name and
    /// type. Resolved against the same input the wrangle's completions
    /// read, because two answers to "which lanes exist here" would be one
    /// too many.
    pub lanes: &'a [(String, String)],
    /// The subject's last cook failure, which is where a snippet's bad
    /// line comes from: a wrangle parse error is a cook error, not a
    /// second channel.
    pub error: Option<&'a str>,
}

/// What the panel draws, or nothing.
#[derive(Clone, Copy)]
pub(crate) enum ParamPanelSource<'a> {
    Empty,
    Scene(&'a ParamScene<'a>),
}

/// The panel's own state: what it is pinned to, and which tab it was on.
#[derive(Debug, Default)]
pub(crate) struct ParamPanelState {
    /// A node the panel follows instead of the selection.
    pin: Option<NodeId>,
    /// The row being typed into, if any. Held here rather than in the
    /// widget because a text editor needs a buffer that survives the
    /// frame, and re-reading the document every frame would discard
    /// every keystroke. See [`super::draft`].
    draft: Option<super::draft::Draft>,
    /// The tab last chosen, by name. Asked of the shared resolver every
    /// frame rather than trusted, because a group whose parameters all
    /// hide stops being a tab and the stored name then names nothing.
    tab: String,
}

impl ParamPanelState {
    /// Forget the pin and the tab.
    ///
    /// **Called when the document is replaced, and it has to be.** Node
    /// identifiers are minted from a per-document counter, so ids one,
    /// two and three exist in every scene; a pin carried across an open
    /// would point at whatever holds that id in the incoming document and
    /// the panel would edit it without a word.
    pub(crate) fn reset(&mut self) {
        self.pin = None;
        self.tab.clear();
        self.draft = None;
    }

    pub(crate) fn pinned(&self) -> Option<NodeId> {
        self.pin
    }
}

/// Which node the panel is editing, and whether it is reachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Target {
    /// Nothing selected and nothing pinned.
    None,
    /// The node to edit, and whether a pin is what chose it.
    Node(NodeId, bool),
    /// Pinned to a node that is not in the graph being shown. Named
    /// rather than fallen back from, because falling back would edit a
    /// different node than the one the pin promises.
    PinnedElsewhere(NodeId),
}

/// Resolve what the panel edits.
///
/// **A pin wins over the selection**, and it does not follow a dive: that
/// is the whole point of pinning, and it is why an out-of-context pin is
/// its own answer rather than a silent fallback.
///
/// The selection's **first** node is the subject, not its last. The
/// canvas's viewport outline follows the last, because that is the one
/// just added to the set; a panel follows the first, because that is the
/// one the set was started from and it does not move as the set grows.
#[must_use]
pub(super) fn target(pin: Option<NodeId>, selection: &[NodeId], present: bool) -> Target {
    match pin {
        Some(node) if present => Target::Node(node, true),
        Some(node) => Target::PinnedElsewhere(node),
        None => selection
            .first()
            .map_or(Target::None, |id| Target::Node(*id, false)),
    }
}

/// The header's statistics line, or nothing.
///
/// **Nothing and zero are different.** A node that has never cooked has
/// no statistics and says nothing; a cooked node with neither geometry
/// nor an image says zero, and that is a fact about it rather than an
/// absence of one.
#[must_use]
pub(super) fn stats_line(stats: Option<NodeCookStats>) -> Option<String> {
    let stats = stats?;
    if let Some((width, height)) = stats.image {
        return Some(format!("{width} x {height}"));
    }
    Some(format!(
        "{} pts, {} tris, {} mesh",
        stats.points, stats.prims, stats.meshes
    ))
}

/// The tabs a node offers, and which of them is active.
#[must_use]
pub(super) fn tabs(
    specs: &[ParamSpec],
    params: &std::collections::BTreeMap<String, solarxy_graph::params::ParamSource>,
    has_report: bool,
    stored: &str,
) -> (Vec<String>, Option<String>) {
    let offered = params::param_tabs(specs, has_report, |spec| param_visible(spec, specs, params));
    let active = params::resolve_active_tab(&offered, stored).map(str::to_owned);
    (offered, active)
}

/// Whether a tab reset applies, and which keys it writes.
///
/// **The whole group, not the visible rows.** A hidden variant row keeps
/// its stored value, and a reset that skipped it would leave it stale to
/// surface later as a node behaving oddly after its mode is switched
/// back. The validation tab is not a group at all, so it resets nothing
/// rather than dispatching a command that does nothing and still costs an
/// undo step.
#[must_use]
pub(super) fn reset_keys<'a>(specs: &'a [ParamSpec], tab: &str) -> Option<Vec<&'a str>> {
    if tab == VALIDATION_TAB {
        return None;
    }
    let keys = params::group_keys(specs, tab);
    (!keys.is_empty()).then_some(keys)
}

/// Whether a parameter is driven by a connected input.
///
/// A driven parameter is dimmed rather than hidden or reset: the stored
/// value survives so it comes back when the map disconnects.
#[must_use]
pub(super) fn driven(
    spec: &ParamSpec,
    graph: &solarxy_graph::document::Graph,
    node: NodeId,
) -> bool {
    let Some(port) = spec.driven_by_port.as_deref() else {
        return false;
    };
    graph
        .edges()
        .any(|edge| edge.to == node && edge.to_port == port)
}

/// Draw the panel.
pub(crate) fn draw_params_content(
    ui: &mut Ui,
    source: ParamPanelSource<'_>,
    state: &mut ParamPanelState,
    intents: &mut Intents,
    theme: Theme,
) {
    let ParamPanelSource::Scene(scene) = source else {
        return placeholder(ui, "No document open", theme);
    };
    let &ParamScene {
        doc,
        registry,
        ctx,
        stats,
        has_report,
        counts,
        assets,
        lanes,
        error,
    } = scene;
    let Ok(graph) = doc.graph(ctx) else {
        return placeholder(ui, "No document open", theme);
    };

    let present = state.pin.is_some_and(|id| graph.node(id).is_some());
    match target(state.pin, &graph.selection, present) {
        Target::None => placeholder(ui, "Select a node to edit its parameters.", theme),
        Target::PinnedElsewhere(_) => {
            ui.add_space(20.0);
            ui.vertical_centered(|ui| {
                ui.label(egui::RichText::new("Pinned to a node in another network.").weak());
                if ui.button("Unpin").clicked() {
                    state.pin = None;
                }
            });
        }
        Target::Node(node, pinned) => {
            let Some(data) = graph.node(node) else {
                return placeholder(ui, "Select a node to edit its parameters.", theme);
            };
            let Some(desc) = registry.get(&data.type_id) else {
                return placeholder(ui, "This node type is not registered.", theme);
            };

            // The header and the strip are fixed; only the body scrolls,
            // or the tabs scroll out of reach of the rows they choose.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(solarxy_graph::naming::node_name(data, registry))
                        .size(13.0),
                );
                if pinned
                    && ui
                        .small_button("pinned")
                        .on_hover_text("Pinned to this node. Click to follow the selection again.")
                        .clicked()
                {
                    state.pin = None;
                }
                if !pinned
                    && ui
                        .small_button("pin")
                        .on_hover_text("Follow this node")
                        .clicked()
                {
                    state.pin = Some(node);
                }
            });
            if let Some(line) = stats_line(stats) {
                ui.label(egui::RichText::new(line).color(theme.muted).size(10.0));
            }

            let (offered, active) = tabs(&desc.params, &data.params, has_report, &state.tab);
            let resettable = active.as_deref().and_then(|tab| {
                reset_keys(&desc.params, tab).map(|keys| {
                    (
                        tab.to_string(),
                        keys.into_iter().map(str::to_owned).collect::<Vec<_>>(),
                    )
                })
            });
            // Hidden at one, not at zero: a node with a single tab draws
            // no strip, and a node whose only tab is Validation draws no
            // strip and still renders the report.
            if offered.len() > 1 {
                ui.horizontal_wrapped(|ui| {
                    for tab in &offered {
                        let label = if tab == VALIDATION_TAB && counts.0 + counts.1 > 0 {
                            format!("{} ({})", params::tab_label(tab), counts.0 + counts.1)
                        } else {
                            params::tab_label(tab)
                        };
                        if ui
                            .selectable_label(active.as_deref() == Some(tab.as_str()), label)
                            .clicked()
                        {
                            state.tab.clone_from(tab);
                        }
                    }
                });
            }
            if let Some((tab, keys)) = resettable
                && ui
                    .small_button("Reset tab")
                    .on_hover_text(format!("Return every parameter in {tab} to its default"))
                    .clicked()
            {
                // The whole group, hidden rows included, which is what
                // `group_keys` returns and why. The menu entry the
                // browser puts this behind arrives with the menu work.
                intents.panel(crate::gui::PanelIntent::ResetParams(ctx, node, keys));
            }
            ui.separator();

            let Some(active) = active else {
                return placeholder(ui, "No parameters.", theme);
            };
            // Node-path candidates come from the **root** graph, which is
            // the browser's rule; offering nested containers would give
            // the two shells different candidate lists for one document.
            let candidates = node_path_candidates(doc, registry, &desc.params);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if active == VALIDATION_TAB {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} error(s), {} warning(s)",
                            counts.0, counts.1
                        ))
                        .color(if counts.0 > 0 {
                            theme.severity_error
                        } else {
                            theme.muted
                        }),
                    );
                    return;
                }
                for section in params::param_sections(&desc.params) {
                    let rows: Vec<&ParamSpec> = section
                        .params
                        .iter()
                        .filter(|spec| spec.group == active)
                        .filter(|spec| param_visible(spec, &desc.params, &data.params))
                        .copied()
                        .collect();
                    if rows.is_empty() {
                        continue;
                    }
                    if let Some(subgroup) = section.subgroup {
                        ui.label(egui::RichText::new(subgroup).color(theme.muted).size(10.0));
                    }
                    for spec in rows {
                        let is_driven = driven(spec, graph, node);
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(&spec.label).size(11.0))
                                .on_hover_text(&spec.doc);
                            // A driven row draws its control disabled
                            // rather than omitting it: the stored value is
                            // still what the node falls back to when the
                            // wire goes, and hiding the row makes that
                            // impossible to read or to set in advance.
                            let env = ControlEnv {
                                node,
                                candidates: &candidates,
                                assets,
                                lanes,
                                specs: &desc.params,
                                error,
                                theme,
                            };
                            let edit = ui
                                .add_enabled_ui(!is_driven, |ui| {
                                    controls::draw(
                                        ui,
                                        spec,
                                        data.params.get(&spec.key),
                                        &env,
                                        &mut state.draft,
                                    )
                                })
                                .inner;
                            match edit {
                                Some(ControlEdit::Write(writes)) => {
                                    intents.panel(crate::gui::PanelIntent::Canvas(
                                        CanvasAction::SetParams(
                                            ctx,
                                            node,
                                            writes
                                                .into_iter()
                                                .map(|(key, value)| {
                                                    (
                                                        key,
                                                        solarxy_graph::params::ParamSource::Literal(
                                                            value,
                                                        ),
                                                    )
                                                })
                                                .collect(),
                                        ),
                                    ));
                                }
                                Some(ControlEdit::Invoke) => {
                                    intents.panel(crate::gui::PanelIntent::InvokeAction {
                                        ctx,
                                        node,
                                        key: spec.key.clone(),
                                    });
                                }
                                Some(ControlEdit::ChooseAsset) => {
                                    intents.panel(crate::gui::PanelIntent::ChooseAsset {
                                        ctx,
                                        node,
                                        key: spec.key.clone(),
                                    });
                                }
                                None => {}
                            }
                        });
                        if is_driven {
                            ui.label(
                                egui::RichText::new("Driven by connected input")
                                    .color(theme.muted)
                                    .italics()
                                    .size(9.0),
                            );
                        }
                    }
                }
            });
        }
    }
}

/// Every node the node-path parameters on this type could point at.
///
/// Gathered once per frame rather than once per row, and only when a row
/// actually asks: the walk is over the whole root graph, and most node
/// types declare no node-path parameter at all.
fn node_path_candidates(
    doc: &Document,
    registry: &Registry,
    specs: &[ParamSpec],
) -> Vec<(NodeId, String)> {
    let accepts: Vec<_> = specs
        .iter()
        .filter_map(|spec| match &spec.ty {
            solarxy_graph::registry::param_spec::ParamType::NodePath { accept } => Some(accept),
            _ => None,
        })
        .collect();
    if accepts.is_empty() {
        return Vec::new();
    }
    let Ok(root) = doc.graph(GraphContext::Root) else {
        return Vec::new();
    };
    root.nodes()
        .filter(|data| {
            registry
                .get(&data.type_id)
                .is_some_and(|desc| accepts.iter().any(|accept| controls::accepts(accept, desc)))
        })
        .map(|data| (data.id, solarxy_graph::naming::node_name(data, registry)))
        .collect()
}

fn placeholder(ui: &mut Ui, message: &str, theme: Theme) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(message).color(theme.muted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::cook::state::NodeCookStats;

    fn stats(points: u64, image: Option<(u32, u32)>) -> NodeCookStats {
        NodeCookStats {
            duration_us: 10,
            points,
            prims: 2,
            meshes: 1,
            bounds: None,
            image,
        }
    }

    /// A pin wins over the selection, does not follow a dive, and says so
    /// rather than quietly editing whatever is to hand.
    #[test]
    fn a_pin_wins_and_an_unreachable_pin_is_named() {
        let selection = [NodeId(7), NodeId(8)];
        assert_eq!(
            target(None, &selection, false),
            Target::Node(NodeId(7), false)
        );
        assert_eq!(
            target(Some(NodeId(3)), &selection, true),
            Target::Node(NodeId(3), true),
            "a pin outranks the selection"
        );
        assert_eq!(
            target(Some(NodeId(3)), &selection, false),
            Target::PinnedElsewhere(NodeId(3)),
            "a pin out of context must not fall back to the selection"
        );
        assert_eq!(target(None, &[], false), Target::None);
    }

    /// The panel follows the FIRST selected node, not the last.
    ///
    /// The canvas outline follows the last, because that is the one just
    /// added to the set. A panel follows the first, because that is the
    /// one the set was started from and it does not move as the set
    /// grows: a box selection would otherwise walk the panel across every
    /// node it enclosed.
    #[test]
    fn the_panel_follows_the_first_selected_node() {
        assert_eq!(
            target(None, &[NodeId(4), NodeId(9), NodeId(1)], false),
            Target::Node(NodeId(4), false)
        );
    }

    /// Nothing and zero are different answers, and a cooked node with
    /// neither geometry nor an image genuinely reads zero.
    #[test]
    fn no_cook_says_nothing_and_a_cooked_one_says_its_counts() {
        assert_eq!(stats_line(None), None);
        assert_eq!(
            stats_line(Some(stats(0, None))).as_deref(),
            Some("0 pts, 2 tris, 1 mesh"),
            "a cooked node with no geometry reports zero rather than nothing"
        );
        assert_eq!(
            stats_line(Some(stats(120, None))).as_deref(),
            Some("120 pts, 2 tris, 1 mesh")
        );
    }

    /// An image node reads its dimensions instead of its counts, which
    /// stay zero for it.
    #[test]
    fn an_image_node_reads_its_dimensions() {
        assert_eq!(
            stats_line(Some(stats(0, Some((512, 256))))).as_deref(),
            Some("512 x 256")
        );
    }

    /// A reset writes the whole group, hidden rows included, and the
    /// validation tab resets nothing at all.
    #[test]
    fn a_reset_writes_the_whole_group_and_never_the_validation_tab() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let desc = registry.get("array").expect("the array node is registered");

        assert_eq!(
            reset_keys(&desc.params, VALIDATION_TAB),
            None,
            "the validation tab is not a group and resets nothing"
        );

        let group = desc
            .params
            .iter()
            .find(|spec| !spec.show_if.is_empty())
            .map(|spec| spec.group.clone())
            .expect("the array node hides a parameter behind a condition");
        let keys = reset_keys(&desc.params, &group).expect("a real group resets");
        let declared = desc.params.iter().filter(|s| s.group == group).count();
        assert_eq!(
            keys.len(),
            declared,
            "a reset must carry every key in the group, including the hidden ones"
        );
    }

    /// A group whose every parameter is hidden stops being a tab, and the
    /// active tab is asked of the shared resolver each frame rather than
    /// trusted, because the stored name then names nothing.
    #[test]
    fn an_emptied_group_stops_being_a_tab_and_the_active_one_falls_back() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let desc = registry.get("array").expect("the array node is registered");
        let params = std::collections::BTreeMap::new();

        let (offered, active) = tabs(&desc.params, &params, false, "nowhere");
        assert!(!offered.is_empty());
        assert_eq!(
            active.as_deref(),
            offered.first().map(String::as_str),
            "a stored tab that is not offered falls back to the first"
        );

        // Nothing visible at all, and no report: no tabs and no active.
        let (none, no_active) = tabs(&[], &params, false, "");
        assert!(none.is_empty() && no_active.is_none());
    }

    /// The validation tab appears on presence, not on there being
    /// something wrong, so a clean node keeps the tab that says so.
    #[test]
    fn the_validation_tab_appears_on_presence_rather_than_on_trouble() {
        let registry = solarxy_graph::nodes::builtin_registry().expect("builtin registry");
        let desc = registry
            .get("validate")
            .expect("the validate node is registered");
        let params = std::collections::BTreeMap::new();

        let (without, _) = tabs(&desc.params, &params, false, "");
        let (with, _) = tabs(&desc.params, &params, true, "");
        assert_eq!(with.len(), without.len() + 1);
        assert_eq!(with.last().map(String::as_str), Some(VALIDATION_TAB));
    }
}
