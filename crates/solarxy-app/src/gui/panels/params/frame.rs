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
use super::expression::{self, Toggle};
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
    /// The report itself, for the Validation tab's rows.
    pub report: Option<&'a solarxy_core::validation::ValidationReport>,
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
    /// What each expression-driven parameter currently resolves to.
    ///
    /// **Pulled once a frame, for the driven rows only.** An expression's
    /// value moves whenever what it reads moves, which is every applied
    /// command, so pushing it as an event would be one event per
    /// expression per frame under a playing runtime.
    pub resolved: &'a super::expression::Resolved,
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
    /// The numeric gesture in flight, if any. On the panel rather than in
    /// the widget for the same reason the draft is, and additionally
    /// because a gesture that outlives its row has a preview to drop.
    drag: Option<super::drag::NumericDrag>,
    /// Expression text switched off and not yet discarded, so the same
    /// click brings it back. Interface memory rather than document state:
    /// the scene schema is frozen and a per-session convenience is not
    /// worth a schema version.
    parked: super::expression::Parked,
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
        self.drag = None;
        self.parked.clear();
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
pub(in crate::gui::panels) fn driven(
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
        report,
        assets,
        lanes,
        error,
        resolved,
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
            // A gesture belongs to the row it opened on. Selecting another
            // node abandons it, and its preview has to be dropped by hand
            // or the viewport goes on asserting a value nothing else holds.
            if let Some(stale) = state
                .drag
                .as_ref()
                .filter(|held| held.node() != node || held.ctx() != ctx)
            {
                intents.panel(crate::gui::PanelIntent::Canvas(
                    CanvasAction::ClearPreviews(
                        stale.ctx(),
                        stale.node(),
                        vec![stale.key().to_string()],
                    ),
                ));
                state.drag = None;
            }
            // Node-path candidates come from the **root** graph, which is
            // the browser's rule; offering nested containers would give
            // the two shells different candidate lists for one document.
            let candidates = node_path_candidates(doc, registry, &desc.params);
            egui::ScrollArea::vertical().show(ui, |ui| {
                if active == VALIDATION_TAB {
                    draw_validation_tab(ui, report, counts, ctx, node, intents, theme);
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
                            let expr = expression::driving(data.params.get(&spec.key))
                                .map(ToString::to_string);
                            let park = expression::park_key(ctx, node, &spec.key);
                            if expression::offers_toggle(spec)
                                && let Some(toggle) =
                                    expression::draw_toggle(ui, expr.is_some(), theme)
                            {
                                let write = match toggle {
                                    Toggle::On => expression::switch_on(
                                        state.parked.get(&park).map(String::as_str),
                                        &controls::shown_value(spec, data.params.get(&spec.key)),
                                    ),
                                    Toggle::Off => {
                                        // Parked on the way out, so the
                                        // same click brings it back. The
                                        // field's clear control is the one
                                        // that discards.
                                        if let Some(text) = expr.clone() {
                                            state.parked.insert(park.clone(), text);
                                        }
                                        solarxy_graph::params::ParamSource::Literal(
                                            expression::switch_off(resolved.get(&spec.key), spec),
                                        )
                                    }
                                };
                                intents.panel(crate::gui::PanelIntent::Canvas(
                                    CanvasAction::SetParams(
                                        ctx,
                                        node,
                                        vec![(spec.key.clone(), write)],
                                    ),
                                ));
                            }
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
                                .add_enabled_ui(!is_driven, |ui| match expr.as_deref() {
                                    Some(text) => expression::draw_row(
                                        ui,
                                        spec,
                                        text,
                                        resolved.get(&spec.key),
                                        &env,
                                        &mut state.draft,
                                        theme,
                                    ),
                                    None => controls::draw(
                                        ui,
                                        spec,
                                        data.params.get(&spec.key),
                                        &env,
                                        &mut controls::Gesture {
                                            draft: &mut state.draft,
                                            drag: &mut state.drag,
                                            ctx,
                                        },
                                    ),
                                })
                                .inner;
                            match edit {
                                Some(ControlEdit::Write(writes)) => {
                                    intents.panel(crate::gui::PanelIntent::Canvas(
                                        CanvasAction::SetParams(ctx, node, writes),
                                    ));
                                }
                                Some(ControlEdit::Preview(values)) => {
                                    intents.panel(crate::gui::PanelIntent::Canvas(
                                        CanvasAction::PreviewParams(
                                            ctx,
                                            node,
                                            values
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
                                // On a literal row this drops an
                                // abandoned preview; on an expression row
                                // it is the destructive control, which
                                // discards the parked text as well as the
                                // expression. Two meanings for one shape
                                // because the row it came from is what
                                // decides, and the row is right here.
                                Some(ControlEdit::Clear(keys)) => {
                                    if expr.is_some() {
                                        state.parked.remove(&park);
                                        intents.panel(crate::gui::PanelIntent::Canvas(
                                            CanvasAction::SetParams(
                                                ctx,
                                                node,
                                                vec![(
                                                    spec.key.clone(),
                                                    solarxy_graph::params::ParamSource::Literal(
                                                        expression::switch_off(
                                                            resolved.get(&spec.key),
                                                            spec,
                                                        ),
                                                    ),
                                                )],
                                            ),
                                        ));
                                    } else {
                                        intents.panel(crate::gui::PanelIntent::Canvas(
                                            CanvasAction::ClearPreviews(ctx, node, keys),
                                        ));
                                    }
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

/// The Validation tab: the counts, then one row per issue, each a click
/// away from the geometry it names.
///
/// The rows fly through the node's own report rather than through a
/// scene-wide merged list, because this panel is about one node; the
/// object the camera frames is the one the node's network belongs to.
fn draw_validation_tab(
    ui: &mut Ui,
    report: Option<&solarxy_core::validation::ValidationReport>,
    counts: (u32, u32),
    ctx: GraphContext,
    node: NodeId,
    intents: &mut Intents,
    theme: Theme,
) {
    let Some(report) = report else {
        ui.label(egui::RichText::new("No report yet.").color(theme.muted));
        return;
    };
    if report.is_clean() {
        ui.label("No issues found.");
        return;
    }
    ui.label(
        egui::RichText::new(format!("{} error(s), {} warning(s)", counts.0, counts.1)).color(
            if counts.0 > 0 {
                theme.severity_error
            } else {
                theme.muted
            },
        ),
    );
    ui.add_space(2.0);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text_color = ui.visuals().text_color();
    for (index, issue) in report.issues.iter().enumerate() {
        let c = solarxy_renderer::validation::issue_category(issue).color();
        let dot = egui::Color32::from_rgb(
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
        );
        let mut job = egui::text::LayoutJob::default();
        job.append(
            "\u{25cf}  ",
            0.0,
            egui::TextFormat {
                color: dot,
                font_id: font.clone(),
                ..Default::default()
            },
        );
        job.append(
            &format!("{}: {}", issue.scope, issue.message),
            0.0,
            egui::TextFormat {
                color: text_color,
                font_id: font.clone(),
                ..Default::default()
            },
        );
        job.wrap.max_width = ui.available_width();
        if ui
            .selectable_label(false, job)
            .on_hover_text("Click to frame this issue in the active viewport")
            .clicked()
        {
            intents.panel(crate::gui::PanelIntent::FlyToIssue { ctx, node, index });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::cook::state::NodeCookStats;

    use solarxy_core::preferences::ThemeChoice;
    use solarxy_graph::{Command, Engine};

    fn theme() -> Theme {
        Theme::from_choice(ThemeChoice::default())
    }

    /// A document with one box in it, cooked.
    fn scene() -> (Engine, GraphContext, NodeId) {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let ctx = GraphContext::Subflow(geo);
        let box_node = added(&mut engine, ctx, "box");
        engine
            .apply(Command::SetSelection {
                ctx,
                ids: vec![box_node],
            })
            .expect("select the box");
        (engine, ctx, box_node)
    }

    fn added(engine: &mut Engine, ctx: GraphContext, ty: &str) -> NodeId {
        let before: Vec<NodeId> = engine
            .document()
            .graph(ctx)
            .map(|g| g.nodes().map(|n| n.id).collect())
            .unwrap_or_default();
        engine
            .apply(Command::AddNode {
                ctx,
                node_type: ty.to_string(),
                position: [0.0, 0.0],
            })
            .expect("add a node");
        engine
            .document()
            .graph(ctx)
            .expect("the graph")
            .nodes()
            .map(|n| n.id)
            .find(|id| !before.contains(id))
            .expect("exactly one node was added")
    }

    /// Draw one real interface pass over a real document.
    fn one_frame(
        engine: &Engine,
        ctx: GraphContext,
        state: &mut ParamPanelState,
        intents: &mut Intents,
        input: egui::RawInput,
    ) {
        let egui_ctx = egui::Context::default();
        let _ = egui_ctx.run(input, |c| {
            egui::CentralPanel::default().show(c, |ui| {
                draw_params_content(
                    ui,
                    ParamPanelSource::Scene(&ParamScene {
                        doc: engine.document(),
                        registry: engine.registry(),
                        ctx,
                        stats: None,
                        has_report: false,
                        report: None,
                        counts: (0, 0),
                        assets: &[],
                        lanes: &[],
                        error: None,
                        resolved: &super::expression::Resolved::new(),
                    }),
                    state,
                    intents,
                    theme(),
                );
            });
        });
    }

    /// Input with the middle button held and the pointer somewhere.
    fn held(x: f32, y: f32) -> egui::RawInput {
        let pos = egui::pos2(x, y);
        egui::RawInput {
            events: vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Middle,
                    pressed: true,
                    modifiers: egui::Modifiers::default(),
                },
            ],
            ..Default::default()
        }
    }

    fn writes(intents: &mut Intents) -> (usize, usize, usize) {
        let mut written = 0;
        let mut previewed = 0;
        let mut cleared = 0;
        for intent in intents.take_ordered() {
            match intent {
                crate::gui::Intent::Panel(crate::gui::PanelIntent::Canvas(
                    CanvasAction::SetParams(..),
                )) => written += 1,
                crate::gui::Intent::Panel(crate::gui::PanelIntent::Canvas(
                    CanvasAction::PreviewParams(..),
                )) => previewed += 1,
                crate::gui::Intent::Panel(crate::gui::PanelIntent::Canvas(
                    CanvasAction::ClearPreviews(..),
                )) => cleared += 1,
                _ => {}
            }
        }
        (written, previewed, cleared)
    }

    /// Looking at a parameter must not change it.
    ///
    /// Drawn twice deliberately: egui runs a pass twice on any frame a
    /// layout is still settling, and a queue fed from a condition that is
    /// merely true while the panel is open passes the first and fails the
    /// second.
    #[test]
    fn an_idle_frame_raises_no_commands() {
        let (engine, ctx, _) = scene();
        let mut state = ParamPanelState::default();
        let mut intents = Intents::default();
        one_frame(
            &engine,
            ctx,
            &mut state,
            &mut intents,
            egui::RawInput::default(),
        );
        one_frame(
            &engine,
            ctx,
            &mut state,
            &mut intents,
            egui::RawInput::default(),
        );
        assert!(
            intents.take_ordered().is_empty(),
            "a panel nobody touched must ask for nothing"
        );
    }

    /// **One gesture is one undo step.** A drag streams previews for as
    /// long as it is held and writes exactly once when it lets go.
    ///
    /// The gesture is opened directly rather than by hovering a widget,
    /// because where a field lands depends on the panel's layout and a
    /// test that hunts for it would be measuring egui rather than this.
    /// Everything after the first frame is the real branch order in
    /// `numeric_row`, driven by real pointer input.
    #[test]
    fn a_drag_previews_while_it_is_held_and_writes_once_on_release() {
        let (engine, ctx, node) = scene();
        // The box's numbers live on its geometry tab, and only the rows of
        // the active tab draw.
        let mut state = ParamPanelState {
            tab: "geometry".to_string(),
            ..Default::default()
        };
        let mut intents = Intents::default();
        state.drag = Some(super::super::drag::NumericDrag::begin(
            ctx,
            node,
            "width",
            vec![1.0],
            0,
            super::super::drag::DragKind::Precision {
                origin: egui::pos2(100.0, 100.0),
                last_change_y: 100.0,
                decade: super::super::drag::DEFAULT_DECADE,
            },
        ));

        for step in 1..=3 {
            #[allow(clippy::cast_precision_loss)]
            let x = 100.0 + (step as f32) * 40.0;
            one_frame(&engine, ctx, &mut state, &mut intents, held(x, 100.0));
            let (written, previewed, cleared) = writes(&mut intents);
            assert_eq!(
                (written, cleared),
                (0, 0),
                "frame {step} of a held drag wrote to the document"
            );
            assert!(
                previewed > 0,
                "frame {step} of a held drag previewed nothing"
            );
        }

        // The button comes up: one write, and no further preview.
        one_frame(
            &engine,
            ctx,
            &mut state,
            &mut intents,
            egui::RawInput::default(),
        );
        let (written, previewed, cleared) = writes(&mut intents);
        assert_eq!(
            (written, previewed, cleared),
            (1, 0, 0),
            "a released drag is one write and nothing else"
        );
        assert!(state.drag.is_none(), "the gesture outlived its release");

        // And an idle frame after it is quiet again.
        one_frame(
            &engine,
            ctx,
            &mut state,
            &mut intents,
            egui::RawInput::default(),
        );
        assert_eq!(writes(&mut intents), (0, 0, 0));
    }

    /// A gesture that scrubbed nothing writes nothing, and still drops its
    /// preview: one was streamed the moment it opened.
    #[test]
    fn a_gesture_that_moved_nothing_clears_instead_of_writing() {
        let (engine, ctx, node) = scene();
        let mut state = ParamPanelState {
            tab: "geometry".to_string(),
            ..Default::default()
        };
        let mut intents = Intents::default();
        state.drag = Some(super::super::drag::NumericDrag::begin(
            ctx,
            node,
            "width",
            vec![1.0],
            0,
            super::super::drag::DragKind::Widget,
        ));
        one_frame(
            &engine,
            ctx,
            &mut state,
            &mut intents,
            egui::RawInput::default(),
        );
        assert_eq!(
            writes(&mut intents),
            (0, 0, 1),
            "a gesture that did not move must clear its preview and write nothing"
        );
    }

    /// Selecting another node abandons an open gesture and drops its
    /// preview, which nothing else would.
    #[test]
    fn changing_the_subject_clears_a_stale_preview() {
        let (mut engine, ctx, node) = scene();
        let other = added(&mut engine, ctx, "sphere");
        engine
            .apply(Command::SetSelection {
                ctx,
                ids: vec![other],
            })
            .expect("select the sphere");

        let mut state = ParamPanelState::default();
        let mut intents = Intents::default();
        // A gesture left open on the node that is no longer the subject.
        state.drag = Some(super::super::drag::NumericDrag::begin(
            ctx,
            node,
            "width",
            vec![1.0],
            0,
            super::super::drag::DragKind::Widget,
        ));
        one_frame(
            &engine,
            ctx,
            &mut state,
            &mut intents,
            egui::RawInput::default(),
        );
        let (written, _, cleared) = writes(&mut intents);
        assert_eq!(
            (written, cleared),
            (0, 1),
            "a gesture stranded by a selection change must be cleared, not committed"
        );
        assert!(state.drag.is_none());
    }

    /// An expression row draws, says what it resolves to, and asks for
    /// nothing until someone touches it.
    ///
    /// Two passes for the reason the idle guard runs two: egui runs a pass
    /// twice on any frame a layout is still settling, and the readout is
    /// pulled every pass.
    #[test]
    fn an_expression_row_is_quiet_until_it_is_touched() {
        let (mut engine, ctx, node) = scene();
        engine
            .apply(Command::SetParam {
                ctx,
                node,
                key: "width".to_string(),
                value: solarxy_graph::params::ParamSource::Expression {
                    expr: "2 + 3".to_string(),
                },
            })
            .expect("a float takes an expression");

        // The readout the panel would draw, pulled the way the shell
        // pulls it.
        let resolved: super::super::expression::Resolved = [(
            "width".to_string(),
            engine.resolved_param(ctx, node, "width"),
        )]
        .into_iter()
        .collect();
        assert_eq!(
            resolved.get("width"),
            Some(&Ok(solarxy_graph::params::ParamValue::Float(5.0))),
            "the readout comes from the engine, not from the text"
        );

        let mut state = ParamPanelState {
            tab: "geometry".to_string(),
            ..Default::default()
        };
        let mut intents = Intents::default();
        let egui_ctx = egui::Context::default();
        for _ in 0..2 {
            let _ = egui_ctx.run(egui::RawInput::default(), |c| {
                egui::CentralPanel::default().show(c, |ui| {
                    draw_params_content(
                        ui,
                        ParamPanelSource::Scene(&ParamScene {
                            doc: engine.document(),
                            registry: engine.registry(),
                            ctx,
                            stats: None,
                            has_report: false,
                            report: None,
                            counts: (0, 0),
                            assets: &[],
                            lanes: &[],
                            error: None,
                            resolved: &resolved,
                        }),
                        &mut state,
                        &mut intents,
                        theme(),
                    );
                });
            });
        }
        assert!(
            intents.take_ordered().is_empty(),
            "an expression row nobody touched must ask for nothing"
        );
    }

    /// Switching off and back on in one sitting returns the text
    /// verbatim, through the store the panel actually keeps.
    ///
    /// The two halves are written and read under one key function, so the
    /// failure this closes is not a wrong lookup but a second spelling of
    /// the key appearing later.
    #[test]
    fn a_parked_expression_survives_the_round_trip() {
        use super::super::expression;
        let (engine, ctx, node) = scene();
        let desc = engine
            .registry()
            .get("box")
            .expect("the box type is registered");
        let spec = desc.param("width").expect("the box has a width");

        let mut parked = expression::Parked::new();
        let key = expression::park_key(ctx, node, "width");

        // Off: the text is parked and the value written back is what the
        // expression resolved to, not the declared default.
        parked.insert(key.clone(), "$F * 2".to_string());
        let kept = expression::switch_off(
            Some(&Ok(solarxy_graph::params::ParamValue::Float(8.0))),
            spec,
        );
        assert_eq!(kept, solarxy_graph::params::ParamValue::Float(8.0));
        assert_ne!(
            kept, spec.default,
            "switching off must not restore the default"
        );

        // On again: the same key finds the same text.
        let solarxy_graph::params::ParamSource::Expression { expr } = expression::switch_on(
            parked
                .get(&expression::park_key(ctx, node, "width"))
                .map(String::as_str),
            &kept,
        ) else {
            panic!("switching on must write an expression");
        };
        assert_eq!(expr, "$F * 2");

        // Discarded, and the same click now seeds from the value instead.
        parked.remove(&key);
        let solarxy_graph::params::ParamSource::Expression { expr } = expression::switch_on(
            parked
                .get(&expression::park_key(ctx, node, "width"))
                .map(String::as_str),
            &kept,
        ) else {
            panic!("switching on must write an expression");
        };
        assert_eq!(expr, "8");
    }

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
