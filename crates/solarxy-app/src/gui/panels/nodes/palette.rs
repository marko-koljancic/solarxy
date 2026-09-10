//! The node palette: every type the current context will take, filtered
//! and searched, placed where the pointer is.
//!
//! **A pure interpreter of the registry.** There is no list of node types
//! here and no branch on any type identifier; what a user can add is what
//! the registry declares is legal in the graph they are looking at. That
//! is the whole of the zero-frontend-change contract on this surface: a
//! node type added in Rust appears here, searchable, with its glyph, its
//! category and its documentation, and this file does not move.
//!
//! ## Two orderings that look like mistakes and are not
//!
//! **Rows are in type-identifier order**, because the registry is a map
//! keyed by type id and that is the order it iterates. So "Attribute from
//! Image" really does come before "Attribute Promote". Sorting by display
//! name to look tidy would be a visible divergence from the browser.
//!
//! **The query is not trimmed**, and the three search fields are joined
//! into one string before the test, so a needle may straddle the boundary
//! between a display name and a type id. Both are the browser's
//! behaviour rather than accidents of it.

use egui::{Key, Modifiers, Pos2, Rect, Ui, Vec2, vec2};
use solarxy_graph::document::ContextKind;
use solarxy_graph::registry::{Category, NodeTypeDescriptor, Registry};
use solarxy_studio::palette::{self, MARGIN_PX};

use crate::gui::theme::Theme;

/// The panel's size.
///
/// **Load-bearing, and shared with the browser.** The placement rule
/// clamps the panel inside the pane, so a different size clamps
/// differently near an edge and the two shells would drop a node in
/// different places. The browser's own placement tests use exactly this.
const PANEL: Vec2 = vec2(440.0, 300.0);

/// The category rail's width.
const RAIL: f32 = 128.0;

/// The row that shows everything, which is what a palette opens on.
const ALL: &str = "All";

/// The palette's own state, which is a search and a cursor and nothing
/// about the document.
#[derive(Debug, Default)]
pub(super) struct PaletteState {
    pub open: bool,
    pub query: String,
    /// `None` is the All row.
    pub category: Option<Category>,
    /// **An option rather than an index**, because the browser's cursor
    /// goes to minus one on an empty list and survives only because its
    /// pick is guarded. An unsigned index would panic there.
    pub cursor: Option<usize>,
    /// Where the panel was placed, which is also where an added node
    /// lands.
    pub at: Pos2,
}

impl PaletteState {
    /// Open it, forgetting whatever the last search was.
    ///
    /// The browser resets query, category and cursor on every open, so a
    /// palette is always the same palette rather than remembering a
    /// filter the user has forgotten setting.
    pub(super) fn open(&mut self, at: Pos2) {
        self.open = true;
        self.query.clear();
        self.category = None;
        self.cursor = Some(0);
        self.at = at;
    }

    pub(super) fn close(&mut self) {
        self.open = false;
    }
}

/// What the palette asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PaletteAction {
    /// Add this node type. The position is the caller's to project.
    Add(String),
}

/// The node types this context will take, in registry order.
///
/// Filtered by what the descriptor declares rather than by any list
/// here, which is what makes a node type added in Rust appear with no
/// change to this file.
#[must_use]
pub(super) fn candidates<'a>(
    registry: &'a Registry,
    kind: ContextKind,
    category: Option<Category>,
    query: &str,
) -> Vec<&'a NodeTypeDescriptor> {
    registry
        .descriptors()
        .filter(|desc| desc.contexts.contains(kind))
        .filter(|desc| category.is_none_or(|want| desc.category == want))
        .filter(|desc| matches(desc, query))
        .collect()
}

/// Whether a node type answers to a search.
///
/// The three sources are joined before the test, so a needle may straddle
/// them, and the query is not trimmed. Both are the browser's behaviour:
/// a trailing space is part of what was typed.
#[must_use]
pub(super) fn matches(desc: &NodeTypeDescriptor, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let mut hay = String::with_capacity(64);
    hay.push_str(desc.display_name);
    hay.push(' ');
    hay.push_str(desc.type_id);
    for alias in desc.search_aliases {
        hay.push(' ');
        hay.push_str(alias);
    }
    hay.to_lowercase().contains(&query.to_lowercase())
}

/// The categories this context offers, in the shared order.
#[must_use]
pub(super) fn categories(registry: &Registry, kind: ContextKind) -> Vec<Category> {
    let mut seen: Vec<Category> = Vec::new();
    for desc in registry.descriptors().filter(|d| d.contexts.contains(kind)) {
        if !seen.contains(&desc.category) {
            seen.push(desc.category);
        }
    }
    seen.sort_by(|a, b| solarxy_studio::types::compare_categories(*a, *b));
    seen
}

/// Where the panel opens.
///
/// The shared rule, which is this crate's first caller: the browser runs
/// its own copy and nothing has held the two together, so the desktop
/// calling the Rust one is what makes that duplication visible.
#[must_use]
pub(super) fn placement(pointer: Option<Pos2>, pane: Rect) -> Pos2 {
    let at = palette::palette_placement(
        pointer.map(|p| palette::Point { x: p.x, y: p.y }),
        palette::Rect {
            left: pane.left(),
            top: pane.top(),
            width: pane.width(),
            height: pane.height(),
        },
        palette::Size {
            width: PANEL.x,
            height: PANEL.y,
        },
        MARGIN_PX,
    );
    egui::pos2(at.x, at.y)
}

/// Move the cursor without leaving the list.
///
/// Clamped on the way out rather than on the way in, which is the
/// difference between this and the browser: there, an arrow on an empty
/// list sets the cursor to minus one and it stays there until the list
/// changes.
#[must_use]
pub(super) fn step(cursor: Option<usize>, delta: i32, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    // An absent cursor lands on the first row rather than moving off it:
    // a list that has just gained its first entry should answer an arrow
    // with the top of the list, not the second row.
    let Some(at) = cursor else {
        return Some(0);
    };
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let moved = (at as i32 + delta).clamp(0, len as i32 - 1);
    #[allow(clippy::cast_sign_loss)]
    Some(moved as usize)
}

/// The next category in the rail, wrapping, or `None` for the All row.
#[must_use]
pub(super) fn cycle_category(
    current: Option<Category>,
    offered: &[Category],
    delta: i32,
) -> Option<Category> {
    if offered.is_empty() {
        return None;
    }
    // The All row sits at index zero and the categories after it.
    let len = offered.len() + 1;
    let at = current
        .and_then(|c| offered.iter().position(|o| *o == c).map(|i| i + 1))
        .unwrap_or(0);
    #[allow(clippy::cast_possible_wrap, clippy::cast_possible_truncation)]
    let moved = (at as i32 + delta).rem_euclid(len as i32);
    #[allow(clippy::cast_sign_loss)]
    let moved = moved as usize;
    if moved == 0 {
        None
    } else {
        offered.get(moved - 1).copied()
    }
}

/// Draw the palette, and answer what it asked for.
#[allow(clippy::too_many_lines)]
pub(super) fn draw(
    ui: &Ui,
    state: &mut PaletteState,
    registry: &Registry,
    kind: ContextKind,
    theme: Theme,
) -> Option<PaletteAction> {
    if !state.open {
        return None;
    }
    let offered = categories(registry, kind);
    let visible = candidates(registry, kind, state.category, &state.query);
    state.cursor = state.cursor.filter(|c| *c < visible.len());

    let mut action = None;
    let mut close = false;

    egui::Area::new(ui.id().with("node-palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(state.at)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme.bg_elevated)
                .show(ui, |ui| {
                    ui.set_width(PANEL.x);
                    ui.set_height(PANEL.y);

                    let field = ui.add(
                        egui::TextEdit::singleline(&mut state.query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Search nodes..."),
                    );
                    field.request_focus();

                    // The caret keys belong to the field while there is
                    // something to move a caret through. Cycling the rail
                    // with them only on an empty query is what stops the
                    // palette stealing them.
                    let empty_query = state.query.is_empty();
                    ui.input_mut(|i| {
                        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                            state.cursor = step(state.cursor, 1, visible.len());
                        }
                        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                            state.cursor = step(state.cursor, -1, visible.len());
                        }
                        if empty_query && i.consume_key(Modifiers::NONE, Key::ArrowRight) {
                            state.category = cycle_category(state.category, &offered, 1);
                            state.cursor = Some(0);
                        }
                        if empty_query && i.consume_key(Modifiers::NONE, Key::ArrowLeft) {
                            state.category = cycle_category(state.category, &offered, -1);
                            state.cursor = Some(0);
                        }
                    });

                    let confirmed = ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter));
                    if confirmed && let Some(desc) = state.cursor.and_then(|c| visible.get(c)) {
                        action = Some(PaletteAction::Add(desc.type_id.to_string()));
                        close = true;
                    }

                    ui.separator();
                    ui.horizontal_top(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(RAIL);
                            egui::ScrollArea::vertical()
                                .id_salt("palette-rail")
                                .show(ui, |ui| {
                                    if ui.selectable_label(state.category.is_none(), ALL).clicked()
                                    {
                                        state.category = None;
                                        state.cursor = Some(0);
                                    }
                                    for category in &offered {
                                        // The rail prints the label and
                                        // filters on the enum: four of
                                        // the fifteen have labels that
                                        // are not their identifiers.
                                        if ui
                                            .selectable_label(
                                                state.category == Some(*category),
                                                category.display_name(),
                                            )
                                            .clicked()
                                        {
                                            state.category = Some(*category);
                                            state.cursor = Some(0);
                                        }
                                    }
                                });
                        });
                        ui.separator();
                        egui::ScrollArea::vertical()
                            .id_salt("palette-rows")
                            .show(ui, |ui| {
                                if visible.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No nodes for this context.")
                                            .color(theme.muted),
                                    );
                                }
                                for (index, desc) in visible.iter().enumerate() {
                                    let row = ui
                                        .selectable_label(
                                            state.cursor == Some(index),
                                            format!(
                                                "{}    {}",
                                                desc.display_name,
                                                desc.category.display_name()
                                            ),
                                        )
                                        .on_hover_text(desc.doc);
                                    if row.hovered() {
                                        state.cursor = Some(index);
                                    }
                                    if row.clicked() {
                                        action = Some(PaletteAction::Add(desc.type_id.to_string()));
                                        close = true;
                                    }
                                }
                            });
                    });
                });
        });

    if ui
        .ctx()
        .input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
    {
        close = true;
    }
    if close {
        state.close();
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> Registry {
        solarxy_graph::nodes::builtin_registry().expect("builtin registry")
    }

    /// The palette lists what the registry says is legal here, not a
    /// list kept in this file. Two contexts, two different sets, and
    /// neither is empty.
    #[test]
    fn the_offer_is_the_registry_filtered_by_context() {
        let registry = registry();
        let sop = candidates(&registry, ContextKind::Sop, None, "");
        let cop = candidates(&registry, ContextKind::Cop, None, "");
        assert!(sop.len() > 20 && cop.len() > 5);

        for desc in &sop {
            assert!(
                desc.contexts.contains(ContextKind::Sop),
                "{} is offered in a network it cannot go in",
                desc.type_id
            );
        }
        let sop_ids: Vec<&str> = sop.iter().map(|d| d.type_id).collect();
        let cop_ids: Vec<&str> = cop.iter().map(|d| d.type_id).collect();
        assert_ne!(sop_ids, cop_ids, "two contexts must offer different sets");
    }

    /// Rows come out in type-identifier order, because that is the order
    /// the registry iterates. Sorting by display name to look tidy would
    /// be a visible divergence from the browser.
    #[test]
    fn rows_are_in_type_identifier_order() {
        let registry = registry();
        let ids: Vec<&str> = candidates(&registry, ContextKind::Sop, None, "")
            .iter()
            .map(|d| d.type_id)
            .collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        assert_eq!(ids, sorted);
    }

    /// Search covers all three sources, and a needle may straddle them
    /// because they are joined before the test.
    #[test]
    fn search_covers_the_name_the_identifier_and_the_aliases() {
        let registry = registry();
        let find = |q: &str| -> Vec<&str> {
            candidates(&registry, ContextKind::Sop, None, q)
                .iter()
                .map(|d| d.type_id)
                .collect()
        };
        assert!(find("box").contains(&"box"), "the display name matches");
        assert!(
            find("compute_normals").contains(&"compute_normals"),
            "the type identifier matches"
        );

        let aliased = registry
            .descriptors()
            .find(|d| d.contexts.contains(ContextKind::Sop) && !d.search_aliases.is_empty())
            .expect("some node type declares a search alias");
        let alias = aliased.search_aliases[0];
        assert!(
            find(alias).contains(&aliased.type_id),
            "the alias {alias} must find {}",
            aliased.type_id
        );
    }

    /// The three sources are joined before the test, so a needle may
    /// straddle them, and the query is not trimmed.
    ///
    /// **A trailing space does not narrow the search**, which is the
    /// thing that looks like a bug and is not: the haystack has joining
    /// spaces in it, so "box " matches "box box" at the boundary between
    /// the display name and the type identifier. The first version of
    /// this test asserted the opposite and was wrong about what an
    /// untrimmed query does.
    #[test]
    fn the_search_joins_its_sources_and_does_not_trim() {
        let registry = registry();
        let count = |q: &str| candidates(&registry, ContextKind::Sop, None, q).len();
        assert_eq!(count("box"), count("BOX"), "the match is case-insensitive");

        let straddling = candidates(&registry, ContextKind::Sop, None, "normals compute");
        assert!(
            straddling.iter().any(|d| d.type_id == "compute_normals"),
            "a needle spanning the display name and the identifier must match"
        );

        // And a needle that is only a boundary still matches, because the
        // boundary is a real character in the haystack.
        assert!(count(" ") > 0, "a lone space matches every joined haystack");
    }

    /// The categories offered are the ones this context actually has,
    /// in the shared order rather than in any order this file picks.
    #[test]
    fn categories_are_the_contexts_own_in_the_shared_order() {
        let registry = registry();
        let offered = categories(&registry, ContextKind::Sop);
        assert!(offered.len() > 3);

        let mut sorted = offered.clone();
        sorted.sort_by(|a, b| solarxy_studio::types::compare_categories(*a, *b));
        assert_eq!(offered, sorted);

        for category in &offered {
            assert!(
                registry
                    .descriptors()
                    .any(|d| d.contexts.contains(ContextKind::Sop) && d.category == *category),
                "{category:?} is offered in a context that has none of it"
            );
        }
    }

    /// Filtering happens on the enum, not on the label. Four of the
    /// fifteen categories have labels that are not their identifiers, so
    /// a filter written against the label would silently offer nothing
    /// for those four.
    #[test]
    fn the_category_filter_is_the_enum_rather_than_the_label() {
        let registry = registry();
        let renamed: Vec<Category> = [
            Category::Copy,
            Category::CopGenerate,
            Category::CopAdjust,
            Category::CopComposite,
        ]
        .into_iter()
        .collect();
        for category in renamed {
            let kind = if matches!(category, Category::Copy) {
                ContextKind::Sop
            } else {
                ContextKind::Cop
            };
            let filtered = candidates(&registry, kind, Some(category), "");
            assert!(
                !filtered.is_empty(),
                "{category:?} offers nothing, which is what a label filter would do"
            );
            for desc in filtered {
                assert_eq!(desc.category, category);
            }
        }
    }

    /// The cursor never leaves the list, and an empty list has no cursor
    /// rather than a negative one.
    #[test]
    fn the_cursor_stays_inside_the_list() {
        assert_eq!(step(None, 1, 0), None, "an empty list has no cursor");
        assert_eq!(step(Some(0), -1, 3), Some(0), "it does not go below zero");
        assert_eq!(step(Some(2), 1, 3), Some(2), "nor past the end");
        assert_eq!(step(Some(0), 1, 3), Some(1));
        assert_eq!(
            step(None, 1, 3),
            Some(0),
            "an absent cursor starts at the top"
        );
    }

    /// The rail cycles through All and every category, and closes.
    #[test]
    fn the_category_cycle_visits_all_and_closes() {
        let offered = vec![Category::Generators, Category::Transform, Category::Utility];
        let mut at = None;
        let mut seen = vec![at];
        for _ in 0..offered.len() {
            at = cycle_category(at, &offered, 1);
            assert!(!seen.contains(&at), "the cycle repeats before it closes");
            seen.push(at);
        }
        assert_eq!(
            cycle_category(at, &offered, 1),
            None,
            "and it comes back to All"
        );
        assert_eq!(
            cycle_category(None, &offered, -1),
            Some(Category::Utility),
            "backwards from All is the last category"
        );
    }

    /// Placement is the shared rule, and this is its first caller. The
    /// vector is the browser's own test vector, so the two shells put
    /// the panel in the same place.
    #[test]
    fn placement_is_the_shared_rule_and_agrees_with_the_browsers_vector() {
        // A pane smaller than the panel pins to the near edge, which is
        // the case the inverted-range clamp exists for.
        let pane = Rect::from_min_size(egui::pos2(10.0, 20.0), vec2(100.0, 80.0));
        let at = placement(Some(egui::pos2(60.0, 60.0)), pane);
        assert!((at.x - 18.0).abs() < 0.01 && (at.y - 28.0).abs() < 0.01);

        // With no pointer the panel is centred horizontally and a sixth
        // of the way down.
        let roomy = Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(1200.0, 900.0));
        let centred = placement(None, roomy);
        assert!((centred.x - (1200.0 - PANEL.x) / 2.0).abs() < 0.01);
        assert!((centred.y - 900.0 / 6.0).abs() < 0.01);
    }
}
