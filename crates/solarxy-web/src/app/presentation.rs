//! The shared interface derivation, re-exported across the boundary.
//!
//! `solarxy-studio` holds the rules and deliberately takes no
//! `wasm-bindgen` dependency, so it cannot export anything itself. This
//! slice is how the browser reaches it: the same read-only shape
//! [`queries`](super::queries) already uses, for the same reason, and with
//! the same discipline about how often a thing crosses.
//!
//! **Two frequencies, and the split is the whole design.** A rule that
//! reads only a node *type* is answered once at boot, in
//! [`SolarxyApp::presentation_tables`], because the registry does not
//! change while a document is open. A rule that reads a node's own
//! parameters is answered per node, and the frontend memoizes on the node
//! it mirrors so the call runs when the node changes rather than when
//! React renders. Before this existed the browser recomputed all of it on
//! every render of every node, so the memoized crossing is fewer calls
//! than the same-heap version it replaces, not more.

use super::*;

use solarxy_graph::registry::coerce::DataType;
use solarxy_studio::attributes;
use solarxy_studio::node;
use solarxy_studio::params;
use solarxy_studio::tree;
use solarxy_studio::types;

/// How one port data type presents itself.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DataTypeStyle {
    /// The palette token the wire colour comes from, without the leading
    /// `--`. A reference rather than a value, so neither shell can author
    /// a wire colour of its own.
    token: &'static str,
    shape: types::HandleShape,
}

/// How one node category presents a node that declares nothing more
/// specific.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CategoryStyle {
    /// Position in the palette and menu order.
    order: usize,
    /// The glyph key to fall back on when a node's declared glyph has no
    /// art. Which art exists is the shell's own question.
    glyph: &'static str,
    role: solarxy_graph::registry::NodeRole,
}

/// Everything derived from a node type alone.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PresentationTables {
    data_types: BTreeMap<String, DataTypeStyle>,
    categories: BTreeMap<String, CategoryStyle>,
}

/// One window of an attribute table, already presented.
///
/// The engine yields numbers and this yields text, because the rule that
/// turns one into the other is called per visible cell while a table
/// scrolls, hundreds of times a frame. Crossing per cell is out of the
/// question, so the formatting happens once where the page is assembled.
/// The engine's own page keeps its numbers: a later reader that wants the
/// value rather than the text asks the engine, and nothing here narrows
/// what it can answer.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AttributeText {
    total: u64,
    offset: u32,
    /// One heading per column component: a scalar lane keeps its name, a
    /// vector lane fans out.
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
}

/// The scene outline, and what a query matched in it.
///
/// This shape exists so that the boundary *declares* the two shared types
/// it returns. The mirror check derives what to compare by walking type
/// references out of the boundary modules, so a shared type handed back
/// through an untyped value would cross with nothing holding it to its
/// TypeScript counterpart.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SceneOutline {
    rows: Vec<tree::TreeRow>,
    search: Option<tree::TreeSearch>,
    /// The keys of every row that has children: the collapse-all set.
    /// Carried with the fold rather than asked for separately, because it
    /// is one walk of the same tree.
    branches: Vec<String>,
}

/// The presentation a node needs that its mirror does not already carry.
///
/// A node's name, whether it is visible and whether its type offers the
/// affordance all ride the mirror instead, because they are wanted almost
/// everywhere and a query for each would be a crossing per reader. What is
/// left here is what only a panel asks for.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NodePresentation {
    /// The muted line under the label, or nothing when the node has no
    /// parameter worth summarising.
    info_line: Option<String>,
    /// The parameter keys currently passing their conditions, in
    /// declaration order.
    visible_params: Vec<String>,
    /// The tabs to show, emptied groups already dropped and the validation
    /// tab appended when there is a report.
    tabs: Vec<ParamTab>,
    /// Which tab is showing: the stored one while the node still offers
    /// it, else the first. The fallback is what keeps a sensible tab when
    /// the selection moves to a different node type.
    active_tab: Option<String>,
    /// The active tab's parameters, split into subgroup runs.
    sections: Vec<ParamSection>,
    /// Every parameter key in the active tab, visible or not.
    ///
    /// The whole group is here because that is what a tab reset writes: a
    /// hidden variant row still holds its stored value, and skipping it
    /// would leave it stale to surprise someone later.
    active_tab_keys: Vec<String>,
}

/// One tab, with the label the strip prints.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ParamTab {
    key: String,
    label: String,
    /// The validation report's tab, which is a sentinel rather than a
    /// group any node declares.
    is_validation: bool,
}

/// One run of parameters under an optional subgroup heading.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ParamSection {
    subgroup: Option<String>,
    param_keys: Vec<String>,
}

#[wasm_bindgen]
impl SolarxyApp {
    /// The rules that read only a node type: wire colour tokens, handle
    /// shapes, category order, and the glyph and silhouette fallbacks.
    ///
    /// Read once at boot beside the registry snapshot and cached, because
    /// the registry does not move while a document is open.
    pub fn presentation_tables(&self) -> Result<JsValue, JsError> {
        let mut data_types = BTreeMap::new();
        for dt in DataType::ALL {
            let key = serde_json::to_value(dt)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            data_types.insert(
                key,
                DataTypeStyle {
                    token: types::wire_token(dt),
                    shape: types::handle_shape(dt),
                },
            );
        }

        let mut categories = BTreeMap::new();
        for desc in self.engine.registry().descriptors() {
            let category = desc.category;
            let key = serde_json::to_value(category)
                .ok()
                .and_then(|v| v.as_str().map(str::to_string))
                .unwrap_or_default();
            categories.entry(key).or_insert_with(|| CategoryStyle {
                order: category as usize,
                glyph: types::category_glyph(category),
                role: types::category_role(category),
            });
        }

        to_js(&PresentationTables {
            data_types,
            categories,
        })
    }

    /// The rules that read one node's own parameters, answered for a whole
    /// node in a single call.
    ///
    /// A call per rule would be seven crossings per node per render, and a
    /// call per parameter would be one per row of the parameter panel.
    pub fn node_presentation(
        &self,
        ctx: JsValue,
        node: f64,
        has_report: bool,
        stored_tab: &str,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let id = NodeId(node as u64);
        let registry = self.engine.registry();
        let Ok(graph) = self.engine.document().graph(ctx) else {
            return Ok(JsValue::NULL);
        };
        let Some(data) = graph.node(id) else {
            return Ok(JsValue::NULL);
        };
        let Some(desc) = registry.get(&data.type_id) else {
            return Ok(JsValue::NULL);
        };

        let manifest: BTreeMap<String, String> = self.engine.asset_manifest().into_iter().collect();
        let lookup = |hash: &str| manifest.get(hash).cloned();
        let visible_params =
            solarxy_graph::registry::visibility::visible_param_keys(&desc.params, &data.params);
        let tab_keys = params::param_tabs(&desc.params, has_report, |spec| {
            visible_params.contains(&spec.key)
        });
        let active = params::resolve_active_tab(&tab_keys, stored_tab);
        let active_params: Vec<solarxy_graph::registry::param_spec::ParamSpec> = active
            .map(|tab| {
                desc.params
                    .iter()
                    .filter(|p| p.group == tab && visible_params.contains(&p.key))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let sections = params::param_sections(&active_params)
            .into_iter()
            .map(|section| ParamSection {
                subgroup: section.subgroup.map(str::to_string),
                param_keys: section.params.iter().map(|p| p.key.clone()).collect(),
            })
            .collect();
        let active_tab_keys = active
            .map(|tab| {
                params::group_keys(&desc.params, tab)
                    .into_iter()
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();

        to_js(&NodePresentation {
            info_line: node::node_info_line(desc, &data.params, Some(&lookup)),
            visible_params,
            tabs: tab_keys
                .iter()
                .map(|key| ParamTab {
                    key: key.clone(),
                    label: params::tab_label(key),
                    is_validation: key == params::VALIDATION_TAB,
                })
                .collect(),
            active_tab: active.map(str::to_string),
            sections,
            active_tab_keys,
        })
    }

    /// One window of a node's attribute values, formatted.
    ///
    /// The headings come with the page rather than being derived beside
    /// it, so the column count and the heading count cannot disagree.
    pub fn attribute_text(
        &self,
        node: f64,
        domain: String,
        offset: u32,
        limit: u32,
    ) -> Result<JsValue, JsError> {
        // The same two-word vocabulary the numeric page takes, rather than
        // a serde round trip for one enum with two members.
        let domain = match domain.as_str() {
            "primitive" => solarxy_kernel::AttributeDomain::Primitive,
            _ => solarxy_kernel::AttributeDomain::Point,
        };
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let id = NodeId(node as u64);
        let Some(page) = self.engine.attribute_page(id, domain, offset, limit) else {
            return Ok(JsValue::NULL);
        };
        to_js(&AttributeText {
            total: page.total,
            offset: page.offset,
            headers: attributes::header_cells(&page.columns),
            rows: page
                .rows
                .iter()
                .map(|row| row.iter().map(|v| attributes::cell_text(*v)).collect())
                .collect(),
        })
    }

    /// The whole document as one outline, rooted at the scene network,
    /// with the search folded into the same call.
    ///
    /// One crossing rather than two, because the pane wants both together
    /// and a separate search would refold the same document. Pull-read
    /// when the pane is open and the revision moves: a fold of every
    /// context is the wrong thing to put on the wire once per cook.
    ///
    /// An empty query leaves `search` null and the pane on whatever the
    /// reader had opened by hand.
    pub fn scene_outline(&self, query: &str) -> Result<JsValue, JsError> {
        let rows = tree::scene_tree(self.engine.document(), self.engine.registry());
        let search = if query.trim().is_empty() {
            None
        } else {
            Some(tree::search_tree(&rows, query))
        };
        let branches = tree::branch_keys(&rows);
        to_js(&SceneOutline {
            rows,
            search,
            branches,
        })
    }
}
