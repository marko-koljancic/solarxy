//! The attributes panel: a paged, read-only table of the watched
//! geometry's attributes, with a tab per domain.
//!
//! **Paging is the design, not an optimisation.** A geometry can hold
//! millions of points and the engine's query is paged for that reason;
//! the panel asks for the pages a scroll position needs and keeps a small
//! cache of them, so paging never recooks and a heavy graph stays usable.
//! The window arithmetic is the browser's `pageWindow`, and a drift test
//! reads its constants.
//!
//! **The cells are formatted by the shared rules.** `cell_text` and
//! `header_cells` come from `solarxy-studio`, so a value reads the same on
//! both shells; a second implementation would diverge silently, and the
//! values would be right while the display differed, which reads as a
//! data problem rather than a presentation one.
//!
//! **What is watched is the shared rule too**: the first selected node in
//! the graph the user is looking at, else its display node, else nothing.
//! Changing it, or the domain, or the context, or the geometry the node
//! last cooked, empties the cache; the page cache is keyed by page index
//! alone, so a domain switch must clear it or the other domain's pages
//! would serve.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use solarxy_graph::Engine;
use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::attr_table::{AttributeDomain, AttributePage, AttributeSummary};
use solarxy_studio::attributes::{cell_text, header_cells, watched_node};

use crate::gui::theme::Theme;

/// A row's height in points.
pub(super) const ROW_H: f32 = 22.0;
/// Rows per page asked of the engine.
pub(super) const PAGE_SIZE: usize = 128;
/// Pages kept; the oldest goes first.
pub(super) const PAGE_CACHE_CAP: usize = 16;
/// Rows materialised above and below the viewport.
pub(super) const OVERSCAN: usize = 8;

/// What the panel draws: the engine to ask and the graph the user is in.
#[derive(Clone, Copy)]
pub(crate) enum AttributesSource<'a> {
    Empty,
    Scene {
        engine: &'a Engine,
        ctx: GraphContext,
    },
}

/// The row window a scroll position needs, padded by `overscan` rows, and
/// the page indices covering it. The browser's `pageWindow`, line for line:
/// `first` is clamped into the data, because a stale scroll offset can
/// outlive a shrinking total.
pub(super) fn page_window(
    scroll_top: f32,
    viewport_height: f32,
    row_height: f32,
    total: usize,
    page_size: usize,
    overscan: usize,
) -> (usize, usize, Vec<usize>) {
    if total == 0 {
        return (0, 0, Vec::new());
    }
    let first_row = (scroll_top / row_height).floor().max(0.0) as usize;
    let first = first_row.saturating_sub(overscan).min(total);
    let last_row = ((scroll_top + viewport_height) / row_height)
        .ceil()
        .max(0.0) as usize;
    let last = (last_row + overscan).min(total);
    let mut pages = Vec::new();
    let mut p = first / page_size;
    while p * page_size < last {
        pages.push(p);
        p += 1;
    }
    (first, last, pages)
}

/// What the cache was filled for; any change empties it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CacheKey {
    ctx: GraphContext,
    node: NodeId,
    domain: AttributeDomain,
    /// The cooked geometry's identity, which moves when the node recooks.
    generation: usize,
}

/// The fetched pages, oldest first, capped.
#[derive(Default)]
pub(super) struct PageCache {
    pages: BTreeMap<usize, AttributePage>,
    order: VecDeque<usize>,
}

impl PageCache {
    pub(super) fn get(&self, index: usize) -> Option<&AttributePage> {
        self.pages.get(&index)
    }

    /// Keep `page`, dropping the oldest beyond the cap.
    pub(super) fn insert(&mut self, index: usize, page: AttributePage) {
        if self.pages.insert(index, page).is_none() {
            self.order.push_back(index);
        }
        while self.pages.len() > PAGE_CACHE_CAP {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.pages.remove(&oldest);
        }
    }

    pub(super) fn clear(&mut self) {
        self.pages.clear();
        self.order.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.pages.len()
    }
}

/// The panel's own state: the domain tab, and the cache with its key.
pub(crate) struct AttributesState {
    domain: AttributeDomain,
    key: Option<CacheKey>,
    summary: Option<AttributeSummary>,
    cache: PageCache,
}

impl Default for AttributesState {
    fn default() -> Self {
        Self {
            domain: AttributeDomain::Point,
            key: None,
            summary: None,
            cache: PageCache::default(),
        }
    }
}

impl AttributesState {
    /// Bring the cache in step with what is watched: a new key empties it
    /// and refetches the summary.
    fn retarget(&mut self, engine: &Engine, ctx: GraphContext, node: NodeId) {
        let generation = engine
            .geometry_output(node)
            .map_or(0, |set| Arc::as_ptr(set) as usize);
        let key = CacheKey {
            ctx,
            node,
            domain: self.domain,
            generation,
        };
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.cache.clear();
            self.summary = engine.attribute_summary(node);
        }
    }

    /// The page holding `row`, fetched if the cache lacks it.
    fn page_for(&mut self, engine: &Engine, node: NodeId, index: usize) -> Option<&AttributePage> {
        if self.cache.get(index).is_none() {
            let page = engine.attribute_page(
                node,
                self.domain,
                u32::try_from(index * PAGE_SIZE).ok()?,
                u32::try_from(PAGE_SIZE).ok()?,
            )?;
            self.cache.insert(index, page);
        }
        self.cache.get(index)
    }
}

/// The empty state for a domain with nothing in it.
pub(super) fn empty_domain(domain: AttributeDomain) -> &'static str {
    match domain {
        AttributeDomain::Point => "No points.",
        AttributeDomain::Primitive => "No primitive attributes on this geometry.",
    }
}

/// Render the attributes panel into `ui`.
pub(in crate::gui) fn draw_attributes_content(
    ui: &mut egui::Ui,
    source: AttributesSource<'_>,
    state: &mut AttributesState,
    theme: Theme,
) {
    let AttributesSource::Scene { engine, ctx } = source else {
        return placeholder(ui, "No node selected and no display flag set.", theme);
    };
    let Ok(graph) = engine.document().graph(ctx) else {
        return placeholder(ui, "No node selected and no display flag set.", theme);
    };
    let Some(node) = watched_node(&graph.selection, graph.active_output) else {
        return placeholder(ui, "No node selected and no display flag set.", theme);
    };
    let label = graph
        .node(node)
        .map(|n| solarxy_graph::naming::node_name(n, engine.registry()))
        .unwrap_or_default();
    state.retarget(engine, ctx, node);

    // The header: the watched node, the two tabs, and the count.
    let total = state.summary.as_ref().map_or(0, |s| match state.domain {
        AttributeDomain::Point => s.points,
        AttributeDomain::Primitive => s.primitive_elements,
    });
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(&label).color(theme.fg))
            .on_hover_text("The watched node");
        ui.separator();
        for (domain, name) in [
            (AttributeDomain::Point, "Point"),
            (AttributeDomain::Primitive, "Primitive"),
        ] {
            if ui.selectable_label(state.domain == domain, name).clicked() {
                state.domain = domain;
                state.retarget(engine, ctx, node);
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let unit = match state.domain {
                AttributeDomain::Point => "points",
                AttributeDomain::Primitive => "prims",
            };
            ui.label(
                egui::RichText::new(format!(
                    "{} {unit}",
                    solarxy_core::format_number(usize::try_from(total).unwrap_or(usize::MAX))
                ))
                .monospace()
                .color(theme.muted),
            );
        });
    });
    ui.separator();

    if state.summary.is_none() {
        return placeholder(ui, "No cooked geometry on this node yet.", theme);
    }
    let total = usize::try_from(total).unwrap_or(usize::MAX);
    if total == 0 {
        return placeholder(ui, empty_domain(state.domain), theme);
    }

    egui::ScrollArea::both()
        .auto_shrink([false, false])
        .show_rows(ui, ROW_H, total, |ui, range| {
            // The pages this window needs, fetched now; the browser fetches
            // the same set from the same arithmetic.
            let (_, _, pages) = page_window(
                range.start as f32 * ROW_H,
                range.len() as f32 * ROW_H,
                ROW_H,
                total,
                PAGE_SIZE,
                OVERSCAN,
            );
            for index in &pages {
                state.page_for(engine, node, *index);
            }
            let headers: Vec<String> = pages
                .first()
                .and_then(|p| state.cache.get(*p))
                .map(|page| header_cells(&page.columns))
                .unwrap_or_default();
            let font = egui::TextStyle::Monospace.resolve(ui.style());
            draw_header_row(ui, &headers, &font, theme);
            for row in range {
                let page = state.cache.get(row / PAGE_SIZE);
                let cells: Vec<String> = match page {
                    Some(page) => page.rows.get(row - page.offset as usize).map_or_else(
                        || vec!["\u{00b7}".to_string(); headers.len()],
                        |values| values.iter().map(|v| cell_text(*v)).collect(),
                    ),
                    None => vec!["\u{00b7}".to_string(); headers.len()],
                };
                draw_row(ui, row, &cells, &font, theme);
            }
        });
}

const INDEX_WIDTH: f32 = 72.0;
const CELL_WIDTH: f32 = 120.0;

fn draw_header_row(ui: &mut egui::Ui, headers: &[String], font: &egui::FontId, theme: Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        cell(ui, INDEX_WIDTH, "#", font, theme.muted, ROW_H);
        for header in headers {
            cell(ui, CELL_WIDTH, header, font, theme.fg, ROW_H);
        }
    });
}

fn draw_row(ui: &mut egui::Ui, row: usize, cells: &[String], font: &egui::FontId, theme: Theme) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // Zebra by data parity rather than by drawn position, so the
        // stripes do not shift as the window scrolls.
        if row % 2 == 1 {
            let rect = egui::Rect::from_min_size(
                ui.cursor().min,
                egui::vec2(INDEX_WIDTH + CELL_WIDTH * cells.len() as f32, ROW_H),
            );
            ui.painter().rect_filled(rect, 0.0, theme.bg_elevated);
        }
        cell(ui, INDEX_WIDTH, &row.to_string(), font, theme.muted, ROW_H);
        for text in cells {
            let color = if text == "\u{00b7}" {
                theme.muted
            } else {
                theme.fg
            };
            cell(ui, CELL_WIDTH, text, font, color, ROW_H);
        }
    });
}

fn cell(
    ui: &mut egui::Ui,
    width: f32,
    text: &str,
    font: &egui::FontId,
    color: egui::Color32,
    height: f32,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    ui.painter().text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        text,
        font.clone(),
        color,
    );
}

fn placeholder(ui: &mut egui::Ui, text: &str, theme: Theme) {
    ui.add_space(20.0);
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(text).color(theme.muted));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(offset: u32) -> AttributePage {
        AttributePage {
            total: 1000,
            offset,
            columns: Vec::new(),
            rows: Vec::new(),
        }
    }

    /// The window is the browser's: nothing for no rows, the overscan on
    /// both sides, pages covering the window, and a stale scroll clamped
    /// into a shrunken total.
    #[test]
    fn the_window_is_the_browsers_arithmetic() {
        assert_eq!(page_window(0.0, 300.0, 22.0, 0, 128, 8), (0, 0, vec![]));
        // Scroll at the top, a 300px viewport: rows 0 to 14 plus 8 overscan.
        assert_eq!(
            page_window(0.0, 300.0, 22.0, 1000, 128, 8),
            (0, 22, vec![0])
        );
        // Deep in: 2000px is row 90, less 8; the window spans page 0 and 1.
        assert_eq!(
            page_window(2000.0, 300.0, 22.0, 1000, 128, 8),
            (82, 113, vec![0])
        );
        assert_eq!(
            page_window(2600.0, 300.0, 22.0, 1000, 128, 8),
            (110, 140, vec![0, 1])
        );
        // A stale scroll past a total that shrank clamps into the data; the
        // page loop still names page zero, because it runs while its start
        // is short of `last`, as the browser's does.
        assert_eq!(
            page_window(50_000.0, 300.0, 22.0, 100, 128, 8),
            (100, 100, vec![0])
        );
    }

    /// The constants are the browser's, read from its source.
    #[test]
    fn the_constants_are_the_browsers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let pane = std::fs::read_to_string(root.join("web/src/components/AttributesPane.tsx"))
            .expect("the browser's attributes pane");
        let table = std::fs::read_to_string(root.join("web/src/components/attributesTable.ts"))
            .expect("the browser's table helpers");
        assert!(pane.contains(&format!("const ROW_H = {};", ROW_H as u32)));
        assert!(pane.contains(&format!("const PAGE_SIZE = {PAGE_SIZE};")));
        assert!(pane.contains(&format!("const PAGE_CACHE_CAP = {PAGE_CACHE_CAP};")));
        assert!(table.contains(&format!("overscan = {OVERSCAN},")));
    }

    /// The cache keeps at most the cap, dropping the oldest first, and a
    /// clear empties it.
    #[test]
    fn the_cache_drops_the_oldest_beyond_the_cap() {
        let mut cache = PageCache::default();
        for i in 0..PAGE_CACHE_CAP + 3 {
            cache.insert(i, page(u32::try_from(i * PAGE_SIZE).expect("small")));
        }
        assert_eq!(cache.len(), PAGE_CACHE_CAP);
        assert!(cache.get(0).is_none() && cache.get(1).is_none() && cache.get(2).is_none());
        assert!(cache.get(3).is_some() && cache.get(PAGE_CACHE_CAP + 2).is_some());
        cache.insert(3, page(3 * 128));
        assert_eq!(
            cache.len(),
            PAGE_CACHE_CAP,
            "re-inserting a page is not a second entry"
        );
        cache.clear();
        assert_eq!(cache.len(), 0);
    }

    /// The two empty states are the browser's.
    #[test]
    fn the_empty_states_are_the_browsers() {
        assert_eq!(empty_domain(AttributeDomain::Point), "No points.");
        assert_eq!(
            empty_domain(AttributeDomain::Primitive),
            "No primitive attributes on this geometry."
        );
    }
}
