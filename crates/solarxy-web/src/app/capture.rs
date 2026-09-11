//! Screenshots and turntable frames: rendered offscreen, polled rather
//! than waited on.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    /// Requests a screenshot of the active pane, rendered offscreen at the
    /// given resolution at the end of the current frame. One capture at a
    /// time; poll with [`SolarxyApp::poll_screenshot`].
    pub fn request_screenshot(&mut self, opts: JsValue) -> Result<(), JsError> {
        // The capture resizes the shared MSAA HDR chain to capture
        // resolution for one frame, so VRAM cost is ~4x the pixel count.
        // Empirically (Chrome/Apple GPU) captures in the 7-8M px range
        // lose the device NONDETERMINISTICALLY, and web wgpu has no
        // device-loss recovery yet, so the budget stays far below the
        // failure zone: a modest supersample of typical panes. True
        // hi-res needs tiled off-axis capture (logged follow-up); the
        // result reports the effective size either way.
        const MAX_CAPTURE_PIXELS: u64 = 4_000_000;
        if self.screenshot_request.is_some() || self.pending_screenshot.is_some() {
            return Err(JsError::new("a screenshot is already in flight"));
        }
        let mut opts: ScreenshotOptsDto = serde_wasm_bindgen::from_value(opts)
            .map_err(|e| JsError::new(&format!("bad opts: {e}")))?;
        let max = self.device.limits().max_texture_dimension_2d;
        opts.width = opts.width.clamp(16, max);
        opts.height = opts.height.clamp(16, max);
        let pixels = u64::from(opts.width) * u64::from(opts.height);
        if pixels > MAX_CAPTURE_PIXELS {
            #[allow(clippy::cast_precision_loss)]
            let scale = ((MAX_CAPTURE_PIXELS as f64) / (pixels as f64)).sqrt();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                opts.width = ((f64::from(opts.width) * scale) as u32).max(16);
                opts.height = ((f64::from(opts.height) * scale) as u32).max(16);
            }
        }
        self.screenshot_request = Some(opts);
        Ok(())
    }

    /// Requests one turntable-export frame: pane `pane` rendered offscreen from
    /// its render-through camera rotated by `azimuth_deg`, at the given opts.
    /// Uses the same single capture slot as the screenshot; the frontend drives
    /// one azimuth at a time (poll with `poll_screenshot`). Deterministic: it
    /// renders a rotated clone, never disturbing the live view.
    pub fn request_turntable_frame(
        &mut self,
        pane: usize,
        azimuth_deg: f32,
        opts: JsValue,
    ) -> Result<(), JsError> {
        const MAX_CAPTURE_PIXELS: u64 = 4_000_000;
        if self.screenshot_request.is_some()
            || self.turntable_request.is_some()
            || self.pending_screenshot.is_some()
        {
            return Err(JsError::new("a capture is already in flight"));
        }
        let mut opts: ScreenshotOptsDto = serde_wasm_bindgen::from_value(opts)
            .map_err(|e| JsError::new(&format!("bad opts: {e}")))?;
        let max = self.device.limits().max_texture_dimension_2d;
        opts.width = opts.width.clamp(16, max);
        opts.height = opts.height.clamp(16, max);
        let pixels = u64::from(opts.width) * u64::from(opts.height);
        if pixels > MAX_CAPTURE_PIXELS {
            #[allow(clippy::cast_precision_loss)]
            let scale = ((MAX_CAPTURE_PIXELS as f64) / (pixels as f64)).sqrt();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            {
                opts.width = ((f64::from(opts.width) * scale) as u32).max(16);
                opts.height = ((f64::from(opts.height) * scale) as u32).max(16);
            }
        }
        self.turntable_request = Some((pane.min(3), azimuth_deg, opts));
        Ok(())
    }

    /// Polls the in-flight capture. `undefined` while pending (or when no
    /// capture is in flight); on completion returns
    /// `{ width, height, pixels: Uint8Array }` (tightly-packed RGBA8).
    pub fn poll_screenshot(&mut self) -> Result<JsValue, JsError> {
        use solarxy_renderer::capture::CapturePoll;
        let Some(pending) = &self.pending_screenshot else {
            return Ok(JsValue::UNDEFINED);
        };
        match pending.poll(&self.device, self.render_format) {
            CapturePoll::Pending => Ok(JsValue::UNDEFINED),
            CapturePoll::Failed => {
                self.pending_screenshot = None;
                Err(JsError::new("screenshot readback failed"))
            }
            CapturePoll::Ready(pixels) => {
                let (width, height) = (pending.width, pending.height);
                self.pending_screenshot = None;
                let obj = js_sys::Object::new();
                let set = |k: &str, v: &JsValue| {
                    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
                };
                set("width", &JsValue::from_f64(f64::from(width)));
                set("height", &JsValue::from_f64(f64::from(height)));
                set(
                    "pixels",
                    &JsValue::from(js_sys::Uint8Array::from(pixels.as_slice())),
                );
                Ok(obj.into())
            }
        }
    }

    /// The displayed image of a texture network, for the
    /// texture viewer pane: `{ width, height, pixels }` (RGBA8) or
    /// `undefined` when the network publishes nothing. The pixel copy is
    /// display-only and pull-based, so cooked images still never ride the
    /// event stream; the viewer fetches on cook changes.
    pub fn texture_preview(&self, owner: f64) -> JsValue {
        let Some(img) = self
            .engine
            .display_image(solarxy_graph::document::NodeId(owner as u64))
        else {
            return JsValue::UNDEFINED;
        };
        let obj = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
        };
        set("width", &JsValue::from_f64(f64::from(img.width)));
        set("height", &JsValue::from_f64(f64::from(img.height)));
        set(
            "pixels",
            &JsValue::from(js_sys::Uint8ClampedArray::from(img.pixels.as_slice())),
        );
        obj.into()
    }

    /// Executes an export node's Action param: the engine
    /// encodes the committed output, and the returned
    /// `{ filename, mime, bytes }` goes to the frontend's save path (the
    /// File System Access flow `.slxy` already uses).
    pub fn invoke_action(&self, ctx: JsValue, node: f64, key: String) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let result = self
            .engine
            .invoke_action(ctx, NodeId(node as u64), &key)
            .map_err(|e| JsError::new(&e.to_string()))?;
        let obj = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
        };
        set("filename", &JsValue::from_str(&result.filename));
        set("mime", &JsValue::from_str(&result.mime));
        set(
            "bytes",
            &JsValue::from(js_sys::Uint8Array::from(result.bytes.as_slice())),
        );
        Ok(obj.into())
    }

    /// Renders the active pane offscreen at capture resolution and encodes
    /// the readback copy. The pane's display settings are copied with the
    /// requested overlay toggles applied; the composite always clears (a
    /// fresh texture has no prior pane to load).
    pub(super) fn render_screenshot(&mut self, opts: &ScreenshotOptsDto) {
        let target = CaptureTarget::new(&self.device, self.render_format, opts.width, opts.height);
        let (w, h, full) = (target.width, target.height, target.rect);
        self.set_target_dims(w, h);
        let pane_idx = self.view.active_pane;
        let mut pds = self.view.pane_settings[pane_idx];
        if !opts.overlays.grid {
            pds.show_grid = false;
        }
        if !opts.overlays.axes {
            pds.show_axis_gizmo = false;
            pds.show_local_axes = false;
        }
        if !opts.overlays.validation {
            pds.show_validation = false;
        }
        // Markers stay out of a saved image. They are an aiming aid rather
        // than something the picture is of, and the person saving a frame is
        // framing a shot.
        pds.show_light_markers = false;
        // So does the manipulator, which is transient tool state rather than
        // anything the viewport was configured to show, and which would come
        // out at the previous pane's scale besides. The camera and light
        // helpers deliberately stay: those are switched on per node and behave
        // like the grid toggle, so a screenshot of the viewport keeps them.
        self.renderer.set_manipulator(None);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Screenshot Encoder"),
            });
        let cam_data = self.view.cameras[pane_idx].as_ref().map(|c| c.camera);
        let is_uv_map = pds.pane_mode == PaneMode::UvMap;
        let background = self.resolve_background(&pds);
        let bounds = self.scene_bounds();
        let look = self.pane_look(pane_idx);
        let grid_plane = self.view.cameras[pane_idx]
            .as_ref()
            .map(|c| grid_plane_for(&c.destination_camera()));

        let content = match cam_data {
            None => PaneContent::Empty,
            Some(_) if is_uv_map => PaneContent::Uv {
                source: if self.uv_use_preview {
                    self.uv_scene
                        .draw_object(UV_PREVIEW_ID)
                        .map_or(UvSource::None, UvSource::External)
                } else {
                    UvSource::Scene {
                        preferred: self.selected_object,
                    }
                },
            },
            Some(cam_data) => PaneContent::Scene {
                selected: self.selected_object,
                cam_data,
                // A capture is one pane on its own, so it owns the shadow map
                // the way pane 0 does in the frame loop.
                shadow: true,
            },
        };

        let capture_scene_present = self.raster.scene().draw_objects().next().is_some();
        let hdr_target = self.renderer.targets.hdr_resolve_view.clone();
        let out = self.raster.encode(
            &mut FrameCtx {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
                encoder: &mut encoder,
                index: 0,
                rect: full,
                is_split: false,
                pds: &pds,
                display: &self.view.display,
                background,
                camera: self.view.cameras[pane_idx].as_mut(),
                env: &self.env,
                bounds: Some(&bounds),
                grid_plane,
                look,
                scene_present: capture_scene_present,
                // A capture never carries the selection rim.
                outline: false,
                // A screenshot is the whole picture in one pass, capped at four
                // megapixels. Rendering one larger than that is the still job's,
                // and windowing is how it does it.
                window: None,
                content,
            },
            &hdr_target,
        );
        // Read back at slot 0, not at `pane_idx`: the context above encoded as
        // pane 0, because a capture composites as the pane that clears. It
        // overwrites what pane 0 recorded during the last frame, which is
        // harmless only because a capture runs outside the frame loop and the
        // next frame re-encodes every pane before compositing any of them.
        let pass = self.raster.encoded(0).unwrap_or(solarxy_host::EncodedPane {
            is_uv_map: false,
            scene_present: false,
        });
        debug_assert!(matches!(
            out,
            solarxy_renderer::backend::FrameOutcome::Complete
        ));

        self.finish_capture(encoder, &target, pane_idx, pds.inspection_mode, pass);
    }

    /// Composite an encoded capture into its offscreen target and arm the
    /// readback.
    ///
    /// Deliberately not `composite_and_submit`: a capture always clears, uses
    /// a full-rect viewport rather than a pane rect, and carries no selection
    /// rim. Those three are the whole difference between this and the frame
    /// loop's tail, and everything above it is now shared.
    pub(super) fn finish_capture(
        &mut self,
        mut encoder: wgpu::CommandEncoder,
        target: &CaptureTarget,
        pane_idx: usize,
        inspection: InspectionMode,
        out: solarxy_host::EncodedPane,
    ) {
        let bloom = self.renderer.post.bloom_enabled && !out.is_uv_map && out.scene_present;
        let ssao = self.renderer.post.ssao_enabled && !out.is_uv_map && out.scene_present;
        // A capture runs outside the frame loop, so the slots still hold
        // whatever the last pane drawn happened to bind. Without this, a
        // screenshot of one camera could carry another camera's tables.
        self.bind_pane_luts(pane_idx);
        self.renderer.post.composite.write_params(
            &self.queue,
            bloom,
            ssao,
            &self.pane_look(pane_idx),
            &self.renderer.post.luts,
            inspection,
            false,
        );
        let rect = target.rect;
        self.renderer.post.composite.render(
            &mut encoder,
            &self.renderer.pipelines,
            &target.view,
            ssao,
            &self.renderer.post.ssao,
            Some([rect.x, rect.y, rect.width, rect.height]),
            true,
            None,
        );
        self.queue.submit(std::iter::once(encoder.finish()));

        // The readback copy rides its own submission after the composite.
        let mut copy_encoder =
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Screenshot Copy Encoder"),
                });
        let (buffer, padded) = solarxy_renderer::capture::encode_capture(
            &self.device,
            &mut copy_encoder,
            &target.texture,
            (0, 0, target.width, target.height),
        );
        self.queue.submit(std::iter::once(copy_encoder.finish()));
        self.pending_screenshot = Some(solarxy_renderer::capture::PendingCapture::arm(
            buffer,
            padded,
            target.width,
            target.height,
        ));
    }

    /// The camera a pane renders through: its bound camera's saved `CameraDef`
    /// if any, else its scratch camera. The base pose for a turntable sweep.
    pub(super) fn render_through_camera(&self, pane: usize) -> Option<Camera> {
        let scratch = self
            .view
            .cameras
            .get(pane)
            .and_then(|c| c.as_ref())
            .map(|c| c.camera)?;
        if let Some(node) = self.look_through.get(pane).copied().flatten()
            && let Some(def) = self
                .raster
                .scene()
                .cameras()
                .and_then(|cams| cams.iter().find(|c| c.id == SceneObjectId(node.0)))
        {
            let mut cam = scratch;
            solarxy_host::cameras::apply_camera_def(&mut cam, def);
            return Some(cam);
        }
        Some(scratch)
    }

    /// Renders one turntable frame: the render-through camera rotated by
    /// `azimuth_deg`, offscreen at capture resolution, into the capture slot.
    /// The pane's live camera is swapped in and restored within this call, so a
    /// deterministic sweep never depends on or disturbs the live view / follow.
    pub(super) fn render_turntable_frame(
        &mut self,
        pane: usize,
        azimuth_deg: f32,
        opts: &ScreenshotOptsDto,
    ) {
        let Some(mut cam) = self.render_through_camera(pane) else {
            return;
        };
        solarxy_host::preview::orbit_yaw(&mut cam, azimuth_deg.to_radians());
        let saved = self.view.cameras[pane].as_ref().map(|c| c.camera);
        if let Some(cs) = self.view.cameras[pane].as_mut() {
            cs.camera = cam;
        }
        let prev_active = self.view.active_pane;
        self.view.active_pane = pane;
        self.render_screenshot(opts);
        self.view.active_pane = prev_active;
        if let (Some(saved), Some(cs)) = (saved, self.view.cameras[pane].as_mut()) {
            cs.camera = saved;
        }
    }
}
