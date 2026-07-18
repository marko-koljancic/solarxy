//! Solarxy's user preferences: the `~/.config/solarxy/config.toml` schema
//! plus every cycle-able enum shared between sidebar UI and shader uniforms.
//!
//! - [`Preferences`] — the root struct, loaded by [`load`] and saved by
//!   [`save`]. Sub-structs (`display`/`rendering`/`lighting`/`window`/
//!   `history`/`ui`/`updater`) each use `#[serde(default)]` so older config
//!   files upgrade cleanly when new fields are added.
//! - [`config_path`] — platform-specific location via `dirs::config_dir()`.
//! - Cycle-able enums ([`ViewMode`], [`IblMode`], [`ToneMode`], etc.) are
//!   produced by an internal `cycle_enum!` macro that emits `Display` plus
//!   an `ALL: &[Self]` slice — sidebar dropdowns iterate `ALL` directly.
//!
//! `IblMode` toggles drive the `rebuild_light_bind_group` chokepoint in
//! `solarxy-app/src/state/update.rs`. See `IblMode` variants for semantics.
//!
//! Available with the `serialization` feature.

use serde::{Deserialize, Serialize};
#[cfg(feature = "fs")]
use std::path::PathBuf;

macro_rules! cycle_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $display:expr ),+ $(,)?
        }
        ; cycle
    ) => {
        cycle_enum!(@base $(#[$meta])* $vis enum $name {
            $( $(#[$vmeta])* $variant => $display ),+
        });
        impl $name {
            pub fn next(self) -> Self {
                let i = Self::ALL.iter().position(|v| *v == self).unwrap_or(0);
                Self::ALL[(i + 1) % Self::ALL.len()]
            }
        }
    };

    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $display:expr ),+ $(,)?
        }
    ) => {
        cycle_enum!(@base $(#[$meta])* $vis enum $name {
            $( $(#[$vmeta])* $variant => $display ),+
        });
    };

    (@base
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident => $display:expr ),+
        }
    ) => {
        $(#[$meta])*
        $vis enum $name { $( $(#[$vmeta])* $variant ),+ }

        impl $name {
            pub const ALL: &[Self] = &[$( Self::$variant ),+];
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( Self::$variant => write!(f, $display) ),+
                }
            }
        }
    };
}

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

cycle_enum! {
    /// Wireframe stroke weight, in screen pixels (the edge wireframe is a
    /// screen-space quad expansion, so these are true widths, not a GPU
    /// line-width limit).
    ///
    /// `Light` is the default on every shell. It used to be stated twice —
    /// here via `RenderingPrefs` and again as a hardcoded `Medium` in the web
    /// host — and the two disagreed, so the same scene drew a 1 px wireframe
    /// on desktop and a 2 px one on web.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum LineWeight {
        #[default]
        Light => "Light",
        Medium => "Medium",
        Bold => "Bold",
    }
    ; cycle
}

impl LineWeight {
    pub fn width_px(self) -> f32 {
        match self {
            Self::Light => 1.0,
            Self::Medium => 2.0,
            Self::Bold => 3.0,
        }
    }

    /// Name paired with the pixel width — `"Light (1 px)"` etc. The bare
    /// `Display` string ("Light"/"Medium"/"Bold") is meaningless without
    /// context; this is used wherever the control stands alone (the UV
    /// pane toolbar's wireframe-weight dropdown).
    pub fn descriptive_label(self) -> &'static str {
        match self {
            Self::Light => "Light (1 px)",
            Self::Medium => "Medium (2 px)",
            Self::Bold => "Bold (3 px)",
        }
    }
}

cycle_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum NormalsMode {
        Off => "Off",
        Face => "Face",
        Vertex => "Vertex",
        FaceAndVertex => "Face+Vertex",
    }
    ; cycle
}

cycle_enum! {
    /// The six predefined viewport backgrounds. `HdriSky` renders the
    /// loaded HDRI as a visible sky (gated on an HDRI being present).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum BuiltinBg {
        White => "White",
        Gradient => "Gradient",
        DarkGray => "Dark",
        AyuMirage => "Ayu Mirage",
        Black => "Black",
        HdriSky => "HDRI Sky",
    }
}

cycle_enum! {
    /// Authoring kind of a user [`CustomBackground`].
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum CustomBgKind {
        #[default]
        Solid => "Solid",
        Gradient => "Gradient",
    }
}

/// How a [`ResolvedBackground`] is drawn by the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgKind {
    /// Flat fill — only the render-pass clear colour is used.
    Solid,
    /// Vertical two-colour gradient pass.
    Gradient,
    /// HDRI equirect skybox pass.
    Hdri,
}

/// A background resolved to concrete colours — the registry-free form the
/// renderer and IBL consume. Builtins and customs both resolve to this
/// via [`BackgroundMode::resolve`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedBackground {
    pub kind: BgKind,
    /// Render-pass clear colour (linear RGB).
    pub clear: [f32; 3],
    /// Upper sky colour — gradient-pass top + IBL upper hemisphere.
    pub sky_top: [f32; 3],
    /// Lower sky colour — gradient-pass bottom + IBL lower hemisphere.
    pub sky_bottom: [f32; 3],
}

impl BuiltinBg {
    /// Concrete colours for each builtin. Hand-tuned; the renderer's
    /// `BackgroundModeExt` derives clear / grid / wireframe from these.
    #[must_use]
    pub fn resolved(self) -> ResolvedBackground {
        match self {
            Self::White => ResolvedBackground {
                kind: BgKind::Solid,
                clear: [1.0, 1.0, 1.0],
                sky_top: [1.0, 1.0, 1.0],
                sky_bottom: [0.85, 0.85, 0.85],
            },
            Self::Gradient => ResolvedBackground {
                kind: BgKind::Gradient,
                clear: [0.165, 0.165, 0.180],
                sky_top: [0.66, 0.70, 0.72],
                sky_bottom: [0.35, 0.41, 0.47],
            },
            Self::DarkGray => ResolvedBackground {
                kind: BgKind::Solid,
                clear: [0.12, 0.12, 0.12],
                sky_top: [0.30, 0.32, 0.35],
                sky_bottom: [0.15, 0.14, 0.13],
            },
            Self::AyuMirage => ResolvedBackground {
                kind: BgKind::Solid,
                clear: [0.122, 0.141, 0.188],
                sky_top: [0.122 * 1.4, 0.141 * 1.4, 0.188 * 1.4],
                sky_bottom: [0.122 * 0.6, 0.141 * 0.6, 0.188 * 0.6],
            },
            Self::Black => ResolvedBackground {
                kind: BgKind::Solid,
                clear: [0.0, 0.0, 0.0],
                sky_top: [0.20, 0.22, 0.25],
                sky_bottom: [0.08, 0.07, 0.06],
            },
            // HDRI shares Gradient's neutral fallback — the skybox pass
            // covers the pane once an HDRI is loaded.
            Self::HdriSky => ResolvedBackground {
                kind: BgKind::Hdri,
                clear: [0.165, 0.165, 0.180],
                sky_top: [0.66, 0.70, 0.72],
                sky_bottom: [0.35, 0.41, 0.47],
            },
        }
    }
}

/// A user-defined background — a named solid fill or two-colour vertical
/// gradient. Stored in [`ViewPrefs::custom_backgrounds`]; panes reference
/// it by [`CustomBackground::id`] through [`BackgroundMode::Custom`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomBackground {
    /// Stable identifier — assigned from [`ViewPrefs::next_custom_id`] and
    /// never reused, so a pane referencing a deleted custom falls back
    /// cleanly rather than aliasing a different one.
    pub id: u32,
    pub name: String,
    pub kind: CustomBgKind,
    /// Solid: the fill colour. Gradient: the top colour. Linear RGB.
    pub top: [f32; 3],
    /// Gradient: the bottom colour. Unused when `kind` is `Solid`.
    pub bottom: [f32; 3],
}

impl CustomBackground {
    #[must_use]
    pub fn resolved(&self) -> ResolvedBackground {
        match self.kind {
            CustomBgKind::Solid => ResolvedBackground {
                kind: BgKind::Solid,
                clear: self.top,
                sky_top: self.top,
                sky_bottom: self.top,
            },
            CustomBgKind::Gradient => ResolvedBackground {
                kind: BgKind::Gradient,
                clear: self.bottom,
                sky_top: self.top,
                sky_bottom: self.bottom,
            },
        }
    }
}

/// A pane's background choice — a builtin or a reference to a user
/// [`CustomBackground`]. `#[serde(untagged)]` keeps builtins serialized
/// as plain strings (so pre-RC2 `config.toml` files still load) and
/// customs as their integer id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BackgroundMode {
    Builtin(BuiltinBg),
    Custom(u32),
}

impl BackgroundMode {
    pub const WHITE: Self = Self::Builtin(BuiltinBg::White);
    pub const GRADIENT: Self = Self::Builtin(BuiltinBg::Gradient);
    pub const BLACK: Self = Self::Builtin(BuiltinBg::Black);
    pub const HDRI_SKY: Self = Self::Builtin(BuiltinBg::HdriSky);

    /// Resolve to concrete colours, looking a `Custom` up in `customs`.
    /// A dangling id (custom deleted while a pane still referenced it)
    /// falls back to the builtin Gradient.
    #[must_use]
    pub fn resolve(self, customs: &[CustomBackground]) -> ResolvedBackground {
        match self {
            Self::Builtin(b) => b.resolved(),
            Self::Custom(id) => customs.iter().find(|c| c.id == id).map_or_else(
                || BuiltinBg::Gradient.resolved(),
                CustomBackground::resolved,
            ),
        }
    }

    /// `true` when this is the HDRI-sky builtin.
    #[must_use]
    pub fn is_hdri_sky(self) -> bool {
        self == Self::HDRI_SKY
    }

    /// Human-readable name — builtin display string, or the custom's name
    /// (a placeholder if the id is dangling).
    #[must_use]
    pub fn label(self, customs: &[CustomBackground]) -> String {
        match self {
            Self::Builtin(b) => b.to_string(),
            Self::Custom(id) => customs
                .iter()
                .find(|c| c.id == id)
                .map_or_else(|| format!("Custom #{id}"), |c| c.name.clone()),
        }
    }
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum UvMode {
        #[default]
        Off => "Off",
        Gradient => "Gradient",
        Checker => "Checker",
    }
    ; cycle
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum ProjectionMode {
        #[default]
        Perspective => "Perspective",
        Orthographic => "Orthographic",
    }
    ; cycle
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum IblMode {
        Off => "Off",
        Diffuse => "Diffuse",
        #[default]
        Full => "Full",
    }
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum PaneMode {
        #[default]
        Scene3D => "Scene 3D",
        UvMap => "UV Map",
    }
    ; cycle
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum UvMapBackground {
        #[default]
        Dark => "Dark",
        Charcoal => "Charcoal",
        Gray => "Gray",
        Checker => "Checker",
        Texture => "Texture",
    }
    ; cycle
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum InspectionMode {
        #[default]
        Shaded => "Shaded",
        MaterialId => "Material ID",
        TexelDensity => "Texel Density",
        Depth => "Depth",
        Overdraw => "Overdraw",
        AoPreview => "AO Preview",
    }
}

impl InspectionMode {
    /// Discriminant passed to shader-side `inspection_mode` uniforms.
    ///
    /// - `Shaded` (0): default PBR rendering — no special case in shaders.
    /// - `MaterialId` (1): per-material hashed colors — see `shader.wgsl`.
    /// - `TexelDensity` (2): UV-derivative-based density ramp — see `shader.wgsl`.
    /// - `Depth` (3): linearized depth ramp — see `shader.wgsl`.
    /// - `Overdraw` (4): handled outside `shader.wgsl` via a dedicated
    ///   count+show pipeline pair in `solarxy-renderer/src/overdraw.rs`.
    /// - `AoPreview` (5): handled in `composite.wgsl` — composite samples
    ///   the SSAO buffer directly and bypasses scene tone-mapping.
    pub fn as_u32(self) -> u32 {
        match self {
            Self::Shaded => 0,
            Self::MaterialId => 1,
            Self::TexelDensity => 2,
            Self::Depth => 3,
            Self::Overdraw => 4,
            Self::AoPreview => 5,
        }
    }
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum MaterialOverride {
        #[default]
        None => "Textured",
        Clay => "Clay Light",
        ClayDark => "Clay Dark",
        Chrome => "Chrome",
        Silhouette => "Silhouette",
    }
    ; cycle
}

impl MaterialOverride {
    pub fn as_u32(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Clay => 1,
            Self::ClayDark => 2,
            Self::Chrome => 3,
            Self::Silhouette => 4,
        }
    }
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Serialize, Deserialize)]
    pub enum ToneMode {
        None => "None (clip)",
        Linear => "Linear",
        Reinhard => "Reinhard",
        #[default]
        AcesFilmic => "ACES Filmic",
    }
    ; cycle
}

impl ToneMode {
    pub fn as_u32(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Linear => 1,
            Self::Reinhard => 2,
            Self::AcesFilmic => 3,
        }
    }
}

cycle_enum! {
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum UpdaterChannel {
        #[default]
        Stable => "Stable",
        Prerelease => "Prerelease",
    }
    ; cycle
}

cycle_enum! {
    /// User-selectable interface theme. Resolves to a
    /// [`crate::theme::Palette`], which is shared with the web frontend and
    /// the analyze TUI. The GUI hot-swaps between the two without a restart.
    ///
    /// The variants were `AyuMirageDark`/`AyuMirageLight` before 0.7.1, when
    /// the desktop carried its own Ayu-derived palette. The aliases are load
    /// bearing: this enum is serialized into `~/.config/solarxy/config.toml`,
    /// so without them every existing user's theme choice fails to
    /// deserialize and silently resets to the default.
    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    pub enum ThemeChoice {
        #[default]
        #[serde(alias = "AyuMirageDark")]
        Dark => "Dark",
        #[serde(alias = "AyuMirageLight")]
        Light => "Light",
    }
    ; cycle
}

impl ThemeChoice {
    /// The palette this choice selects.
    pub const fn palette(self) -> crate::theme::Palette {
        match self {
            Self::Dark => crate::theme::Palette::dark(),
            Self::Light => crate::theme::Palette::light(),
        }
    }
}

/// Root TOML structure persisted at `~/.config/solarxy/config.toml`
/// (via [`config_path`]).
///
/// Every sub-section is `#[serde(default)]` so older config files load
/// cleanly when new sections are added across releases. `config_version`
/// is reserved for future migrations; the loader currently treats every
/// version as readable and lets serde fill in unknown fields via defaults.
///
/// Three edit surfaces mutate this struct, each authoritative for a
/// different slice (see CLAUDE.md "Key Patterns" for the canonical
/// split):
/// - GUI **Edit → Preferences…** (`Ctrl/⌘+,`) — startup-only fields
///   (window size, MSAA), UI defaults, updater behaviour.
/// - GUI sidebar + **Edit → Save View Settings as Default** — live
///   per-session display / rendering / lighting settings.
/// - Direct TOML editing — anything; reload via the modal's
///   **Open config file** button.
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
    #[serde(default)]
    pub ui: UiPrefs,
    #[serde(default)]
    pub updater: UpdaterPrefs,
    #[serde(default)]
    pub review: ReviewPrefs,
    #[serde(default)]
    pub dock: DockPrefs,
    #[serde(default)]
    pub view: ViewPrefs,
}

/// View-related preferences — currently the user's custom-background
/// registry. `default_background` keeps living on [`DisplayPrefs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct ViewPrefs {
    /// User-defined backgrounds, in display order. Referenced by panes
    /// through [`BackgroundMode::Custom`].
    #[serde(default)]
    pub custom_backgrounds: Vec<CustomBackground>,
    /// Monotonic id allocator for new customs — never decremented, so a
    /// deleted id is never reused. See [`CustomBackground::id`].
    #[serde(default)]
    pub next_custom_id: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayPrefs {
    #[serde(default = "default_background")]
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

fn default_background() -> BackgroundMode {
    BackgroundMode::GRADIENT
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

pub const MAX_RECENT_FILES_CAP: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiPrefs {
    #[serde(default = "default_max_recent_files")]
    pub max_recent_files: usize,
    #[serde(default = "default_true")]
    pub status_bar_visible: bool,
    #[serde(default)]
    pub theme: ThemeChoice,
}

fn default_max_recent_files() -> usize {
    20
}

impl Default for UiPrefs {
    fn default() -> Self {
        Self {
            max_recent_files: default_max_recent_files(),
            status_bar_visible: true,
            theme: ThemeChoice::default(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct UpdaterPrefs {
    #[serde(default)]
    pub check_on_launch: bool,
    #[serde(default)]
    pub channel: UpdaterChannel,
}

/// Persisted dock layout. JSON strings hold a serialized
/// `egui_dock::DockState<SolarxyTab>` (the `egui_dock` 0.18 `serde` feature
/// derives `Serialize`/`Deserialize` on `DockState<T: Serialize>`).
///
/// Two slots:
/// - `last_layout_json` is auto-written on app quit and restored on launch.
/// - `saved_layout_json` is written only by Window → Save Layout and
///   replayed by Window → Restore Saved Layout. Independent from auto-save
///   so the user can mess up the live layout without losing their snapshot.
///
/// Deserialization failures (e.g. after a `SolarxyTab` variant bump) fall
/// back to the default layout and log a debug line — never panic.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DockPrefs {
    #[serde(default)]
    pub last_layout_json: Option<String>,
    #[serde(default)]
    pub saved_layout_json: Option<String>,
}

/// User-level review-mode preferences. Project-level settings (sidecar
/// location, etc.) live in [`crate::project_config::ReviewSettings`].
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ReviewPrefs {
    /// Display name written to `ReviewAnnotation.author` on new annotations.
    /// `None` ⇒ anonymous. Solarxy deliberately does NOT auto-derive from
    /// `git config user.name` or OS username — attribution is opt-in.
    #[serde(default)]
    pub author: Option<String>,

    /// Whether the review side panel is visible by default on app launch.
    /// Persists across sessions; mirror flag for the panel's open state.
    #[serde(default)]
    pub panel_open: bool,
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
            ui: UiPrefs::default(),
            updater: UpdaterPrefs::default(),
            review: ReviewPrefs::default(),
            dock: DockPrefs::default(),
            view: ViewPrefs::default(),
        }
    }
}

impl Default for DisplayPrefs {
    fn default() -> Self {
        Self {
            background: BackgroundMode::GRADIENT,
            view_mode: ViewMode::Shaded,
            normals_mode: NormalsMode::Off,
            grid_visible: true,
            axis_gizmo_visible: true,
            bloom_enabled: true,
            uv_mode: UvMode::Off,
            projection_mode: ProjectionMode::Perspective,
            turntable_active: false,
            ibl_mode: IblMode::Full,
            ssao_enabled: true,
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
            wireframe_line_weight: LineWeight::default(),
            msaa_sample_count: 4,
            shadow_map_size: 2048,
        }
    }
}

#[cfg(feature = "fs")]
pub fn config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("solarxy").join("config.toml"))
}

#[cfg(feature = "fs")]
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
            prefs.ui.max_recent_files = prefs.ui.max_recent_files.clamp(1, MAX_RECENT_FILES_CAP);
            prefs
        }
        Err(e) => {
            tracing::warn!("Failed to parse {}: {}", path.display(), e);
            Preferences::default()
        }
    }
}

#[cfg(feature = "fs")]
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

#[cfg(feature = "fs")]
pub fn add_recent_file(prefs: &mut Preferences, path: &str) {
    let cap = prefs.ui.max_recent_files.max(1);
    let files = &mut prefs.history.recent_files;
    files.retain(|p| p != path);
    files.insert(0, path.to_string());
    files.truncate(cap);
    if let Err(e) = save(prefs) {
        tracing::warn!("Failed to save recent files: {e}");
    }
}

// The roundtrip tests serialize through toml, which rides the `fs` feature.
#[cfg(all(test, feature = "fs"))]
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
                background: BackgroundMode::BLACK,
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
            ui: UiPrefs {
                max_recent_files: 10,
                status_bar_visible: false,
                theme: ThemeChoice::Light,
            },
            updater: UpdaterPrefs {
                check_on_launch: true,
                channel: UpdaterChannel::Prerelease,
            },
            review: ReviewPrefs {
                author: Some("Marko".to_string()),
                panel_open: true,
            },
            dock: DockPrefs {
                last_layout_json: Some(r#"{"surfaces":[]}"#.to_string()),
                saved_layout_json: None,
            },
            view: ViewPrefs {
                custom_backgrounds: vec![
                    CustomBackground {
                        id: 0,
                        name: "Studio".to_string(),
                        kind: CustomBgKind::Solid,
                        top: [0.2, 0.2, 0.22],
                        bottom: [0.0, 0.0, 0.0],
                    },
                    CustomBackground {
                        id: 1,
                        name: "Sunset".to_string(),
                        kind: CustomBgKind::Gradient,
                        top: [0.9, 0.5, 0.2],
                        bottom: [0.1, 0.1, 0.3],
                    },
                ],
                next_custom_id: 2,
            },
        };
        let toml_str = toml::to_string_pretty(&prefs).unwrap();
        let parsed: Preferences = toml::from_str(&toml_str).unwrap();
        assert_eq!(prefs, parsed);
    }

    #[test]
    fn review_prefs_default_is_anonymous_closed() {
        let r = ReviewPrefs::default();
        assert!(
            r.author.is_none(),
            "default author must be None (anonymous)"
        );
        assert!(!r.panel_open, "default panel_open must be false");
    }

    #[test]
    fn review_prefs_missing_section_uses_defaults() {
        let toml_str = r"
            config_version = 1
        ";
        let parsed: Preferences = toml::from_str(toml_str).expect("parses without [review]");
        assert!(parsed.review.author.is_none());
        assert!(!parsed.review.panel_open);
    }

    #[test]
    fn review_prefs_partial_section_fills_missing_with_defaults() {
        let toml_str = r#"
            config_version = 1

            [review]
            author = "Marko"
        "#;
        let parsed: Preferences = toml::from_str(toml_str).expect("parses with partial [review]");
        assert_eq!(parsed.review.author.as_deref(), Some("Marko"));
        assert!(
            !parsed.review.panel_open,
            "panel_open defaults when omitted"
        );
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
            bloom_enabled = false
            future_toggle = true

            [rendering]
            wireframe_line_weight = "Medium"
            msaa_sample_count = 8
            future_quality = 9001

            [lighting]
            lock = true

            [some_future_section]
            key = "value"
        "#;
        let parsed: Preferences = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.display.background, BackgroundMode::BLACK);
        assert!(!parsed.display.bloom_enabled);
        assert_eq!(parsed.rendering.wireframe_line_weight, LineWeight::Medium);
        assert_eq!(parsed.rendering.msaa_sample_count, 8);
        assert!(parsed.lighting.lock);
        assert!(parsed.display.ssao_enabled);
        assert!((parsed.display.exposure - 1.0).abs() < f32::EPSILON);
        assert_eq!(parsed.window, WindowPrefs::default());
        assert_eq!(parsed.history, HistoryPrefs::default());
        assert_eq!(parsed.ui, UiPrefs::default());
        assert_eq!(parsed.updater, UpdaterPrefs::default());
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
        assert_eq!(parsed.ui, UiPrefs::default());
        assert_eq!(parsed.updater, UpdaterPrefs::default());
        assert_eq!(parsed.display.background, BackgroundMode::BLACK);
    }

    #[test]
    fn corrupt_toml_fails_parse() {
        let result = toml::from_str::<Preferences>("{{{{garbage}}}}");
        assert!(result.is_err());
    }

    #[test]
    fn recent_files_dedup_and_truncate() {
        let mut prefs = Preferences::default();
        let cap = prefs.ui.max_recent_files;
        for i in 0..(cap + 5) {
            let files = &mut prefs.history.recent_files;
            let path = format!("/tmp/model_{}.obj", i);
            files.retain(|p| *p != path);
            files.insert(0, path);
            files.truncate(cap);
        }
        assert_eq!(prefs.history.recent_files.len(), cap);
        assert_eq!(
            prefs.history.recent_files[0],
            format!("/tmp/model_{}.obj", cap + 4)
        );

        let files = &mut prefs.history.recent_files;
        let dup = format!("/tmp/model_{}.obj", cap / 2);
        files.retain(|p| *p != dup);
        files.insert(0, dup.clone());
        files.truncate(cap);

        assert_eq!(prefs.history.recent_files.len(), cap);
        assert_eq!(prefs.history.recent_files[0], dup);
    }

    #[test]
    fn ui_prefs_defaults_match_observed() {
        let ui = UiPrefs::default();
        assert!(ui.status_bar_visible);
        assert_eq!(ui.max_recent_files, 20);
    }

    #[test]
    fn updater_prefs_defaults_are_conservative() {
        let u = UpdaterPrefs::default();
        assert!(!u.check_on_launch);
        assert_eq!(u.channel, UpdaterChannel::Stable);
    }

    #[test]
    fn updater_channel_cycles() {
        assert_eq!(UpdaterChannel::Stable.next(), UpdaterChannel::Prerelease);
        assert_eq!(UpdaterChannel::Prerelease.next(), UpdaterChannel::Stable);
    }

    #[test]
    fn theme_choice_defaults_to_dark_and_cycles() {
        assert_eq!(ThemeChoice::default(), ThemeChoice::Dark);
        assert_eq!(UiPrefs::default().theme, ThemeChoice::Dark);
        assert_eq!(ThemeChoice::Dark.next(), ThemeChoice::Light);
        assert_eq!(ThemeChoice::Light.next(), ThemeChoice::Dark);
    }

    /// 0.7.1 renamed the variants from `AyuMirageDark`/`AyuMirageLight`.
    /// A config.toml written by any earlier release must still load, or the
    /// upgrade silently throws away the user's theme choice.
    #[test]
    fn pre_0_7_1_theme_names_still_deserialize() {
        let old: UiPrefs = toml::from_str(r#"theme = "AyuMirageLight""#).expect("legacy light");
        assert_eq!(old.theme, ThemeChoice::Light);

        let old: UiPrefs = toml::from_str(r#"theme = "AyuMirageDark""#).expect("legacy dark");
        assert_eq!(old.theme, ThemeChoice::Dark);

        // And the current names round-trip.
        for choice in [ThemeChoice::Dark, ThemeChoice::Light] {
            let ui = UiPrefs {
                theme: choice,
                ..UiPrefs::default()
            };
            let text = toml::to_string(&ui).expect("serialize");
            let back: UiPrefs = toml::from_str(&text).expect("round trip");
            assert_eq!(back.theme, choice);
        }
    }

    /// A whole legacy `config.toml` section, not just the enum: this is the
    /// shape that actually sits on users' disks today.
    #[test]
    fn a_v0_7_0_config_file_loads_with_its_theme_intact() {
        let toml_text = r#"
            config_version = 1

            [ui]
            theme = "AyuMirageLight"
        "#;
        let prefs: Preferences = toml::from_str(toml_text).expect("v0.7.0 config must load");
        assert_eq!(prefs.ui.theme, ThemeChoice::Light);
    }

    #[test]
    fn theme_choice_selects_the_matching_palette() {
        assert!(ThemeChoice::Dark.palette().dark);
        assert!(!ThemeChoice::Light.palette().dark);
    }

    /// The wireframe default is stated in two places that must agree: this
    /// enum (which the web host seeds its renderer from) and `RenderingPrefs`
    /// (which the desktop persists). They did not agree before 0.7.1 — the
    /// web host hardcoded `Medium` against the desktop's `Light`, so the same
    /// scene drew a 2 px wireframe in one shell and 1 px in the other.
    #[test]
    fn the_wireframe_default_is_the_same_on_every_shell() {
        assert_eq!(LineWeight::default(), LineWeight::Light);
        assert_eq!(
            RenderingPrefs::default().wireframe_line_weight,
            LineWeight::default(),
        );
        assert!((LineWeight::default().width_px() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn line_weights_are_ordered_and_distinct() {
        let widths: Vec<f32> = LineWeight::ALL.iter().map(|w| w.width_px()).collect();
        assert_eq!(widths, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn rc8_era_toml_upgrades_cleanly() {
        let toml_str = r#"
            config_version = 1

            [display]
            background = "Gradient"
            view_mode = "Shaded"
            normals_mode = "Off"
            grid_visible = true
            axis_gizmo_visible = true
            bloom_enabled = true
        "#;
        let parsed: Preferences = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.ui, UiPrefs::default());
        assert_eq!(parsed.updater, UpdaterPrefs::default());
    }

    #[test]
    fn window_prefs_clamped() {
        let toml_str = r"
            config_version = 1

            [window]
            window_width = 100
            window_height = 99999
        ";
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

    #[test]
    fn background_mode_untagged_serializes_compactly() {
        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct Wrap {
            bg: BackgroundMode,
        }
        // Builtin → bare string (pre-RC2 config.toml stays readable);
        // custom → bare integer id.
        let builtin = toml::to_string(&Wrap {
            bg: BackgroundMode::GRADIENT,
        })
        .unwrap();
        assert_eq!(builtin.trim(), r#"bg = "Gradient""#);
        let custom = toml::to_string(&Wrap {
            bg: BackgroundMode::Custom(7),
        })
        .unwrap();
        assert_eq!(custom.trim(), "bg = 7");

        for mode in [
            BackgroundMode::GRADIENT,
            BackgroundMode::BLACK,
            BackgroundMode::HDRI_SKY,
            BackgroundMode::Custom(42),
        ] {
            let w = Wrap { bg: mode };
            let s = toml::to_string(&w).unwrap();
            assert_eq!(toml::from_str::<Wrap>(&s).unwrap(), w);
        }
    }

    #[test]
    fn legacy_background_string_still_parses() {
        // A pre-RC2 config wrote `background = "AyuMirage"` (the serde
        // variant name) — it must still load as the matching builtin.
        let toml_str = r#"
            config_version = 1
            [display]
            background = "AyuMirage"
            view_mode = "Shaded"
            normals_mode = "Off"
            grid_visible = true
            axis_gizmo_visible = true
            bloom_enabled = true
        "#;
        let parsed: Preferences = toml::from_str(toml_str).unwrap();
        assert_eq!(
            parsed.display.background,
            BackgroundMode::Builtin(BuiltinBg::AyuMirage)
        );
    }

    #[test]
    fn background_resolve_falls_back_for_dangling_custom() {
        let customs = vec![CustomBackground {
            id: 3,
            name: "Studio".to_string(),
            kind: CustomBgKind::Solid,
            top: [0.4, 0.4, 0.4],
            bottom: [0.0, 0.0, 0.0],
        }];
        // Present id resolves to the custom's colours.
        let hit = BackgroundMode::Custom(3).resolve(&customs);
        assert_eq!(hit.kind, BgKind::Solid);
        for channel in hit.clear {
            assert!((channel - 0.4).abs() < 1e-6);
        }
        // Dangling id falls back to the builtin Gradient.
        let miss = BackgroundMode::Custom(99).resolve(&customs);
        assert_eq!(miss, BuiltinBg::Gradient.resolved());
    }
}
