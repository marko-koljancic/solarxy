//! Per-pane and per-session view configuration: [`ViewLayout`] (single /
//! split), [`DisplaySettings`] (global, e.g. turntable, lights lock),
//! [`PaneDisplaySettings`] (per-pane view/inspection mode), [`BoundsMode`].
//!
//! Lives in `solarxy-core` because both `solarxy-renderer` (consumes for
//! drawing) and `solarxy-app` (mutates from the sidebar) need access — keeps
//! the dependency graph acyclic.
//!
//! Available with the `serialization` feature.

use crate::preferences::{
    BackgroundMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    UvMapBackground, UvMode, ViewMode,
};

/// Pane arrangement: one viewport, two side-by-side, or two top/bottom.
/// Toggled via `F1` / `F2` / `F3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewLayout {
    #[default]
    Single,
    SplitVertical,
    SplitHorizontal,
}

#[derive(Debug, Clone, Copy)]
pub struct DisplaySettings {
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
    pub layout: ViewLayout,
    /// Split-divider ratio in [0.05, 0.95] (pane-1 fraction of the split
    /// axis). Defaults to 0.5 (centered). Single layout ignores this.
    /// Per-orientation ratios (separate vertical / horizontal) are
    /// deferred — both currently share this single value.
    pub split_ratio: f32,
    pub roughness_scale: f32,
    pub metallic_scale: f32,
}

impl DisplaySettings {
    /// Default split ratio (centered). Exported so callers wiring new
    /// `DisplaySettings` instances don't repeat the magic number.
    pub const DEFAULT_SPLIT_RATIO: f32 = 0.5;

    /// Smallest legal divider ratio — keeps either pane visible.
    pub const MIN_SPLIT_RATIO: f32 = 0.05;
    /// Largest legal divider ratio — keeps either pane visible.
    pub const MAX_SPLIT_RATIO: f32 = 0.95;

    /// Clamp a candidate ratio to the legal `[MIN_SPLIT_RATIO,
    /// MAX_SPLIT_RATIO]` range.
    pub fn clamp_split_ratio(ratio: f32) -> f32 {
        ratio.clamp(Self::MIN_SPLIT_RATIO, Self::MAX_SPLIT_RATIO)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundsMode {
    Off,
    WholeModel,
    PerMesh,
}

impl BoundsMode {
    pub const ALL: &[Self] = &[Self::Off, Self::WholeModel, Self::PerMesh];
}

impl std::fmt::Display for BoundsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BoundsMode::Off => write!(f, "Off"),
            BoundsMode::WholeModel => write!(f, "Model"),
            BoundsMode::PerMesh => write!(f, "Per Mesh"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct PaneDisplaySettings {
    pub view_mode: ViewMode,
    pub prev_non_ghosted_mode: ViewMode,
    pub ghosted_wireframe: bool,
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
    pub uv_offset: [f32; 2],
    pub uv_zoom: f32,
    pub show_uv_overlap: bool,
    pub show_validation: bool,
}
