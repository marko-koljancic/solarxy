//! What a drop onto the window does.
//!
//! A drop adds to the scene rather than becoming it, which is what the
//! browser does and what a document-rooted shell should do: every model in
//! the drop becomes an import node in the network the Node Tree is showing,
//! with the companions it names staged beside it, and the whole drop is one
//! undo step. The window delivers one event per dropped item with nothing to
//! say the gesture is complete, so the paths are collected and handled once
//! per frame, which is what lets a folder of models be one gesture rather
//! than a document replaced once per file.
//!
//! Two things keep their old route. A lone scene file opens as a document,
//! since a scene is not something to import into another; and a single model
//! dropped with no document open goes through the ordinary open path, which
//! parses on a worker and frames the panes at adoption. Environment maps in
//! a drop install as the environment, as they always did.
//!
//! The parse itself runs on the interface thread inside the frame's cook, as
//! it does for any opened scene that holds an import node, so a large drop
//! holds the frame for the parse. The engine resolves that job from the
//! assets alone, so moving it to a worker later is the shell's change and
//! not the engine's.

use std::path::{Path, PathBuf};

use solarxy_graph::document::{ContextKind, GraphContext, NodeId};
use solarxy_graph::model_document;
use solarxy_graph::{Command, Engine};

use super::State;
use crate::gui::ToastSeverity;

/// The formats a drop can import, for the message when it holds none.
const IMPORTABLE: &str = ".obj / .gltf / .glb / .stl / .ply";

/// Every file under the dropped paths, folders walked to any depth.
///
/// Hidden entries are skipped and the result is sorted, so a folder imports
/// in a stable order whatever the file system returns. Capped, because a
/// drop of a home directory is a mistake rather than a request.
pub(super) fn expand_drop(paths: &[PathBuf]) -> Vec<PathBuf> {
    const CAP: usize = 10_000;
    let mut out = Vec::new();
    for path in paths {
        walk(path, &mut out, CAP);
    }
    out.sort();
    out
}

fn walk(path: &Path, out: &mut Vec<PathBuf>, cap: usize) {
    if out.len() >= cap {
        return;
    }
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        let mut children: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        children.sort();
        for child in children {
            let hidden = child
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'));
            if !hidden {
                walk(&child, out, cap);
            }
        }
    } else if path.is_file() {
        out.push(path.to_path_buf());
    }
}

fn ext_of(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
}

/// What a drop asks for, decided from its files alone.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum DropPlan {
    /// A lone scene file: open it as the document.
    Scene(PathBuf),
    /// Models to import into the scene, and environment maps to install.
    Import {
        models: Vec<PathBuf>,
        hdris: Vec<PathBuf>,
    },
    /// Nothing this shell imports.
    Nothing,
}

pub(super) fn plan_drop(files: Vec<PathBuf>) -> DropPlan {
    if let [only] = files.as_slice()
        && ext_of(only) == "slxy"
    {
        return DropPlan::Scene(only.clone());
    }
    let mut models = Vec::new();
    let mut hdris = Vec::new();
    for file in files {
        let ext = ext_of(&file);
        if model_document::import_type_for(&ext).is_some() {
            models.push(file);
        } else if ext == "hdr" || ext == "exr" {
            hdris.push(file);
        }
    }
    if models.is_empty() && hdris.is_empty() {
        DropPlan::Nothing
    } else {
        DropPlan::Import { models, hdris }
    }
}

/// One model staged into a network: read, its companions collected and
/// staged, the import node added. The open path's build minus the engine
/// and the cook.
pub(super) struct Staged {
    pub node: NodeId,
    pub warnings: Vec<String>,
}

pub(super) fn stage_model(
    engine: &mut Engine,
    ctx: GraphContext,
    path: &Path,
) -> Result<Staged, String> {
    let bytes =
        std::fs::read(path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let ext = ext_of(path);
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("model")
        .to_string();
    // Companions before the primary, for the reason the open path states:
    // a required companion that cannot be read fails naming the file.
    let companions =
        solarxy_formats::companions::collect(path, &ext, &bytes).map_err(|e| e.to_string())?;
    for asset in companions.assets {
        engine.stage_asset(asset.name, String::new(), asset.bytes);
    }
    let node = model_document::add_import_node(engine, ctx, &name, &ext, bytes)
        .map_err(|e| e.to_string())?;
    Ok(Staged {
        node,
        warnings: companions.warnings,
    })
}

/// Where a dropped model lands: the network the Node Tree is showing when
/// it is a geometry network, else a new one at the root. Asked of the
/// registry rather than named, as the model synthesis asks.
pub(super) fn import_context(
    engine: &mut Engine,
    shown: GraphContext,
) -> Result<GraphContext, String> {
    if let GraphContext::Subflow(_) = shown
        && engine
            .document()
            .graph(shown)
            .is_ok_and(|g| g.kind == ContextKind::Sop)
    {
        return Ok(shown);
    }
    let container = engine
        .registry()
        .container_for(ContextKind::Sop)
        .ok_or("no registered type opens a geometry network")?
        .type_id
        .to_string();
    let batch = engine
        .apply(Command::AddNode {
            ctx: GraphContext::Root,
            node_type: container,
            position: [0.0, 0.0],
        })
        .map_err(|e| e.to_string())?;
    model_document::added_node(&batch)
        .map(GraphContext::Subflow)
        .ok_or_else(|| "the geometry container was not created".to_string())
}

impl State {
    /// Queue a dropped path. The batch is handled once per frame.
    pub fn drop_path(&mut self, path: PathBuf) {
        self.pending_drop.push(path);
    }

    /// Act on everything dropped since the last frame.
    pub(super) fn handle_pending_drop(&mut self) {
        if self.pending_drop.is_empty() {
            return;
        }
        let paths = std::mem::take(&mut self.pending_drop);
        let folder = paths.iter().find(|p| p.is_dir()).cloned();
        match plan_drop(expand_drop(&paths)) {
            DropPlan::Scene(path) => self.open_file(path),
            DropPlan::Nothing => {
                let what = match (&folder, paths.as_slice()) {
                    (Some(folder), _) => format!(
                        "No model file in {}",
                        folder
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("the folder")
                    ),
                    (None, [single]) => format!(
                        "Unsupported format: {}",
                        single
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("the file")
                    ),
                    _ => "No model file in the drop".to_string(),
                };
                self.gui
                    .set_toast(&format!("{what} ({IMPORTABLE})"), ToastSeverity::Warning);
            }
            DropPlan::Import { models, mut hdris } => {
                // The last environment map wins, which is what dropping them
                // one at a time would have done.
                if let Some(hdri) = hdris.pop() {
                    self.open_file(hdri);
                }
                match (self.engine.is_some(), models.as_slice()) {
                    (_, []) => {}
                    // The ordinary open: worker-parsed, framed at adoption.
                    (false, [single]) => self.open_file(single.clone()),
                    (false, _) => {
                        if self.adopt_untitled_document() {
                            self.import_models(&models);
                        }
                    }
                    (true, _) => self.import_models(&models),
                }
            }
        }
    }

    /// Import every model into the open document as one undo step.
    fn import_models(&mut self, models: &[PathBuf]) {
        let shown = self.gui.graph_ctx();
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        let label = match models {
            [single] => format!(
                "Import {}",
                single
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("model")
            ),
            _ => format!("Import {} models", models.len()),
        };
        let mut messages: Vec<(String, ToastSeverity)> = Vec::new();
        let mut imported = Vec::new();
        if let Err(e) = engine.apply(Command::BeginTransaction {
            label: label.clone(),
        }) {
            messages.push((format!("Could not import: {e}"), ToastSeverity::Error));
        } else {
            match import_context(engine, shown) {
                Ok(ctx) => {
                    for path in models {
                        match stage_model(engine, ctx, path) {
                            Ok(staged) => {
                                imported.push(staged.node);
                                messages.extend(
                                    staged
                                        .warnings
                                        .into_iter()
                                        .map(|w| (w, ToastSeverity::Warning)),
                                );
                            }
                            Err(e) => messages.push((e, ToastSeverity::Error)),
                        }
                    }
                    if let Err(e) = engine.apply(Command::EndTransaction) {
                        messages.push((format!("Could not import: {e}"), ToastSeverity::Error));
                    }
                }
                Err(e) => {
                    let _ = engine.apply(Command::CancelTransaction);
                    messages.push((format!("Could not import: {e}"), ToastSeverity::Error));
                }
            }
        }
        for (message, severity) in messages {
            self.gui.set_toast(&message, severity);
        }
        match imported.len() {
            0 => {}
            1 => self
                .gui
                .set_toast(&format!("Importing {label}"), ToastSeverity::Info),
            n => self
                .gui
                .set_toast(&format!("Importing {n} models"), ToastSeverity::Info),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../res/models")
            .join(rel)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("solarxy-drop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    /// Folders walk to any depth, hidden entries stay out, and the order is
    /// the paths' own rather than the file system's.
    #[test]
    fn a_folder_expands_to_its_files_sorted() {
        let dir = scratch("walk");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("b.obj"), b"").unwrap();
        std::fs::write(dir.join("sub/a.stl"), b"").unwrap();
        std::fs::write(dir.join(".hidden.obj"), b"").unwrap();
        std::fs::write(dir.join("a.txt"), b"").unwrap();

        let files = expand_drop(std::slice::from_ref(&dir));
        let names: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["a.txt", "b.obj", "sub/a.stl"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A scene dropped alone opens as the document; dropped beside a model
    /// it is not something to import, so only the model counts.
    #[test]
    fn a_lone_scene_file_opens_as_a_document_but_not_beside_a_model() {
        let scene = PathBuf::from("shot.slxy");
        assert_eq!(
            plan_drop(vec![scene.clone()]),
            DropPlan::Scene(scene.clone())
        );
        assert_eq!(
            plan_drop(vec![
                scene,
                PathBuf::from("x.OBJ"),
                PathBuf::from("sky.hdr")
            ]),
            DropPlan::Import {
                models: vec![PathBuf::from("x.OBJ")],
                hdris: vec![PathBuf::from("sky.hdr")],
            }
        );
    }

    #[test]
    fn a_drop_with_no_model_is_nothing() {
        assert_eq!(
            plan_drop(vec![PathBuf::from("readme.txt"), PathBuf::from("notes")]),
            DropPlan::Nothing
        );
        assert_eq!(plan_drop(Vec::new()), DropPlan::Nothing);
    }

    /// A model dropped into a fresh document lands as an import node whose
    /// file parameter names its staged bytes, with the companions the model
    /// names staged beside it.
    #[test]
    fn an_import_lands_in_the_given_context_with_its_file_param() {
        let mut engine = Engine::new().expect("engine");
        let ctx = import_context(&mut engine, GraphContext::Root).expect("a network");
        assert!(matches!(ctx, GraphContext::Subflow(_)));
        let staged = stage_model(&mut engine, ctx, &model("knot/knot.obj")).expect("stages");
        assert!(staged.warnings.is_empty(), "{:?}", staged.warnings);

        let node = engine
            .document()
            .graph(ctx)
            .expect("the network")
            .node(staged.node)
            .expect("the import");
        assert_eq!(node.type_id, "import_obj");
        assert!(
            matches!(
                node.params.get("file"),
                Some(solarxy_graph::params::ParamSource::Literal(
                    solarxy_graph::params::ParamValue::Asset(_)
                ))
            ),
            "the file parameter names the staged bytes"
        );
        // The manifest pairs an id with a name; either column may carry the
        // name depending on the row, so both are searched.
        let names: Vec<String> = engine
            .asset_manifest()
            .into_iter()
            .flat_map(|(a, b)| [a, b])
            .collect();
        assert!(names.iter().any(|n| n.ends_with("knot.obj")), "{names:?}");
        assert!(names.iter().any(|n| n.ends_with("knot.mtl")), "{names:?}");
    }

    /// The network the tree is showing takes the drop when it is a geometry
    /// network; the root, or any other kind, gets a new one.
    #[test]
    fn a_dropped_model_lands_in_the_shown_geometry_network() {
        let mut engine = Engine::new().expect("engine");
        let first = import_context(&mut engine, GraphContext::Root).expect("a network");
        let again = import_context(&mut engine, first).expect("the same network");
        assert_eq!(again, first);
        let other = import_context(&mut engine, GraphContext::Root).expect("a second network");
        assert_ne!(other, first);
    }

    /// The first model into an empty network is what it displays; a second
    /// one does not take that over.
    #[test]
    fn a_second_model_does_not_steal_the_display_flag() {
        let mut engine = Engine::new().expect("engine");
        let ctx = import_context(&mut engine, GraphContext::Root).expect("a network");
        let first = stage_model(&mut engine, ctx, &model("knot/knot.obj")).expect("first");
        let second = stage_model(&mut engine, ctx, &model("xyzrgb_dragon.obj")).expect("second");
        assert_ne!(first.node, second.node);
        let shown = engine
            .document()
            .graph(ctx)
            .expect("the network")
            .active_output;
        assert_eq!(shown, Some(first.node));
    }
}
