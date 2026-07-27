//! The `scene.json` schema: the Rust-owned, serde + schemars image of a
//! Solarxy document, following the plan's section 6.6 shape. These types
//! are format, not engine: they carry plain-JSON param literals and string
//! ids, deliberately decoupled from `solarxy-graph`'s in-memory model so
//! the file format has a single owner here. The `solarxy-graph` mapping
//! layer converts a live document to and from these types.
//!
//! Naming is `snake_case` on disk (the Rust field names serialize
//! verbatim), distinct from the `camelCase` engine-to-JS boundary.
//! Forward-looking
//! sections whose features landed later (`view` panes beyond one,
//! `environment`, `review` UI) are present in the schema with defaults so
//! the format is shape-stable from day one.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A JSON object node, used for param maps and the still-opaque display /
/// background / import-settings blobs. `BTreeMap` (not `serde_json::Map`)
/// gives deterministic key order and a `schemars` impl.
pub type JsonObject = BTreeMap<String, serde_json::Value>;

/// serde `skip_serializing_if` predicate: a `false` bool is omitted (the
/// `bypass` field is present only when set).
#[allow(clippy::trivially_copy_pass_by_ref)]
fn is_false(b: &bool) -> bool {
    !*b
}

fn default_units() -> String {
    "meters".to_string()
}

fn default_cook_mode() -> String {
    "auto".to_string()
}

fn default_layout() -> String {
    "single".to_string()
}

fn default_inspection() -> String {
    "shaded".to_string()
}

fn default_asset_role() -> String {
    "import".to_string()
}

/// The whole `scene.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct SceneJson {
    /// The document schema version (0 pre-beta, no guarantees; frozen at 1
    /// at public beta).
    pub schema_version: u32,
    /// The lowest reader version able to open this file; a reader below it
    /// hard-rejects with an upgrade message.
    pub min_reader: u32,
    /// The tool + version that wrote the file (diagnostic only).
    pub generator: String,
    #[serde(default = "default_units")]
    pub units: String,
    pub graph: GraphJson,
    #[serde(default)]
    pub view: ViewJson,
    #[serde(default)]
    pub environment: EnvironmentJson,
    #[serde(default)]
    pub review: ReviewJson,
    /// Semantic asset records (role + settings); the byte-level records
    /// live in `manifest.json`, keyed by the same content hash.
    #[serde(default)]
    pub assets: Vec<AssetRecordJson>,
    #[serde(default)]
    pub editor: EditorJson,
    /// The scene clock's persisted half. Defaulted like every other
    /// optional section, so a pre-0.8.1 file loads with a stopped default
    /// clock and `schema_version` stays 1.
    #[serde(default)]
    pub runtime: RuntimeJson,
    #[serde(default)]
    pub meta: MetaJson,
}

/// The node graph: the root canvas plus one entry per subflow, keyed by the
/// owning `geo` node id.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct GraphJson {
    #[serde(default)]
    pub nodes: Vec<NodeJson>,
    #[serde(default)]
    pub edges: Vec<EdgeJson>,
    /// Subflows keyed by the owning `geo` node id (string).
    #[serde(default)]
    pub subflows: BTreeMap<String, SubGraphJson>,
}

/// One subflow's contents plus its display node.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct SubGraphJson {
    #[serde(default)]
    pub nodes: Vec<NodeJson>,
    #[serde(default)]
    pub edges: Vec<EdgeJson>,
    /// The node id whose output this subflow displays, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_output: Option<String>,
    /// The network kind (`"geo"`, `"mat"`, `"tex"`); absent in
    /// pre-context files, whose subflows were all geometry networks. The
    /// engine resolves an absent kind from the owning node's registry
    /// descriptor on load, so this field is advisory redundancy that keeps
    /// the file self-describing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

/// One node instance. Params are plain JSON literals keyed by param id (the
/// schema-v1 shape; the `{"$expr": "..."}` object form is reserved for the
/// future expression variant). The display name is surfaced top-level for
/// readability; the mapping layer keeps it authoritative there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct NodeJson {
    pub id: String,
    #[serde(rename = "type")]
    pub type_id: String,
    pub type_version: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Present (and true) only when the node is bypassed.
    #[serde(default, skip_serializing_if = "is_false")]
    pub bypass: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: JsonObject,
    /// Explicit edge order per variadic input port (edge id strings);
    /// omitted for nodes with no variadic ports.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub port_order: BTreeMap<String, Vec<String>>,
    /// Canvas position `[x, y]`.
    pub position: [f32; 2],
}

/// One edge. `from`/`to` are `[node_id, port_key]` pairs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct EdgeJson {
    pub id: String,
    pub from: (String, String),
    pub to: (String, String),
}

/// The viewport layout and per-pane state: one entry per pane, each with
/// its own camera and display settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ViewJson {
    #[serde(default = "default_layout")]
    pub layout: String,
    #[serde(default)]
    pub active_pane: u32,
    /// Divider position for the two-pane layouts; 0.5 when unset.
    #[serde(default = "default_split_ratio")]
    pub split_ratio: f32,
    #[serde(default)]
    pub panes: Vec<PaneJson>,
}

fn default_split_ratio() -> f32 {
    0.5
}

impl Default for ViewJson {
    fn default() -> Self {
        Self {
            layout: default_layout(),
            active_pane: 0,
            split_ratio: default_split_ratio(),
            panes: vec![PaneJson::default()],
        }
    }
}

/// One viewport pane: its camera, display flags, inspection mode, and
/// background. `display` and `background` are opaque JSON: the reader
/// round-trips them without interpreting their contents.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct PaneJson {
    #[serde(default)]
    pub camera: CameraJson,
    #[serde(default, skip_serializing_if = "JsonObject::is_empty")]
    pub display: JsonObject,
    #[serde(default = "default_inspection")]
    pub inspection: String,
    #[serde(default, skip_serializing_if = "JsonObject::is_empty")]
    pub background: JsonObject,
    /// The `camera` node this pane looks through (its id), or `None` for a free
    /// view. Serde-default so older scenes load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look_through: Option<u64>,
    /// Whether the look-through pane is locked (reframes the camera).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub camera_locked: bool,
}

impl Default for PaneJson {
    fn default() -> Self {
        Self {
            camera: CameraJson::default(),
            display: JsonObject::new(),
            inspection: default_inspection(),
            background: JsonObject::new(),
            look_through: None,
            camera_locked: false,
        }
    }
}

/// The orbit camera state (aspect is viewport-derived and not persisted).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct CameraJson {
    pub target: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    /// `"perspective"` (default) or `"orthographic"`, per pane.
    /// Serde-default, so a file written before per-pane projection existed
    /// loads unchanged.
    #[serde(default = "default_projection")]
    pub projection: String,
    /// Half-height of the orthographic view volume; unused in perspective.
    #[serde(default)]
    pub ortho_scale: f32,
}

fn default_projection() -> String {
    "perspective".to_string()
}

impl Default for CameraJson {
    fn default() -> Self {
        Self {
            target: [0.0; 3],
            yaw: 0.0,
            pitch: 0.0,
            distance: 0.0,
            fov_y: 0.0,
            projection: default_projection(),
            ortho_scale: 0.0,
        }
    }
}

/// The lighting environment. Reserved for the HDRI/IBL flow;
/// defaults are inert.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct EnvironmentJson {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ibl_mode: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hdri_asset: Option<String>,
    #[serde(default, skip_serializing_if = "JsonObject::is_empty")]
    pub background: JsonObject,
}

/// Review annotations. The annotation shape is owned by
/// `solarxy_core::review` (which has its own schema); the format carries
/// each annotation opaquely, so it survives a save/load round trip without
/// the format layer needing to understand it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ReviewJson {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<serde_json::Value>,
}

/// One semantic asset record in `scene.json`. `id` and `sha256` are the
/// same content hash today; `role` distinguishes imports from future
/// environment/texture assets.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct AssetRecordJson {
    pub id: String,
    #[serde(default = "default_asset_role")]
    pub role: String,
    pub sha256: String,
    pub original_name: String,
    /// Additional names the same bytes were staged under. Content-addressing
    /// collapses byte-identical companions into one entry, so without these a
    /// reload would forget every name but the first and report the others as
    /// missing companions.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alias_names: Vec<String>,
    #[serde(default, skip_serializing_if = "JsonObject::is_empty")]
    pub import_settings: JsonObject,
}

/// The scene clock, as saved.
///
/// Deliberately only the persisted half: `playing` and the current frame are
/// session state. A format that could round-trip "I was playing when I hit
/// save" would make a scene's meaning depend on the author's transport, and
/// would break the reproducibility every golden capture and CLI cook relies
/// on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub struct RuntimeJson {
    #[serde(default = "default_fps")]
    pub fps: f64,
    #[serde(default = "default_frame_start")]
    pub frame_start: i64,
    #[serde(default = "default_frame_end")]
    pub frame_end: i64,
    /// `once`, `loop` or `pingPong`.
    #[serde(default = "default_loop_mode")]
    pub loop_mode: String,
    /// Whether a published player starts playing on load. The editor saves
    /// it and never acts on it.
    #[serde(default)]
    pub autoplay: bool,
}

fn default_fps() -> f64 {
    24.0
}

fn default_frame_start() -> i64 {
    1
}

fn default_frame_end() -> i64 {
    240
}

fn default_loop_mode() -> String {
    "loop".to_string()
}

impl Default for RuntimeJson {
    fn default() -> Self {
        Self {
            fps: default_fps(),
            frame_start: default_frame_start(),
            frame_end: default_frame_end(),
            loop_mode: default_loop_mode(),
            autoplay: false,
        }
    }
}

/// Editor-only state: the cook mode and per-context canvas viewports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct EditorJson {
    #[serde(default = "default_cook_mode")]
    pub cook_mode: String,
    /// Canvas pan/zoom per context, keyed by `"root"` or a geo node id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub canvas_viewports: BTreeMap<String, CanvasViewportJson>,
}

impl Default for EditorJson {
    fn default() -> Self {
        Self {
            cook_mode: default_cook_mode(),
            canvas_viewports: BTreeMap::new(),
        }
    }
}

/// One context's canvas pan/zoom.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct CanvasViewportJson {
    pub x: f32,
    pub y: f32,
    pub zoom: f32,
}

/// Document metadata. Timestamps are ISO-8601 strings supplied by the host
/// (the format layer never reads a clock).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct MetaJson {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub project_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub created: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modified: String,
}

/// The known top-level keys of `scene.json`, for unknown-field warnings on
/// read (the format does not `deny_unknown_fields`; it warns instead).
pub const SCENE_TOP_LEVEL_KEYS: &[&str] = &[
    "schema_version",
    "min_reader",
    "generator",
    "units",
    "graph",
    "view",
    "environment",
    "review",
    "assets",
    "editor",
    "runtime",
    "meta",
];
