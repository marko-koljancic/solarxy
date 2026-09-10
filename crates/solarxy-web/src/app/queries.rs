//! Read-only questions the frontend asks about the document, and the
//! selection and label state it mirrors back.

use super::*;

#[wasm_bindgen]
impl SolarxyApp {
    /// The annotation set with runtime staleness (the review store's
    /// structure channel): re-read by the frontend on every `reviewChanged`
    /// event. Positions are the separate per-frame channel
    /// ([`SolarxyApp::review_markers`]).
    pub fn review_annotations(&self) -> Result<JsValue, JsError> {
        let annotations: Vec<solarxy_graph::engine::AnnotationSnapshot> = self
            .engine
            .document()
            .review()
            .iter()
            .map(|a| solarxy_graph::engine::AnnotationSnapshot {
                needs_reanchor: self.engine.annotation_stale(a.id),
                annotation: a.clone(),
            })
            .collect();
        to_js(&annotations)
    }

    /// The lane inventory of a node's cooked geometry (names, types,
    /// counts, both domains), or `null` while nothing is committed. Feeds
    /// the attribute-name pickers and the Attributes pane header; values
    /// page separately through [`SolarxyApp::attribute_table`].
    pub fn attribute_summary(&self, node: f64) -> Result<JsValue, JsError> {
        to_js(&self.engine.attribute_summary(NodeId(node as u64)))
    }

    /// The last completed cook's warnings for one node (a plain string
    /// array; empty when the cook was quiet). Pull-read by the node info
    /// card when it opens or the node's cook status changes.
    pub fn cook_warnings(&self, node: f64) -> Result<JsValue, JsError> {
        to_js(&self.engine.cook_warnings(NodeId(node as u64)))
    }

    /// Bounds, cook accounting, placeholder reason and timestamps for one
    /// node, or `null` when the node is gone.
    ///
    /// Pull-read for the same reason `resolved_param` below is: every field
    /// moves on each cook of a time-dependent node, so as events these
    /// would be one message per node per frame under playback.
    pub fn node_report(&self, ctx: JsValue, node: f64) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let report = self.engine.node_report(ctx, NodeId(node as u64));
        to_js(&report)
    }

    /// One param's current value as the panel should display it, or the
    /// message explaining why it has none.
    ///
    /// Pull-read per row rather than pushed: under playback a per-cook
    /// resolved value would be one event per expression per frame across
    /// this boundary.
    pub fn resolved_param(&self, ctx: JsValue, node: f64, key: String) -> Result<JsValue, JsError> {
        let ctx: GraphContext = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        match self.engine.resolved_param(ctx, NodeId(node as u64), &key) {
            Ok(value) => to_js(&ResolvedParamDto {
                text: solarxy_studio::expression::format_resolved(&value),
                ok: true,
                value: Some(value),
                error: None,
            }),
            Err(error) => to_js(&ResolvedParamDto {
                ok: false,
                value: None,
                text: error.clone(),
                error: Some(error),
            }),
        }
    }

    /// One window of a node's cooked attribute values
    /// (`domain` is `"point"` or `"primitive"`). Only the requested page
    /// crosses the boundary; the geometry stays in wasm.
    pub fn attribute_table(
        &self,
        node: f64,
        domain: String,
        offset: u32,
        limit: u32,
    ) -> Result<JsValue, JsError> {
        let domain = match domain.as_str() {
            "primitive" => solarxy_kernel::AttributeDomain::Primitive,
            _ => solarxy_kernel::AttributeDomain::Point,
        };
        to_js(
            &self
                .engine
                .attribute_page(NodeId(node as u64), domain, offset, limit),
        )
    }

    /// Replaces the host-owned attribute-visualization state (the right
    /// strip's toggles and picked lane). Session-only: never saved, never
    /// in undo. Returns the full view state, the mutator convention.
    pub fn set_attr_viz(&mut self, state: JsValue) -> Result<JsValue, JsError> {
        let next: AttrVizState = serde_wasm_bindgen::from_value(state)
            .map_err(|e| JsError::new(&format!("attrViz: {e}")))?;
        if next != self.attr_viz {
            // Size, background and opacity live in the GPU uniform, not in
            // the label set, so they need a style push as well as the
            // rebuild. Decimals go the other way (they change the text), and
            // `attr_dirty` covers those.
            self.attr_viz = next;
            self.attr_dirty = true;
            self.push_label_style();
        }
        to_js(&self.view_state_dto())
    }

    /// Marker pin positions in PANE-RELATIVE CSS pixels (the DOM overlay
    /// clips one absolutely-positioned box per pane, so pins offset from
    /// their pane's origin), one entry per visible (marker x 3D pane) pair,
    /// resolved through each pane's camera (the desktop projection: clip ->
    /// NDC -> pane pixel, small NDC slack). Called once per animation frame
    /// by the host loop and applied to the DOM imperatively; markers absent
    /// from the list are hidden. UV panes carry no markers.
    pub fn review_markers(&self) -> Result<JsValue, JsError> {
        let mut out: Vec<MarkerScreenDto> = Vec::new();
        if self.player_mode {
            // Review notes are an authoring conversation, not part of the
            // published scene.
            return to_js(&out);
        }
        let markers = self.engine.review_markers_world();
        if markers.is_empty() {
            return to_js(&out);
        }
        let rects = self.compute_panes();
        for (i, pane) in rects.iter().enumerate() {
            if self.view.pane_settings[i].pane_mode == PaneMode::UvMap || pane.height <= 0.0 {
                continue;
            }
            let Some(cam_state) = self.view.cameras[i].as_ref() else {
                continue;
            };
            let mut cam = cam_state.camera;
            cam.aspect = pane.width / pane.height.max(1.0);
            let vp = cam.build_view_projection_matrix();
            for m in &markers {
                let Some(world) = m.world else { continue };
                let clip = vp * cgmath::Vector4::new(world[0], world[1], world[2], 1.0);
                if clip.w <= 0.0 {
                    continue;
                }
                let ndc = (clip.x / clip.w, clip.y / clip.w, clip.z / clip.w);
                // Same culls as the attribute pins; the z range is what
                // rejects behind-camera markers under orthographic
                // projection (clip.w is a constant 1 there).
                if ndc.0.abs() > NDC_XY_SLACK
                    || ndc.1.abs() > NDC_XY_SLACK
                    || !(NDC_Z_MIN..=NDC_Z_MAX).contains(&ndc.2)
                {
                    continue;
                }
                out.push(MarkerScreenDto {
                    id: m.id.0 as f64,
                    pane: i,
                    x: f32::midpoint(ndc.0, 1.0) * pane.width / self.dpr,
                    y: (1.0 - ndc.1) * 0.5 * pane.height / self.dpr,
                });
            }
        }
        to_js(&out)
    }

    /// Mirrors the graph context the node canvas currently shows (the UV
    /// pane's selected-node source resolves against it).
    pub fn set_current_context(&mut self, ctx: JsValue) -> Result<(), JsError> {
        self.current_ctx = serde_wasm_bindgen::from_value(ctx)
            .map_err(|e| JsError::new(&format!("bad ctx: {e}")))?;
        Ok(())
    }

    /// Marks the scene object produced by `node` as selected (viewport
    /// outline tint); `undefined`/null clears it.
    pub fn set_scene_selection(&mut self, node: Option<f64>) {
        self.selected_object = node.map(|n| SceneObjectId(n as u64));
    }

    /// Applies the selection-highlight preference: `style` is
    /// `"outline"`, `"tint"`, or `"none"`; color is linear RGBA; `width`
    /// is the rim width in pixels (clamped 1..16 renderer-side). The
    /// legacy tint reuses the same color at its fixed 0.35 alpha.
    pub fn set_selection_highlight(
        &mut self,
        style: String,
        r: f32,
        g: f32,
        b: f32,
        a: f32,
        width: f32,
    ) {
        use solarxy_renderer::frame::SelectionStyle;
        let style = match style.as_str() {
            "tint" => SelectionStyle::Tint,
            "none" => SelectionStyle::None,
            _ => SelectionStyle::Outline,
        };
        self.renderer
            .set_selection_highlight(&self.queue, style, [r, g, b, a], width);
    }

    /// Pushes the attribute-label theme colors (linear RGB, converted from
    /// the CSS tokens frontend-side like the selection highlight): text,
    /// background chip, anchor dot. Called at boot and on theme change.
    #[allow(clippy::too_many_arguments)]
    pub fn set_label_colors(
        &mut self,
        text_r: f32,
        text_g: f32,
        text_b: f32,
        chip_r: f32,
        chip_g: f32,
        chip_b: f32,
        dot_r: f32,
        dot_g: f32,
        dot_b: f32,
    ) {
        self.label_colors = [
            [text_r, text_g, text_b],
            [chip_r, chip_g, chip_b],
            [dot_r, dot_g, dot_b],
        ];
        self.push_label_style();
    }

    /// Rebuilds the label style from the two halves that own it: the theme
    /// colors pushed by `set_label_colors`, and the size / background /
    /// opacity the user picked in the attribute settings. Kept as one
    /// chokepoint so neither half can push a style that drops the other.
    pub(super) fn push_label_style(&mut self) {
        let base = solarxy_renderer::labels::LabelStyle {
            text: self.label_colors[0],
            chip: self.label_colors[1],
            dot: self.label_colors[2],
            dpr: self.dpr,
            ..solarxy_renderer::labels::LabelStyle::new_default()
        };
        let style = self.attr_viz.apply_to_style(base);
        self.renderer.write_label_style(&self.queue, &style);
    }
}
