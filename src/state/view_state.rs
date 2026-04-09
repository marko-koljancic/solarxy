use crate::cgi::camera_state::CameraState;
use crate::preferences::{
    BackgroundMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode,
    UvMapBackground, UvMode, ViewMode,
};

#[derive(Clone, Copy, PartialEq, Default)]
pub(crate) enum ViewLayout {
    #[default]
    Single,
    SplitVertical,
    SplitHorizontal,
}

pub(crate) struct DisplaySettings {
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
    pub layout: ViewLayout,
    pub roughness_scale: f32,
    pub metallic_scale: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum BoundsMode {
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

#[derive(Clone, Copy)]
pub(crate) struct PaneDisplaySettings {
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

pub(crate) struct ViewState {
    pub(super) pane_settings: [PaneDisplaySettings; 2],
    pub(super) display: DisplaySettings,
    pub(super) secondary_cam: Option<CameraState>,
    pub(super) active_pane: usize,
    pub(super) cameras_linked: bool,
}
