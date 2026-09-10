//! Action parameters: the button a node declares, pressed from the
//! Properties panel.
//!
//! One rule routes a press, and it is the browser's rule restated in one
//! place. The render node's action is interpreted by the host, because it
//! opens the still dialog rather than producing bytes; every other action
//! runs the engine's encoder over the node's last cook and the bytes go to a
//! native save dialog. The declaration does not yet say which kind it is, so
//! the type id decides, here and nowhere else in this shell.

use std::path::Path;

use solarxy_graph::document::{GraphContext, NodeId};
use solarxy_graph::engine::ActionResult;

use super::State;
use super::update::find_node_name;
use crate::gui::ToastSeverity;

/// Where a press goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ActionRoute {
    /// The host opens the still dialog for the node.
    Still,
    /// The engine encodes the node's committed output.
    Engine,
}

/// The routing rule. The key is carried so a second host-interpreted action
/// can be told apart when one exists; today the render node has exactly one.
pub(super) fn action_route(type_id: &str, _key: &str) -> ActionRoute {
    if type_id == "render" {
        ActionRoute::Still
    } else {
        ActionRoute::Engine
    }
}

/// What a press produced, decided while the engine is borrowed and acted on
/// after the borrow ends.
enum Outcome {
    OpenStill,
    Save(ActionResult),
}

impl State {
    /// Press a node's action parameter.
    ///
    /// The engine's invocation mutates nothing, so a failure leaves the
    /// document as it was and reports why in a toast.
    pub(super) fn invoke_action(&mut self, ctx: GraphContext, node: NodeId, key: &str) {
        let outcome = {
            let Some(engine) = self.engine.as_deref() else {
                return;
            };
            let Some(data) = engine.document().graph(ctx).ok().and_then(|g| g.node(node)) else {
                self.gui.set_toast(
                    "That node is no longer in the document",
                    ToastSeverity::Warning,
                );
                return;
            };
            match action_route(&data.type_id, key) {
                ActionRoute::Still => Ok(Outcome::OpenStill),
                ActionRoute::Engine => engine
                    .invoke_action(ctx, node, key)
                    .map(Outcome::Save)
                    .map_err(|e| format!("{}: {e}", find_node_name(engine, node))),
            }
        };
        match outcome {
            Ok(Outcome::OpenStill) => {
                self.still_target = Some((ctx, node));
                self.open_still_dialog();
            }
            Ok(Outcome::Save(result)) => self.save_action_result(&result),
            Err(message) => self.gui.set_toast(&message, ToastSeverity::Error),
        }
    }

    /// Pick files for a file-reference parameter, stage them, and point
    /// the parameter at the primary.
    ///
    /// **Multi-select, and with no extension filter**, which is the
    /// browser's rule and looks like a bug until the reason is stated: a
    /// multi-file model is a primary plus companions (a glTF beside its
    /// binary buffer and its textures), the companions are resolved by
    /// name at parse time, and a filter built from `accept` would make
    /// them unselectable. So everything selected is staged, and `accept`
    /// decides only which of the staged files the parameter names.
    pub(super) fn choose_asset(&mut self, ctx: GraphContext, node: NodeId, key: &str) {
        let accept: Vec<String> = {
            let Some(engine) = self.engine.as_deref() else {
                return;
            };
            let Some(spec) = engine
                .document()
                .graph(ctx)
                .ok()
                .and_then(|g| g.node(node))
                .and_then(|data| engine.registry().get(&data.type_id))
                .and_then(|desc| desc.param(key))
            else {
                return;
            };
            match &spec.ty {
                solarxy_graph::registry::param_spec::ParamType::AssetRef { accept } => {
                    accept.iter().map(|ext| ext.to_ascii_lowercase()).collect()
                }
                _ => return,
            }
        };
        let Some(paths) = rfd::FileDialog::new().set_title("Select file").pick_files() else {
            return;
        };
        let mut primary: Option<solarxy_graph::params::AssetId> = None;
        let mut first: Option<solarxy_graph::params::AssetId> = None;
        let mut failed = Vec::new();
        for path in &paths {
            let Ok(bytes) = std::fs::read(path) else {
                failed.push(file_name(path));
                continue;
            };
            let name = file_name(path);
            let matches = accept
                .iter()
                .any(|ext| name.to_ascii_lowercase().ends_with(ext));
            let Some(engine) = self.engine.as_mut() else {
                return;
            };
            let id = engine.stage_asset(name, String::new(), bytes);
            if matches && primary.is_none() {
                primary = Some(id.clone());
            }
            if first.is_none() {
                first = Some(id);
            }
        }
        if !failed.is_empty() {
            self.gui.set_toast(
                &format!("Could not read {}", failed.join(", ")),
                ToastSeverity::Warning,
            );
        }
        // The first accepted file, or the first file at all: a selection
        // of only companions is a mistake worth pointing the parameter at
        // something for, so the import can say what it could not parse.
        let Some(id) = primary.or(first) else {
            return;
        };
        self.apply_node_command(solarxy_graph::Command::SetParam {
            ctx,
            node,
            key: key.to_string(),
            value: solarxy_graph::params::ParamSource::Literal(
                solarxy_graph::params::ParamValue::Asset(id),
            ),
        });
    }

    /// Offer the encoded bytes a save path through the native dialog, as
    /// the screenshot and the still do, and write them there.
    fn save_action_result(&mut self, result: &ActionResult) {
        let ext = Path::new(&result.filename)
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_owned);
        let mut dialog = rfd::FileDialog::new().set_file_name(&result.filename);
        if let Some(ext) = &ext {
            dialog = dialog.add_filter(ext.to_uppercase(), &[ext.as_str()]);
        }
        let Some(path) = dialog.save_file() else {
            return;
        };
        match std::fs::write(&path, &result.bytes) {
            Ok(()) => {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(&result.filename);
                self.gui
                    .set_toast(&format!("Exported {name}"), ToastSeverity::Success);
            }
            Err(e) => self.gui.set_toast(
                &format!("Could not write {}: {e}", path.display()),
                ToastSeverity::Error,
            ),
        }
    }
}

/// A path's file name, or the whole path when it has none.
fn file_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one branch on a node type in this shell's action handling, pinned
    /// so a second host-interpreted action is added here and not in a panel.
    #[test]
    fn the_render_action_routes_to_the_still_dialog_and_everything_else_to_the_engine() {
        assert_eq!(action_route("render", "render"), ActionRoute::Still);
        assert_eq!(action_route("geo_export", "save"), ActionRoute::Engine);
        assert_eq!(action_route("image_export", "save"), ActionRoute::Engine);
        assert_eq!(action_route("box", "anything"), ActionRoute::Engine);
    }
}
