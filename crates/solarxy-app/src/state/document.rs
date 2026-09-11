//! The open document as a file: save, save as, a new scene, and the window
//! title that says whether the two agree.
//!
//! **Dirty is a comparison, not a flag.** The engine's revision counter is
//! the authority on whether anything changed, and this shell remembers the
//! revision it last wrote (or opened) and compares. A shell-side boolean
//! drifts the moment a command path is added and not wired to it, and that
//! bug shows up only as data loss. The comparison carries the browser's two
//! consequences honestly: a selection change dirties, because it is a
//! command, and undoing back to the saved point stays dirty, because the
//! revision moved. Both are true in the browser for the same reason.
//!
//! **A save writes the same archive the browser writes.** `Engine::save_slxy`
//! takes the host sidecar: the per-pane cameras and display settings, the
//! lighting environment, the generator, and the document's metadata. Only
//! the view is this shell's to author; the document and the staged assets
//! are the engine's, and both cross unchanged.
//!
//! The write is atomic (a temporary sibling, then a rename) so a save that
//! fails halfway leaves the previous file intact rather than a partial one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use solarxy_core::view_config::PaneDisplaySettings;
use solarxy_graph::engine::SceneSidecar;
use solarxy_renderer::camera::Camera;

use super::{State, ViewLayout};
use crate::gui::ToastSeverity;
use crate::state::preferences;

/// The scene-file extension, without the dot.
pub(super) const SCENE_EXTENSION: &str = "slxy";

/// What the window is called for the open document.
///
/// `None` is no document at all. The trailing asterisk is the marker every
/// desktop platform reads as "unsaved changes"; macOS additionally dims its
/// proxy icon, but the text is what a user glances at.
pub(super) fn window_title(filename: Option<&str>, dirty: bool) -> String {
    match filename {
        None => "Solarxy".to_string(),
        Some(name) if dirty => format!("Solarxy - {name}*"),
        Some(name) => format!("Solarxy - {name}"),
    }
}

/// The renderer camera in the scene file's orbit shape: target, yaw, pitch
/// and distance, from `dir = eye - target`.
///
/// A near-copy of the web shell's, for the reason `apply_camera_json` beside
/// it states: the shared home would have to know both the scene format and
/// the renderer, and no crate beneath the two shells knows either.
pub(super) fn camera_to_json(cam: &Camera) -> solarxy_scenefile::CameraJson {
    use cgmath::InnerSpace;
    let offset = cam.eye - cam.target;
    let distance = offset.magnitude().max(1e-4);
    let dir = offset / distance;
    solarxy_scenefile::CameraJson {
        target: [cam.target.x, cam.target.y, cam.target.z],
        yaw: dir.x.atan2(dir.z),
        pitch: dir.y.clamp(-1.0, 1.0).asin(),
        distance,
        fov_y: cam.fovy.to_radians(),
        projection: match cam.projection {
            solarxy_core::preferences::ProjectionMode::Perspective => "perspective",
            solarxy_core::preferences::ProjectionMode::Orthographic => "orthographic",
        }
        .to_string(),
        ortho_scale: cam.ortho_scale,
    }
}

/// One pane's contribution to the saved view.
///
/// A borrowed bundle rather than a method on the shell, so the writer can be
/// driven in a test with cameras built by hand: the shell's own camera
/// bundle needs a device to exist.
pub(super) struct PaneSource<'a> {
    pub camera: Option<&'a Camera>,
    pub settings: &'a PaneDisplaySettings,
    /// The `camera` node this pane looks through, by id.
    pub look_through: Option<u64>,
}

/// The saved view: layout, active pane, divider, and every pane's camera,
/// display settings and binding.
///
/// A pane with no camera writes the default, whose zero distance is what
/// the reader treats as "frame this pane on open" (`apply_scene_view`).
/// The camera lock beside the binding is the write-back this shell does not
/// do, so it is written false.
pub(super) fn view_json(
    layout: ViewLayout,
    active_pane: usize,
    split_ratio: f32,
    panes: [PaneSource<'_>; 4],
) -> solarxy_scenefile::ViewJson {
    let layout = serde_json::to_value(layout)
        .ok()
        .and_then(|v| v.as_str().map(String::from))
        .unwrap_or_else(|| "single".to_string());
    let panes = panes
        .iter()
        .map(|pane| {
            let camera = pane
                .camera
                .map_or_else(solarxy_scenefile::CameraJson::default, camera_to_json);
            let display = serde_json::to_value(pane.settings)
                .ok()
                .and_then(|v| match v {
                    serde_json::Value::Object(map) => Some(map.into_iter().collect()),
                    _ => None,
                })
                .unwrap_or_default();
            solarxy_scenefile::PaneJson {
                camera,
                display,
                look_through: pane.look_through,
                camera_locked: false,
                ..solarxy_scenefile::PaneJson::default()
            }
        })
        .collect();
    solarxy_scenefile::ViewJson {
        layout,
        active_pane: u32::try_from(active_pane).unwrap_or(0),
        split_ratio,
        panes,
    }
}

/// Write `bytes` to `path` through a temporary sibling and a rename, so a
/// failure at any point leaves whatever was at `path` untouched.
pub(super) fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("{SCENE_EXTENSION}.tmp"));
    std::fs::write(&tmp, bytes)?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// A path a save dialog produced, made to end in the scene extension.
///
/// A dialog that lets a user type a bare name hands back one with no
/// extension, and a file that does not end in `.slxy` is one the Open
/// dialog's filter hides from the same user a minute later.
pub(super) fn with_scene_extension(path: PathBuf) -> PathBuf {
    let has = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(SCENE_EXTENSION));
    if has {
        path
    } else {
        let mut name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().to_string());
        name.push('.');
        name.push_str(SCENE_EXTENSION);
        path.with_file_name(name)
    }
}

/// The present moment as the scene file's metadata writes it.
fn now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default()
}

impl State {
    /// Whether the document has changed since it was opened or last written.
    pub(crate) fn is_dirty(&self) -> bool {
        self.engine
            .as_ref()
            .is_some_and(|engine| engine.revision() != self.saved_revision)
    }

    /// Record that what the engine holds now is what the file holds.
    pub(super) fn mark_saved(&mut self) {
        if let Some(engine) = &self.engine {
            self.saved_revision = engine.revision();
        }
    }

    /// Put the document's name and its dirty state in the window title.
    ///
    /// Called once per frame and on every document change; it writes the
    /// window only when the text moved, because the title is a platform
    /// call and the comparison is a string.
    pub(super) fn refresh_title(&mut self) {
        let title = window_title(
            self.engine_scene.as_ref().map(|s| s.filename.as_str()),
            self.is_dirty(),
        );
        if title != self.last_title {
            self.window.set_title(&title);
            self.last_title = title;
        }
    }

    /// The saved view, from this shell's panes.
    fn view_json_now(&self) -> solarxy_scenefile::ViewJson {
        let sources = std::array::from_fn(|i| PaneSource {
            camera: self.view.cameras[i].as_ref().map(|c| &c.camera),
            settings: &self.view.pane_settings[i],
            look_through: self.look_through[i].map(|id| id.0),
        });
        view_json(
            self.view.display.layout,
            self.view.active_pane,
            self.view.display.split_ratio,
            sources,
        )
    }

    /// The saved lighting environment: the image-based lighting mode, the
    /// staged HDRI by content hash, and the scene-wide rotation riding the
    /// free-form background object, which is where the browser puts it.
    fn environment_json(&mut self) -> solarxy_scenefile::EnvironmentJson {
        use solarxy_core::preferences::IblMode;
        let mut background = BTreeMap::new();
        background.insert(
            "hdriRotation".to_string(),
            serde_json::json!(self.view.display.hdri_rotation),
        );
        solarxy_scenefile::EnvironmentJson {
            ibl_mode: match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => "off",
                IblMode::Diffuse => "diffuse",
                IblMode::Full => "full",
            }
            .to_string(),
            hdri_asset: self.ensure_hdri_staged(),
            background,
        }
    }

    /// The staged HDRI's content hash, staging the loaded file first if it
    /// has not been.
    ///
    /// An HDRI imported natively is decoded straight from its path and never
    /// enters the engine's asset table, so a save would name nothing and the
    /// browser would open the scene unlit. Staging happens here, at save
    /// time, rather than at import, because an HDRI loaded before any
    /// document existed has no engine to stage into when it loads.
    fn ensure_hdri_staged(&mut self) -> Option<String> {
        if self.hdri_hash.is_some() {
            return self.hdri_hash.clone();
        }
        self.renderer.ibl_res.ibl.equirect.as_ref()?;
        let path = self.gui.hdri_info().map(|h| h.path.clone())?;
        let bytes = std::fs::read(&path).ok()?;
        let name = Path::new(&path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("environment.hdr")
            .to_string();
        let mime = if name.to_ascii_lowercase().ends_with(".exr") {
            "image/x-exr"
        } else {
            "image/vnd.radiance"
        };
        let id = self.engine.as_mut()?.stage_asset(name, mime, bytes);
        self.hdri_hash = Some(id.0);
        self.hdri_hash.clone()
    }

    /// Everything the scene file carries that is not the document.
    fn scene_sidecar(&mut self) -> SceneSidecar {
        let now = now_rfc3339();
        let (name, created) = self.engine_scene.as_ref().map_or_else(
            || (String::new(), String::new()),
            |s| {
                let stem = Path::new(&s.filename)
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .to_string();
                (stem, s.created.clone())
            },
        );
        let created = if created.is_empty() {
            now.clone()
        } else {
            created
        };
        SceneSidecar {
            generator: format!("solarxy {}", env!("CARGO_PKG_VERSION")),
            view: self.view_json_now(),
            environment: self.environment_json(),
            canvas_viewports: BTreeMap::new(),
            meta: solarxy_scenefile::MetaJson {
                name,
                description: String::new(),
                project_id: String::new(),
                created,
                modified: now,
            },
        }
    }

    /// Save to the document's own path, or ask for one when it has none.
    ///
    /// Returns whether the file was written, which is what the close guard
    /// needs to know before it lets the window go.
    pub fn save_document(&mut self) -> bool {
        let path = match &self.engine_scene {
            None => {
                self.gui
                    .set_toast("Nothing to save", ToastSeverity::Warning);
                return false;
            }
            Some(scene) if scene.path.is_empty() => return self.save_document_as(),
            Some(scene) => PathBuf::from(&scene.path),
        };
        self.write_scene_to(path)
    }

    /// Ask for a path, then save there.
    pub fn save_document_as(&mut self) -> bool {
        let Some(scene) = &self.engine_scene else {
            self.gui
                .set_toast("Nothing to save", ToastSeverity::Warning);
            return false;
        };
        let suggested = if scene.path.is_empty() {
            format!("Untitled.{SCENE_EXTENSION}")
        } else {
            scene.filename.clone()
        };
        let Some(path) = Self::save_scene_dialog(&suggested) else {
            return false;
        };
        self.write_scene_to(with_scene_extension(path))
    }

    /// Write the document and its sidecar to `path`, and adopt the path as
    /// the document's own.
    fn write_scene_to(&mut self, path: PathBuf) -> bool {
        let filename = path
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or("scene")
            .to_string();
        let sidecar = self.scene_sidecar();
        let Some(engine) = &self.engine else {
            return false;
        };
        let bytes = match engine.save_slxy(&sidecar) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.gui.set_toast(
                    &format!("Could not save {filename}: {e}"),
                    ToastSeverity::Error,
                );
                return false;
            }
        };
        if let Err(e) = write_atomically(&path, &bytes) {
            self.gui.set_toast(
                &format!("Could not save {filename}: {e}"),
                ToastSeverity::Error,
            );
            return false;
        }

        let display_path = path.canonicalize().map_or_else(
            |_| path.display().to_string(),
            |p| p.to_string_lossy().to_string(),
        );
        if let Some(scene) = &mut self.engine_scene {
            scene.filename.clone_from(&filename);
            scene.path.clone_from(&display_path);
            scene.file_size = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
            if scene.created.is_empty() {
                scene.created.clone_from(&sidecar.meta.created);
            }
        }
        preferences::add_recent_file(&mut self.preferences, &display_path);
        self.mark_saved();
        self.refresh_title();
        self.gui
            .set_toast(&format!("Saved {filename}"), ToastSeverity::Success);
        true
    }

    /// Replace the document with an empty one.
    ///
    /// Asking first when the current one has unsaved changes is the guard's
    /// job, which routes every discarding action through one prompt; this is
    /// the action itself.
    pub fn new_scene(&mut self) {
        if self.adopt_untitled_document() {
            self.reset_pane_zero_for_new_document();
            self.gui.set_toast("New scene", ToastSeverity::Success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_graph::Command;
    use solarxy_graph::document::GraphContext;
    use solarxy_graph::engine::Engine;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("solarxy-document-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    /// A camera whose every field is unlike the default, so a field the
    /// writer forgets cannot pass by coincidence.
    fn authored_camera() -> Camera {
        let target = cgmath::Point3::new(1.5, -2.0, 3.25);
        let yaw: f32 = 0.7;
        let pitch: f32 = -0.4;
        let dir = cgmath::Vector3::new(
            pitch.cos() * yaw.sin(),
            pitch.sin(),
            pitch.cos() * yaw.cos(),
        );
        Camera {
            eye: target + dir * 6.5,
            target,
            up: solarxy_renderer::camera::turntable_up(yaw, pitch),
            aspect: 1.6,
            fovy: 37.0,
            znear: 0.01,
            zfar: 100.0,
            projection: ProjectionMode::Orthographic,
            ortho_scale: 4.25,
        }
    }

    fn shape(engine: &Engine) -> serde_json::Value {
        serde_json::to_value(engine.document().to_data()).expect("a document serializes")
    }

    /// No document, a clean one, and a dirty one read as three different
    /// titles, and the marker is the only thing that separates the last two.
    #[test]
    fn the_title_names_the_file_and_marks_unsaved_work() {
        assert_eq!(window_title(None, false), "Solarxy");
        assert_eq!(window_title(None, true), "Solarxy");
        assert_eq!(
            window_title(Some("shot.slxy"), false),
            "Solarxy - shot.slxy"
        );
        assert_eq!(
            window_title(Some("shot.slxy"), true),
            "Solarxy - shot.slxy*"
        );
    }

    /// The writer and the reader are one pair: what one puts into the file,
    /// the other gets back, on a camera unlike the default in every field.
    #[test]
    fn a_camera_written_to_the_sidecar_is_read_back_as_it_was() {
        let authored = authored_camera();
        let json = camera_to_json(&authored);
        assert!(
            (json.distance - 6.5).abs() < 1e-4,
            "distance {}",
            json.distance
        );
        assert!((json.yaw - 0.7).abs() < 1e-4, "yaw {}", json.yaw);
        assert!((json.pitch + 0.4).abs() < 1e-4, "pitch {}", json.pitch);
        assert_eq!(json.projection, "orthographic");

        let mut back = Camera {
            projection: ProjectionMode::Perspective,
            ortho_scale: 1.0,
            fovy: 45.0,
            ..authored_camera()
        };
        back.eye = cgmath::Point3::new(0.0, 0.0, 0.0);
        back.target = cgmath::Point3::new(0.0, 0.0, 0.0);
        super::super::open::apply_camera_json(&mut back, &json);

        for axis in 0..3 {
            assert!((back.target[axis] - authored.target[axis]).abs() < 1e-4);
            assert!(
                (back.eye[axis] - authored.eye[axis]).abs() < 1e-3,
                "eye axis {axis}"
            );
        }
        assert!((back.fovy - 37.0).abs() < 1e-3, "fovy {}", back.fovy);
        assert_eq!(back.projection, ProjectionMode::Orthographic);
        assert!((back.ortho_scale - 4.25).abs() < 1e-6);
    }

    /// Every pane reaches the saved view: its camera, its display settings
    /// and its binding, with a pane that has no camera writing the zero
    /// distance the reader frames on.
    #[test]
    fn every_pane_reaches_the_saved_view() {
        let mut cams: Vec<Camera> = (1..=3)
            .map(|i| {
                let mut c = authored_camera();
                c.eye = c.target + (c.eye - c.target) * (i as f32 / 6.5);
                c
            })
            .collect();
        cams[1].projection = ProjectionMode::Perspective;
        let mut settings =
            [PaneDisplaySettings::for_still(solarxy_core::preferences::BackgroundMode::GRADIENT);
                4];
        settings[3].show_grid = !settings[3].show_grid;
        let panes = [
            PaneSource {
                camera: Some(&cams[0]),
                settings: &settings[0],
                look_through: None,
            },
            PaneSource {
                camera: Some(&cams[1]),
                settings: &settings[1],
                look_through: Some(7),
            },
            PaneSource {
                camera: Some(&cams[2]),
                settings: &settings[2],
                look_through: None,
            },
            PaneSource {
                camera: None,
                settings: &settings[3],
                look_through: None,
            },
        ];
        let view = view_json(ViewLayout::Quad, 2, 0.35, panes);

        assert_eq!(view.layout, "quad");
        assert_eq!(view.active_pane, 2);
        assert!((view.split_ratio - 0.35).abs() < 1e-6);
        assert_eq!(view.panes.len(), 4);
        for (i, expected) in [1.0f32, 2.0, 3.0].iter().enumerate() {
            let d = view.panes[i].camera.distance;
            assert!((d - expected).abs() < 1e-4, "pane {i} distance {d}");
        }
        assert_eq!(view.panes[1].camera.projection, "perspective");
        assert_eq!(view.panes[0].camera.projection, "orthographic");
        assert_eq!(view.panes[1].look_through, Some(7));
        assert_eq!(view.panes[0].look_through, None);
        assert!(
            view.panes[3].camera.distance.abs() < f32::EPSILON,
            "a pane with no camera frames on open"
        );
        assert!(
            !view.panes[3].display.is_empty(),
            "display settings ride every pane"
        );
        assert_eq!(
            view.panes[3]
                .display
                .get("showGrid")
                .and_then(serde_json::Value::as_bool),
            Some(settings[3].show_grid)
        );
        assert!(
            !view.panes[1].camera_locked,
            "the lock is the write-back this shell lacks"
        );
    }

    /// An authored document saved with this shell's sidecar reopens as the
    /// same document, with the view it was saved with.
    #[test]
    fn an_authored_document_saves_and_reopens_as_the_same_document() {
        let mut engine = Engine::new().expect("engine");
        let batch = engine
            .apply(Command::AddNode {
                ctx: GraphContext::Root,
                node_type: "sopnet".to_string(),
                position: [10.0, 20.0],
            })
            .expect("the container adds");
        let container = batch
            .events
            .iter()
            .find_map(|ev| match ev {
                solarxy_graph::engine::EngineEvent::NodeAdded { node, .. } => Some(node.id),
                _ => None,
            })
            .expect("a container was added");
        for ty in ["box", "sphere"] {
            engine
                .apply(Command::AddNode {
                    ctx: GraphContext::Subflow(container),
                    node_type: ty.to_string(),
                    position: [10.0, 20.0],
                })
                .expect("the node adds");
        }
        let cam = authored_camera();
        let settings =
            [PaneDisplaySettings::for_still(solarxy_core::preferences::BackgroundMode::GRADIENT);
                4];
        let panes = std::array::from_fn(|i| PaneSource {
            camera: (i == 0).then_some(&cam),
            settings: &settings[i],
            look_through: None,
        });
        let sidecar = SceneSidecar {
            generator: "solarxy test".to_string(),
            view: view_json(ViewLayout::SplitVertical, 1, 0.5, panes),
            ..SceneSidecar::default()
        };

        let bytes = engine.save_slxy(&sidecar).expect("the document saves");
        let mut reopened = Engine::new().expect("engine");
        let loaded = reopened.load_slxy(&bytes).expect("the saved scene opens");

        assert_eq!(shape(&engine), shape(&reopened));
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.sidecar.view.layout, "splitVertical");
        assert!((loaded.sidecar.view.panes[0].camera.distance - 6.5).abs() < 1e-4);
        assert!(loaded.sidecar.view.panes[1].camera.distance.abs() < f32::EPSILON);
    }

    /// A write that cannot finish leaves nothing behind, and one that can
    /// replaces the whole file rather than part of it.
    #[test]
    fn a_failed_write_leaves_no_partial_file_and_a_good_one_replaces_the_whole() {
        let dir = scratch("atomic");
        let missing = dir.join("no-such-dir").join("scene.slxy");
        assert!(write_atomically(&missing, b"abc").is_err());
        assert!(!missing.exists());
        assert!(!missing.with_extension("slxy.tmp").exists());

        // A rename that fails after the temporary sibling exists: the target
        // is a directory, so the sibling is written and the rename is refused.
        let occupied = dir.join("occupied.slxy");
        std::fs::create_dir(&occupied).expect("a directory in the way");
        assert!(write_atomically(&occupied, b"abc").is_err());
        assert!(occupied.is_dir(), "the thing in the way is untouched");
        assert!(
            !occupied.with_extension("slxy.tmp").exists(),
            "a refused rename leaves no temporary sibling behind"
        );

        let path = dir.join("scene.slxy");
        write_atomically(&path, b"first, and longer").expect("first write");
        write_atomically(&path, b"second").expect("second write");
        assert_eq!(std::fs::read(&path).expect("read"), b"second");
        assert!(
            !path.with_extension("slxy.tmp").exists(),
            "the temporary sibling is gone"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dialog result ends in the scene extension whatever the user typed.
    #[test]
    fn a_save_path_always_ends_in_the_scene_extension() {
        assert_eq!(
            with_scene_extension(PathBuf::from("/a/b/shot")),
            PathBuf::from("/a/b/shot.slxy")
        );
        assert_eq!(
            with_scene_extension(PathBuf::from("/a/b/shot.slxy")),
            PathBuf::from("/a/b/shot.slxy")
        );
        assert_eq!(
            with_scene_extension(PathBuf::from("/a/b/shot.SLXY")),
            PathBuf::from("/a/b/shot.SLXY")
        );
        assert_eq!(
            with_scene_extension(PathBuf::from("/a/b/shot.v2")),
            PathBuf::from("/a/b/shot.v2.slxy")
        );
    }
}
