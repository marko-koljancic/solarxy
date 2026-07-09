//! The `.slxy` mapping: converting a live document (plus the host's
//! view/editor/meta sidecar and the staged asset bytes) to and from the
//! `solarxy-scenefile` schema, and the [`Engine`] save/load entry points.
//!
//! The format crate owns the on-disk schema; this module owns the
//! translation to the engine's in-memory model. On write, typed params
//! flatten to plain JSON literals via [`param_source_to_json`]; on read,
//! the raw JSON goes through [`crate::migration::load_node`], which types
//! each param under the registry's `ParamSpec` (and migrates or
//! placeholder-marks as needed). Ids are decimal strings of the engine's
//! `u64` ids; `next_id` is recomputed as `max(all ids) + 1` on load.

use std::collections::BTreeMap;

use serde_json::{Map, Value as Json};
use solarxy_scenefile as sf;

use super::{CookMode, DocumentFile, Engine, EventBatch};
use crate::document::{DocumentData, Edge, EdgeId, GraphData, NodeData, NodeId};
use crate::migration::load_node;
use crate::params::ParamValue;
use crate::registry::Registry;
use crate::registry::resolve::param_source_to_json;

/// The non-engine parts of a scene the host supplies on save and receives
/// on load: the generator string, the viewport/camera state, the lighting
/// environment, per-context canvas pan/zoom, and document metadata.
#[derive(Debug, Clone, Default)]
pub struct SceneSidecar {
    pub generator: String,
    pub view: sf::ViewJson,
    pub environment: sf::EnvironmentJson,
    pub canvas_viewports: BTreeMap<String, sf::CanvasViewportJson>,
    pub meta: sf::MetaJson,
}

fn cook_mode_str(mode: CookMode) -> &'static str {
    match mode {
        CookMode::Auto => "auto",
        CookMode::Manual => "manual",
    }
}

fn cook_mode_from(s: &str) -> CookMode {
    if s.eq_ignore_ascii_case("manual") {
        CookMode::Manual
    } else {
        CookMode::Auto
    }
}

fn parse_id(s: &str) -> Option<u64> {
    s.parse().ok()
}

// ---- write direction: DocumentData -> SceneJson ----

fn node_to_json(n: &NodeData) -> sf::NodeJson {
    // The display `name` param is surfaced top-level (section 6.6) and not
    // duplicated in the params map; everything else flattens to a literal.
    let mut params = sf::JsonObject::new();
    let mut name = String::new();
    for (key, src) in &n.params {
        if key == "name" {
            if let Some(ParamValue::Text(text)) = src.literal() {
                name.clone_from(text);
            }
            continue;
        }
        params.insert(key.clone(), param_source_to_json(src));
    }
    let port_order = n
        .port_order
        .iter()
        .map(|(port, edges)| {
            (
                port.clone(),
                edges.iter().map(|e| e.0.to_string()).collect(),
            )
        })
        .collect();
    sf::NodeJson {
        id: n.id.0.to_string(),
        type_id: n.type_id.clone(),
        type_version: n.type_version,
        name,
        bypass: n.bypassed,
        params,
        port_order,
        position: n.position,
    }
}

fn edge_to_json(e: &Edge) -> sf::EdgeJson {
    sf::EdgeJson {
        id: e.id.0.to_string(),
        from: (e.from.0.to_string(), e.from_port.clone()),
        to: (e.to.0.to_string(), e.to_port.clone()),
    }
}

/// Maps a whole document (already `to_data`'d) plus the host sidecar and the
/// asset records into a [`sf::SceneJson`].
#[must_use]
pub fn document_to_scene(
    data: &DocumentData,
    cook_mode: CookMode,
    sidecar: &SceneSidecar,
    asset_records: Vec<sf::AssetRecordJson>,
) -> sf::SceneJson {
    let graph = sf::GraphJson {
        nodes: data.root.nodes.iter().map(node_to_json).collect(),
        edges: data.root.edges.iter().map(edge_to_json).collect(),
        subflows: data
            .subflows
            .iter()
            .map(|(owner, g)| {
                (
                    owner.0.to_string(),
                    sf::SubGraphJson {
                        nodes: g.nodes.iter().map(node_to_json).collect(),
                        edges: g.edges.iter().map(edge_to_json).collect(),
                        active_output: g.active_output.map(|n| n.0.to_string()),
                    },
                )
            })
            .collect(),
    };
    let review = sf::ReviewJson {
        annotations: data
            .annotations
            .iter()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect(),
    };
    sf::SceneJson {
        schema_version: sf::SCHEMA_VERSION_CURRENT,
        min_reader: sf::MIN_READER_CURRENT,
        generator: sidecar.generator.clone(),
        units: "meters".to_string(),
        graph,
        view: sidecar.view.clone(),
        environment: sidecar.environment.clone(),
        review,
        assets: asset_records,
        editor: sf::EditorJson {
            cook_mode: cook_mode_str(cook_mode).to_string(),
            canvas_viewports: sidecar.canvas_viewports.clone(),
        },
        meta: sidecar.meta.clone(),
    }
}

// ---- read direction: SceneJson -> DocumentData ----

fn graph_from_json(
    nodes: &[sf::NodeJson],
    edges: &[sf::EdgeJson],
    active_output: Option<&str>,
    registry: &Registry,
    warnings: &mut Vec<String>,
    max_id: &mut u64,
) -> GraphData {
    let mut out_nodes = Vec::with_capacity(nodes.len());
    for nj in nodes {
        let Some(id) = parse_id(&nj.id) else {
            warnings.push(format!("skipping node with non-numeric id '{}'", nj.id));
            continue;
        };
        *max_id = (*max_id).max(id);

        // Rebuild the raw param map and re-inject the top-level display name
        // as the `name` param so it types like any other.
        let mut raw: Map<String, Json> = nj.params.clone().into_iter().collect();
        if !nj.name.is_empty() {
            raw.insert("name".to_string(), Json::String(nj.name.clone()));
        }

        let loaded = load_node(
            registry,
            NodeId(id),
            &nj.type_id,
            nj.type_version,
            raw,
            nj.position,
            nj.bypass,
        );
        warnings.extend(loaded.warnings);
        let mut node = loaded.node;

        // Restore the explicit variadic port order (edge id strings).
        node.port_order = nj
            .port_order
            .iter()
            .filter_map(|(port, ids)| {
                let parsed: Vec<EdgeId> =
                    ids.iter().filter_map(|s| parse_id(s).map(EdgeId)).collect();
                (!parsed.is_empty()).then(|| (port.clone(), parsed))
            })
            .collect();
        out_nodes.push(node);
    }

    let mut out_edges = Vec::with_capacity(edges.len());
    for ej in edges {
        let (Some(eid), Some(from), Some(to)) =
            (parse_id(&ej.id), parse_id(&ej.from.0), parse_id(&ej.to.0))
        else {
            warnings.push(format!("skipping edge with a non-numeric id '{}'", ej.id));
            continue;
        };
        *max_id = (*max_id).max(eid).max(from).max(to);
        out_edges.push(Edge {
            id: EdgeId(eid),
            from: NodeId(from),
            from_port: ej.from.1.clone(),
            to: NodeId(to),
            to_port: ej.to.1.clone(),
        });
    }

    GraphData {
        nodes: out_nodes,
        edges: out_edges,
        active_output: active_output.and_then(parse_id).map(NodeId),
        selection: Vec::new(),
    }
}

/// Maps a [`sf::SceneJson`] into a [`DocumentData`], typing every param
/// under the registry and recomputing `next_id`. Returns the data and any
/// non-fatal load warnings.
#[must_use]
pub fn scene_to_document(
    scene: &sf::SceneJson,
    registry: &Registry,
) -> (DocumentData, Vec<String>) {
    let mut warnings = Vec::new();
    let mut max_id = 0u64;

    let root = graph_from_json(
        &scene.graph.nodes,
        &scene.graph.edges,
        None,
        registry,
        &mut warnings,
        &mut max_id,
    );

    let mut subflows = Vec::with_capacity(scene.graph.subflows.len());
    for (owner_s, sg) in &scene.graph.subflows {
        let Some(owner) = parse_id(owner_s) else {
            warnings.push(format!(
                "skipping subflow with non-numeric owner '{owner_s}'"
            ));
            continue;
        };
        max_id = max_id.max(owner);
        let g = graph_from_json(
            &sg.nodes,
            &sg.edges,
            sg.active_output.as_deref(),
            registry,
            &mut warnings,
            &mut max_id,
        );
        subflows.push((NodeId(owner), g));
    }

    let mut annotations = Vec::new();
    for value in &scene.review.annotations {
        match serde_json::from_value::<crate::review::Annotation>(value.clone()) {
            Ok(a) => {
                max_id = max_id.max(a.id.0);
                annotations.push(a);
            }
            Err(e) => warnings.push(format!("dropping an unreadable annotation: {e}")),
        }
    }

    let data = DocumentData {
        root,
        subflows,
        annotations,
        next_id: max_id + 1,
    };
    (data, warnings)
}

fn sidecar_from_scene(scene: &sf::SceneJson) -> SceneSidecar {
    SceneSidecar {
        generator: scene.generator.clone(),
        view: scene.view.clone(),
        environment: scene.environment.clone(),
        canvas_viewports: scene.editor.canvas_viewports.clone(),
        meta: scene.meta.clone(),
    }
}

/// The result of loading a `.slxy`: the replace batch, the host sidecar to
/// apply (camera, canvas viewports, meta), and non-fatal load warnings.
pub struct LoadedScene {
    pub batch: EventBatch,
    pub sidecar: SceneSidecar,
    pub warnings: Vec<String>,
}

impl Engine {
    /// Serializes the whole document, the staged asset bytes, and the host
    /// sidecar into `.slxy` archive bytes.
    ///
    /// Every staged asset is embedded, not just the ones a live `AssetRef`
    /// points at: model parsers resolve companion files (OBJ's `.mtl` and
    /// textures, glTF's `.bin` and images) by NAME through the resolver, so
    /// an unreferenced staged asset may still be load-bearing. The
    /// param-reachability walk (`Document::referenced_assets`) marks the
    /// primary models (`role: "import"`); everything else records as
    /// `role: "companion"`. True garbage collection needs resolver
    /// read-tracking through the worker (a recorded follow-up).
    pub fn save_slxy(&self, sidecar: &SceneSidecar) -> Result<Vec<u8>, sf::SceneFileError> {
        let referenced = self.doc.referenced_assets();
        let mut records = Vec::with_capacity(self.assets.len());
        let mut blobs = Vec::with_capacity(self.assets.len());
        for (id, entry) in self.assets.entries() {
            let role = if referenced.contains(id) {
                "import"
            } else {
                "companion"
            };
            records.push(sf::AssetRecordJson {
                id: id.0.clone(),
                role: role.to_string(),
                sha256: id.0.clone(),
                original_name: entry.name.clone(),
                import_settings: sf::JsonObject::new(),
            });
            blobs.push(sf::AssetBlob {
                sha256: id.0.clone(),
                name: entry.name.clone(),
                mime: entry.mime.clone(),
                bytes: (*entry.bytes).clone(),
            });
        }

        let scene = document_to_scene(&self.doc.to_data(), self.cook_mode, sidecar, records);
        sf::write(&sf::SceneFile {
            scene,
            assets: blobs,
        })
    }

    /// Replaces the whole document from `.slxy` bytes: stages every embedded
    /// asset, maps and types the graph under the registry, and drives the
    /// same load path as a JSON open (reset cook, dirty all, emit
    /// `DocumentReplaced`). Returns the batch, the sidecar to apply, and
    /// warnings (integrity failures and version rejection are errors).
    pub fn load_slxy(&mut self, bytes: &[u8]) -> Result<LoadedScene, sf::SceneFileError> {
        let read = sf::read(bytes)?;
        let mut warnings = read.warnings;

        for blob in &read.file.assets {
            // Content-addressed staging; the returned id equals blob.sha256.
            self.assets
                .stage(blob.name.clone(), blob.mime.clone(), blob.bytes.clone());
        }

        let (document, map_warnings) = scene_to_document(&read.file.scene, &self.registry);
        warnings.extend(map_warnings);

        let file = DocumentFile {
            format_version: 1,
            document,
            cook_mode: cook_mode_from(&read.file.scene.editor.cook_mode),
        };
        let batch = self.load_document(&file);
        let sidecar = sidecar_from_scene(&read.file.scene);

        Ok(LoadedScene {
            batch,
            sidecar,
            warnings,
        })
    }
}
