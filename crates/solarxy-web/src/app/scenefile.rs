//! Reading and writing a scene, and the view sidecar that travels with it.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    // ---- .slxy save / load ----

    /// Builds `.slxy` archive bytes from the current document, its referenced
    /// assets, and the host `extra` (generator, canvas viewports, meta). The
    /// full view state (layout, split, all four pane cameras + display
    /// settings) rides the sidecar.
    /// Installs a prepared HDRI environment: unpacks the worker's
    /// `prepare_hdri_job` blob, finishes the IBL on the GPU, points the
    /// skybox at the new equirect, and rebuilds the light bind group (the
    /// desktop `rebuild_light_bind_group` chokepoint). `hash`/`name`
    /// identify the staged HDRI asset for the `.slxy` environment section.
    pub fn set_environment_prepared(
        &mut self,
        hash: String,
        name: String,
        prepared: Vec<u8>,
    ) -> Result<(), JsError> {
        let prepared = solarxy_renderer::ibl::PreparedHdri::unpack(&prepared)
            .map_err(|e| JsError::new(&format!("bad prepared HDRI: {e}")))?;
        self.renderer.ibl_res.ibl =
            solarxy_renderer::ibl::IblState::from_prepared(&self.device, &self.queue, &prepared);
        self.hdri = Some(HdriMeta { hash, name });
        self.environment.invalidate();
        self.traced_env_dirty = true;
        self.rebuild_light_bind_group();
        Ok(())
    }

    /// Clears the HDRI back to the procedural sky derived from the primary
    /// pane's background (the desktop clear-HDRI behavior).
    pub fn clear_environment(&mut self) {
        let (top, bottom) = self
            .resolve_background(&self.view.pane_settings[0])
            .sky_colors();
        self.renderer.ibl_res.ibl = solarxy_renderer::ibl::IblState::from_sky_colors(
            &self.device,
            &self.queue,
            top,
            bottom,
        );
        self.hdri = None;
        self.environment.invalidate();
        self.traced_env_dirty = true;
        self.rebuild_light_bind_group();
    }

    /// Sets the IBL contribution mode (`"off"` / `"diffuse"` / `"full"`).
    pub fn set_ibl_mode(&mut self, mode: String) {
        self.renderer.ibl_res.ibl_mode = match mode.to_ascii_lowercase().as_str() {
            "off" => IblMode::Off,
            "diffuse" => IblMode::Diffuse,
            _ => IblMode::Full,
        };
        self.rebuild_light_bind_group();
    }

    /// The current environment as a DTO (`{ iblMode, hdriHash, hdriName }`)
    /// for the frontend panel.
    pub fn environment_state(&self) -> Result<JsValue, JsError> {
        to_js(&EnvironmentDto {
            ibl_mode: match self.renderer.ibl_res.ibl_mode {
                IblMode::Off => "off",
                IblMode::Diffuse => "diffuse",
                IblMode::Full => "full",
            }
            .to_string(),
            hdri_hash: self.hdri.as_ref().map(|h| h.hash.clone()),
            hdri_name: self.hdri.as_ref().map(|h| h.name.clone()),
            from_node: self.engine.has_environment_node(),
        })
    }

    pub fn save_slxy(&self, extra: JsValue) -> Result<Vec<u8>, JsError> {
        let extra: SaveExtra = serde_wasm_bindgen::from_value(extra).unwrap_or_default();
        let mut sidecar = SceneSidecar {
            generator: if extra.generator.is_empty() {
                "solarxy-web".to_string()
            } else {
                extra.generator
            },
            ..SceneSidecar::default()
        };
        sidecar.view = self.view_json();
        sidecar.environment = self.environment_json();
        sidecar.canvas_viewports = extra
            .canvas_viewports
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();
        sidecar.meta = extra.meta.into();
        self.engine
            .save_slxy(&sidecar)
            .map_err(|e| JsError::new(&format!("save .slxy: {e}")))
    }

    /// Replaces the document from `.slxy` bytes: stages the embedded assets,
    /// applies the saved view state (layout, split, pane cameras + display
    /// settings), and returns `{ batch, warnings, canvasViewports, meta }`
    /// for the mirror and the frontend view state.
    pub fn load_slxy(&mut self, bytes: Vec<u8>) -> Result<JsValue, JsError> {
        let loaded = self
            .engine
            .load_slxy(&bytes)
            .map_err(|e| JsError::new(&format!("load .slxy: {e}")))?;
        self.apply_view_json(&loaded.sidecar.view);
        // The saved IBL mode applies immediately; the HDRI itself needs the
        // worker's CPU stages, so the frontend re-prepares it from the
        // restored asset bytes and calls `set_environment_prepared`.
        let env = &loaded.sidecar.environment;
        if !env.ibl_mode.is_empty() {
            self.set_ibl_mode(env.ibl_mode.clone());
        }
        if let Some(rotation) = env
            .background
            .get("hdriRotation")
            .and_then(serde_json::Value::as_f64)
        {
            self.view.display.hdri_rotation = rotation as f32;
        }
        self.hdri = None;
        let from_node = self.engine.has_environment_node();
        // A node-authored environment supersedes the sidecar, so forget
        // whatever the tracker holds and let the node's first delta
        // install afresh.
        if from_node {
            self.environment.invalidate();
        }
        let environment = EnvironmentDto {
            ibl_mode: env.ibl_mode.clone(),
            hdri_hash: env.hdri_asset.clone(),
            hdri_name: None,
            from_node,
        };
        let result = LoadResultDto {
            batch: loaded.batch,
            warnings: loaded.warnings,
            canvas_viewports: loaded
                .sidecar
                .canvas_viewports
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
            meta: loaded.sidecar.meta.into(),
            environment,
        };
        to_js(&result)
    }
}

impl SolarxyApp {
    // ---- .slxy view sidecar bridge ----

    pub(super) fn view_json(&self) -> solarxy_scenefile::ViewJson {
        let layout = serde_json::to_value(self.view.display.layout)
            .ok()
            .and_then(|v| v.as_str().map(String::from))
            .unwrap_or_else(|| "single".to_string());
        let panes_json = (0..4)
            .map(|i| {
                let camera = self.view.cameras[i]
                    .as_ref()
                    .map_or_else(solarxy_scenefile::CameraJson::default, |c| {
                        camera_to_json(&c.camera)
                    });
                // The pane's look rides inside the display blob rather than
                // taking a schema field of its own. `PaneJson::display` is
                // declared opaque and round-tripped uninterpreted, and
                // `PaneDisplaySettings` ignores keys it does not know, so
                // this persists a free pane's exposure and grade with no
                // scene-schema change and no reader-version gate.
                let display = serde_json::to_value(self.view.pane_settings[i])
                    .ok()
                    .and_then(|v| {
                        if let serde_json::Value::Object(mut map) = v {
                            if let Ok(look) = serde_json::to_value(self.pane_looks[i]) {
                                map.insert(PANE_LOOK_KEY.to_string(), look);
                            }
                            Some(map.into_iter().collect())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_default();
                solarxy_scenefile::PaneJson {
                    camera,
                    display,
                    look_through: self.look_through[i].map(|n| n.0),
                    camera_locked: self.camera_locked[i],
                    ..solarxy_scenefile::PaneJson::default()
                }
            })
            .collect();
        solarxy_scenefile::ViewJson {
            layout,
            active_pane: self.view.active_pane as u32,
            split_ratio: self.view.display.split_ratio,
            panes: panes_json,
        }
    }

    pub(super) fn apply_view_json(&mut self, view: &solarxy_scenefile::ViewJson) {
        if let Ok(layout) =
            serde_json::from_value::<ViewLayout>(serde_json::Value::String(view.layout.clone()))
        {
            self.view.display.layout = layout;
        }
        self.view.display.split_ratio = DisplaySettings::clamp_split_ratio(view.split_ratio);
        self.view.active_pane =
            (view.active_pane as usize).min(self.view.display.layout.pane_count() - 1);

        for (i, pane) in view.panes.iter().take(4).enumerate() {
            if !pane.display.is_empty() {
                let value = serde_json::Value::Object(pane.display.clone().into_iter().collect());
                // Absent on any scene saved before the look existed, which
                // is the whole reason it defaults rather than failing.
                self.pane_looks[i] = pane
                    .display
                    .get(PANE_LOOK_KEY)
                    .and_then(|v| serde_json::from_value::<PaneLook>(v.clone()).ok())
                    .unwrap_or_default();
                if let Ok(mut settings) = serde_json::from_value::<PaneDisplaySettings>(value) {
                    // Viewport shading overrides and the turntable spin are
                    // session-temporary (items 7, 9): never restored from a
                    // saved scene, so a reopened scene starts Textured and still.
                    settings.material_override = solarxy_core::preferences::MaterialOverride::None;
                    settings.turntable_active = false;
                    self.view.pane_settings[i] = settings;
                }
            }
            if pane.camera.distance > 0.0 {
                let bounds = self.scene_bounds();
                let aspect =
                    self.renderer.target_width as f32 / self.renderer.target_height.max(1) as f32;
                let cam_state = self.view.cameras[i].get_or_insert_with(|| {
                    CameraState::new(&self.device, &self.renderer.layouts.camera, &bounds, aspect)
                });
                apply_camera_json(&mut cam_state.camera, &pane.camera);
            }
            // Restore the look-through binding + lock. The follow will
            // drive the pane from the node once it cooks.
            self.look_through[i] = pane.look_through.map(NodeId);
            self.camera_locked[i] = pane.camera_locked;
            self.camera_editing[i] = false;
        }
        self.ensure_pane_cameras();
    }
}
