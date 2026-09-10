//! The Node Tree panel — a read-only outline of an open scene's graph.
//!
//! The desktop has no node canvas, so this is the only place a user can see
//! what produced the geometry in the viewport: every context from the root
//! down, with node names, type ids, the display flag and bypass state.
//!
//! **A viewer, not an editor.** No creation, no rewiring, no renaming and
//! no parameter edits. The one thing it writes is the selection, and that
//! travels as a [`Command::SetSelection`] like every other engine write.
//!
//! [`Command::SetSelection`]: solarxy_graph::Command::SetSelection
//!
//! ## Two ways down, mirroring the web
//!
//! A twisty expands a container's children in place, and double-clicking a
//! container dives into it so the panel shows that context alone with a
//! breadcrumb back out. Both come from `web/src/components/TreePane.tsx`;
//! the desktop keeps the same gestures so the two shells are learned once.
//!
//! ## One fold, both views, and one fold across both shells
//!
//! [`solarxy_studio::tree::scene_tree`] folds the whole document from the
//! root exactly once, and the dived view is a subtree of that result
//! ([`find_subtree`]). The breadcrumb falls out of the same walk, so there
//! is no second parent lookup that could disagree with the first about who
//! owns what.
//!
//! The fold itself is **not** this panel's. It was, and it was a
//! line-for-line reimplementation of the browser's, down to the depth
//! guard and the choice to store collapsed keys rather than expanded ones.
//! Two implementations of one outline is exactly the pair the shared
//! interface crate exists to remove, so this panel now reads
//! `solarxy-studio` and draws what comes back.

use std::collections::HashSet;

use solarxy_graph::document::{Document, GraphContext, NodeId};
use solarxy_graph::registry::Registry;
use solarxy_studio::tree::TreeRow;

use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;

/// One breadcrumb step: where it jumps to, and what it reads.
pub(crate) struct Crumb {
    pub ctx: GraphContext,
    pub label: String,
}

/// Resolve a dived context to the rows it shows plus the breadcrumb back
/// out. `None` when the context no longer exists in the tree, which is how
/// a dive survives the scene it was made in being replaced: the caller
/// falls back to the root instead of showing an empty panel.
///
/// The root crumb is always present and always first, so a dived view can
/// always be escaped.
pub(crate) fn find_subtree(
    rows: &[TreeRow],
    ctx: GraphContext,
) -> Option<(&[TreeRow], Vec<Crumb>)> {
    let mut crumbs = vec![Crumb {
        ctx: GraphContext::Root,
        label: "/obj".to_string(),
    }];
    if ctx == GraphContext::Root {
        return Some((rows, crumbs));
    }
    descend(rows, ctx, &mut crumbs).map(|found| (found, crumbs))
}

/// Depth-first hunt for `ctx`, pushing a crumb on the way down and popping
/// it on the way back up, so `crumbs` ends as the path to whatever is
/// returned and is left untouched when nothing is.
fn descend<'a>(
    rows: &'a [TreeRow],
    ctx: GraphContext,
    crumbs: &mut Vec<Crumb>,
) -> Option<&'a [TreeRow]> {
    for row in rows {
        if row.opens.is_none() {
            continue;
        }
        crumbs.push(Crumb {
            ctx: GraphContext::Subflow(row.node),
            label: row.label.clone(),
        });
        if GraphContext::Subflow(row.node) == ctx {
            return Some(&row.children);
        }
        if let Some(found) = descend(&row.children, ctx, crumbs) {
            return Some(found);
        }
        crumbs.pop();
    }
    None
}

/// What the panel draws: the open document, or nothing.
#[derive(Clone, Copy)]
pub(crate) enum NodeTreeSource<'a> {
    /// Nothing is open at all.
    Empty,
    /// A cooked scene's document.
    Scene {
        doc: &'a Document,
        registry: &'a Registry,
    },
}

/// The panel's own view state: where the user has dived to, and which
/// containers they have folded shut.
///
/// **Collapsed keys, not expanded ones**, so the empty default reads as a
/// fully expanded tree and a container that appears later arrives expanded
/// too. A graph context holds a handful of nodes, unlike the Outliner's
/// object rows, which is why this defaults the opposite way to that panel.
pub(crate) struct NodeTreeState {
    /// The context the panel is showing. `Root` unless the user dived.
    ctx: GraphContext,
    collapsed: HashSet<(GraphContext, NodeId)>,
}

impl Default for NodeTreeState {
    fn default() -> Self {
        Self {
            ctx: GraphContext::Root,
            collapsed: HashSet::new(),
        }
    }
}

impl NodeTreeState {
    /// Return to the root and unfold everything. Called whenever the open
    /// document is replaced, since both halves address nodes that the new
    /// document need not contain.
    pub(crate) fn reset(&mut self) {
        self.ctx = GraphContext::Root;
        self.collapsed.clear();
    }

    /// The context the panel is showing, which is where a selection made in
    /// it lives and where a dropped model lands.
    pub(crate) fn ctx(&self) -> GraphContext {
        self.ctx
    }
}

/// One Node Tree interaction, raised during an egui pass and drained by
/// `state/intents.rs` after it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum NodeTreeAction {
    /// Select this node in the context it lives in.
    Select(GraphContext, NodeId),
}

/// Render the Node Tree into `ui` (the `egui_dock` tab supplies the `Ui`).
pub(in crate::gui) fn draw_node_tree_content(
    ui: &mut egui::Ui,
    source: NodeTreeSource<'_>,
    state: &mut NodeTreeState,
    intents: &mut Intents,
    theme: Theme,
) {
    let (doc, registry) = match source {
        NodeTreeSource::Empty => return draw_placeholder(ui, "No document open", None),
        NodeTreeSource::Scene { doc, registry } => (doc, registry),
    };

    let rows = solarxy_studio::tree::scene_tree(doc, registry);
    // A dive that no longer resolves falls back to the root rather than
    // leaving the panel blank with no way out.
    if find_subtree(&rows, state.ctx).is_none() {
        state.reset();
    }
    let Some((visible, crumbs)) = find_subtree(&rows, state.ctx) else {
        return;
    };

    if crumbs.len() > 1 {
        draw_breadcrumb(ui, &crumbs, state, theme);
    }

    if visible.is_empty() {
        draw_placeholder(ui, "This context is empty", None);
        return;
    }

    let selection = doc
        .graph(state.ctx)
        .ok()
        .map(|g| g.selection.as_slice())
        .unwrap_or_default();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);
        for row in visible {
            draw_row(ui, row, 0, selection, state, intents, theme);
        }
        ui.add_space(8.0);
    });
}

/// The breadcrumb out of a dived context. Every crumb but the last is a
/// jump target; the last one is where you already are.
fn draw_breadcrumb(ui: &mut egui::Ui, crumbs: &[Crumb], state: &mut NodeTreeState, theme: Theme) {
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
                        state.ctx = crumb.ctx;
                    }
                }
            });
        });
    ui.separator();
}

/// One row, then its children if it is an expanded container.
///
/// `depth` is the indent level **within the current view**, so diving
/// re-zeroes it and the dived context reads as its own tree rather than as
/// a fragment indented off the edge of the panel.
fn draw_row(
    ui: &mut egui::Ui,
    row: &TreeRow,
    depth: usize,
    selection: &[NodeId],
    state: &mut NodeTreeState,
    intents: &mut Intents,
    theme: Theme,
) {
    let key = (row.ctx, row.node);
    let expanded = !state.collapsed.contains(&key);
    let selected = selection.contains(&row.node);

    let mut toggle = false;
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);

        // Leaves reserve the twisty's width so every label in the view
        // shares one left edge.
        let (twisty_rect, twisty) =
            ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::click());
        if row.opens.is_some() {
            let openness = if expanded { 1.0 } else { 0.0 };
            egui::collapsing_header::paint_default_icon(ui, openness, &twisty);
            if twisty.clicked() {
                toggle = true;
            }
        } else {
            // A leaf still gets a mark, so the eye can tell "no children"
            // from "children, folded shut" without reading the indent.
            ui.painter()
                .circle_filled(twisty_rect.center(), 1.5, theme.muted);
        }

        let mut label = egui::RichText::new(&row.label);
        if row.bypassed {
            // Bypass is engine state a viewer must show: a bypassed node
            // is often the whole explanation for a missing object.
            label = label.strikethrough().color(theme.muted);
        }
        let response = ui
            .selectable_label(selected, label)
            .on_hover_text(if row.opens.is_some() {
                "Click to select, double-click to open"
            } else {
                "Click to select"
            });
        if response.clicked() {
            intents.panel(PanelIntent::NodeTree(NodeTreeAction::Select(
                row.ctx, row.node,
            )));
        }
        if response.double_clicked() && row.opens.is_some() {
            state.ctx = GraphContext::Subflow(row.node);
        }

        if row.is_display {
            let (dot, response) =
                ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.0, theme.accent);
            response.on_hover_text("Display flag: this node's output is what the context shows");
        }

        // The type id sits hard right, muted, so the name column stays
        // scannable and the type is there when it is wanted.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(&row.type_id)
                    .size(10.0)
                    .color(theme.muted),
            );
        });
    });

    if toggle {
        if expanded {
            state.collapsed.insert(key);
        } else {
            state.collapsed.remove(&key);
        }
    }

    if row.opens.is_some() && expanded {
        for child in &row.children {
            draw_row(ui, child, depth + 1, selection, state, intents, theme);
        }
    }
}

fn draw_placeholder(ui: &mut egui::Ui, headline: &str, detail: Option<&str>) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(headline).weak());
        if let Some(detail) = detail {
            ui.add_space(4.0);
            ui.label(egui::RichText::new(detail).weak().small());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_graph::{Command, Engine};

    /// Build a document with one geo container holding a box, plus a
    /// portless root node, and return the engine holding it.
    fn scene() -> (Engine, NodeId, NodeId) {
        let mut engine = Engine::new().expect("registry builds");
        let geo = added(&mut engine, GraphContext::Root, "sopnet");
        let _light = added(&mut engine, GraphContext::Root, "point_light");
        let leaf = added(&mut engine, GraphContext::Subflow(geo), "box");
        (engine, geo, leaf)
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

    #[test]
    fn containers_nest_and_leaves_do_not() {
        let (engine, geo, leaf) = scene();
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());

        let container = rows
            .iter()
            .find(|r| r.node == geo)
            .expect("geo row present");
        assert!(container.opens.is_some(), "a geo opens a child network");
        assert_eq!(container.children.len(), 1, "the box is its only child");
        assert_eq!(container.children[0].node, leaf);

        let leaf = rows
            .iter()
            .find(|r| r.node != geo)
            .expect("the light row is present");
        assert!(leaf.opens.is_none(), "a light opens nothing");
        assert!(leaf.children.is_empty());
    }

    /// A child's `ctx` must be the context it LIVES in, because that is
    /// what its selection dispatches against. Recording the parent's
    /// context here would select the wrong graph.
    #[test]
    fn a_child_row_carries_its_own_context() {
        let (engine, geo, leaf) = scene();
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());
        let container = rows
            .iter()
            .find(|r| r.node == geo)
            .expect("geo row present");

        assert_eq!(container.ctx, GraphContext::Root);
        assert_eq!(container.children[0].ctx, GraphContext::Subflow(geo));
        assert_eq!(container.children[0].node, leaf);
    }

    /// The display flag is per-context: the box holds the geo subflow's,
    /// and the root's containers hold none of it.
    #[test]
    fn the_display_flag_is_read_per_context() {
        let (engine, geo, leaf) = scene();
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());
        let container = rows
            .iter()
            .find(|r| r.node == geo)
            .expect("geo row present");

        assert!(
            container.children[0].is_display,
            "the first node added to a subflow takes its display flag"
        );
        assert!(
            !container.is_display,
            "the root context's flag is not the subflow's"
        );
        let _ = leaf;
    }

    #[test]
    fn bypass_is_carried_onto_the_row() {
        let (mut engine, geo, leaf) = scene();
        engine
            .apply(Command::SetBypass {
                ctx: GraphContext::Subflow(geo),
                node: leaf,
                bypassed: true,
            })
            .expect("a box is bypassable");

        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());
        let container = rows
            .iter()
            .find(|r| r.node == geo)
            .expect("geo row present");
        assert!(container.children[0].bypassed);
    }

    #[test]
    fn the_root_subtree_is_the_whole_tree_with_one_crumb() {
        let (engine, _, _) = scene();
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());

        let (visible, crumbs) =
            find_subtree(&rows, GraphContext::Root).expect("the root always resolves");
        assert_eq!(visible.len(), rows.len());
        assert_eq!(crumbs.len(), 1);
        assert_eq!(crumbs[0].label, "/obj");
        assert_eq!(crumbs[0].ctx, GraphContext::Root);
    }

    /// The breadcrumb is what gets a user back out, so it must always
    /// start at the root and end at where they are.
    #[test]
    fn diving_yields_the_children_and_a_walkable_breadcrumb() {
        let (engine, geo, leaf) = scene();
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());

        let (visible, crumbs) =
            find_subtree(&rows, GraphContext::Subflow(geo)).expect("the geo subflow resolves");
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].node, leaf);

        assert_eq!(crumbs.len(), 2, "root, then the container dived into");
        assert_eq!(crumbs[0].ctx, GraphContext::Root);
        assert_eq!(crumbs[1].ctx, GraphContext::Subflow(geo));
    }

    /// The case that keeps a stale dive from blanking the panel: opening a
    /// second scene leaves `NodeTreeState.ctx` pointing at a node the new
    /// document does not have.
    #[test]
    fn a_context_outside_the_tree_does_not_resolve() {
        let (engine, _, _) = scene();
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());
        assert!(find_subtree(&rows, GraphContext::Subflow(NodeId(9_999))).is_none());
    }
}
