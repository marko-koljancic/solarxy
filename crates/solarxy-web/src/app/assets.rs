//! Asset staging and the worker pumps that parse, validate and decode
//! outside this host's own instance.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    // ---- asset staging + the import-worker pump ----

    /// Stages asset bytes into the engine, returning the content id (its
    /// SHA-256 hex) the import node's `file` param references. The `_sha256`
    /// the caller computed for its OPFS cache is not trusted; the engine
    /// recomputes and the returned id is authoritative.
    pub fn stage_asset(
        &mut self,
        name: String,
        mime: String,
        _sha256: String,
        bytes: Vec<u8>,
    ) -> String {
        self.engine.stage_asset(name, mime, bytes).0
    }

    /// The staged bytes for an asset id, as a `Uint8Array`, or `undefined`.
    /// Lets the frontend feed the worker after a scene load, when its own
    /// JS-side byte cache is cold.
    pub fn asset_bytes(&self, hash: String) -> Option<js_sys::Uint8Array> {
        self.engine
            .asset_bytes(&solarxy_graph::params::AssetId(hash))
            .map(js_sys::Uint8Array::from)
    }

    /// Every staged asset as `[{ hash, name }]`. The sidecar preflight
    /// diffs a model's referenced companions against it, so the check is
    /// authoritative across reloads and `.slxy` restores (a JS-side cache
    /// of staged names would go cold on both).
    pub fn asset_manifest(&self) -> Result<JsValue, JsError> {
        let manifest: Vec<AssetRefDto> = self
            .engine
            .asset_manifest()
            .iter()
            .map(|(h, n)| AssetRefDto {
                hash: h.clone(),
                name: n.clone(),
            })
            .collect();
        to_js(&manifest)
    }

    /// Drains the import jobs the last cook spawned into a JS array of
    /// `{ ctx, jobId, hash, name, format, options, sidecars }`. The frontend
    /// gathers each job's bytes, posts them to the import worker, and returns
    /// the result through `submit_parsed_model` / `submit_parse_error`.
    /// `ValidateGeometry` jobs drained alongside are stashed for
    /// [`SolarxyApp::take_validate_jobs`].
    pub fn take_import_jobs(&mut self) -> Result<JsValue, JsError> {
        let manifest = self.engine.asset_manifest();
        let mut payloads: Vec<ImportJobDto> = Vec::new();
        for (ctx, job, req) in self.engine.take_jobs() {
            let (asset, format, options) = match req {
                JobRequest::ParseModel {
                    asset,
                    format,
                    options,
                } => (asset, format, options),
                JobRequest::ValidateGeometry {
                    geometry,
                    config,
                    budget,
                } => {
                    // Pack the geometry once, at drain time; the frontend
                    // moves plain bytes to the worker.
                    let config_json = serde_json::to_string(&config)
                        .map_err(|e| JsError::new(&format!("serialize config: {e}")))?;
                    self.pending_validate.push(PendingValidateJob {
                        ctx,
                        job_id: job.0,
                        blob: transfer::pack(&geometry),
                        config_json,
                        budget,
                    });
                    continue;
                }
                JobRequest::DecodeImage { asset } => {
                    let name = manifest
                        .iter()
                        .find(|(h, _)| *h == asset.0)
                        .map_or_else(String::new, |(_, n)| n.clone());
                    self.pending_image.push(PendingImageJob {
                        ctx,
                        job_id: job.0,
                        hash: asset.0,
                        name,
                    });
                    continue;
                }
                JobRequest::DecodeHdrImage { asset } => {
                    let name = manifest
                        .iter()
                        .find(|(h, _)| *h == asset.0)
                        .map_or_else(String::new, |(_, n)| n.clone());
                    self.pending_hdri.push(PendingHdriJob {
                        ctx,
                        job_id: job.0,
                        hash: asset.0,
                        name,
                    });
                    continue;
                }
            };
            let name = manifest
                .iter()
                .find(|(h, _)| *h == asset.0)
                .map_or_else(String::new, |(_, n)| n.clone());
            // OBJ/glTF resolve companions (mtl, bin, textures) by name;
            // hand the worker the other staged files as candidate
            // sidecars. Self-contained STL/PLY need none.
            let sidecars = if matches!(format.as_str(), "obj" | "gltf" | "glb") {
                manifest
                    .iter()
                    .filter(|(h, _)| *h != asset.0)
                    .map(|(h, n)| AssetRefDto {
                        hash: h.clone(),
                        name: n.clone(),
                    })
                    .collect()
            } else {
                Vec::new()
            };
            payloads.push(ImportJobDto {
                ctx,
                job_id: job.0 as f64,
                hash: asset.0,
                name,
                format,
                options,
                sidecars,
            });
        }
        to_js(&payloads)
    }

    /// Drains the stashed geometry-validation jobs into a JS array of
    /// `{ ctx, jobId, blob, config, budget }` (`blob` is a `Uint8Array`
    /// transfer blob of the geometry; `config` a JSON `ValidationConfig`).
    /// The frontend posts each to the worker's `validate_geometry_job` and
    /// returns the result through `submit_validation_result` /
    /// `submit_validation_error`. Call after `take_import_jobs` (which
    /// performs the drain from the engine).
    pub fn take_validate_jobs(&mut self) -> Result<JsValue, JsError> {
        let out = js_sys::Array::new();
        for job in self.pending_validate.drain(..) {
            let o = js_sys::Object::new();
            let set = |key: &str, value: &JsValue| {
                js_sys::Reflect::set(&o, &JsValue::from_str(key), value)
                    .map_err(|_| JsError::new("take_validate_jobs: reflect set failed"))
                    .map(|_| ())
            };
            set("ctx", &to_js(&job.ctx)?)?;
            set("jobId", &JsValue::from_f64(job.job_id as f64))?;
            set("blob", &js_sys::Uint8Array::from(job.blob.as_slice()))?;
            set("config", &JsValue::from_str(&job.config_json))?;
            set(
                "budget",
                &job.budget
                    .map_or(JsValue::UNDEFINED, |b| JsValue::from_f64(f64::from(b))),
            )?;
            out.push(&o);
        }
        Ok(out.into())
    }

    /// Drains the stashed image-decode jobs into a JS array of
    /// `{ ctx, jobId, hash, name }`. The frontend pulls the encoded bytes
    /// by hash (`asset_bytes`), posts them to the worker's decode-image
    /// path (`createImageBitmap`), and returns the RGBA result through
    /// `submit_decoded_image` / `submit_image_error`. Call after
    /// `take_import_jobs` (which performs the drain from the engine).
    pub fn take_image_jobs(&mut self) -> Result<JsValue, JsError> {
        let out = js_sys::Array::new();
        for job in self.pending_image.drain(..) {
            let o = js_sys::Object::new();
            let set = |key: &str, value: &JsValue| {
                js_sys::Reflect::set(&o, &JsValue::from_str(key), value)
                    .map_err(|_| JsError::new("take_image_jobs: reflect set failed"))
                    .map(|_| ())
            };
            set("ctx", &to_js(&job.ctx)?)?;
            set("jobId", &JsValue::from_f64(job.job_id as f64))?;
            set("hash", &JsValue::from_str(&job.hash))?;
            set("name", &JsValue::from_str(&job.name))?;
            out.push(&o);
        }
        Ok(out.into())
    }

    /// Drains the stashed HDRI-decode jobs into a JS array of
    /// `{ ctx, jobId, hash, name }`. The frontend pulls the encoded bytes
    /// by hash and posts them to the worker's HDRI path, returning the
    /// prepared result through `submit_decoded_hdri` / `submit_hdri_error`.
    /// Call after `take_import_jobs` (which performs the drain from the
    /// engine).
    pub fn take_hdri_jobs(&mut self) -> Result<JsValue, JsError> {
        let out = js_sys::Array::new();
        for job in self.pending_hdri.drain(..) {
            let o = js_sys::Object::new();
            let set = |key: &str, value: &JsValue| {
                js_sys::Reflect::set(&o, &JsValue::from_str(key), value)
                    .map_err(|_| JsError::new("take_hdri_jobs: reflect set failed"))
                    .map(|_| ())
            };
            set("ctx", &to_js(&job.ctx)?)?;
            set("jobId", &JsValue::from_f64(job.job_id as f64))?;
            set("hash", &JsValue::from_str(&job.hash))?;
            set("name", &JsValue::from_str(&job.name))?;
            out.push(&o);
        }
        Ok(out.into())
    }

    /// Commits a worker-decoded HDRI under the per-node generation guard.
    ///
    /// Takes the packed `PreparedHdri` the worker already produces, which
    /// carries the CPU lighting stages alongside the pixels. That is why
    /// the environment node reuses the existing worker entry point rather
    /// than a lean decode: the irradiance convolution stays off the main
    /// thread, and this call installs the result directly instead of
    /// making the tracker convolve it again on the next delta.
    pub fn submit_decoded_hdri(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        prepared: Vec<u8>,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let prepared = solarxy_renderer::ibl::PreparedHdri::unpack(&prepared)
            .map_err(|e| JsError::new(&format!("bad prepared HDRI: {e}")))?;

        // Install on the GPU now, while the convolved faces are in hand.
        self.renderer.ibl_res.ibl =
            solarxy_renderer::ibl::IblState::from_prepared(&self.device, &self.queue, &prepared);

        // Hand the engine the image itself, so the scene delta can carry
        // it and a save can embed it. The hash is stamped here, Rust-side,
        // by the same constructor the native path uses.
        let image = std::sync::Arc::new(solarxy_core::RawImageHdr::new(
            prepared.pixels,
            prepared.width,
            prepared.height,
        ));
        // Tell the tracker this hash is already live, so the delta that
        // follows sees `Unchanged` rather than convolving it a second time
        // on the main thread.
        self.environment.note_installed(image.hash);
        self.rebuild_light_bind_group();

        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::HdrImage(Ok(image)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker HDRI-decode failure: the `environment` node badges
    /// the error and the viewport keeps the environment it had.
    pub fn submit_hdri_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::HdrImage(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Commits a worker-decoded image (raw RGBA8 plus dimensions) under
    /// the per-node generation guard, returning the cook `EventBatch`.
    /// The content hash is stamped here, Rust-side, so every producer
    /// (native decode, worker decode, transfer unpack) yields identical
    /// hashes for identical pixels.
    pub fn submit_decoded_image(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        width: u32,
        height: u32,
        pixels: Vec<u8>,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let expected = (width as usize) * (height as usize) * 4;
        if pixels.len() != expected {
            return Err(JsError::new(&format!(
                "decoded image is {} bytes, expected {expected} ({width}x{height} RGBA)",
                pixels.len()
            )));
        }
        let image = std::sync::Arc::new(solarxy_core::RawImageData::new(pixels, width, height));
        let events =
            self.engine
                .submit_job_result(ctx, JobId(job_id as u64), JobResult::Image(Ok(image)));
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker image-decode failure: the `import_image` node badges
    /// the error while keep-last-good holds the previous image.
    pub fn submit_image_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Image(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Commits a worker validation result (the JSON `ValidationResult` from
    /// `validate_geometry_job`) under the generation guard, returning the
    /// cook `EventBatch`.
    pub fn submit_validation_result(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        result_json: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let result: ValidationResult = serde_json::from_str(&result_json)
            .map_err(|e| JsError::new(&format!("bad validation result: {e}")))?;
        let events =
            self.engine
                .submit_job_result(ctx, JobId(job_id as u64), JobResult::Report(Ok(result)));
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker validation failure for a job (the validate node
    /// badges the error, keep-last-good holds its previous outputs).
    pub fn submit_validation_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Report(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Commits a worker-parsed model (the transfer blob from
    /// `parse_model_job`, plus its implicit load-validation JSON) under the
    /// per-node generation guard, returning the cook `EventBatch`. A
    /// superseded result is dropped inside the engine.
    pub fn submit_parsed_model(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        blob: Vec<u8>,
        validation_json: Option<String>,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let set =
            transfer::unpack(&blob).map_err(|e| JsError::new(&format!("bad model blob: {e}")))?;
        let validation: Option<ValidationResult> = match validation_json {
            Some(json) => Some(
                serde_json::from_str(&json)
                    .map_err(|e| JsError::new(&format!("bad validation payload: {e}")))?,
            ),
            None => None,
        };
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Model(Ok(ParsedModel { set, validation })),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }

    /// Reports a worker parse failure for a job: the import node badges the
    /// error while keep-last-good holds the last valid geometry.
    pub fn submit_parse_error(
        &mut self,
        ctx: JsValue,
        job_id: f64,
        message: String,
    ) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        let events = self.engine.submit_job_result(
            ctx,
            JobId(job_id as u64),
            JobResult::Model(Err(message)),
        );
        to_js(&EventBatch {
            revision: self.engine.revision(),
            events,
        })
    }
}
