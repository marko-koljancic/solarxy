//! The asset preview on the desktop: the model half.
//!
//! The scene, the camera and the render are [`solarxy_host::preview`],
//! shared with the browser. What is this shell's is the target and the
//! parse. The browser draws into a second canvas's surface; this shell
//! renders into a texture of its own and hands egui a handle to it, so the
//! preview is drawn inside a dock tab with no readback and no second
//! device, and the main viewport's device and surface are never touched.
//! The parse runs on a worker thread, the way a model opens, because it
//! blocks for as long as the file is large.
//!
//! **Rendered on demand, never in the frame loop.** A render happens when
//! the parse lands, when the tab's size changes, and on every orbit and
//! dolly, and each borrows the shared render chain at the preview's size;
//! the next frame's target sync puts it back, as it does after a
//! screenshot. While a still owns the targets the render is owed rather
//! than run, and runs when the still is done.

use std::sync::mpsc;

use solarxy_graph::cook::ImportOptions;
use solarxy_graph::params::AssetId;
use solarxy_host::preview::{PreviewScene, preview_pane_settings};
use solarxy_host::preview::GeometrySet;
use solarxy_studio::assets::{AssetKind, asset_kind};

use super::State;
use crate::gui::ToastSeverity;

/// One gesture on the preview, in physical pixels and wheel units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum PreviewGesture {
    Orbit(f32, f32),
    /// Positive moves in.
    Zoom(f32),
}

/// The size a scene is built at before the panel has reported its own.
const DEFAULT_SIZE: (u32, u32) = (512, 512);

/// The model preview's resources and bookkeeping.
#[derive(Default)]
pub(crate) struct PreviewState {
    scene: Option<PreviewScene>,
    texture: Option<wgpu::Texture>,
    texture_id: Option<egui::TextureId>,
    pending: Option<mpsc::Receiver<Result<GeometrySet, String>>>,
    error: Option<String>,
    /// The size the panel last drew at, in physical pixels.
    size: (u32, u32),
    /// A render owed: something moved, or a still held the targets.
    dirty: bool,
}

impl PreviewState {
    /// The handle the panel draws, and the size it was rendered at.
    pub(crate) fn texture(&self) -> Option<(egui::TextureId, [u32; 2])> {
        let id = self.texture_id?;
        let (w, h) = self.scene.as_ref()?.size();
        Some((id, [w, h]))
    }

    pub(crate) fn loading(&self) -> bool {
        self.pending.is_some()
    }

    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl State {
    /// Preview one asset: bring the tab up, and for a model start its parse.
    ///
    /// An image previews inside the panel itself, decoded there; nothing
    /// else previews at all, and the panel says so.
    pub(super) fn open_asset_preview(&mut self, hash: String, name: String) {
        self.gui.open_asset_preview(hash.clone(), name.clone());
        self.close_model_preview();
        if asset_kind(&name) != AssetKind::Model {
            return;
        }
        let Some(engine) = &self.engine else {
            return;
        };
        let Some(bytes) = engine.asset_bytes(&AssetId(hash)).map(<[u8]>::to_vec) else {
            self.preview.error = Some("asset is not staged".to_string());
            return;
        };
        // The table travels for the companions a model names beside
        // itself; its entries share their bytes, so the clone is cheap.
        let table = engine.asset_table().clone();
        let format = name
            .rsplit('.')
            .next()
            .map(str::to_lowercase)
            .unwrap_or_default();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let options = ImportOptions {
                scale: 1.0,
                center_to_origin: false,
                recompute_normals: None,
                preserve_materials: None,
                vertex_colors: None,
            };
            let _ = tx.send(solarxy_graph::nodes::parse_model(
                &format, &bytes, &name, &table, &options,
            ));
        });
        self.preview.pending = Some(rx);
    }

    /// Release the scene, the texture and egui's handle to it.
    pub(super) fn close_model_preview(&mut self) {
        self.preview.scene = None;
        self.preview.texture = None;
        if let Some(id) = self.preview.texture_id.take() {
            self.gui.free_native_texture(id);
        }
        self.preview.pending = None;
        self.preview.error = None;
        self.preview.dirty = false;
    }

    /// Once per frame: release a preview whose tab closed, take a landed
    /// parse, follow the panel's size, and run a render that is owed.
    pub(super) fn poll_preview(&mut self) {
        let mounted = self.gui.asset_preview_tab_present();
        if !mounted {
            if self.preview.scene.is_some() || self.preview.pending.is_some() {
                self.close_model_preview();
            }
            return;
        }
        if let Some((w, h)) = self.gui.preview_size()
            && (w, h) != self.preview.size
        {
            self.preview.size = (w, h);
            if let Some(scene) = &mut self.preview.scene
                && scene.resize(w, h)
            {
                self.preview.dirty = true;
            }
        }
        if let Some(rx) = &self.preview.pending {
            match rx.try_recv() {
                Ok(Ok(set)) => {
                    self.preview.pending = None;
                    let (w, h) = if self.preview.size == (0, 0) {
                        DEFAULT_SIZE
                    } else {
                        self.preview.size
                    };
                    match PreviewScene::new(
                        &self.device,
                        &self.queue,
                        &self.renderer.layouts,
                        &set,
                        w,
                        h,
                    ) {
                        Ok(scene) => {
                            self.preview.scene = Some(scene);
                            self.preview.dirty = true;
                        }
                        Err(e) => self.preview.error = Some(format!("preview upload: {e}")),
                    }
                }
                Ok(Err(e)) => {
                    self.preview.pending = None;
                    self.gui
                        .set_toast(&format!("Model preview failed: {e}"), ToastSeverity::Error);
                    self.preview.error = Some(e);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.preview.pending = None;
                    self.preview.error = Some("the preview parse stopped".to_string());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.preview.dirty {
            self.render_preview();
        }
    }

    /// One orbit or dolly, and the render it asks for.
    pub(super) fn preview_gesture(&mut self, gesture: PreviewGesture) {
        let Some(scene) = &mut self.preview.scene else {
            return;
        };
        match gesture {
            PreviewGesture::Orbit(dx, dy) => scene.orbit(dx, dy),
            PreviewGesture::Zoom(delta) => scene.zoom(delta),
        }
        self.preview.dirty = true;
        self.render_preview();
    }

    /// Render into the preview's own texture, making or remaking it at the
    /// scene's size first and keeping egui's handle current.
    fn render_preview(&mut self) {
        if self.still.is_some() {
            // Owed rather than run: the still owns the shared targets.
            return;
        }
        let background = self.resolve_background(&preview_pane_settings());
        let Some(scene) = self.preview.scene.as_mut() else {
            self.preview.dirty = false;
            return;
        };
        let (w, h) = scene.size();
        let stale = self
            .preview
            .texture
            .as_ref()
            .is_none_or(|t| t.width() != w || t.height() != h);
        if stale {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("Asset Preview"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                // The composite was built for the surface's format, and
                // egui samples any filterable format.
                format: self.config.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            match self.preview.texture_id {
                Some(id) => self.gui.update_native_texture(&self.device, &view, id),
                None => {
                    self.preview.texture_id =
                        Some(self.gui.register_native_texture(&self.device, &view));
                }
            }
            self.preview.texture = Some(texture);
        }
        let Some(texture) = &self.preview.texture else {
            return;
        };
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        scene.render(
            &mut self.renderer,
            &self.env,
            &self.device,
            &self.queue,
            &view,
            background,
        );
        self.preview.dirty = false;
    }
}
