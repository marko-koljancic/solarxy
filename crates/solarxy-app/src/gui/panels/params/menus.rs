//! The parameter panel's own menu bar: Node, Params and View.
//!
//! **The bar acts on the node the panel is showing.** A pin wins over the
//! selection here exactly as it does in the panel underneath, because the
//! bar asks the panel's own target rule rather than reading the selection
//! for itself. A bar that reset the selected node's parameters while the
//! panel showed a pinned one would be a menu acting on something the user
//! cannot see.
//!
//! **Which tab is current is the panel's answer, not the bar's.** Resetting
//! a tab needs to know which one is active, so the bar reads the same
//! resolver the tab strip reads and the two cannot disagree about what "the
//! current tab" means.
//!
//! Everything the three menus need is worked out by [`bar_model`], a pure
//! function over the document and the panel's state, so the rules about
//! which entry applies to which node are tested without an interface.

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::registry::BypassBehavior;
use solarxy_studio::params;

use super::frame::{ParamScene, Target, reset_keys, tabs, target};
use crate::gui::chrome::menu_items::{check_entry, entry_if};
use crate::gui::chrome::panel_bar::{MAXIMIZE_LABEL, maximize_entry, panel_bar};
use crate::gui::dock::SolarxyTab;
use crate::gui::intent::{Intent, Intents, LayoutIntent, PanelIntent};
use crate::gui::panels::nodes::CanvasAction;
use crate::gui::theme::Theme;
use crate::state::keymap::Action;

const NO_NODE: &str = "Select a node first";

/// What the bar knows about the node the panel is showing.
#[derive(Debug, Default, PartialEq)]
pub(super) struct BarModel {
    /// The node the panel shows, in the graph it is shown in.
    pub node: Option<(GraphContext, NodeId)>,
    /// Its absolute path, in the form an expression accepts.
    pub path: Option<String>,
    /// Whether it is bypassed now, or `None` when its type cannot be.
    pub bypassed: Option<bool>,
    /// Whether it sits inside a container, which is the only place a
    /// display flag means anything.
    pub in_container: bool,
    pub has_params: bool,
    /// The current tab's label and the keys a reset of it writes, or `None`
    /// when the current tab is not one that resets.
    pub reset_tab: Option<(String, Vec<String>)>,
    pub pinned: bool,
}

/// Work out what the bar can do, from the scene and the panel's own state.
#[must_use]
pub(super) fn bar_model(
    scene: Option<&ParamScene<'_>>,
    pin: Option<NodeId>,
    stored_tab: &str,
) -> BarModel {
    let pinned = pin.is_some();
    let empty = BarModel {
        pinned,
        ..BarModel::default()
    };
    let Some(scene) = scene else {
        return empty;
    };
    let Ok(graph) = scene.doc.graph(scene.ctx) else {
        return empty;
    };
    let present = pin.is_some_and(|id| graph.node(id).is_some());
    let Target::Node(node, _) = target(pin, &graph.selection, present) else {
        return empty;
    };
    let Some(data) = graph.node(node) else {
        return empty;
    };
    let Some(desc) = scene.registry.get(&data.type_id) else {
        return empty;
    };

    let (_, active) = tabs(&desc.params, &data.params, scene.has_report, stored_tab);
    let reset_tab = active.as_deref().and_then(|tab| {
        reset_keys(&desc.params, tab).map(|keys| {
            (
                params::tab_label(tab),
                keys.into_iter().map(str::to_owned).collect(),
            )
        })
    });
    BarModel {
        node: Some((scene.ctx, node)),
        path: solarxy_graph::refs::node_path(scene.doc, scene.registry, scene.ctx, node),
        bypassed: (!matches!(desc.bypass, BypassBehavior::NotBypassable)).then_some(data.bypassed),
        in_container: scene.ctx != GraphContext::Root,
        has_params: !desc.params.is_empty(),
        reset_tab,
        pinned,
    }
}

/// What the bar asked of the panel it sits on. Everything else it does is
/// an intent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct BarRequest {
    /// Pin to the node shown, or let go of the pin.
    pub toggle_pin: bool,
}

/// Draw the bar across the top of the docked panel.
pub(super) fn draw_bar(
    ui: &mut egui::Ui,
    model: &BarModel,
    floating_open: bool,
    intents: &mut Intents,
    theme: Theme,
) -> BarRequest {
    let mut request = BarRequest::default();
    panel_bar(ui, theme, |ui| {
        ui.menu_button("Node", |ui| draw_node(ui, model, intents));
        ui.menu_button("Params", |ui| draw_params(ui, model, intents));
        ui.menu_button("View", |ui| {
            draw_view(ui, model, floating_open, intents, &mut request);
        });
    });
    request
}

/// One row of one of the three menus, in the order it is drawn. A divider
/// is a row like any other, so each table below is its menu rather than a
/// list of its entries, and the draw walks the table rather than writing
/// the entries out. The tests that hold these menus against the browser
/// read the same tables.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Divider,
    NodeInfo,
    CopyNodePath,
    ToggleBypass,
    SetDisplayFlag,
    ResetAllParameters,
    ResetTab,
    PinToThisNode,
    FloatingProperties,
    Maximize,
}

impl Row {
    /// `None` is the divider. [`Row::ResetTab`] gives the wording used when
    /// no tab can be reset; when one can, the entry is named after it, and
    /// the browser does the same from the same fallback.
    const fn label(self) -> Option<&'static str> {
        Some(match self {
            Self::Divider => return None,
            Self::NodeInfo => "Node Info",
            Self::CopyNodePath => "Copy Node Path",
            Self::ToggleBypass => "Toggle Bypass",
            Self::SetDisplayFlag => "Set Display Flag",
            Self::ResetAllParameters => "Reset All Parameters",
            Self::ResetTab => "Reset Current Tab",
            Self::PinToThisNode => "Pin to This Node",
            Self::FloatingProperties => "Floating Properties",
            Self::Maximize => MAXIMIZE_LABEL,
        })
    }

    /// The binding whose key the row shows, read from the table rather than
    /// typed beside the entry.
    const fn action(self) -> Option<Action> {
        match self {
            Self::NodeInfo => Some(Action::NodeInfo),
            Self::ToggleBypass => Some(Action::Bypass),
            Self::SetDisplayFlag => Some(Action::DisplayFlag),
            Self::FloatingProperties => Some(Action::FloatingProps),
            Self::Maximize => Some(Action::PanelMaximize),
            _ => None,
        }
    }
}

use Row::Divider;

const NODE_MENU: &[Row] = &[
    Row::NodeInfo,
    Row::CopyNodePath,
    Divider,
    Row::ToggleBypass,
    Row::SetDisplayFlag,
];

const PARAMS_MENU: &[Row] = &[Row::ResetAllParameters, Row::ResetTab];

const VIEW_MENU: &[Row] = &[
    Row::PinToThisNode,
    Row::FloatingProperties,
    Divider,
    Row::Maximize,
];

fn draw_node(ui: &mut egui::Ui, model: &BarModel, intents: &mut Intents) {
    let node = model.node;
    for row in NODE_MENU {
        let Some(label) = row.label() else {
            ui.separator();
            continue;
        };
        let action = row.action();
        match row {
            Row::NodeInfo => {
                if entry_if(ui, node.is_some(), label, action, NO_NODE).clicked()
                    && let Some((_, id)) = node
                {
                    intents.panel(PanelIntent::OpenNodeInfo(id));
                    ui.close();
                }
            }
            Row::CopyNodePath => {
                if entry_if(ui, model.path.is_some(), label, action, NO_NODE).clicked()
                    && let Some(path) = &model.path
                {
                    ui.ctx().copy_text(path.clone());
                    intents.panel(PanelIntent::Canvas(CanvasAction::Notify(format!(
                        "Copied {path}"
                    ))));
                    ui.close();
                }
            }
            Row::ToggleBypass => {
                let why_not = if node.is_none() {
                    NO_NODE
                } else {
                    "This node type cannot be bypassed"
                };
                if entry_if(ui, model.bypassed.is_some(), label, action, why_not).clicked()
                    && let (Some((ctx, id)), Some(bypassed)) = (node, model.bypassed)
                {
                    intents.panel(PanelIntent::Canvas(CanvasAction::SetBypass(
                        ctx, id, !bypassed,
                    )));
                    ui.close();
                }
            }
            Row::SetDisplayFlag => {
                let why_not = if node.is_none() {
                    NO_NODE
                } else {
                    "A display flag is set inside a network; at the top level every object shows"
                };
                let can = node.is_some() && model.in_container;
                if entry_if(ui, can, label, action, why_not).clicked()
                    && let Some((ctx, id)) = node
                {
                    intents.panel(PanelIntent::Canvas(CanvasAction::SetActiveOutput(ctx, id)));
                    ui.close();
                }
            }
            _ => {}
        }
    }
}

fn draw_params(ui: &mut egui::Ui, model: &BarModel, intents: &mut Intents) {
    for row in PARAMS_MENU {
        let Some(label) = row.label() else {
            ui.separator();
            continue;
        };
        let action = row.action();
        match row {
            Row::ResetAllParameters => {
                let why_not = if model.node.is_none() {
                    NO_NODE
                } else {
                    "This node has no parameters"
                };
                let can = model.node.is_some() && model.has_params;
                if entry_if(ui, can, label, action, why_not).clicked()
                    && let Some((ctx, id)) = model.node
                {
                    intents.panel(PanelIntent::ResetParams(ctx, id, None));
                    ui.close();
                }
            }
            Row::ResetTab => {
                // Named after the tab it resets, so the entry says what it
                // will do before it does it. The table's wording is the
                // fallback, which is what the browser falls back to too.
                let named = model
                    .reset_tab
                    .as_ref()
                    .map_or_else(|| label.to_string(), |(tab, _)| format!("Reset {tab} Tab"));
                let why_not = if model.node.is_none() {
                    NO_NODE
                } else {
                    "The current tab holds no parameters to reset"
                };
                if entry_if(ui, model.reset_tab.is_some(), &named, action, why_not).clicked()
                    && let (Some((ctx, id)), Some((_, keys))) = (model.node, &model.reset_tab)
                {
                    intents.panel(PanelIntent::ResetParams(ctx, id, Some(keys.clone())));
                    ui.close();
                }
            }
            _ => {}
        }
    }
}

fn draw_view(
    ui: &mut egui::Ui,
    model: &BarModel,
    floating_open: bool,
    intents: &mut Intents,
    request: &mut BarRequest,
) {
    for row in VIEW_MENU {
        let Some(label) = row.label() else {
            ui.separator();
            continue;
        };
        match row {
            // Pinning is the panel's own state, so it sits beside Maximize
            // rather than in the Node menu, which acts on the node.
            Row::PinToThisNode => {
                let can_pin = model.pinned || model.node.is_some();
                if ui
                    .add_enabled(can_pin, egui::Button::new(label).selected(model.pinned))
                    .on_disabled_hover_text(NO_NODE)
                    .clicked()
                {
                    request.toggle_pin = true;
                    ui.close();
                }
            }
            Row::FloatingProperties => {
                if check_entry(ui, floating_open, label, row.action()).clicked() {
                    intents.raise(Intent::Layout(LayoutIntent::ToggleFloatingProps));
                    ui.close();
                }
            }
            Row::Maximize => maximize_entry(ui, SolarxyTab::Properties, intents),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::keymap::hint;
    use solarxy_graph::{Command, Engine};

    fn browser_source() -> String {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("the repository root");
        std::fs::read_to_string(root.join("web/src/components/menu/PropertiesMenus.tsx"))
            .expect("the browser's properties menus")
    }

    /// One of the browser's three entry lists, as `(label, key)` pairs in
    /// its order. The key is whatever it types beside the entry, or `None`
    /// where it types nothing; each entry runs from its own label to the
    /// next, so a key is attributed to the entry it was written under.
    ///
    /// Its `Reset ... Tab` row is a template string, so what a reader sees
    /// there is the literal it falls back to, which is the wording this
    /// shell keeps in its own table for the same reason.
    fn browser_entries(source: &str, decl: &str) -> Vec<(String, Option<String>)> {
        let start = source
            .find(decl)
            .unwrap_or_else(|| panic!("no entry list declared {decl}"));
        let block = &source[start..];
        let block = &block[..block.find("\n  ];").expect("the list's end")];
        block
            .split("label: ")
            .skip(1)
            .filter_map(|rest| {
                // The dynamic row quotes its fallback last, after the ternary.
                let label = rest
                    .split('\n')
                    .next()?
                    .rsplit_once('"')
                    .and_then(|(head, _)| head.rsplit('"').next())?
                    .to_string();
                let body = rest.split("label: ").next().unwrap_or(rest);
                let key = body
                    .split("shortcut: menuHint(\"")
                    .nth(1)
                    .and_then(|k| k.split('"').next())
                    .map(str::to_string);
                Some((label, key))
            })
            .collect()
    }

    /// The three menus list the browser's entries, in its order and under
    /// its words.
    #[test]
    fn the_three_menus_list_the_browsers_entries_in_its_order() {
        let source = browser_source();
        for (decl, rows) in [
            ("const nodeEntries: MenuEntry[] = [", NODE_MENU),
            ("const paramsEntries: MenuEntry[] = [", PARAMS_MENU),
            ("const viewEntries: MenuEntry[] = [", VIEW_MENU),
        ] {
            let browser: Vec<String> = browser_entries(&source, decl)
                .into_iter()
                .map(|(label, _)| label)
                .collect();
            assert!(
                browser.len() >= 2,
                "read {} entries for {decl}, so the reader is broken",
                browser.len()
            );
            let here: Vec<String> = rows
                .iter()
                .filter_map(|row| row.label())
                .map(|label| label.replace('\u{2026}', "..."))
                .collect();
            assert_eq!(here, browser, "{decl}");
        }
    }

    /// Each row shows the same key the browser shows beside the same entry,
    /// with the one disagreement named and explained.
    #[test]
    fn the_three_menus_show_the_browsers_keys() {
        let source = browser_source();
        let browser: Vec<(String, Option<String>)> = [
            "const nodeEntries: MenuEntry[] = [",
            "const paramsEntries: MenuEntry[] = [",
            "const viewEntries: MenuEntry[] = [",
        ]
        .into_iter()
        .flat_map(|decl| browser_entries(&source, decl))
        .collect();
        assert!(
            browser.iter().filter(|(_, key)| key.is_some()).count() >= 4,
            "the reader found no keys, so it is broken"
        );

        for row in NODE_MENU.iter().chain(PARAMS_MENU).chain(VIEW_MENU) {
            let Some(label) = row.label() else { continue };
            let Some((_, theirs)) = browser.iter().find(|(l, _)| l == label) else {
                panic!("{label} is in no browser menu");
            };
            let ours = row.action().map(Action::id);
            assert_eq!(ours, theirs.as_deref(), "the binding named beside {label}");
        }
    }

    /// Every row that names a binding shows the key the table gives it, so
    /// a hint here cannot name a key that does nothing.
    #[test]
    fn every_row_that_names_a_binding_shows_the_tables_key() {
        for row in NODE_MENU.iter().chain(PARAMS_MENU).chain(VIEW_MENU) {
            if let Some(action) = row.action() {
                assert!(
                    hint(action).is_some(),
                    "{:?} names an unbound action",
                    row.label()
                );
            }
        }
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

    fn select(engine: &mut Engine, ctx: GraphContext, ids: Vec<NodeId>) {
        engine
            .apply(Command::SetSelection { ctx, ids })
            .expect("select");
    }

    fn scene(engine: &Engine, ctx: GraphContext, has_report: bool) -> ParamScene<'_> {
        static NOTHING: std::sync::OnceLock<super::super::expression::Resolved> =
            std::sync::OnceLock::new();
        ParamScene {
            doc: engine.document(),
            registry: engine.registry(),
            ctx,
            stats: None,
            has_report,
            report: None,
            counts: (0, 0),
            assets: &[],
            lanes: &[],
            error: None,
            resolved: NOTHING.get_or_init(super::super::expression::Resolved::new),
        }
    }

    /// With nothing open and nothing selected the bar offers nothing, and
    /// still knows whether the panel is pinned, because letting go of a pin
    /// has to stay possible when the pinned node is out of reach.
    #[test]
    fn with_no_subject_the_bar_offers_nothing_but_the_way_out_of_a_pin() {
        assert_eq!(bar_model(None, None, ""), BarModel::default());
        assert!(bar_model(None, Some(NodeId(7)), "").pinned);

        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        select(&mut engine, GraphContext::Root, vec![]);
        let model = bar_model(Some(&scene(&engine, GraphContext::Root, false)), None, "");
        assert_eq!(model, BarModel::default());

        // Pinned to a node that is not in the graph being shown: no
        // subject, and the pin is still reported.
        let inside = GraphContext::Subflow(geo);
        let model = bar_model(Some(&scene(&engine, inside, false)), Some(geo), "");
        assert_eq!(model.node, None);
        assert!(model.pinned);
    }

    /// A node at the top level has a one-segment path and no display flag
    /// to set; one inside a network has both segments and the flag.
    #[test]
    fn the_display_flag_and_the_path_follow_where_the_node_sits() {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let inside = GraphContext::Subflow(geo);
        let sphere = added(&mut engine, inside, "sphere");

        select(&mut engine, GraphContext::Root, vec![geo]);
        let top = bar_model(Some(&scene(&engine, GraphContext::Root, false)), None, "");
        assert_eq!(top.node, Some((GraphContext::Root, geo)));
        assert!(!top.in_container);
        let top_path = top.path.expect("a path");
        assert_eq!(top_path.matches('/').count(), 1, "{top_path}");

        select(&mut engine, inside, vec![sphere]);
        let inner = bar_model(Some(&scene(&engine, inside, false)), None, "");
        assert_eq!(inner.node, Some((inside, sphere)));
        assert!(inner.in_container);
        let inner_path = inner.path.expect("a path");
        assert!(inner_path.starts_with(&top_path), "{inner_path}");
        assert_eq!(inner_path.matches('/').count(), 2, "{inner_path}");
    }

    /// The bar acts on the node the panel shows. A pin wins over the
    /// selection here as it does underneath, so a reset from the menu
    /// cannot land on a node the panel is not showing.
    #[test]
    fn the_bar_acts_on_the_pinned_node_rather_than_the_selection() {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let inside = GraphContext::Subflow(geo);
        let sphere = added(&mut engine, inside, "sphere");
        let cube = added(&mut engine, inside, "box");
        select(&mut engine, inside, vec![cube]);

        let following = bar_model(Some(&scene(&engine, inside, false)), None, "");
        assert_eq!(following.node, Some((inside, cube)));
        assert!(!following.pinned);

        let pinned = bar_model(Some(&scene(&engine, inside, false)), Some(sphere), "");
        assert_eq!(pinned.node, Some((inside, sphere)));
        assert!(pinned.pinned);
    }

    /// Bypass is offered with the node's current state, and not at all for
    /// a type that cannot be bypassed, which the descriptor declares.
    #[test]
    fn bypass_is_offered_only_where_the_type_allows_it() {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let inside = GraphContext::Subflow(geo);
        let sphere = added(&mut engine, inside, "sphere");
        select(&mut engine, inside, vec![sphere]);
        let model = bar_model(Some(&scene(&engine, inside, false)), None, "");
        assert_eq!(model.bypassed, Some(false));

        engine
            .apply(Command::SetBypass {
                ctx: inside,
                node: sphere,
                bypassed: true,
            })
            .expect("bypass");
        let model = bar_model(Some(&scene(&engine, inside, false)), None, "");
        assert_eq!(model.bypassed, Some(true));

        // Every type that declares it cannot be bypassed, in whichever
        // context takes it.
        let fixed: Vec<(String, solarxy_graph::document::ContextKind)> = engine
            .registry()
            .descriptors()
            .filter(|d| matches!(d.bypass, BypassBehavior::NotBypassable))
            .filter_map(|d| {
                [
                    solarxy_graph::document::ContextKind::Obj,
                    solarxy_graph::document::ContextKind::Sop,
                ]
                .into_iter()
                .find(|kind| d.contexts.contains(*kind))
                .map(|kind| (d.type_id.to_string(), kind))
            })
            .collect();
        assert!(
            !fixed.is_empty(),
            "the registry has a type that cannot be bypassed"
        );
        for (type_id, kind) in fixed {
            let ctx = if kind == solarxy_graph::document::ContextKind::Obj {
                GraphContext::Root
            } else {
                inside
            };
            let node = added(&mut engine, ctx, &type_id);
            select(&mut engine, ctx, vec![node]);
            let model = bar_model(Some(&scene(&engine, ctx, false)), None, "");
            assert_eq!(model.node, Some((ctx, node)), "{type_id}");
            assert_eq!(model.bypassed, None, "{type_id} cannot be bypassed");
        }
    }

    /// The tab a reset names is the one the panel shows, asked of the same
    /// resolver, and the validation tab resets nothing.
    #[test]
    fn the_reset_names_the_panels_own_current_tab() {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let inside = GraphContext::Subflow(geo);
        let sphere = added(&mut engine, inside, "sphere");
        select(&mut engine, inside, vec![sphere]);

        let data = engine
            .document()
            .graph(inside)
            .expect("the graph")
            .node(sphere)
            .expect("the node");
        let desc = engine.registry().get(&data.type_id).expect("registered");
        let (offered, active) = tabs(&desc.params, &data.params, false, "");
        let active = active.expect("a node with parameters has a current tab");

        let model = bar_model(Some(&scene(&engine, inside, false)), None, "");
        assert!(model.has_params);
        let (label, keys) = model.reset_tab.expect("the current tab resets");
        assert_eq!(label, params::tab_label(&active));
        let expected: Vec<String> = reset_keys(&desc.params, &active)
            .expect("keys")
            .into_iter()
            .map(str::to_owned)
            .collect();
        assert_eq!(keys, expected);

        // A stored tab that the node offers is the one named.
        if let Some(other) = offered.iter().find(|tab| **tab != active) {
            let model = bar_model(Some(&scene(&engine, inside, false)), None, other);
            assert_eq!(
                model.reset_tab.map(|(label, _)| label),
                reset_keys(&desc.params, other).map(|_| params::tab_label(other))
            );
        }

        // On the validation tab there is nothing to reset.
        let model = bar_model(
            Some(&scene(&engine, inside, true)),
            None,
            params::VALIDATION_TAB,
        );
        assert_eq!(model.reset_tab, None);
        assert!(model.has_params, "Reset All still applies");
    }
}
