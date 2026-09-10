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
}

/// Everything derived from one node's own parameters.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NodePresentation {
    /// The name the node answers to, which is what expressions address it
    /// by.
    label: String,
    /// The muted line under the label, or nothing when the node has no
    /// parameter worth summarising.
    info_line: Option<String>,
    /// Whether the type declares the root visibility parameter at all.
    declares_visibility: bool,
    /// Whether it is currently visible.
    visible: bool,
    /// The parameter keys currently passing their conditions, in
    /// declaration order.
    visible_params: Vec<String>,
    /// The tabs the parameter panel should show, emptied groups already
    /// dropped.
    tabs: Vec<String>,
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
    pub fn node_presentation(&self, ctx: JsValue, node: f64) -> Result<JsValue, JsError> {
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
        let tabs = params::param_tabs(&desc.params, false, |spec| {
            visible_params.contains(&spec.key)
        });

        to_js(&NodePresentation {
            label: solarxy_graph::naming::node_name(data, registry),
            info_line: node::node_info_line(desc, &data.params, Some(&lookup)),
            declares_visibility: node::declares_visibility(desc),
            visible: node::is_visible(&data.params),
            visible_params,
            tabs,
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
        to_js(&SceneOutline { rows, search })
    }
}
