use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ViewMode {
    Shaded,
    ShadedWireframe,
    WireframeOnly,
    Ghosted,
}

impl ViewMode {
    pub const ALL: &[Self] = &[
        Self::Shaded,
        Self::ShadedWireframe,
        Self::WireframeOnly,
        Self::Ghosted,
    ];

    pub fn next(self) -> Self {
        match self {
            Self::Shaded => Self::ShadedWireframe,
            Self::ShadedWireframe => Self::WireframeOnly,
            Self::WireframeOnly => Self::Shaded,
            Self::Ghosted => Self::Ghosted,
        }
    }
}

impl std::fmt::Display for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shaded => write!(f, "Shaded"),
            Self::ShadedWireframe => write!(f, "Shaded+Wire"),
            Self::WireframeOnly => write!(f, "Wireframe"),
            Self::Ghosted => write!(f, "Ghosted"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LineWeight {
    Light,
    Medium,
    Bold,
}

impl LineWeight {
    pub const ALL: &[Self] = &[Self::Light, Self::Medium, Self::Bold];

    pub fn width_px(self) -> f32 {
        match self {
            Self::Light => 1.0,
            Self::Medium => 2.0,
            Self::Bold => 3.0,
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Light => Self::Medium,
            Self::Medium => Self::Bold,
            Self::Bold => Self::Light,
        }
    }
}

impl std::fmt::Display for LineWeight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Light => write!(f, "Light"),
            Self::Medium => write!(f, "Medium"),
            Self::Bold => write!(f, "Bold"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NormalsMode {
    Off,
    Face,
    Vertex,
    FaceAndVertex,
}

impl NormalsMode {
    pub const ALL: &[Self] = &[Self::Off, Self::Face, Self::Vertex, Self::FaceAndVertex];

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Face,
            Self::Face => Self::Vertex,
            Self::Vertex => Self::FaceAndVertex,
            Self::FaceAndVertex => Self::Off,
        }
    }
}

impl std::fmt::Display for NormalsMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::Face => write!(f, "Face"),
            Self::Vertex => write!(f, "Vertex"),
            Self::FaceAndVertex => write!(f, "Face+Vertex"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BackgroundMode {
    White,
    Gradient,
    DarkGray,
    Black,
}

impl BackgroundMode {
    pub const ALL: &[Self] = &[Self::White, Self::Gradient, Self::DarkGray, Self::Black];

    pub fn next(self) -> Self {
        match self {
            Self::White => Self::Gradient,
            Self::Gradient => Self::DarkGray,
            Self::DarkGray => Self::Black,
            Self::Black => Self::White,
        }
    }
}

impl std::fmt::Display for BackgroundMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::White => write!(f, "White"),
            Self::Gradient => write!(f, "Gradient"),
            Self::DarkGray => write!(f, "Dark"),
            Self::Black => write!(f, "Black"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UvMode {
    #[default]
    Off,
    Gradient,
    Checker,
}

impl UvMode {
    pub const ALL: &[Self] = &[Self::Off, Self::Gradient, Self::Checker];

    pub fn next(self) -> Self {
        match self {
            Self::Off => Self::Gradient,
            Self::Gradient => Self::Checker,
            Self::Checker => Self::Off,
        }
    }
}

impl std::fmt::Display for UvMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::Gradient => write!(f, "Gradient"),
            Self::Checker => write!(f, "Checker"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ProjectionMode {
    #[default]
    Perspective,
    Orthographic,
}

impl ProjectionMode {
    pub fn next(self) -> Self {
        match self {
            Self::Perspective => Self::Orthographic,
            Self::Orthographic => Self::Perspective,
        }
    }
}

impl std::fmt::Display for ProjectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Perspective => write!(f, "Perspective"),
            Self::Orthographic => write!(f, "Orthographic"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IblMode {
    Off,
    Diffuse,
    #[default]
    Full,
}

impl IblMode {
    pub const ALL: &[Self] = &[Self::Off, Self::Diffuse, Self::Full];
}

impl std::fmt::Display for IblMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Off => write!(f, "Off"),
            Self::Diffuse => write!(f, "Diffuse"),
            Self::Full => write!(f, "Full"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PaneMode {
    #[default]
    Scene3D,
    UvMap,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum UvMapBackground {
    #[default]
    Dark,
    Checker,
    Texture,
}

impl UvMapBackground {
    pub const ALL: &[Self] = &[Self::Dark, Self::Checker, Self::Texture];

    pub fn next(self) -> Self {
        match self {
            Self::Dark => Self::Checker,
            Self::Checker => Self::Texture,
            Self::Texture => Self::Dark,
        }
    }
}

impl std::fmt::Display for UvMapBackground {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dark => write!(f, "Dark"),
            Self::Checker => write!(f, "Checker"),
            Self::Texture => write!(f, "Texture"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum InspectionMode {
    #[default]
    Shaded,
    MaterialId,
    TexelDensity,
    Depth,
}

impl InspectionMode {
    pub const ALL: &[Self] = &[
        Self::Shaded,
        Self::MaterialId,
        Self::TexelDensity,
        Self::Depth,
    ];

    pub fn as_u32(self) -> u32 {
        match self {
            Self::Shaded => 0,
            Self::MaterialId => 1,
            Self::TexelDensity => 2,
            Self::Depth => 3,
        }
    }
}

impl std::fmt::Display for InspectionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shaded => write!(f, "Shaded"),
            Self::MaterialId => write!(f, "Material ID"),
            Self::TexelDensity => write!(f, "Texel Density"),
            Self::Depth => write!(f, "Depth"),
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ToneMode {
    None,
    Linear,
    Reinhard,
    #[default]
    AcesFilmic,
}

impl ToneMode {
    pub const ALL: &[Self] = &[Self::None, Self::Linear, Self::Reinhard, Self::AcesFilmic];

    pub fn next(self) -> Self {
        match self {
            Self::None => Self::Linear,
            Self::Linear => Self::Reinhard,
            Self::Reinhard => Self::AcesFilmic,
            Self::AcesFilmic => Self::None,
        }
    }

    pub fn as_u32(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Linear => 1,
            Self::Reinhard => 2,
            Self::AcesFilmic => 3,
        }
    }
}

impl std::fmt::Display for ToneMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::None => write!(f, "None (clip)"),
            Self::Linear => write!(f, "Linear"),
            Self::Reinhard => write!(f, "Reinhard"),
            Self::AcesFilmic => write!(f, "ACES Filmic"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    pub config_version: u32,
    #[serde(default)]
    pub display: DisplayPrefs,
    #[serde(default)]
    pub rendering: RenderingPrefs,
    #[serde(default)]
    pub lighting: LightingPrefs,
    #[serde(default)]
    pub window: WindowPrefs,
    #[serde(default)]
    pub history: HistoryPrefs,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayPrefs {
    pub background: BackgroundMode,
    pub view_mode: ViewMode,
    pub normals_mode: NormalsMode,
    pub grid_visible: bool,
    pub axis_gizmo_visible: bool,
    pub bloom_enabled: bool,
    #[serde(default)]
    pub uv_mode: UvMode,
    #[serde(default)]
    pub projection_mode: ProjectionMode,
    #[serde(default)]
    pub turntable_active: bool,
    #[serde(default)]
    pub ibl_mode: IblMode,
    #[serde(default = "default_true")]
    pub ssao_enabled: bool,
    #[serde(default)]
    pub tone_mode: ToneMode,
    #[serde(default = "default_exposure")]
    pub exposure: f32,
    #[serde(default)]
    pub local_axes_visible: bool,
    #[serde(default = "default_turntable_rpm")]
    pub turntable_rpm: f32,
    #[serde(default)]
    pub inspection_mode: InspectionMode,
    #[serde(default = "default_texel_density_target")]
    pub texel_density_target: f32,
}

fn default_exposure() -> f32 {
    1.0
}

fn default_texel_density_target() -> f32 {
    1.0
}

fn default_turntable_rpm() -> f32 {
    5.0
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderingPrefs {
    pub wireframe_line_weight: LineWeight,
    pub msaa_sample_count: u32,
    #[serde(default = "default_shadow_map_size")]
    pub shadow_map_size: u32,
}

fn default_shadow_map_size() -> u32 {
    2048
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LightingPrefs {
    pub lock: bool,
}

pub const MIN_WINDOW_WIDTH: u32 = 640;
pub const MIN_WINDOW_HEIGHT: u32 = 480;
pub const MAX_WINDOW_WIDTH: u32 = 7680;
pub const MAX_WINDOW_HEIGHT: u32 = 4320;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowPrefs {
    pub window_width: u32,
    pub window_height: u32,
}

impl Default for WindowPrefs {
    fn default() -> Self {
        Self {
            window_width: 1280,
            window_height: 720,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryPrefs {
    pub recent_files: Vec<String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            config_version: 1,
            display: DisplayPrefs::default(),
            rendering: RenderingPrefs::default(),
            lighting: LightingPrefs::default(),
            window: WindowPrefs::default(),
            history: HistoryPrefs::default(),
        }
    }
}

impl Default for DisplayPrefs {
    fn default() -> Self {
        Self {
            background: BackgroundMode::Black,
            view_mode: ViewMode::Shaded,
            normals_mode: NormalsMode::Off,
            grid_visible: true,
            axis_gizmo_visible: true,
            bloom_enabled: true,
            uv_mode: UvMode::Off,
            projection_mode: ProjectionMode::Perspective,
            turntable_active: false,
            ibl_mode: IblMode::Full,
            ssao_enabled: false,
            tone_mode: ToneMode::AcesFilmic,
            exposure: 1.0,
            local_axes_visible: false,
            turntable_rpm: 5.0,
            inspection_mode: InspectionMode::Shaded,
            texel_density_target: 1.0,
        }
    }
}

impl Default for RenderingPrefs {
    fn default() -> Self {
        Self {
            wireframe_line_weight: LineWeight::Medium,
            msaa_sample_count: 4,
            shadow_map_size: 2048,
        }
    }
}

const MAX_RECENT_FILES: usize = 20;

pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("solarxy").join("config.toml"))
}

pub fn load() -> Preferences {
    #[cfg(debug_assertions)]
    if let Some(ref path) = config_path() {
        tracing::debug!("Config path: {}", path.display());
    }

    let Some(path) = config_path() else {
        return Preferences::default();
    };

    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Preferences::default();
    };

    match toml::from_str::<Preferences>(&contents) {
        Ok(mut prefs) => {
            if !matches!(prefs.rendering.msaa_sample_count, 1 | 2 | 4) {
                tracing::warn!(
                    "Invalid msaa_sample_count {} in config, falling back to 4",
                    prefs.rendering.msaa_sample_count
                );
                prefs.rendering.msaa_sample_count = 4;
            }
            prefs.window.window_width = prefs
                .window
                .window_width
                .clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH);
            prefs.window.window_height = prefs
                .window
                .window_height
                .clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT);
            prefs
        }
        Err(e) => {
            tracing::warn!("Failed to parse {}: {}", path.display(), e);
            Preferences::default()
        }
    }
}

pub fn save(prefs: &Preferences) -> Result<(), String> {
    let path = config_path().ok_or("Could not determine config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let toml_str = toml::to_string_pretty(prefs)
        .map_err(|e| format!("Failed to serialize preferences: {}", e))?;

    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, &toml_str).map_err(|e| format!("Failed to write config: {}", e))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to finalize config: {}", e))?;

    Ok(())
}

pub fn add_recent_file(prefs: &mut Preferences, path: &str) {
    let files = &mut prefs.history.recent_files;
    files.retain(|p| p != path);
    files.insert(0, path.to_string());
    files.truncate(MAX_RECENT_FILES);
    let _ = save(prefs);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrip() {
        let prefs = Preferences::default();
        let toml_str = toml::to_string_pretty(&prefs).unwrap();
        let parsed: Preferences = toml::from_str(&toml_str).unwrap();
        assert_eq!(prefs, parsed);
    }

    #[test]
    fn non_default_roundtrip() {
        let prefs = Preferences {
            config_version: 1,
            display: DisplayPrefs {
                background: BackgroundMode::Black,
                view_mode: ViewMode::WireframeOnly,
                normals_mode: NormalsMode::FaceAndVertex,
                grid_visible: false,
                axis_gizmo_visible: true,
                bloom_enabled: false,
                uv_mode: UvMode::Checker,
                projection_mode: ProjectionMode::Orthographic,
                turntable_active: true,
                ibl_mode: IblMode::Diffuse,
                ssao_enabled: false,
                tone_mode: ToneMode::Reinhard,
                exposure: 1.5,
                local_axes_visible: true,
                turntable_rpm: 30.0,
                inspection_mode: InspectionMode::TexelDensity,
                texel_density_target: 2.5,
            },
            rendering: RenderingPrefs {
                wireframe_line_weight: LineWeight::Bold,
                msaa_sample_count: 2,
                shadow_map_size: 2048,
            },
            lighting: LightingPrefs { lock: true },
            window: WindowPrefs {
                window_width: 1920,
                window_height: 1080,
            },
            history: HistoryPrefs {
                recent_files: vec!["/tmp/model.obj".to_string()],
            },
        };
        let toml_str = toml::to_string_pretty(&prefs).unwrap();
        let parsed: Preferences = toml::from_str(&toml_str).unwrap();
        assert_eq!(prefs, parsed);
    }

    #[test]
    fn unknown_fields_ignored() {
        let toml_str = r#"
            config_version = 1
            some_future_field = "hello"

            [display]
            background = "Black"
            view_mode = "Shaded"
            normals_mode = "Off"
            grid_visible = true
            axis_gizmo_visible = true
            bloom_enabled = true
            uv_mode = "Off"
            projection_mode = "Perspective"
            turntable_active = false
            ibl_mode = "Full"
            ssao_enabled = false
            tone_mode = "AcesFilmic"
            future_toggle = true

            [rendering]
            wireframe_line_weight = "Medium"
            msaa_sample_count = 4

            [lighting]
            lock = false

            [history]
            recent_files = []

            [some_future_section]
            key = "value"
        "#;
        let parsed: Preferences = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed, Preferences::default());
    }

    #[test]
    fn partial_toml_uses_defaults() {
        let toml_str = r#"
            config_version = 1

            [display]
            background = "Black"
            view_mode = "Shaded"
            normals_mode = "Off"
            grid_visible = true
            axis_gizmo_visible = false
            bloom_enabled = true
        "#;
        let parsed: Preferences = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.rendering, RenderingPrefs::default());
        assert_eq!(parsed.lighting, LightingPrefs::default());
        assert_eq!(parsed.window, WindowPrefs::default());
        assert_eq!(parsed.history, HistoryPrefs::default());
        assert_eq!(parsed.display.background, BackgroundMode::Black);
    }

    #[test]
    fn corrupt_toml_fails_parse() {
        let result = toml::from_str::<Preferences>("{{{{garbage}}}}");
        assert!(result.is_err());
    }

    #[test]
    fn recent_files_dedup_and_truncate() {
        let mut prefs = Preferences::default();
        for i in 0..25 {
            let files = &mut prefs.history.recent_files;
            let path = format!("/tmp/model_{}.obj", i);
            files.retain(|p| *p != path);
            files.insert(0, path);
            files.truncate(MAX_RECENT_FILES);
        }
        assert_eq!(prefs.history.recent_files.len(), MAX_RECENT_FILES);
        assert_eq!(prefs.history.recent_files[0], "/tmp/model_24.obj");

        let files = &mut prefs.history.recent_files;
        let dup = "/tmp/model_10.obj".to_string();
        files.retain(|p| *p != dup);
        files.insert(0, dup.clone());
        files.truncate(MAX_RECENT_FILES);

        assert_eq!(prefs.history.recent_files.len(), MAX_RECENT_FILES);
        assert_eq!(prefs.history.recent_files[0], "/tmp/model_10.obj");
    }

    #[test]
    fn window_prefs_clamped() {
        let toml_str = r#"
            config_version = 1

            [window]
            window_width = 100
            window_height = 99999
        "#;
        let mut parsed: Preferences = toml::from_str(toml_str).unwrap();
        parsed.window.window_width = parsed
            .window
            .window_width
            .clamp(MIN_WINDOW_WIDTH, MAX_WINDOW_WIDTH);
        parsed.window.window_height = parsed
            .window
            .window_height
            .clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT);
        assert_eq!(parsed.window.window_width, MIN_WINDOW_WIDTH);
        assert_eq!(parsed.window.window_height, MAX_WINDOW_HEIGHT);
    }

    #[test]
    fn config_path_returns_some() {
        assert!(config_path().is_some());
    }
}
