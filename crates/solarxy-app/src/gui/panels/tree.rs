//! The Tree panel: a searchable outline of the whole scene.
//!
//! It answers the question the canvas beside it does not: what the whole
//! document contains, every context from the root down in one fold, with
//! node names, glyphs, type ids, the display flag and bypass state. The
//! canvas shows one context at a time and shows it as a graph; this shows
//! all of them at once and shows them as a list.
//!
//! **A viewer, not an editor.** No creation, no rewiring, no renaming and
//! no parameter edits; that is the canvas's job. The one thing it writes
//! is the selection, and that travels as a [`Command::SetSelection`] like
//! every other engine write. Where the user has dived is written too, but
//! that is session state shared with the canvas rather than the document.
//!
//! [`Command::SetSelection`]: solarxy_graph::Command::SetSelection
//!
//! ## The browser's gestures, exactly
//!
//! A chevron folds a container's children in place. Double-clicking a
//! container dives into it; double-clicking a leaf reveals it, switching
//! the shared context to the one the node lives in **before** selecting
//! it, so the canvas mounts the right graph before the selection paints.
//! The search strip narrows the tree to matches and their ancestors,
//! force-expanding the ancestors while leaving the folds the user set
//! alone, so clearing the query restores them. Expand all and Collapse
//! all act on every branch. All of it is `web/src/components/TreePane.tsx`.
//!
//! ## One fold, both views, and one fold across both shells
//!
//! [`solarxy_studio::tree::scene_tree`] folds the whole document from the
//! root exactly once; the dived view is a subtree of that result
//! ([`solarxy_studio::tree::subtree`]), the search is
//! [`solarxy_studio::tree::search_tree`] and the fold-all set is
//! [`solarxy_studio::tree::branch_keys`]. The breadcrumb falls out of the
//! same walk, so there is no second parent lookup that could disagree
//! with the first about who owns what.
//!
//! This panel replaced two: the Node Tree, which was this outline without
//! the search, and the Outliner, which listed a file model's meshes and
//! materials and had no data source once the document became the one
//! root. The browser has one `tree` panel, and now so does this shell.

use std::collections::HashSet;

use solarxy_graph::document::{Document, GraphContext, NodeId};
use solarxy_graph::registry::Registry;
use solarxy_studio::tree::{Crumb, TreeRow, TreeSearch, branch_keys, search_tree, subtree};

use crate::gui::intent::{Intents, PanelIntent};
use crate::gui::theme::Theme;

/// What the panel draws: the open document, or nothing.
#[derive(Clone, Copy)]
pub(crate) enum TreeSource<'a> {
    /// Nothing is open at all.
    Empty,
    /// A cooked scene's document.
    Scene {
        doc: &'a Document,
        registry: &'a Registry,
    },
}

/// The panel's own view state: which containers are folded shut, and the
/// search.
///
/// **Collapsed keys, not expanded ones**, so the empty default reads as a
/// fully expanded tree and a container that appears later arrives expanded
/// too. Keyed on the fold's stable row key, which survives a rename.
#[derive(Default)]
pub(crate) struct TreeState {
    collapsed: HashSet<String>,
    query: String,
}

impl TreeState {
    /// Unfold everything and drop the search. Called whenever the open
    /// document is replaced, since every key held here addresses nodes the
    /// new document need not contain.
    ///
    /// Where the user has dived is **not** here. It is one fact about the
    /// session rather than one per panel, so this tree and the canvas read
    /// and write the same value and a dive made in either is where the
    /// other is looking.
    pub(crate) fn reset(&mut self) {
        self.collapsed.clear();
        self.query.clear();
    }
}

/// One Tree interaction, raised during an egui pass and drained by
/// `state/intents.rs` after it.
#[derive(Debug, Clone, Copy)]
pub(crate) enum TreeAction {
    /// Select this node in the context it lives in.
    Select(GraphContext, NodeId),
}

/// Whether a row draws under the current search: every row with no query,
/// else a match or an ancestor of one.
fn row_visible(search: Option<&TreeSearch>, key: &str) -> bool {
    match search {
        None => true,
        Some(s) => s.matches.iter().any(|k| k == key) || s.expand.iter().any(|k| k == key),
    }
}

/// Whether a container's children draw: a search force-expands an
/// ancestor of a match and otherwise leaves the user's folds alone.
fn row_expanded(search: Option<&TreeSearch>, collapsed: &HashSet<String>, key: &str) -> bool {
    let forced = search.is_some_and(|s| s.expand.iter().any(|k| k == key));
    forced || !collapsed.contains(key)
}

/// Render the Tree into `ui` (the `egui_dock` tab supplies the `Ui`).
pub(in crate::gui) fn draw_tree_content(
    ui: &mut egui::Ui,
    source: TreeSource<'_>,
    state: &mut TreeState,
    ctx: &mut GraphContext,
    intents: &mut Intents,
    theme: Theme,
) {
    let (doc, registry) = match source {
        TreeSource::Empty => return draw_placeholder(ui, "No scene yet."),
        TreeSource::Scene { doc, registry } => (doc, registry),
    };

    let rows = solarxy_studio::tree::scene_tree(doc, registry);
    // A dive that no longer resolves falls back to the root rather than
    // leaving the panel blank with no way out.
    if subtree(&rows, *ctx).is_none() {
        *ctx = GraphContext::Root;
        state.reset();
    }
    let Some((visible, crumbs)) = subtree(&rows, *ctx) else {
        return;
    };

    draw_search_strip(ui, state, &rows, theme);
    if crumbs.len() > 1 {
        draw_breadcrumb(ui, &crumbs, ctx, theme);
    }

    if rows.is_empty() {
        return draw_placeholder(ui, "No nodes in the scene yet.");
    }
    let query = state.query.trim();
    let search = (!query.is_empty()).then(|| search_tree(&rows, query));
    if let Some(s) = &search
        && s.matches.is_empty()
    {
        return draw_placeholder(ui, &format!("No nodes match \"{query}\"."));
    }
    if visible.is_empty() {
        return draw_placeholder(ui, "This context is empty");
    }

    let selection = doc
        .graph(*ctx)
        .ok()
        .map(|g| g.selection.as_slice())
        .unwrap_or_default();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.add_space(2.0);
        for row in visible {
            draw_row(
                ui,
                row,
                registry,
                0,
                selection,
                search.as_ref(),
                state,
                ctx,
                intents,
                theme,
            );
        }
        ui.add_space(8.0);
    });
}

/// The search field and the two fold buttons.
fn draw_search_strip(ui: &mut egui::Ui, state: &mut TreeState, rows: &[TreeRow], theme: Theme) {
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut state.query)
                .hint_text("Search nodes...")
                .desired_width(ui.available_width() - 64.0),
        );
        if ui
            .small_button("\u{25bd}")
            .on_hover_text("Expand all")
            .clicked()
        {
            state.collapsed.clear();
        }
        if ui
            .small_button("\u{25b3}")
            .on_hover_text("Collapse all")
            .clicked()
        {
            state.collapsed = branch_keys(rows).into_iter().collect();
        }
    });
    let _ = theme;
    ui.separator();
}

/// The breadcrumb out of a dived context. Every crumb but the last is a
/// jump target; the last one is where you already are.
fn draw_breadcrumb(ui: &mut egui::Ui, crumbs: &[Crumb], ctx: &mut GraphContext, theme: Theme) {
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
#[allow(clippy::too_many_arguments)]
fn draw_row(
    ui: &mut egui::Ui,
    row: &TreeRow,
    registry: &Registry,
    depth: usize,
    selection: &[NodeId],
    search: Option<&TreeSearch>,
    state: &mut TreeState,
    ctx: &mut GraphContext,
    intents: &mut Intents,
    theme: Theme,
) {
    if !row_visible(search, &row.key) {
        return;
    }
    let expanded = row_expanded(search, &state.collapsed, &row.key);
    let selected = selection.contains(&row.node);
    let is_match = search.is_some_and(|s| s.matches.contains(&row.key));
    let desc = registry.get(&row.type_id);

    let mut toggle = false;
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);

        // Leaves reserve the chevron's width so every label in the view
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
            ui.painter()
                .circle_filled(twisty_rect.center(), 1.5, theme.muted);
        }

        // A container's tint, in the canvas's own colour for its category,
        // so the tree's colour language matches the graph's.
        if row.opens.is_some() {
            let (chip, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
            ui.painter()
                .circle_filled(chip.center(), 3.5, container_tint(desc, theme));
        }

        // The glyph the canvas draws, at row size.
        let (glyph_rect, _) = ui.allocate_exact_size(egui::vec2(13.0, 13.0), egui::Sense::hover());
        super::nodes::paint_glyph(
            ui.painter(),
            &super::nodes::glyph_key(desc),
            glyph_rect,
            theme.fg,
            1.2,
        );

        let mut label = egui::RichText::new(&row.label);
        if row.bypassed {
            // Bypass is engine state a viewer must show: a bypassed node
            // is often the whole explanation for a missing object.
            label = label.strikethrough().color(theme.muted);
        }
        if is_match {
            label = label.color(theme.accent);
        }
        let response = ui
            .selectable_label(selected, label)
            .on_hover_text(if row.opens.is_some() {
                "Click to select, double-click to open"
            } else {
                "Click to select, double-click to reveal"
            });
        if response.clicked() {
            intents.panel(PanelIntent::Tree(TreeAction::Select(row.ctx, row.node)));
        }
        if response.double_clicked() {
            if row.opens.is_some() {
                *ctx = GraphContext::Subflow(row.node);
            } else {
                // Reveal: the context first, so the canvas mounts the right
                // graph before the selection paints, then the selection.
                *ctx = row.ctx;
                intents.panel(PanelIntent::Tree(TreeAction::Select(row.ctx, row.node)));
            }
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
            state.collapsed.insert(row.key.clone());
        } else {
            state.collapsed.remove(&row.key);
        }
    }

    if row.opens.is_some() && expanded {
        for child in &row.children {
            draw_row(
                ui,
                child,
                registry,
                depth + 1,
                selection,
                search,
                state,
                ctx,
                intents,
                theme,
            );
        }
    }
}

/// The category fill the canvas gives a container, for the tree's chip.
fn container_tint(
    desc: Option<&solarxy_graph::registry::NodeTypeDescriptor>,
    theme: Theme,
) -> egui::Color32 {
    let palette = solarxy_core::theme::Palette::for_dark(theme.dark);
    desc.map_or(theme.widget_bg, |d| {
        let rgb = solarxy_studio::types::node_fill(d.category, d.opens, &palette);
        egui::Color32::from_rgb(rgb.r, rgb.g, rgb.b)
    })
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
    /// what its selection dispatches against, and what a reveal switches
    /// to before selecting.
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
        let (engine, geo, _leaf) = scene();
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

    /// A search shows a match and the ancestors that lead to it and
    /// nothing else, force-expands those ancestors, and leaves a fold the
    /// user set on an unrelated branch alone so clearing the query
    /// restores it.
    #[test]
    fn a_search_narrows_to_matches_and_their_ancestors_without_touching_folds() {
        let (mut engine, geo, leaf) = scene();
        let other = added(&mut engine, GraphContext::Root, "sopnet");
        let _sphere = added(&mut engine, GraphContext::Subflow(other), "sphere");
        let rows = solarxy_studio::tree::scene_tree(engine.document(), engine.registry());
        let key = |node: NodeId| -> String {
            fn find(rows: &[TreeRow], node: NodeId) -> Option<&TreeRow> {
                rows.iter().find_map(|r| {
                    (r.node == node)
                        .then_some(r)
                        .or_else(|| find(&r.children, node))
                })
            }
            find(&rows, node).expect("row exists").key.clone()
        };

        let search = search_tree(&rows, "box");
        let s = Some(&search);
        assert!(row_visible(s, &key(leaf)), "the match itself");
        assert!(row_visible(s, &key(geo)), "its ancestor");
        assert!(
            !row_visible(s, &key(other)),
            "an unrelated branch is hidden"
        );
        assert!(row_visible(None, &key(other)), "and back with no query");

        // The user folded both containers; the search forces only the
        // ancestor of the match open.
        let collapsed: HashSet<String> = [key(geo), key(other)].into_iter().collect();
        assert!(
            row_expanded(s, &collapsed, &key(geo)),
            "forced open by the search"
        );
        assert!(
            !row_expanded(s, &collapsed, &key(other)),
            "an unrelated fold stands"
        );
        assert!(
            !row_expanded(None, &collapsed, &key(geo)),
            "cleared, the fold is back"
        );
        assert!(row_expanded(None, &HashSet::new(), &key(geo)));

        // Collapse all is every branch, and only branches.
        let all: HashSet<String> = branch_keys(&rows).into_iter().collect();
        assert_eq!(
            all,
            [key(geo), key(other)].into_iter().collect::<HashSet<_>>()
        );
    }
}
