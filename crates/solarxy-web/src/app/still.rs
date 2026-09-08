//! The still render's boundary: starting a job, draining its tiles and
//! previews, and the passes it can hand back.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    /// Starts a still render: `{ width, height, samples, engine, denoise }`.
    ///
    /// `engine` is `"raster"` or `"pathTraced"`. Rejects while one is already
    /// running, because both would want the shared targets at their own tile
    /// size in the same frame.
    ///
    /// The job then advances one chunk per `frame()`, reports itself through
    /// the `renderProgress` host event, and hands finished tiles over through
    /// `take_still_tile`.
    /// What a `render` node is asking for, for the dialog to show before
    /// anything is rendered.
    ///
    /// A pull-read of the same resolver `startStillRender` runs, so the numbers
    /// on the confirmation screen are the numbers the job will use rather than
    /// a second opinion about the same node.
    #[wasm_bindgen(js_name = renderSettings)]
    pub fn render_settings(&self, ctx: JsValue, node: f64) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let settings = self
            .engine
            .render_settings(ctx, NodeId(node as u64))
            .map_err(|e| JsError::new(&e))?;
        to_js(&RenderSettingsDto::from(settings))
    }

    #[wasm_bindgen(js_name = startStillRender)]
    #[allow(clippy::too_many_lines)] // linear job-start sequence; splitting obscures it
    pub fn start_still_render(
        &mut self,
        ctx: JsValue,
        node: f64,
        format: &str,
        space: &str,
    ) -> Result<(), JsError> {
        use solarxy_host::still::{StillEngine, StillRenderJob};

        if self.still.is_some() {
            return Err(JsError::new("a still render is already running"));
        }
        // Started before the tracer is snapshotted and the camera is built,
        // because a person pressing Render is already waiting through those and
        // an elapsed that began at the first tile would be a number that
        // disagreed with the clock on their wall.
        self.still_started_ms = web_now();
        let readback = solarxy_host::still::readback_for(format, space);
        // Resolved here rather than accepted from the caller, and re-resolved
        // rather than carried over from the dialog's read: what runs is what
        // the node says at the moment Render is pressed.
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let opts = self
            .engine
            .render_settings(ctx, NodeId(node as u64))
            .map_err(|e| JsError::new(&e))?;
        let engine = match opts.engine {
            solarxy_graph::nodes::RenderEngine::PathTraced => StillEngine::PathTraced,
            solarxy_graph::nodes::RenderEngine::Raster => StillEngine::Raster,
        };
        if engine == StillEngine::PathTraced {
            if self.tracer.is_none() {
                self.tracer = Some(solarxy_renderer::pathtrace::backend::PathBackend::new(
                    &self.device,
                    &self.queue,
                ));
                // A tracer built after the environment was installed has
                // missed it, and the snapshot below cannot carry it: the
                // scene cache drops that op by design.
                self.traced_env_dirty = true;
            }
            // The document as it stands now, on every start rather than only
            // at construction: the per-frame delta feed goes to the raster
            // backend alone, so a tracer kept from a previous still has seen
            // nothing since, and a construction-only snapshot rendered every
            // later still against the first scene. A full snapshot rather
            // than a delta because deltas since boot are long gone, and cheap
            // to re-apply: unchanged geometry stays a hierarchy-cache hit,
            // and the snapshot-aware apply drops what the document no longer
            // holds.
            let delta = self.engine.scene_snapshot();
            if let Some(t) = self.tracer.as_mut() {
                t.apply_snapshot(&self.device, &self.queue, &delta);
            }
            if let Some(message) = self
                .tracer
                .as_ref()
                .and_then(solarxy_renderer::backend::RenderBackend::skipped_primitives_warning)
            {
                self.host_events.push(HostEvent::RenderNotice { message });
            }
            self.install_still_environment();
            // The pane path owes an install from the moment the still takes
            // the shared backend, or a traced pane resuming afterwards keeps
            // the still's sky. Set here rather than where the job finishes so
            // that a cancelled or failed render restores the panes too.
            self.traced_env_dirty = true;
        }
        if let Some(t) = self.tracer.as_mut() {
            let current = t.settings();
            t.set_settings(crate::trace_settings::trace_settings_for(&opts, current));
            t.set_denoise_settings(crate::trace_settings::denoise_settings_for(&opts));
            t.invalidate();
        }
        // After the settings, which reset the lens to the pinhole default.
        // Resolved before the job's camera is built below so a still and the
        // pane it was launched from cannot disagree about the aperture.
        let lens = self.still_lens(opts.camera);
        if let Some(t) = self.tracer.as_mut() {
            t.set_lens(lens);
        }
        let spec = self.still_spec(&opts, engine, readback);
        // Refused here rather than at save, because the buffer this needs is
        // allocated as the render runs and a refusal after minutes of work is
        // no kindness. The eight-bit path is unaffected and keeps its full
        // range: it holds four bytes a pixel on a canvas the browser owns,
        // while a float save holds twelve in this module's own heap and then
        // encodes from them.
        if readback != solarxy_host::still::StillReadback::Display8 {
            let pixels = u64::from(spec.width) * u64::from(spec.height);
            if pixels > MAX_FLOAT_STILL_PIXELS {
                return Err(JsError::new(&format!(
                    "a floating-point still is limited to {} megapixels and this one is {:.1}; \
                     render it smaller, or take it from the command line, which writes the same \
                     image with no such limit",
                    MAX_FLOAT_STILL_PIXELS / 1_000_000,
                    megapixels(pixels)
                )));
            }
        }
        // The same argument, for the same reason, about a different buffer: the
        // auxiliary planes are held whole because the depth display normalizes
        // over the whole picture's range, so they are allocated up front and a
        // size that cannot hold them has to be refused before the work starts.
        let plane_cost = StillPasses::cost(&spec);
        if plane_cost > MAX_PASS_PLANE_BYTES {
            let fits = MAX_PASS_PLANE_BYTES * u64::from(spec.width) * u64::from(spec.height)
                / plane_cost.max(1);
            return Err(JsError::new(&format!(
                "the auxiliary passes for a still this size need {} MB and the browser keeps at \
                 most {}; render about {:.1} megapixels or fewer with these passes, ask for fewer \
                 of them, or take it from the command line, which writes the same passes with no \
                 such limit",
                plane_cost / (1024 * 1024),
                MAX_PASS_PLANE_BYTES / (1024 * 1024),
                megapixels(fits)
            )));
        }
        self.still_passes = StillPasses::new(&spec);
        self.still_pass_request = [opts.aov_albedo, opts.aov_normal, opts.aov_depth];
        // Read from the constant rather than from a live backend, which is what
        // lets a window know what a render can produce without a device.
        self.still_writes_aovs = match engine {
            StillEngine::PathTraced => PathBackend::CAPS.writes_aovs,
            StillEngine::Raster => solarxy_host::RasterBackend::CAPS.writes_aovs,
        };
        self.still_float = solarxy_host::still::FloatImage::new(
            readback,
            spec.width,
            spec.height,
            spec.transparent,
        );
        // The job's own camera. Built from the named `camera` node when there
        // is one, and otherwise from the active pane's current view copied by
        // value: either way the panes are untouched, which is what makes
        // pressing Render Still not move what you are looking at.
        let mut camera = self.view.cameras[self.view.active_pane]
            .as_ref()
            .map_or_else(
                || {
                    solarxy_renderer::camera::camera_from_bounds(
                        &self.scene_bounds(),
                        #[allow(clippy::cast_precision_loss)]
                        {
                            opts.width as f32 / opts.height.max(1) as f32
                        },
                    )
                },
                |c| c.camera,
            );
        if let Some(node) = opts.camera {
            let id = SceneObjectId(node.0);
            if let Some(def) = self
                .raster
                .scene()
                .cameras()
                .and_then(|cams| cams.iter().find(|c| c.id == id).cloned())
            {
                solarxy_host::cameras::apply_camera_def(&mut camera, &def);
            }
        }
        // The image's aspect, not a pane's: the render's width and height fix
        // the composition, which is what the node's help says they do.
        #[allow(clippy::cast_precision_loss)]
        {
            camera.aspect = opts.width as f32 / opts.height.max(1) as f32;
        }
        if engine == StillEngine::PathTraced {
            self.light_traced_still(&camera);
        }
        self.still_camera = Some(solarxy_renderer::camera_state::CameraState::from_camera(
            &self.device,
            &self.renderer.layouts.camera,
            camera,
        ));
        self.prepare_still_look(opts.camera);

        self.still_tiles.clear();
        self.still_previews.clear();
        self.still = Some(StillRenderJob::new(spec));
        Ok(())
    }

    /// Cancels the running still render, dropping the job and everything it
    /// allocated. Safe to call when nothing is running.
    #[wasm_bindgen(js_name = cancelStillRender)]
    pub fn cancel_still_render(&mut self) {
        self.still = None;
        self.still_camera = None;
        self.still_tiles.clear();
        // Any outstanding preview goes with the job that armed it, which frees
        // its buffer; anything already queued is a look at a render nobody
        // asked to keep.
        self.still_previews.clear();
        // Half-filled planes for the same reason the half-filled image goes:
        // nothing should be able to save an unfinished pass, and they are the
        // largest thing this render allocated.
        self.still_passes = None;
        self.still_pass_request = [false; 3];
        // Dropped with the job: a cancelled render has a half-filled image and
        // nothing should be able to save it, quite apart from the tens of
        // megabytes it would otherwise sit on until the next render.
        self.still_float = None;
        // The next frame renders the panes again, and the frame after that
        // resizes the targets back to the layout.
    }

    /// A finished tile as `{ x, y, width, height, pixels }` (RGBA8), or
    /// `undefined` when none is waiting.
    ///
    /// Tiles cross one at a time and are assembled on the JavaScript side,
    /// which is what keeps a sixty-seven megapixel image out of the wasm heap
    /// and gives the modal its live preview for nothing.
    #[wasm_bindgen(js_name = takeStillTile)]
    pub fn take_still_tile(&mut self) -> JsValue {
        let Some(tile) = self.still_tiles.pop_front() else {
            return JsValue::UNDEFINED;
        };
        // A float tile lands in the image being assembled here and reaches the
        // dialog as eight bits, which is the whole arrangement: the canvas
        // cannot show sixteen bytes a pixel, and the floats have no business
        // crossing the boundary except as the encoded file they end up in.
        // The auxiliary planes land in their own stores, whole, because the
        // depth display normalizes over the whole picture and a plane mapped
        // tile by tile would band at every seam.
        if let Some(p) = self.still_passes.as_mut() {
            p.place(&tile);
        }
        let rgba8 = if let Some(f) = self.still_float.as_mut() {
            f.place(tile.rect, &tile.pixels);
            std::borrow::Cow::Owned(solarxy_host::still::float_to_rgba8(&tile.pixels))
        } else {
            std::borrow::Cow::Borrowed(tile.pixels.as_slice())
        };
        let obj = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
        };
        set("x", &JsValue::from_f64(f64::from(tile.rect.x)));
        set("y", &JsValue::from_f64(f64::from(tile.rect.y)));
        set("width", &JsValue::from_f64(f64::from(tile.rect.width)));
        set("height", &JsValue::from_f64(f64::from(tile.rect.height)));
        set(
            "pixels",
            &JsValue::from(js_sys::Uint8Array::from(rgba8.as_ref())),
        );
        obj.into()
    }

    /// The picture so far as `{ x, y, width, height, pixels }` (RGBA8), or
    /// `undefined` when none is waiting.
    ///
    /// The same shape a finished tile crosses in, so the modal paints both
    /// through one path and needs no second branch. It is deliberately *not*
    /// placed into the float image being assembled: a preview is an unfinished
    /// look at a tile, and the file must only ever contain tiles that finished.
    #[wasm_bindgen(js_name = takeStillPreview)]
    pub fn take_still_preview(&mut self) -> JsValue {
        let Some(preview) = self.still_previews.pop_front() else {
            return JsValue::UNDEFINED;
        };
        let obj = js_sys::Object::new();
        let set = |k: &str, v: &JsValue| {
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), v);
        };
        set("x", &JsValue::from_f64(f64::from(preview.rect.x)));
        set("y", &JsValue::from_f64(f64::from(preview.rect.y)));
        set("width", &JsValue::from_f64(f64::from(preview.rect.width)));
        set("height", &JsValue::from_f64(f64::from(preview.rect.height)));
        set(
            "pixels",
            &JsValue::from(js_sys::Uint8Array::from(preview.pixels.as_slice())),
        );
        obj.into()
    }

    /// Which passes the running render produces, and whether its engine could
    /// produce any at all.
    ///
    /// Two separate answers because a selector says two different things with
    /// them. A pass the render did not ask for is offered and disabled, naming
    /// what would produce it; a render whose engine writes none shows the
    /// beauty alone, because there is no checkbox anywhere that would have
    /// helped. The second is a capability rather than an identity, so the
    /// window never asks which backend is running.
    #[wasm_bindgen(js_name = stillPasses)]
    pub fn still_passes(&self) -> Result<JsValue, JsError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        // Three passes and one capability. They are four answers to four
        // questions, not a flag set with a shape worth extracting.
        #[allow(clippy::struct_excessive_bools)]
        struct PassesDto {
            albedo: bool,
            normal: bool,
            depth: bool,
            engine_writes_aovs: bool,
        }
        to_js(&PassesDto {
            albedo: self.still_pass_request[0],
            normal: self.still_pass_request[1],
            depth: self.still_pass_request[2],
            engine_writes_aovs: self.still_writes_aovs,
        })
    }

    /// One pass as display pixels for the window, RGBA8 over the whole image.
    ///
    /// Mapped here rather than on the other side of the boundary, through the
    /// same functions the terminal's watch window draws with, so the two
    /// surfaces cannot come to look different. Computed on demand rather than
    /// kept: a person looks at one pass at a time, and holding a display copy
    /// of each beside the float planes would double what a render costs to
    /// show something nobody is looking at.
    ///
    /// `undefined` when the render did not produce that pass, which is what a
    /// selector should already have prevented.
    #[wasm_bindgen(js_name = stillPassDisplay)]
    pub fn still_pass_display(&self, pass: &str) -> JsValue {
        let Some(p) = self.still_passes.as_ref() else {
            return JsValue::UNDEFINED;
        };
        let bytes = match pass {
            "albedo" => p
                .aux
                .as_ref()
                .map(|a| solarxy_host::passes::albedo_rgba8(a)),
            "normal" => p
                .aux
                .as_ref()
                .map(|a| solarxy_host::passes::normal_rgba8(a)),
            "depth" => p
                .depth
                .as_ref()
                .map(|d| solarxy_host::passes::depth_rgba8(d)),
            _ => None,
        };
        bytes.map_or(JsValue::UNDEFINED, |b| {
            JsValue::from(js_sys::Uint8Array::from(b.as_slice()))
        })
    }

    /// One pass as the file it is saved as.
    ///
    /// Always a floating-point image, whatever the beauty is: an eight-bit
    /// albedo is a picture of an albedo and an eight-bit normal is useless, so
    /// the command line writes every pass as float and this writes the same
    /// bytes through the same encoders.
    ///
    /// # Errors
    /// The pass not being one this render produced, or the encode failing.
    #[wasm_bindgen(js_name = stillPassFile)]
    pub fn still_pass_file(&self, pass: &str) -> Result<js_sys::Uint8Array, JsError> {
        let Some(p) = self.still_passes.as_ref() else {
            return Err(JsError::new("this render produced no auxiliary passes"));
        };
        let bytes = match pass {
            "albedo" | "normal" => {
                let aux = p
                    .aux
                    .as_ref()
                    .ok_or_else(|| JsError::new("this render produced no auxiliary plane"))?;
                let floats = solarxy_host::passes::floats_of(aux);
                let plane = if pass == "albedo" {
                    solarxy_host::passes::albedo_from_auxiliary(&floats)
                } else {
                    solarxy_host::passes::normal_from_auxiliary(&floats)
                };
                solarxy_formats::export::encode_exr_rgb_bytes(
                    &solarxy_core::geometry::RawImageHdr::new(plane, p.width, p.height),
                )
                .map_err(|e| JsError::new(&format!("encoding the {pass} pass failed: {e}")))?
            }
            "depth" => {
                let depth = p
                    .depth
                    .as_ref()
                    .ok_or_else(|| JsError::new("this render produced no depth pass"))?;
                let floats = solarxy_host::passes::floats_of(depth);
                solarxy_formats::export::encode_exr_depth_bytes(&floats, p.width, p.height)
                    .map_err(|e| JsError::new(&format!("encoding the depth pass failed: {e}")))?
            }
            other => return Err(JsError::new(&format!("{other} is not a pass"))),
        };
        Ok(js_sys::Uint8Array::from(bytes.as_slice()))
    }

    /// Encodes the finished floating-point still and hands back the file.
    ///
    /// The bytes of a real EXR, written by the same encoder the headless
    /// command uses, so there is one implementation of the format and a file
    /// saved here opens identically to one rendered on the command line. The
    /// download itself is the frontend's: this module has no business knowing
    /// about anchors and object URLs.
    ///
    /// Errors rather than returning nothing when there is no float image,
    /// because reaching this without one means the dialog offered a save it
    /// could not honour.
    #[wasm_bindgen(js_name = saveStillExr)]
    pub fn save_still_exr(&self) -> Result<js_sys::Uint8Array, JsError> {
        let Some(f) = self.still_float.as_ref() else {
            return Err(JsError::new(
                "this still was not rendered as a floating-point image",
            ));
        };
        // A matte still goes through the four-channel writer, which
        // premultiplies on the way out; an opaque one keeps its three
        // channels and its reasoning.
        let bytes = if f.has_matte() {
            solarxy_formats::export::encode_exr_rgba_bytes(f.rgba(), f.width(), f.height())
        } else {
            let img =
                solarxy_core::geometry::RawImageHdr::new(f.rgb().to_vec(), f.width(), f.height());
            solarxy_formats::export::encode_exr_rgb_bytes(&img)
        }
        .map_err(|e| JsError::new(&format!("the image could not be encoded: {e}")))?;
        Ok(js_sys::Uint8Array::from(bytes.as_slice()))
    }

    /// PNG-encodes an assembled RGBA8 still through the same encoder the
    /// command line writes with.
    ///
    /// Exists for the transparent render: a canvas stores its backing
    /// premultiplied, so `toBlob` on one round-trips straight alpha through a
    /// multiply and a divide and corrupts every partially covered pixel's
    /// colour. The window keeps a pristine copy of the finished tiles beside
    /// the canvas and hands it here, so the browser's file and the command
    /// line's carry the same values for the same scene. Stateless on purpose:
    /// the bytes cross once, at save time, and nothing is retained.
    #[wasm_bindgen(js_name = encodeStillPng)]
    pub fn encode_still_png(
        &self,
        pixels: &[u8],
        width: u32,
        height: u32,
    ) -> Result<js_sys::Uint8Array, JsError> {
        if pixels.len() != (width as usize) * (height as usize) * 4 {
            return Err(JsError::new("the buffer does not match the stated size"));
        }
        let bytes = solarxy_formats::export::encode_png_bytes(&solarxy_core::RawImageData::new(
            pixels.to_vec(),
            width,
            height,
        ))
        .map_err(|e| JsError::new(&format!("the image could not be encoded: {e}")))?;
        Ok(js_sys::Uint8Array::from(bytes.as_slice()))
    }

    /// Whether the running or finished still can be saved as a float image,
    /// and in which space, so the dialog labels its own buttons from the
    /// render rather than from what it asked for.
    #[wasm_bindgen(js_name = stillFloatSpace)]
    #[must_use]
    pub fn still_float_space(&self) -> Option<String> {
        self.still_float.as_ref().map(|f| {
            if f.is_scene_linear() {
                "sceneLinear".to_string()
            } else {
                "display".to_string()
            }
        })
    }

    /// Advances the running job by one chunk and reports where it got to.
    pub(super) fn pump_still_render(&mut self) {
        use solarxy_host::still::{StillEngine, StillStep};

        let Some(mut job) = self.still.take() else {
            return;
        };
        let Some(tile) = job.current() else {
            self.still = None;
            return;
        };
        // The shell's half of the arrangement: the job renders into the shared
        // targets and does not resize them, because the two shells resize with
        // different policy around the same body.
        self.set_target_dims(tile.render.width, tile.render.height);

        // A delivered still is a photograph of the scene rather than a
        // screenshot of the pane it was launched from, so it is drawn with the
        // still view rather than with whatever the pane happens to be showing.
        // Before this, a rasterized still carried the pane's grid and gizmo
        // into the saved image, and a terminal render had no pane to inherit
        // from and so could not have matched it anyway.
        //
        // The background is the exception and rides along from the pane: a
        // scene shot against an authored sky should keep it.
        let pds = solarxy_core::view_config::PaneDisplaySettings::for_still(
            self.view.pane_settings[self.view.active_pane].background_mode,
        );
        let display = self.view.display;
        let background = self.resolve_background(&pds);
        let bounds = self.scene_bounds();
        let look = self.still_look;
        let scene_present = self.raster.scene().draw_objects().next().is_some();
        let engine = job.spec().engine;
        let format = self.render_format;

        let step = {
            let Some(camera) = self.still_camera.as_mut() else {
                self.still = None;
                return;
            };
            let mut ctx = solarxy_host::StillCtx {
                device: &self.device,
                queue: &self.queue,
                renderer: &mut self.renderer,
                camera,
                env: &self.env,
                pds: &pds,
                display: &display,
                background,
                bounds: Some(&bounds),
                look,
                format,
                scene_present,
                // The page's own timer, which is the only clock this shell has
                // and the same one the cook budget is measured against.
                now_ms: web_now() as u64,
            };
            match engine {
                StillEngine::Raster => job.advance(&mut ctx, &mut self.raster),
                StillEngine::PathTraced => match self.tracer.as_mut() {
                    Some(t) => job.advance(&mut ctx, t),
                    None => StillStep::Failed,
                },
            }
        };

        if step == StillStep::Tile {
            while let Some(t) = job.take_tile() {
                self.still_tiles.push_back(t);
            }
        }
        // Drained whichever step came back: the job clears anything it holds
        // when the tile it described finishes, so what is here is always newer
        // than the last thing painted and never survives the tile it belongs to.
        if let Some(p) = job.take_preview() {
            self.still_previews.push_back(p);
        }
        let progress = job.progress();
        let done =
            matches!(step, StillStep::Done | StillStep::Failed) || progress.tile >= progress.tiles;
        let elapsed_ms = (web_now() - self.still_started_ms).max(0.0);
        // The same estimator the terminal reads, over the same area-weighted
        // counts, so the two surfaces cannot answer this differently. Nothing
        // is reported once the sampling is over: the job is still assembling,
        // and a zero would say it was finished.
        let remaining_ms = if done {
            None
        } else {
            solarxy_host::still::estimate_remaining_ms(
                progress.drawn,
                progress.total,
                elapsed_ms as u64,
            )
            .map(|ms| ms as f64)
        };
        self.host_events.push(HostEvent::RenderProgress {
            tile: progress.tile,
            tiles: progress.tiles,
            sample: progress.sample,
            samples: progress.samples,
            done,
            elapsed_ms,
            remaining_ms,
        });
        if done {
            self.still = None;
            self.still_camera = None;
        } else {
            self.still = Some(job);
        }
    }
}
