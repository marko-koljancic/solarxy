//! `GuiSnapshot`: the sidebar ↔ state mirror.
//!
//! Each frame, `State::render` builds a [`GuiSnapshot`] from per-pane
//! `PaneDisplaySettings`, global `DisplaySettings`, and `PostProcessing`
//! state via [`GuiSnapshot::from_state`], hands it to the egui sidebar to
//! mutate, then writes the mutated copy back via
//! [`GuiSnapshot::apply_to_state`] — returning a [`SidebarChanges`] flag
//! struct so the caller knows which expensive recomputations (background
//! gradient, wireframe params, composite params, IBL bind group) to
//! re-trigger.
//!
//! Adding a sidebar control: add a field to [`GuiSnapshot`], wire it in
//! [`from_state`], wire it in [`apply_to_state`].

use solarxy_core::preferences::{
    BackgroundMode, IblMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    ProjectionMode, ToneMode, UvMapBackground, UvMode, ViewMode,
};
use solarxy_renderer::frame::PostProcessing;
use crate::state::view_state::{BoundsMode, DisplaySettings, PaneDisplaySettings};

#[derive(Debug, Default)]
pub struct SidebarChanges {
    pub background_changed: bool,
    pub wireframe_params_changed: bool,
    pub composite_params_changed: bool,
    pub ibl_changed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct GuiSnapshot {
    pub view_mode: ViewMode,
    pub normals_mode: NormalsMode,
    pub background_mode: BackgroundMode,
    pub uv_mode: UvMode,
    pub bounds_mode: BoundsMode,
    pub line_weight: LineWeight,
    pub show_grid: bool,
    pub show_axis_gizmo: bool,
    pub show_local_axes: bool,
    pub inspection_mode: InspectionMode,
    pub material_override: MaterialOverride,
    pub texel_density_target: f32,
    pub pane_mode: PaneMode,
    pub uv_bg: UvMapBackground,
    pub show_uv_overlap: bool,
    pub show_validation: bool,
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
    pub roughness_scale: f32,
    pub metallic_scale: f32,
    pub bloom_enabled: bool,
    pub ssao_enabled: bool,
    pub tone_mode: ToneMode,
    pub exposure: f32,
    pub ibl_mode: IblMode,
    pub cameras_linked: bool,
    pub is_split: bool,
    pub projection_mode: ProjectionMode,
}

impl GuiSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn from_state(
        pds: &PaneDisplaySettings,
        display: &DisplaySettings,
        post: &PostProcessing,
        ibl_mode: IblMode,
        cameras_linked: bool,
        is_split: bool,
        projection_mode: ProjectionMode,
    ) -> Self {
        Self {
            view_mode: pds.view_mode,
            normals_mode: pds.normals_mode,
            background_mode: pds.background_mode,
            uv_mode: pds.uv_mode,
            bounds_mode: pds.bounds_mode,
            line_weight: pds.line_weight,
            show_grid: pds.show_grid,
            show_axis_gizmo: pds.show_axis_gizmo,
            show_local_axes: pds.show_local_axes,
            inspection_mode: pds.inspection_mode,
            material_override: pds.material_override,
            texel_density_target: pds.texel_density_target,
            pane_mode: pds.pane_mode,
            uv_bg: pds.uv_bg,
            show_uv_overlap: pds.show_uv_overlap,
            show_validation: pds.show_validation,
            turntable_active: display.turntable_active,
            turntable_rpm: display.turntable_rpm,
            lights_locked: display.lights_locked,
            roughness_scale: display.roughness_scale,
            metallic_scale: display.metallic_scale,
            bloom_enabled: post.bloom_enabled,
            ssao_enabled: post.ssao_enabled,
            tone_mode: post.tone_mode,
            exposure: post.exposure,
            ibl_mode,
            cameras_linked,
            is_split,
            projection_mode,
        }
    }

    pub fn diff(&self, prev: &Self) -> SidebarChanges {
        let bg_changed = self.background_mode != prev.background_mode;
        let lw_changed = self.line_weight != prev.line_weight;
        SidebarChanges {
            background_changed: bg_changed,
            wireframe_params_changed: lw_changed && !bg_changed,
            composite_params_changed: self.bloom_enabled != prev.bloom_enabled
                || self.ssao_enabled != prev.ssao_enabled
                || self.tone_mode != prev.tone_mode
                || (self.exposure - prev.exposure).abs() > f32::EPSILON,
            ibl_changed: self.ibl_mode != prev.ibl_mode,
        }
    }

    /// Diffs `self` against `prev`, writes every mirrored field back to its
    /// destination, and returns [`SidebarChanges`] so the caller knows which
    /// expensive recomputations (background gradient, wireframe params,
    /// composite params, IBL bind group) to re-trigger.
    ///
    /// Takes disjoint borrows rather than `&mut State` to keep `gui::snapshot`
    /// independent of `State`'s field shape — adding a sidebar field means
    /// adding a parameter here, not crossing module-boundary visibility.
    pub fn apply_to_state(
        &self,
        prev: &Self,
        pds: &mut PaneDisplaySettings,
        display: &mut DisplaySettings,
        post: &mut PostProcessing,
        ibl_mode: &mut IblMode,
        cameras_linked: &mut bool,
    ) -> SidebarChanges {
        let changes = self.diff(prev);

        pds.view_mode = self.view_mode;
        pds.normals_mode = self.normals_mode;
        pds.background_mode = self.background_mode;
        pds.uv_mode = self.uv_mode;
        pds.bounds_mode = self.bounds_mode;
        pds.line_weight = self.line_weight;
        pds.show_grid = self.show_grid;
        pds.show_axis_gizmo = self.show_axis_gizmo;
        pds.show_local_axes = self.show_local_axes;
        pds.inspection_mode = self.inspection_mode;
        pds.material_override = self.material_override;
        pds.texel_density_target = self.texel_density_target;
        pds.pane_mode = self.pane_mode;
        pds.uv_bg = self.uv_bg;
        pds.show_uv_overlap = self.show_uv_overlap;
        pds.show_validation = self.show_validation;

        display.turntable_active = self.turntable_active;
        display.turntable_rpm = self.turntable_rpm;
        display.lights_locked = self.lights_locked;
        display.roughness_scale = self.roughness_scale;
        display.metallic_scale = self.metallic_scale;

        post.bloom_enabled = self.bloom_enabled;
        post.ssao_enabled = self.ssao_enabled;
        post.tone_mode = self.tone_mode;
        post.exposure = self.exposure;

        *ibl_mode = self.ibl_mode;
        *cameras_linked = self.cameras_linked;

        changes
    }
}

#[derive(Debug)]
pub(crate) struct HudInfo {
    pub pane_label: String,
    pub cameras_linked: Option<bool>,
    pub has_uvs: bool,
    pub uv_overlap_pct: Option<f32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a baseline snapshot with deterministic non-default values so
    /// each test can mutate one field at a time and assert which
    /// [`SidebarChanges`] flag fires.
    fn baseline() -> GuiSnapshot {
        GuiSnapshot {
            view_mode: ViewMode::Shaded,
            normals_mode: NormalsMode::Off,
            background_mode: BackgroundMode::Gradient,
            uv_mode: UvMode::Off,
            bounds_mode: BoundsMode::Off,
            line_weight: LineWeight::Medium,
            show_grid: true,
            show_axis_gizmo: true,
            show_local_axes: false,
            inspection_mode: InspectionMode::Shaded,
            material_override: MaterialOverride::None,
            texel_density_target: 1.0,
            pane_mode: PaneMode::Scene3D,
            uv_bg: UvMapBackground::Dark,
            show_uv_overlap: false,
            show_validation: false,
            turntable_active: false,
            turntable_rpm: 6.0,
            lights_locked: false,
            roughness_scale: 1.0,
            metallic_scale: 1.0,
            bloom_enabled: false,
            ssao_enabled: false,
            tone_mode: ToneMode::Reinhard,
            exposure: 1.0,
            ibl_mode: IblMode::Full,
            cameras_linked: false,
            is_split: false,
            projection_mode: ProjectionMode::Perspective,
        }
    }

    #[test]
    fn diff_no_changes_when_identical() {
        let s = baseline();
        let c = s.diff(&s);
        assert!(!c.background_changed);
        assert!(!c.wireframe_params_changed);
        assert!(!c.composite_params_changed);
        assert!(!c.ibl_changed);
    }

    #[test]
    fn diff_bloom_toggle_marks_composite_params() {
        let prev = baseline();
        let mut next = prev;
        next.bloom_enabled = !prev.bloom_enabled;
        let c = next.diff(&prev);
        assert!(c.composite_params_changed);
        assert!(!c.background_changed);
        assert!(!c.ibl_changed);
    }

    #[test]
    fn diff_ibl_toggle_marks_ibl_changed() {
        let prev = baseline();
        let mut next = prev;
        next.ibl_mode = IblMode::Off;
        let c = next.diff(&prev);
        assert!(c.ibl_changed);
        assert!(!c.composite_params_changed);
    }

    #[test]
    fn diff_background_change_suppresses_wireframe_signal() {
        let prev = baseline();
        let mut next = prev;
        next.background_mode = BackgroundMode::White;
        next.line_weight = LineWeight::Bold;
        let c = next.diff(&prev);
        assert!(c.background_changed);
        assert!(!c.wireframe_params_changed);
    }
}
