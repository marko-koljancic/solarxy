use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Enums (moved from state.rs — shared between viewer and TUI)
// ---------------------------------------------------------------------------

/// Display mode for model geometry.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ViewMode {
    Shaded,
    ShadedWireframe,
    WireframeOnly,
    Ghosted,
}

impl ViewMode {
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

/// Wireframe line thickness.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LineWeight {
    Light,
    Medium,
    Bold,
}

impl LineWeight {
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

/// Normals visualization mode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum NormalsMode {
    Off,
    Face,
    Vertex,
    FaceAndVertex,
}

impl NormalsMode {
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

/// Named background color presets.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum BackgroundMode {
    White,
    Gradient,
    DarkGray,
    Black,
}

impl BackgroundMode {
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

// ---------------------------------------------------------------------------
// Preferences structs
// ---------------------------------------------------------------------------

/// Top-level preferences, serialized as TOML.
///
/// Schema version is tracked via `config_version` for forward compatibility.
/// Unknown fields are silently ignored on deserialization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preferences {
    /// Schema version — always 1 for now.
    pub config_version: u32,

    /// Display-related preferences (background, modes, toggles).
    #[serde(default)]
    pub display: DisplayPrefs,

    /// Rendering quality settings.
    #[serde(default)]
    pub rendering: RenderingPrefs,

    /// Lighting behavior.
    #[serde(default)]
    pub lighting: LightingPrefs,

    /// Usage history (recent files).
    #[serde(default)]
    pub history: HistoryPrefs,
}

/// Display settings that control visual appearance.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayPrefs {
    /// Background color preset: White, Gradient, DarkGray, or Black.
    pub background: BackgroundMode,

    /// Initial view mode: Shaded, ShadedWireframe, WireframeOnly, or Ghosted.
    pub view_mode: ViewMode,

    /// Normals visualization: Off, Face, Vertex, or FaceAndVertex.
    pub normals_mode: NormalsMode,

    /// Whether the ground grid is visible on launch.
    pub grid_visible: bool,

    /// Whether the axis orientation gizmo is visible on launch.
    pub axis_gizmo_visible: bool,

    /// Whether the bloom post-processing effect is enabled on launch.
    pub bloom_enabled: bool,
}

/// Rendering quality settings.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RenderingPrefs {
    /// Wireframe line thickness: Light (1px), Medium (2px), or Bold (3px).
    pub wireframe_line_weight: LineWeight,

    /// MSAA sample count. Valid values: 1 (off), 2, or 4. Applied on launch only.
    pub msaa_sample_count: u32,
}

/// Lighting behavior settings.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LightingPrefs {
    /// When true, lights are locked in world space.
    /// When false (default), lights follow the camera.
    pub lock: bool,
}

/// Usage history.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HistoryPrefs {
    /// Recently opened model file paths, most recent first. Max 20 entries.
    pub recent_files: Vec<String>,
}

// ---------------------------------------------------------------------------
// Defaults — match the original hardcoded values in state.rs
// ---------------------------------------------------------------------------

impl Default for Preferences {
    fn default() -> Self {
        Self {
            config_version: 1,
            display: DisplayPrefs::default(),
            rendering: RenderingPrefs::default(),
            lighting: LightingPrefs::default(),
            history: HistoryPrefs::default(),
        }
    }
}

impl Default for DisplayPrefs {
    fn default() -> Self {
        Self {
            background: BackgroundMode::Gradient,
            view_mode: ViewMode::Shaded,
            normals_mode: NormalsMode::Off,
            grid_visible: true,
            axis_gizmo_visible: false,
            bloom_enabled: true,
        }
    }
}

impl Default for RenderingPrefs {
    fn default() -> Self {
        Self {
            wireframe_line_weight: LineWeight::Medium,
            msaa_sample_count: 4,
        }
    }
}

// ---------------------------------------------------------------------------
// Config file I/O
// ---------------------------------------------------------------------------

const MAX_RECENT_FILES: usize = 20;

/// Returns the config file path: `<config_dir>/solarxy/config.toml`.
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("solarxy").join("config.toml"))
}

/// Load preferences from disk. Returns compiled-in defaults on any error.
pub fn load() -> Preferences {
    #[cfg(debug_assertions)]
    if let Some(ref path) = config_path() {
        eprintln!("[debug] Config path: {}", path.display());
    }

    let Some(path) = config_path() else {
        return Preferences::default();
    };

    let contents = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return Preferences::default(),
    };

    match toml::from_str::<Preferences>(&contents) {
        Ok(mut prefs) => {
            if !matches!(prefs.rendering.msaa_sample_count, 1 | 2 | 4) {
                eprintln!(
                    "Warning: invalid msaa_sample_count {} in config, falling back to 4",
                    prefs.rendering.msaa_sample_count
                );
                prefs.rendering.msaa_sample_count = 4;
            }
            prefs
        }
        Err(e) => {
            eprintln!("Warning: failed to parse {}: {}", path.display(), e);
            Preferences::default()
        }
    }
}

/// Save preferences to disk. Creates the config directory if needed.
pub fn save(prefs: &Preferences) -> Result<(), String> {
    let path = config_path().ok_or("Could not determine config directory")?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let toml_str = toml::to_string_pretty(prefs).map_err(|e| format!("Failed to serialize preferences: {}", e))?;

    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, &toml_str).map_err(|e| format!("Failed to write config: {}", e))?;
    std::fs::rename(&tmp_path, &path).map_err(|e| format!("Failed to finalize config: {}", e))?;

    Ok(())
}

/// Add a file path to the recent files list, deduplicate, and persist.
pub fn add_recent_file(prefs: &mut Preferences, path: &str) {
    let files = &mut prefs.history.recent_files;
    files.retain(|p| p != path);
    files.insert(0, path.to_string());
    files.truncate(MAX_RECENT_FILES);
    let _ = save(prefs);
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
            },
            rendering: RenderingPrefs {
                wireframe_line_weight: LineWeight::Bold,
                msaa_sample_count: 2,
            },
            lighting: LightingPrefs { lock: true },
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
            background = "Gradient"
            view_mode = "Shaded"
            normals_mode = "Off"
            grid_visible = true
            axis_gizmo_visible = false
            bloom_enabled = true
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
            // Don't actually save to disk in tests
            let files = &mut prefs.history.recent_files;
            let path = format!("/tmp/model_{}.obj", i);
            files.retain(|p| *p != path);
            files.insert(0, path);
            files.truncate(MAX_RECENT_FILES);
        }
        assert_eq!(prefs.history.recent_files.len(), MAX_RECENT_FILES);
        assert_eq!(prefs.history.recent_files[0], "/tmp/model_24.obj");

        // Add a duplicate — should move to front, not increase count
        let files = &mut prefs.history.recent_files;
        let dup = "/tmp/model_10.obj".to_string();
        files.retain(|p| *p != dup);
        files.insert(0, dup.clone());
        files.truncate(MAX_RECENT_FILES);
        assert_eq!(prefs.history.recent_files.len(), MAX_RECENT_FILES);
        assert_eq!(prefs.history.recent_files[0], "/tmp/model_10.obj");
    }

    #[test]
    fn config_path_returns_some() {
        assert!(config_path().is_some());
    }
}
