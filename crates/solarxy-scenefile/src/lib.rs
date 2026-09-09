//! The `.slxy` self-contained scene file.
//!
//! A `.slxy` is a ZIP holding `manifest.json` (the byte-level asset index),
//! `scene.json` (the Rust-owned document schema), and
//! `assets/<sha256>` content-addressed blobs. The schema lives here, not in
//! the engine: `solarxy-graph` maps a live document to and from
//! [`SceneJson`], and this crate owns the container, the SHA-256 integrity
//! check, and the schema-version gate.
//!
//! Self-containment is the promise: the file embeds every referenced asset,
//! and geometry is never baked in (graphs recompute on load), so files stay
//! small and honestly parametric. Versioning is an integer `schema_version`,
//! **frozen at 1** as of the v0.7.0 public beta (0 was the pre-beta format and
//! still reads, migrating up), plus a `min_reader` floor that hard-rejects files
//! from a newer writer; unknown fields are accepted with a warning rather than
//! rejected. See `schemas/README.md` for the change policy.

#![warn(clippy::pedantic)]
#![allow(clippy::missing_errors_doc, clippy::module_name_repetitions)]

mod archive;
mod manifest;
mod scene;

pub use archive::sha256_hex;
pub use manifest::{AssetManifestEntry, ManifestJson};
pub use scene::{
    AssetRecordJson, CameraJson, CanvasViewportJson, EditorJson, EdgeJson, EnvironmentJson,
    GraphJson, JsonObject, MetaJson, NodeJson, PaneJson, ReviewJson, RuntimeJson, SceneJson,
    SubGraphJson, ViewJson, SCENE_TOP_LEVEL_KEYS,
};

use thiserror::Error;

/// The schema version this build writes. A change to the on-disk shape needs
/// a version bump and a step in `migrate_scene`, never a silent edit.
///
/// Version 0 was the pre-beta format and carried no compatibility guarantees.
/// Version 1 was the public-beta freeze (v0.7.0). Version 2 renamed the
/// context vocabulary: a geometry network is a SOP network and an image
/// network is a COP network, so the sub-graph kinds and the two container
/// type ids changed spelling. Both older versions still open; the chain in
/// `migrate_scene` steps them up on load.
pub const SCHEMA_VERSION_CURRENT: u32 = 2;

/// The `min_reader` floor this build writes: the lowest reader version able
/// to open files it produces.
///
/// It moved to 2 with the vocabulary. A version-1 reader handed a version-2
/// file would not fail: it would find container type ids it has never heard
/// of, load each as a parameterless placeholder, and destroy the document on
/// the next save. Refusing with an upgrade message is what this field is for.
pub const MIN_READER_CURRENT: u32 = 2;

/// The reader version this build implements. A file whose `min_reader`
/// exceeds this is refused with an upgrade message.
///
/// Moves in lockstep with [`MIN_READER_CURRENT`]. If it did not, this build
/// would write files whose `min_reader` its own reader then rejected as
/// "too new".
pub const READER_VERSION: u32 = 2;

/// The lockstep above, enforced rather than only described.
///
/// Two unit tests already cover it, `this_build_can_read_what_it_writes` and
/// `a_scene_needing_a_newer_reader_is_refused`. A `const` assert is worth
/// having beside them because it fails the build of anything that links this
/// crate, rather than of whoever remembers to run its tests, and because the
/// two constants are edited together exactly once per format version, which
/// is precisely when nobody is looking at the tests.
const _: () = assert!(
    MIN_READER_CURRENT == READER_VERSION,
    "MIN_READER_CURRENT and READER_VERSION drifted: this build would write \
     files its own reader refuses as too new"
);

/// The two required archive entries.
const MANIFEST_ENTRY: &str = "manifest.json";
const SCENE_ENTRY: &str = "scene.json";
const ASSET_PREFIX: &str = "assets/";

/// Anything that can go wrong reading or writing a `.slxy`.
#[derive(Debug, Error)]
pub enum SceneFileError {
    #[error("zip error: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("missing required archive entry: {0}")]
    MissingEntry(&'static str),
    #[error("asset {id} failed its integrity check: {detail}")]
    Integrity { id: String, detail: String },
    #[error(
        "this scene needs a newer Solarxy to open (requires reader {required}, this build is {reader})"
    )]
    TooNew { required: u32, reader: u32 },
    #[error("unsupported schema_version {0}")]
    UnsupportedVersion(u32),
    #[error("scene.json is missing the required `{0}` field; this is not a Solarxy scene")]
    MissingVersionField(&'static str),
}

/// One embedded asset: its content hash, original name, MIME, and bytes.
/// Used both as write input and read output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetBlob {
    pub sha256: String,
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// A whole scene in memory: the document plus its embedded assets.
#[derive(Debug, Clone, PartialEq)]
pub struct SceneFile {
    pub scene: SceneJson,
    pub assets: Vec<AssetBlob>,
}

/// The outcome of reading a `.slxy`: the file plus any non-fatal warnings
/// (unknown fields, unexpected extra blobs).
#[derive(Debug, Clone)]
pub struct ReadResult {
    pub file: SceneFile,
    pub warnings: Vec<String>,
}

/// Serializes a [`SceneFile`] to `.slxy` archive bytes. The manifest is
/// derived from the scene and the blobs; assets are deduplicated by content
/// hash. `manifest.created_at` reuses `scene.meta.modified`.
pub fn write(file: &SceneFile) -> Result<Vec<u8>, SceneFileError> {
    let mut assets_manifest = Vec::with_capacity(file.assets.len());
    let mut entries: Vec<(String, Vec<u8>)> = Vec::with_capacity(file.assets.len() + 2);
    let mut seen = std::collections::BTreeSet::new();

    // Reserve slots for manifest.json + scene.json (filled after the loop
    // so the manifest can list the assets).
    for blob in &file.assets {
        if !seen.insert(blob.sha256.clone()) {
            continue; // content-addressed: identical bytes embed once
        }
        let path = format!("{ASSET_PREFIX}{}", blob.sha256);
        assets_manifest.push(AssetManifestEntry {
            id: blob.sha256.clone(),
            name: blob.name.clone(),
            mime: blob.mime.clone(),
            size: blob.bytes.len() as u64,
            sha256: blob.sha256.clone(),
            path: path.clone(),
        });
        entries.push((path, blob.bytes.clone()));
    }

    let manifest = ManifestJson {
        schema_version: file.scene.schema_version,
        generator: file.scene.generator.clone(),
        created_at: file.scene.meta.modified.clone(),
        assets: assets_manifest,
    };

    let manifest_bytes = serde_json::to_vec_pretty(&manifest)?;
    let scene_bytes = serde_json::to_vec_pretty(&file.scene)?;

    let mut all = Vec::with_capacity(entries.len() + 2);
    all.push((MANIFEST_ENTRY.to_string(), manifest_bytes));
    all.push((SCENE_ENTRY.to_string(), scene_bytes));
    all.extend(entries);

    archive::zip_bytes(&all)
}

/// Reads `.slxy` archive bytes into a [`SceneFile`], enforcing the
/// `min_reader` gate and every asset's SHA-256 integrity, and collecting
/// unknown-field / unexpected-entry warnings.
pub fn read(bytes: &[u8]) -> Result<ReadResult, SceneFileError> {
    let mut warnings = Vec::new();
    let raw = archive::unzip(bytes)?;

    let manifest_bytes = raw
        .iter()
        .find(|(n, _)| n == MANIFEST_ENTRY)
        .map(|(_, b)| b)
        .ok_or(SceneFileError::MissingEntry(MANIFEST_ENTRY))?;
    let scene_bytes = raw
        .iter()
        .find(|(n, _)| n == SCENE_ENTRY)
        .map(|(_, b)| b)
        .ok_or(SceneFileError::MissingEntry(SCENE_ENTRY))?;

    let manifest: ManifestJson = serde_json::from_slice(manifest_bytes)?;

    // Parse the scene loosely first: the version gate and unknown-field
    // warnings run before we commit to the typed shape.
    let mut scene_value: serde_json::Value = serde_json::from_slice(scene_bytes)?;

    let min_reader = read_u32(&scene_value, "min_reader");
    if min_reader > READER_VERSION {
        return Err(SceneFileError::TooNew {
            required: min_reader,
            reader: READER_VERSION,
        });
    }

    // Absent must not mean zero. `read_u32` defaults to 0, which used to make a
    // corrupt or foreign scene.json indistinguishable from a legitimate v0 file:
    // it would be handed to the v0 migration and half-loaded. Every scene this
    // project has ever written stamps the field, so its absence means the file is
    // not one of ours. Tightened at the freeze, while no v1 files exist yet.
    let schema_version = scene_value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(SceneFileError::MissingVersionField("schema_version"))?;

    match schema_version.cmp(&SCHEMA_VERSION_CURRENT) {
        // A same-reader file from a newer schema (but within min_reader):
        // accept and warn; #[serde(default)] fields absorb the difference.
        std::cmp::Ordering::Greater => warnings.push(format!(
            "scene.json schema_version {schema_version} is newer than this build's {SCHEMA_VERSION_CURRENT}; loading best-effort"
        )),
        std::cmp::Ordering::Less => migrate_scene(&mut scene_value, schema_version)?,
        std::cmp::Ordering::Equal => {}
    }

    // Unknown top-level keys become warnings (the format never
    // deny_unknown_fields; a newer writer's extra section is not fatal).
    if let Some(obj) = scene_value.as_object() {
        for key in obj.keys() {
            if !SCENE_TOP_LEVEL_KEYS.contains(&key.as_str()) {
                warnings.push(format!("scene.json has an unknown top-level field '{key}'"));
            }
        }
    }

    let scene: SceneJson = serde_json::from_value(scene_value)?;

    // Collect and integrity-check the asset blobs against the manifest.
    let mut assets = Vec::with_capacity(manifest.assets.len());
    for entry in &manifest.assets {
        let blob = raw
            .iter()
            .find(|(n, _)| n == &entry.path)
            .map(|(_, b)| b)
            .ok_or_else(|| SceneFileError::Integrity {
                id: entry.id.clone(),
                detail: format!("missing blob at {}", entry.path),
            })?;
        let digest = sha256_hex(blob);
        if digest != entry.sha256 {
            return Err(SceneFileError::Integrity {
                id: entry.id.clone(),
                detail: "content hash mismatch".to_string(),
            });
        }
        if blob.len() as u64 != entry.size {
            return Err(SceneFileError::Integrity {
                id: entry.id.clone(),
                detail: format!("size {} != manifest {}", blob.len(), entry.size),
            });
        }
        assets.push(AssetBlob {
            sha256: entry.sha256.clone(),
            name: entry.name.clone(),
            mime: entry.mime.clone(),
            bytes: blob.clone(),
        });
    }

    // Warn on archived asset blobs the manifest never listed.
    for (name, _) in &raw {
        if name.starts_with(ASSET_PREFIX) && !manifest.assets.iter().any(|e| &e.path == name) {
            warnings.push(format!("archive has an unlisted asset blob '{name}'"));
        }
    }

    Ok(ReadResult {
        file: SceneFile { scene, assets },
        warnings,
    })
}

/// Reads a small unsigned version field from a raw scene value (0 when
/// absent, malformed, or out of `u32` range).
fn read_u32(value: &serde_json::Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0)
}

/// Steps a raw `scene.json` value up from an older `schema_version` to the
/// current one, one version at a time, editing raw JSON before it is typed.
///
/// # It is a chain, and that is the whole point of the loop
///
/// This used to apply exactly one step and return. Correct while only one
/// step existed, and wrong on the day a second was added: a version-0 file
/// would run the 0 step, be restamped to 1, and then be handed to a reader
/// expecting the newest shape, with no error anywhere. The defect was written
/// down before it could bite, in this crate's own comment claiming stepwise
/// behaviour, in the architecture set, and in the scene-format decision
/// record. The loop is what makes the claim true.
///
/// **The driver stamps the version, never a step.** A step that forgot would
/// leave a document that migrates again on every open, which is invisible
/// while the steps happen to be idempotent and destructive the moment one is
/// not.
///
/// # 0 -> 1 (the public-beta freeze, v0.7.0)
///
/// Structurally a no-op, and deliberately written out rather than assumed.
/// Every field a v0 file carries is still valid at v1; the only shape change
/// in the freeze was `AssetRecordJson::alias_names`, which is `#[serde(default)]`
/// and so materialises as an empty list on a v0 file with no raw edit at all.
///
/// The step still exists, and is still tested, because the alternative is worse:
/// without an explicit `0` arm every pre-beta `.slxy` would fall through to the
/// catch-all and be rejected as `UnsupportedVersion(0)`.
fn migrate_scene(value: &mut serde_json::Value, from: u32) -> Result<(), SceneFileError> {
    // A version this build has no step for. The caller warns rather than
    // migrating on a future version, so this is reachable only by a direct
    // call, and it answers the way the flat match it replaced did.
    if from > SCHEMA_VERSION_CURRENT {
        return Err(SceneFileError::UnsupportedVersion(from));
    }

    let mut version = from;
    while version < SCHEMA_VERSION_CURRENT {
        match version {
            // No field rewrites needed (see the doc comment above).
            0 => {}
            1 => rename_context_vocabulary(value),
            other => return Err(SceneFileError::UnsupportedVersion(other)),
        }
        version += 1;
        if let Some(obj) = value.as_object_mut() {
            obj.insert("schema_version".to_string(), serde_json::json!(version));
        }
    }
    Ok(())
}

/// The 1 -> 2 step: the context vocabulary takes the field's names.
///
/// Sub-graph kinds `geo` and `tex` become `sop` and `cop`, and the two
/// container type ids `geo` and `texnet` become `sopnet` and `copnet`. The
/// third container, `matnet`, does not move.
///
/// **Why this runs on raw JSON rather than after typing.** An unmigrated
/// container resolves to no registered descriptor, and the recovery path for
/// an unknown type keeps the node's id and position while discarding every
/// parameter, its port order and its timestamps. A container would therefore
/// lose its transform, its render flags and its name, and its whole network
/// would be orphaned, with nothing but a warning to say so.
///
/// **The walk is fixed at two levels**, because a sub-graph carries no
/// sub-graphs of its own. Container types are declared root-only, so only the
/// root can hold one today; the inner level is rewritten anyway, because a
/// walk that is correct only while a placement rule holds is a trap for
/// whoever relaxes it.
///
/// **The name back-fill is the part with teeth.** A node with no explicit
/// name answers to its type's display name, and expressions address nodes by
/// name, so changing a display name silently changes what `ch("/Geo/...")`
/// refers to. Writing the name the container resolved to *before* the rename
/// turns a silent failure into no failure at all.
///
/// Two details of that back-fill are deliberate. It writes the **top-level**
/// `name` field rather than a `name` parameter, because that is where the
/// writer puts it: a literal-text `name` param is lifted out of the params
/// map on write and re-injected on read, so a back-fill into the params map
/// would produce a file shaped unlike anything this project emits. And it
/// skips any node that already carries a `name` parameter, which can only be
/// an expression-valued one from a hand-edited file: such a name also
/// resolves to the display name, but writing a literal over it would destroy
/// the expression, and a migration that loses data to prevent a rename is a
/// bad trade.
fn rename_context_vocabulary(value: &mut serde_json::Value) {
    /// Container type ids, with the display name each resolved to before the
    /// rename. `matnet` is absent because it does not move.
    const CONTAINERS: &[(&str, &str, &str)] =
        &[("geo", "sopnet", "Geo"), ("texnet", "copnet", "Tex")];
    const KINDS: &[(&str, &str)] = &[("geo", "sop"), ("tex", "cop")];

    fn rewrite_nodes(nodes: &mut serde_json::Value) {
        let Some(nodes) = nodes.as_array_mut() else {
            return;
        };
        for node in nodes {
            let Some(node) = node.as_object_mut() else {
                continue;
            };
            let Some(old_type) = node.get("type").and_then(|t| t.as_str()).map(str::to_owned)
            else {
                continue;
            };
            let Some(&(_, new_type, display)) =
                CONTAINERS.iter().find(|(from, _, _)| *from == old_type)
            else {
                continue;
            };
            node.insert("type".to_string(), serde_json::json!(new_type));

            let unnamed = node
                .get("name")
                .and_then(|n| n.as_str())
                .is_none_or(str::is_empty);
            let has_name_param = node
                .get("params")
                .and_then(|p| p.as_object())
                .is_some_and(|p| p.contains_key("name"));
            if unnamed && !has_name_param {
                node.insert("name".to_string(), serde_json::json!(display));
            }
        }
    }

    let Some(graph) = value.get_mut("graph").and_then(|g| g.as_object_mut()) else {
        return;
    };
    if let Some(nodes) = graph.get_mut("nodes") {
        rewrite_nodes(nodes);
    }
    let Some(subflows) = graph.get_mut("subflows").and_then(|s| s.as_object_mut()) else {
        return;
    };
    for sub in subflows.values_mut() {
        let Some(sub) = sub.as_object_mut() else {
            continue;
        };
        if let Some(kind) = sub.get("kind").and_then(|k| k.as_str())
            && let Some(&(_, new_kind)) = KINDS.iter().find(|(from, _)| *from == kind)
        {
            sub.insert("kind".to_string(), serde_json::json!(new_kind));
        }
        if let Some(nodes) = sub.get_mut("nodes") {
            rewrite_nodes(nodes);
        }
    }
}

/// Generates the [`SceneJson`] JSON Schema as a pretty string, stripping the
/// non-standard numeric `format` annotations `schemars` emits (mirrors the
/// `solarxy-core` precedent so strict validators accept the output).
#[cfg(feature = "schemars-gen")]
pub fn schema_json() -> Result<String, serde_json::Error> {
    let schema = schemars::schema_for!(SceneJson);
    let mut value = serde_json::to_value(&schema)?;
    strip_nonstandard_formats(&mut value);
    serde_json::to_string_pretty(&value)
}

/// Recursively removes the non-standard numeric `format` annotations
/// `schemars` emits for Rust integer and float types.
#[cfg(feature = "schemars-gen")]
fn strip_nonstandard_formats(value: &mut serde_json::Value) {
    const NUMERIC_FORMATS: &[&str] = &[
        "int8", "int16", "int32", "int64", "int128", "isize", "int", "uint8", "uint16", "uint32",
        "uint64", "uint128", "usize", "uint", "float", "double",
    ];
    match value {
        serde_json::Value::Object(map) => {
            let drop_format = matches!(
                map.get("format"),
                Some(serde_json::Value::String(s)) if NUMERIC_FORMATS.contains(&s.as_str())
            );
            if drop_format {
                map.remove("format");
            }
            for v in map.values_mut() {
                strip_nonstandard_formats(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                strip_nonstandard_formats(v);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact literals that round-trip through JSON

    use super::*;
    use std::collections::BTreeMap;

    fn sample_node() -> NodeJson {
        let mut params = JsonObject::new();
        params.insert("width".to_string(), serde_json::json!(1.5));
        params.insert("rotate".to_string(), serde_json::json!([0.0, 45.0, 0.0]));
        NodeJson {
            id: "1".to_string(),
            type_id: "box".to_string(),
            type_version: 1,
            name: "My Box".to_string(),
            bypass: true,
            params,
            port_order: BTreeMap::new(),
            position: [12.0, -3.5],
            created_ms: None,
            modified_ms: None,
        }
    }

    fn minimal_scene() -> SceneJson {
        SceneJson {
            schema_version: SCHEMA_VERSION_CURRENT,
            min_reader: MIN_READER_CURRENT,
            generator: "solarxy-test 0.0.0".to_string(),
            units: "meters".to_string(),
            graph: GraphJson {
                nodes: vec![sample_node()],
                edges: vec![EdgeJson {
                    id: "9".to_string(),
                    from: ("1".to_string(), "geometry".to_string()),
                    to: ("2".to_string(), "inputs".to_string()),
                }],
                subflows: BTreeMap::new(),
            },
            view: ViewJson::default(),
            environment: EnvironmentJson::default(),
            review: ReviewJson::default(),
            assets: Vec::new(),
            editor: EditorJson::default(),
            runtime: RuntimeJson::default(),
            meta: MetaJson::default(),
        }
    }

    fn blob(name: &str, bytes: &[u8]) -> AssetBlob {
        AssetBlob {
            sha256: sha256_hex(bytes),
            name: name.to_string(),
            mime: "model/obj".to_string(),
            bytes: bytes.to_vec(),
        }
    }

    /// The chain reaches the current version from every version behind it.
    ///
    /// Written now rather than when it can fail, because the failure it
    /// catches is silent: with one step defined, applying one step and
    /// applying the chain are indistinguishable, and the day a second step
    /// exists the difference is a document read at the wrong shape with no
    /// error. Looping over every older version means this test grows a case
    /// per format version without anyone remembering to add one.
    #[test]
    fn the_chain_reaches_the_current_version_from_every_older_one() {
        for from in 0..SCHEMA_VERSION_CURRENT {
            let mut value = serde_json::json!({
                "schema_version": from,
                "min_reader": 0,
                "generator": "",
                "graph": {},
            });
            migrate_scene(&mut value, from)
                .unwrap_or_else(|e| panic!("version {from} has no path forward: {e}"));
            assert_eq!(
                read_u32(&value, "schema_version"),
                SCHEMA_VERSION_CURRENT,
                "a document at version {from} stopped short of the current version, \
                 so the migration applied one step rather than the chain"
            );
        }
    }

    /// The two ends the chain must refuse or leave alone.
    #[test]
    fn the_chain_leaves_a_current_document_alone_and_refuses_a_future_one() {
        let at_current = serde_json::json!({ "schema_version": SCHEMA_VERSION_CURRENT });
        let mut value = at_current.clone();
        migrate_scene(&mut value, SCHEMA_VERSION_CURRENT).expect("nothing to do");
        assert_eq!(value, at_current, "a current document was rewritten");

        let future = SCHEMA_VERSION_CURRENT + 1;
        let mut value = serde_json::json!({ "schema_version": future });
        assert!(
            matches!(
                migrate_scene(&mut value, future),
                Err(SceneFileError::UnsupportedVersion(v)) if v == future
            ),
            "a version this build has no step for must be refused by name"
        );
    }

    /// A version-1 document, as the release before the rename wrote one.
    fn v1_document() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "min_reader": 1,
            "graph": {
                "nodes": [
                    { "id": "1", "type": "geo", "name": "terrain" },
                    { "id": "2", "type": "texnet" },
                    { "id": "3", "type": "matnet", "name": "shaders" },
                    { "id": "4", "type": "camera", "name": "hero" },
                    { "id": "5", "type": "geo",
                      "params": { "name": { "$expr": "chs(\"/other/label\")" } } },
                ],
                "subflows": {
                    "1": { "kind": "geo", "nodes": [ { "id": "10", "type": "box" } ] },
                    "2": { "kind": "tex", "nodes": [] },
                    "3": { "kind": "mat", "nodes": [] },
                    "9": { "nodes": [] },
                },
            },
        })
    }

    fn node<'a>(v: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
        v["graph"]["nodes"]
            .as_array()
            .expect("nodes")
            .iter()
            .find(|n| n["id"] == id)
            .unwrap_or_else(|| panic!("no node {id}"))
    }

    /// The container type ids and the sub-graph kinds take the new spellings,
    /// and the container that does not move keeps its own.
    #[test]
    fn the_vocabulary_step_rewrites_types_and_kinds() {
        let mut v = v1_document();
        migrate_scene(&mut v, 1).expect("the vocabulary step");

        assert_eq!(node(&v, "1")["type"], "sopnet");
        assert_eq!(node(&v, "2")["type"], "copnet");
        assert_eq!(node(&v, "3")["type"], "matnet", "the third container stays");
        assert_eq!(
            node(&v, "4")["type"],
            "camera",
            "a non-container is untouched"
        );

        let subs = &v["graph"]["subflows"];
        assert_eq!(subs["1"]["kind"], "sop");
        assert_eq!(subs["2"]["kind"], "cop");
        assert_eq!(subs["3"]["kind"], "mat");
        assert!(
            subs["9"].get("kind").is_none(),
            "a pre-context sub-graph carries no kind and must not be given one; \
             the engine resolves it from the owning container"
        );
        assert_eq!(
            subs["1"]["nodes"][0]["type"], "box",
            "an ordinary node inside a container is untouched"
        );
        assert_eq!(read_u32(&v, "schema_version"), 2);
    }

    /// The regression case for the naming ruling.
    ///
    /// A container with no name answered to its type's display name, and
    /// expressions address nodes by name, so changing the display name would
    /// silently change what `ch("/Geo/...")` refers to. The step writes the
    /// name the container resolved to before the rename, which turns a silent
    /// failure into no failure. A container that already has a name keeps it.
    #[test]
    fn an_unnamed_container_is_given_the_name_it_used_to_answer_to() {
        let mut v = v1_document();
        migrate_scene(&mut v, 1).expect("the vocabulary step");

        assert_eq!(
            node(&v, "2")["name"],
            "Tex",
            "the unnamed image container lost the name it resolved to"
        );
        assert_eq!(node(&v, "1")["name"], "terrain", "an explicit name is kept");
        assert_eq!(node(&v, "3")["name"], "shaders");
        assert!(
            node(&v, "4").get("name").is_some(),
            "a non-container keeps whatever it had"
        );
    }

    /// A name that is an expression is left exactly as it is.
    ///
    /// It also resolves to the display name and so breaks the same way, but a
    /// literal written over it would destroy the expression, and a migration
    /// that loses authored data to prevent a rename is a bad trade. One can
    /// only exist in a hand-edited file: the parameter setter refuses an
    /// expression on a text parameter.
    #[test]
    fn an_expression_valued_name_is_not_overwritten() {
        let mut v = v1_document();
        migrate_scene(&mut v, 1).expect("the vocabulary step");

        let n = node(&v, "5");
        assert_eq!(n["type"], "sopnet", "it is still renamed");
        assert!(
            n.get("name").is_none(),
            "a top-level name was written over an expression-valued one"
        );
        assert!(
            n["params"]["name"]["$expr"].is_string(),
            "the expression survived"
        );
    }

    /// A pre-beta v0 scene must still open, and come back stamped at whatever
    /// this build writes, having passed through every step in between.
    ///
    /// The other tests all build `SCHEMA_VERSION_CURRENT`, so they would pass
    /// vacuously no matter what the migration did or did not do. This one
    /// writes an actual v0 file and reads it back. It was asserting `1` until
    /// the vocabulary step landed, which is the moment "one step" and "the
    /// chain" stopped meaning the same thing.
    #[test]
    fn a_v0_scene_migrates_all_the_way_forward_on_read() {
        let cube = blob("cube.obj", b"v 0 0 0\n");
        let mut scene = minimal_scene();
        // A genuine pre-beta file: v0 stamps, and no `alias_names` concept.
        scene.schema_version = 0;
        scene.min_reader = 0;
        scene.assets.push(AssetRecordJson {
            id: cube.sha256.clone(),
            role: "import".to_string(),
            sha256: cube.sha256.clone(),
            original_name: "cube.obj".to_string(),
            alias_names: Vec::new(),
            import_settings: JsonObject::new(),
        });
        let bytes = write(&SceneFile {
            scene,
            assets: vec![cube.clone()],
        })
        .expect("write a v0 .slxy");

        let result = read(&bytes).expect("a v0 scene must still open after the freeze");

        assert_eq!(
            result.file.scene.schema_version, SCHEMA_VERSION_CURRENT,
            "a v0 file must traverse every step, not just the first"
        );
        assert_eq!(
            result.file.scene.graph.nodes.len(),
            1,
            "the graph survived the migration"
        );
        assert_eq!(result.file.assets, vec![cube], "assets survived");
        assert!(
            result.file.scene.assets[0].alias_names.is_empty(),
            "the field added at the freeze defaults on a v0 file"
        );
    }

    /// A file from the future is refused with an upgrade message rather than
    /// half-loaded. `READER_VERSION` must move with `MIN_READER_CURRENT`, or a
    /// build would reject its own output.
    #[test]
    fn a_scene_needing_a_newer_reader_is_refused() {
        let mut scene = minimal_scene();
        scene.min_reader = READER_VERSION + 1;
        let bytes = write(&SceneFile {
            scene,
            assets: Vec::new(),
        })
        .expect("write");

        assert!(matches!(read(&bytes), Err(SceneFileError::TooNew { .. })));
    }

    /// This build's own output must be readable by this build: if
    /// `MIN_READER_CURRENT` and `READER_VERSION` ever drift apart, every file we
    /// write is rejected as "too new" by our own reader.
    #[test]
    fn this_build_can_read_what_it_writes() {
        let bytes = write(&SceneFile {
            scene: minimal_scene(),
            assets: Vec::new(),
        })
        .expect("write");
        let result = read(&bytes).expect("a build must be able to open its own files");
        assert_eq!(result.file.scene.schema_version, SCHEMA_VERSION_CURRENT);
        assert!(result.warnings.is_empty());
    }

    /// An absent `schema_version` used to default to 0, making a corrupt or
    /// foreign JSON indistinguishable from a legitimate pre-beta file and feeding
    /// it to the migration. It is now an explicit error.
    #[test]
    fn a_scene_without_a_schema_version_is_rejected() {
        let bytes = write(&SceneFile {
            scene: minimal_scene(),
            assets: Vec::new(),
        })
        .expect("write");

        // Strip the field out of scene.json inside the archive.
        let mut src = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).expect("open");
        let mut out = Vec::new();
        {
            let mut zw = zip::ZipWriter::new(std::io::Cursor::new(&mut out));
            let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            for i in 0..src.len() {
                let mut entry = src.by_index(i).expect("entry");
                let name = entry.name().to_string();
                let mut buf = Vec::new();
                std::io::Read::read_to_end(&mut entry, &mut buf).expect("read");
                if name == SCENE_ENTRY {
                    let mut v: serde_json::Value = serde_json::from_slice(&buf).expect("json");
                    v.as_object_mut().unwrap().remove("schema_version");
                    buf = serde_json::to_vec(&v).expect("reserialize");
                }
                zw.start_file(name, opts).expect("start");
                std::io::Write::write_all(&mut zw, &buf).expect("write");
            }
            zw.finish().expect("finish");
        }

        assert!(
            matches!(
                read(&out),
                Err(SceneFileError::MissingVersionField("schema_version"))
            ),
            "an absent schema_version must not silently mean 0"
        );
    }

    #[test]
    fn round_trip_preserves_scene_and_assets() {
        let cube = blob("cube.obj", b"v 0 0 0\nv 1 1 1\n");
        let mut scene = minimal_scene();
        scene.assets.push(AssetRecordJson {
            id: cube.sha256.clone(),
            role: "import".to_string(),
            sha256: cube.sha256.clone(),
            original_name: "cube.obj".to_string(),
            alias_names: Vec::new(),
            import_settings: JsonObject::new(),
        });
        let file = SceneFile {
            scene: scene.clone(),
            assets: vec![cube.clone()],
        };

        let bytes = write(&file).expect("write .slxy");
        let result = read(&bytes).expect("read .slxy");

        assert!(result.warnings.is_empty(), "clean file has no warnings");
        assert_eq!(result.file.scene, scene, "scene round-trips exactly");
        assert_eq!(result.file.assets, vec![cube], "asset bytes round-trip");
        // The bypass flag, params, position, and edge tuple all survived.
        let n = &result.file.scene.graph.nodes[0];
        assert!(n.bypass);
        assert_eq!(n.position, [12.0, -3.5]);
        assert_eq!(n.params["width"], serde_json::json!(1.5));
    }

    #[test]
    fn identical_asset_content_embeds_once() {
        let a = blob("a.obj", b"same bytes");
        let b = blob("b.obj", b"same bytes"); // same content, same hash
        assert_eq!(a.sha256, b.sha256);
        let file = SceneFile {
            scene: minimal_scene(),
            assets: vec![a, b],
        };
        let bytes = write(&file).expect("write");
        let result = read(&bytes).expect("read");
        assert_eq!(result.file.assets.len(), 1, "deduped by content hash");
    }

    #[test]
    fn integrity_mismatch_is_rejected() {
        // A blob whose declared hash does not match its bytes: the reader
        // recomputes and rejects.
        let bad = AssetBlob {
            sha256: "0".repeat(64),
            name: "x.obj".to_string(),
            mime: String::new(),
            bytes: b"real bytes".to_vec(),
        };
        let file = SceneFile {
            scene: minimal_scene(),
            assets: vec![bad],
        };
        let bytes = write(&file).expect("write");
        assert!(matches!(
            read(&bytes),
            Err(SceneFileError::Integrity { .. })
        ));
    }

    #[test]
    fn min_reader_gate_rejects_a_future_file() {
        let mut scene = minimal_scene();
        scene.min_reader = READER_VERSION + 5;
        let file = SceneFile {
            scene,
            assets: Vec::new(),
        };
        let bytes = write(&file).expect("write");
        assert!(matches!(
            read(&bytes),
            Err(SceneFileError::TooNew { required, reader })
                if required == READER_VERSION + 5 && reader == READER_VERSION
        ));
    }

    #[test]
    fn unknown_top_level_field_warns_but_loads() {
        // Craft an archive whose scene.json carries an extra section.
        let mut value = serde_json::to_value(minimal_scene()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("future_section".to_string(), serde_json::json!({ "x": 1 }));
        let scene_bytes = serde_json::to_vec(&value).unwrap();
        let manifest = ManifestJson {
            schema_version: SCHEMA_VERSION_CURRENT,
            generator: "t".to_string(),
            created_at: String::new(),
            assets: Vec::new(),
        };
        let entries = vec![
            (
                MANIFEST_ENTRY.to_string(),
                serde_json::to_vec(&manifest).unwrap(),
            ),
            (SCENE_ENTRY.to_string(), scene_bytes),
        ];
        let bytes = archive::zip_bytes(&entries).unwrap();

        let result = read(&bytes).expect("unknown field is not fatal");
        assert!(
            result.warnings.iter().any(|w| w.contains("future_section")),
            "unknown top-level field is warned: {:?}",
            result.warnings
        );
    }

    #[test]
    fn missing_scene_entry_errors() {
        let manifest = ManifestJson {
            schema_version: SCHEMA_VERSION_CURRENT,
            generator: "t".to_string(),
            created_at: String::new(),
            assets: Vec::new(),
        };
        let entries = vec![(
            MANIFEST_ENTRY.to_string(),
            serde_json::to_vec(&manifest).unwrap(),
        )];
        let bytes = archive::zip_bytes(&entries).unwrap();
        assert!(matches!(
            read(&bytes),
            Err(SceneFileError::MissingEntry(SCENE_ENTRY))
        ));
    }
}
