//! Where an annotation is, and what happens when the geometry under it moves:
//! the screen-space marker hit test, the re-anchor sub-mode, and the staleness
//! sweep a freshly loaded model triggers.
//!
//! Separated from the state because it is the half that does arithmetic. The
//! rest of `ReviewState` is bookkeeping over a list; this projects world
//! anchors through a camera and compares mesh hashes.

use super::ReviewState;

impl ReviewState {
    /// Project every non-reply annotation's world anchor into pane-relative
    /// screen pixels and return the id of the marker nearest to
    /// `cursor_px`, provided it falls within `threshold_px`.
    ///
    /// `cursor_px` and `viewport_size_px` are pane-relative — pass the
    /// same `(local.0, local.1)` cursor the raycaster uses, and the
    /// active pane's `(width, height)`. `view_proj` is the active
    /// camera's `clip = view_proj * world` matrix (same one written to
    /// the GPU camera uniform).
    ///
    /// Skips annotations whose projected NDC `z` is outside `[-1, 1]`
    /// (behind the camera or past the far plane). Replies are skipped
    /// (no 3D marker is drawn for them — see [`marker_instances`]).
    /// Resolved annotations are still hit-tested (they render dimmed,
    /// but remain interactable).
    pub fn marker_at_screen_pos(
        &self,
        cursor_px: (f32, f32),
        viewport_size_px: (f32, f32),
        view_proj: cgmath::Matrix4<f32>,
        threshold_px: f32,
    ) -> Option<String> {
        use cgmath::Vector4;
        let threshold_sq = threshold_px * threshold_px;
        let mut best: Option<(f32, &str)> = None;
        for ann in &self.annotations {
            if ann.reply_to.is_some() {
                continue;
            }
            let [wx, wy, wz] = ann.anchor.world_pos_fallback;
            let clip = view_proj * Vector4::new(wx, wy, wz, 1.0);
            if clip.w <= 0.0 {
                continue;
            }
            let ndc_x = clip.x / clip.w;
            let ndc_y = clip.y / clip.w;
            let ndc_z = clip.z / clip.w;
            if !(-1.0..=1.0).contains(&ndc_z) {
                continue;
            }
            let px = f32::midpoint(ndc_x, 1.0) * viewport_size_px.0;
            let py = (1.0 - ndc_y) * 0.5 * viewport_size_px.1;
            let dx = px - cursor_px.0;
            let dy = py - cursor_px.1;
            let d_sq = dx * dx + dy * dy;
            if d_sq > threshold_sq {
                continue;
            }
            if best.is_none_or(|(b, _)| d_sq < b) {
                best = Some((d_sq, ann.id.as_str()));
            }
        }
        best.map(|(_, id)| id.to_string())
    }

    /// Enter the re-anchor sub-mode for an existing annotation. Sets
    /// `reanchor_target = Some(id)` and selects the row so the panel
    /// scrolls to it. No-op if the id is unknown.
    pub fn begin_reanchor(&mut self, id: String) {
        if !self.annotations.iter().any(|a| a.id == id) {
            return;
        }
        self.selected = Some(id.clone());
        self.scroll_to_selected = true;
        self.reanchor_target = Some(id);
    }

    /// Leave the re-anchor sub-mode without modifying any annotation.
    /// Leaves `selected` intact so the row stays highlighted.
    pub fn cancel_reanchor(&mut self) {
        self.reanchor_target = None;
    }

    /// Finalize the re-anchor: write `hit`'s mesh / face / barycentric /
    /// world-position into the target annotation, clear `stale`, bump
    /// `updated_at`, mark dirty, clear `reanchor_target`. Returns `true`
    /// when a target was pending; `false` (no-op) otherwise.
    pub fn complete_reanchor(&mut self, hit: &crate::state::raycast::RaycastHit) -> bool {
        let Some(id) = self.reanchor_target.take() else {
            return false;
        };
        let Some(ann) = self.annotations.iter_mut().find(|a| a.id == id) else {
            return false;
        };
        ann.anchor.mesh_index = hit.mesh_index;
        ann.anchor.face_index = hit.face_index;
        ann.anchor.barycentric = hit.barycentric;
        ann.anchor.world_pos_fallback = [hit.world_pos.x, hit.world_pos.y, hit.world_pos.z];
        ann.stale = false;
        ann.updated_at = Self::now_rfc3339();
        self.dirty = true;
        true
    }

    /// Mark every annotation whose mesh's current hash differs from the
    /// stored one as stale. Used after loading a sidecar to flag anchors
    /// that need explicit reconciliation by the user.
    pub fn apply_stale_flags(
        &mut self,
        stored_mesh_hashes: &[String],
        current_mesh_hashes: &[String],
    ) -> usize {
        let mut stale_count = 0;
        for ann in &mut self.annotations {
            let mi = ann.anchor.mesh_index as usize;
            let stale = match (stored_mesh_hashes.get(mi), current_mesh_hashes.get(mi)) {
                (Some(old), Some(new)) => old != new,
                _ => true,
            };
            ann.stale = stale;
            if stale {
                stale_count += 1;
            }
        }
        stale_count
    }
}

#[cfg(test)]
mod tests {
    // The two fixtures the state tests also build. Duplicated rather
    // than shared, because a test module that reaches into a sibling's
    // is harder to read than eleven lines of scaffolding.
    use super::super::*;

    fn anchor_at(pos: [f32; 3]) -> AnchorPosition {
        AnchorPosition {
            mesh_index: 0,
            face_index: 0,
            barycentric: [1.0 / 3.0; 3],
            world_pos_fallback: pos,
        }
    }

    fn approx_eq_3(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    #[test]
    fn apply_stale_flags_marks_only_mismatched_meshes() {
        let mut state = ReviewState::default();
        let mut anchor_a = anchor_at([0.0, 0.0, 0.0]);
        anchor_a.mesh_index = 0;
        let mut anchor_b = anchor_at([1.0, 0.0, 0.0]);
        anchor_b.mesh_index = 1;
        for (i, anchor) in [anchor_a, anchor_b].iter().enumerate() {
            state.annotations.push(ReviewAnnotation {
                id: format!("id-{i}"),
                created_at: ReviewState::now_rfc3339(),
                updated_at: ReviewState::now_rfc3339(),
                author: None,
                anchor: anchor.clone(),
                category: AnnotationCategory::Info,
                text: format!("note-{i}"),
                reply_to: None,
                resolved: false,
                stale: false,
            });
        }
        let stored = vec!["mesh0_hash".to_string(), "mesh1_hash".to_string()];
        let current = vec!["mesh0_hash".to_string(), "mesh1_DIFFERENT".to_string()];
        let stale_count = state.apply_stale_flags(&stored, &current);
        assert_eq!(stale_count, 1);
        assert!(!state.annotations[0].stale);
        assert!(state.annotations[1].stale);
    }

    #[test]
    fn apply_stale_flags_marks_out_of_range_as_stale() {
        let mut state = ReviewState::default();
        let mut anchor = anchor_at([0.0, 0.0, 0.0]);
        anchor.mesh_index = 5;
        state.annotations.push(ReviewAnnotation {
            id: "id-5".into(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor,
            category: AnnotationCategory::Info,
            text: "note".into(),
            reply_to: None,
            resolved: false,
            stale: false,
        });

        let stale_count = state.apply_stale_flags(
            &["a".to_string(), "b".to_string(), "c".to_string()],
            &["a".to_string(), "b".to_string()],
        );
        assert_eq!(stale_count, 1);
        assert!(state.annotations[0].stale);
    }

    /// Build an identity-camera `view_proj` that maps `[-1, 1]` world coords
    /// 1:1 to NDC: positive X → right, positive Y → up, Z passes through
    /// `[-1, 1]`. With this projection, `world (0, 0, 0)` lands at the
    /// viewport center.
    fn identity_view_proj() -> cgmath::Matrix4<f32> {
        cgmath::Matrix4::from_cols(
            cgmath::Vector4::new(1.0, 0.0, 0.0, 0.0),
            cgmath::Vector4::new(0.0, 1.0, 0.0, 0.0),
            cgmath::Vector4::new(0.0, 0.0, 1.0, 0.0),
            cgmath::Vector4::new(0.0, 0.0, 0.0, 1.0),
        )
    }

    fn push_annotation(state: &mut ReviewState, id: &str, world_pos: [f32; 3]) {
        state.annotations.push(ReviewAnnotation {
            id: id.into(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor: AnchorPosition {
                mesh_index: 0,
                face_index: 0,
                barycentric: [1.0 / 3.0; 3],
                world_pos_fallback: world_pos,
            },
            category: AnnotationCategory::Question,
            text: id.into(),
            reply_to: None,
            resolved: false,
            stale: false,
        });
    }

    #[test]
    fn marker_at_screen_pos_finds_nearest_within_threshold() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "left", [-0.5, 0.0, 0.0]);
        push_annotation(&mut state, "right", [0.5, 0.0, 0.0]);

        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);

        let hit = state.marker_at_screen_pos((155.0, 100.0), viewport, view_proj, 20.0);
        assert_eq!(
            hit.as_deref(),
            Some("right"),
            "right marker wins by distance"
        );

        let hit = state.marker_at_screen_pos((45.0, 100.0), viewport, view_proj, 20.0);
        assert_eq!(hit.as_deref(), Some("left"));
    }

    #[test]
    fn marker_at_screen_pos_returns_none_when_all_outside_threshold() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);
        let hit = state.marker_at_screen_pos((150.0, 100.0), viewport, view_proj, 20.0);
        assert!(hit.is_none());
    }

    #[test]
    fn marker_at_screen_pos_skips_markers_behind_camera() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "behind", [0.0, 0.0, 2.0]);
        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);
        let hit = state.marker_at_screen_pos((100.0, 100.0), viewport, view_proj, 20.0);
        assert!(hit.is_none(), "out-of-range NDC z is skipped");
    }

    #[test]
    fn marker_at_screen_pos_skips_reply_annotations() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "parent", [0.0, 0.0, 0.0]);
        let reply = ReviewAnnotation {
            id: "reply".into(),
            created_at: ReviewState::now_rfc3339(),
            updated_at: ReviewState::now_rfc3339(),
            author: None,
            anchor: AnchorPosition {
                mesh_index: 0,
                face_index: 0,
                barycentric: [1.0 / 3.0; 3],
                world_pos_fallback: [0.0, 0.0, 0.0],
            },
            category: AnnotationCategory::Info,
            text: "fixed".into(),
            reply_to: Some("parent".into()),
            resolved: false,
            stale: false,
        };
        state.annotations.push(reply);
        let view_proj = identity_view_proj();
        let viewport = (200.0, 200.0);
        let hit = state.marker_at_screen_pos((100.0, 100.0), viewport, view_proj, 20.0);
        assert_eq!(hit.as_deref(), Some("parent"));
    }

    #[test]
    fn begin_reanchor_sets_target_and_selects() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        state.begin_reanchor("a".into());
        assert_eq!(state.reanchor_target.as_deref(), Some("a"));
        assert_eq!(state.selected.as_deref(), Some("a"));
        assert!(state.scroll_to_selected);
    }

    #[test]
    fn begin_reanchor_is_noop_for_unknown_id() {
        let mut state = ReviewState::default();
        state.begin_reanchor("ghost".into());
        assert!(state.reanchor_target.is_none());
        assert!(state.selected.is_none());
        assert!(!state.scroll_to_selected);
    }

    #[test]
    fn cancel_reanchor_clears_target_only() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        state.begin_reanchor("a".into());
        state.cancel_reanchor();
        assert!(state.reanchor_target.is_none());
        assert_eq!(state.selected.as_deref(), Some("a"), "selection preserved");
    }

    fn raycast_hit_at(world: [f32; 3]) -> crate::state::raycast::RaycastHit {
        crate::state::raycast::RaycastHit {
            mesh_index: 2,
            face_index: 17,
            barycentric: [0.25, 0.25, 0.5],
            world_pos: cgmath::Point3::new(world[0], world[1], world[2]),
            distance: 4.2,
        }
    }

    #[test]
    fn complete_reanchor_mutates_anchor_clears_stale_bumps_updated_at() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        state.annotations[0].stale = true;
        let pre_updated = state.annotations[0].updated_at.clone();
        state.annotations[0].updated_at = "2000-01-01T00:00:00Z".into();
        state.begin_reanchor("a".into());

        let hit = raycast_hit_at([3.0, 1.0, -2.0]);
        let ok = state.complete_reanchor(&hit);
        assert!(ok);
        let ann = &state.annotations[0];
        assert_eq!(ann.anchor.mesh_index, 2);
        assert_eq!(ann.anchor.face_index, 17);
        assert!(approx_eq_3(ann.anchor.barycentric, [0.25, 0.25, 0.5]));
        assert!(approx_eq_3(ann.anchor.world_pos_fallback, [3.0, 1.0, -2.0]));
        assert!(!ann.stale, "stale cleared after explicit re-anchor");
        assert_ne!(ann.updated_at, "2000-01-01T00:00:00Z", "updated_at bumped");
        assert!(state.dirty);
        assert!(state.reanchor_target.is_none());
        let _ = pre_updated;
    }

    #[test]
    fn complete_reanchor_returns_false_when_no_target() {
        let mut state = ReviewState::default();
        push_annotation(&mut state, "a", [0.0, 0.0, 0.0]);
        let ok = state.complete_reanchor(&raycast_hit_at([0.0, 0.0, 0.0]));
        assert!(!ok);
        assert!(!state.dirty);
    }
}
