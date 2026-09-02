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
    BackgroundMode, InspectionMode, LineWeight, MaterialOverride, NormalsMode, PaneMode, ToneMode,
    UvMapBackground, UvMode, ViewMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ViewLayout {
    #[default]
    Single,
    SplitVertical,
    SplitHorizontal,
    Quad,
    ThreeLeftBig,
}

impl ViewLayout {
    #[must_use]
    pub fn pane_count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::SplitVertical | Self::SplitHorizontal => 2,
            Self::ThreeLeftBig => 3,
            Self::Quad => 4,
        }
    }
}

/// On-screen size of a rendered point, in pixels.
///
/// The renderer expands point primitives into camera-facing quads (WebGPU
/// rasterizes a point-list at exactly one pixel with no size control), so
/// this is a real screen-space size rather than a hint.
pub const DEFAULT_POINT_SIZE: f32 = 6.0;
/// The usable range. Below 1 a point stops being visible at all; above 32 it
/// stops reading as a point and starts occluding the geometry it annotates.
pub const MIN_POINT_SIZE: f32 = 1.0;
pub const MAX_POINT_SIZE: f32 = 32.0;

/// Multiplier on the HDRI's lighting contribution when it is used as
/// authored. Not zero: an unset intensity must leave the scene lit exactly
/// as it was before the control existed.
pub const DEFAULT_HDRI_INTENSITY: f32 = 1.0;
/// The usable range. Zero kills the image-based lighting entirely, which
/// the IBL mode control already expresses more clearly; the ceiling is
/// where an HDRI stops reading as light and starts blowing out.
pub const MIN_HDRI_INTENSITY: f32 = 0.0;
pub const MAX_HDRI_INTENSITY: f32 = 8.0;

/// Height of the per-pane viewport toolbar strip, in logical pixels.
/// Each pane's 3D content is the pane rect minus this strip at the top.
pub const PANE_TOOLBAR_HEIGHT: f32 = 22.0;

/// How much of the blurred bright pass is added back. Neutral is not zero:
/// these are the values the two effects shipped with as compiled-in
/// constants, so a configuration that has never touched a control has to
/// render exactly what the previous release rendered.
pub const DEFAULT_BLOOM_STRENGTH: f32 = 0.8;
/// Luminance above which a pixel contributes to the bright pass.
pub const DEFAULT_BLOOM_THRESHOLD: f32 = 0.8;
/// How far the composite blends towards the occlusion buffer.
pub const DEFAULT_SSAO_STRENGTH: f32 = 0.8;

/// The usable ranges. Zero on any of the three is the effect turned off by
/// another name, which the existing toggles already express, so it is a
/// legitimate floor rather than a degenerate one. The bloom ceiling is
/// where the add stops reading as glow and starts reading as a blown
/// image; occlusion is a blend factor and so cannot exceed one.
pub const MIN_BLOOM_STRENGTH: f32 = 0.0;
pub const MAX_BLOOM_STRENGTH: f32 = 4.0;
pub const MIN_BLOOM_THRESHOLD: f32 = 0.0;
pub const MAX_BLOOM_THRESHOLD: f32 = 4.0;
pub const MIN_SSAO_STRENGTH: f32 = 0.0;
pub const MAX_SSAO_STRENGTH: f32 = 1.0;

/// The three post-processing intensities, as one value.
///
/// Renderer-global rather than per pane, which is the same shape the two
/// effects themselves have: one post-processing state and one set of
/// targets. The per-pane look the camera owns (exposure, tone, grade)
/// deliberately excludes these, because a strength describes how the
/// effect is built rather than how the shot is graded.
///
/// [`Default`] is the shipped look. That is load-bearing in the same way
/// [`crate::scene::CameraLook`]'s neutral default is: it is what lets the
/// controls ship without moving a single golden capture.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostStrengths {
    pub bloom_strength: f32,
    pub bloom_threshold: f32,
    pub ssao_strength: f32,
}

impl Default for PostStrengths {
    fn default() -> Self {
        Self {
            bloom_strength: DEFAULT_BLOOM_STRENGTH,
            bloom_threshold: DEFAULT_BLOOM_THRESHOLD,
            ssao_strength: DEFAULT_SSAO_STRENGTH,
        }
    }
}

impl PostStrengths {
    /// Clamped into the usable ranges.
    ///
    /// Applied where the value enters the renderer rather than where a
    /// control writes it, because the value arrives from three places (two
    /// preference files and a slider) and only one of them is a widget with
    /// a range of its own.
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            bloom_strength: self
                .bloom_strength
                .clamp(MIN_BLOOM_STRENGTH, MAX_BLOOM_STRENGTH),
            bloom_threshold: self
                .bloom_threshold
                .clamp(MIN_BLOOM_THRESHOLD, MAX_BLOOM_THRESHOLD),
            ssao_strength: self
                .ssao_strength
                .clamp(MIN_SSAO_STRENGTH, MAX_SSAO_STRENGTH),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplaySettings {
    pub turntable_active: bool,
    pub turntable_rpm: f32,
    pub lights_locked: bool,
    pub layout: ViewLayout,
    pub split_ratio: f32,
    pub roughness_scale: f32,
    pub metallic_scale: f32,
    /// Scene-global HDRI yaw, in radians. Rotates the visible HDRI sky
    /// and the IBL it derives together. `0.0` when no HDRI is loaded.
    pub hdri_rotation: f32,
    /// Scene-global multiplier on the HDRI's lighting contribution, with
    /// `1.0` meaning "as authored". Scales the image-based lighting only,
    /// not the visible sky, so a backdrop can stay readable while the key
    /// it casts is dialed up or down.
    ///
    /// Defaulted rather than required because view sidecars written before
    /// the environment node existed carry no such field, and `0.0` there
    /// would load an unlit scene.
    #[serde(default = "default_hdri_intensity")]
    pub hdri_intensity: f32,
    /// On-screen point size in pixels.
    ///
    /// Global rather than per pane, unlike `line_weight`: there is no
    /// comparison worth two point sizes side by side, and a global keeps it
    /// to one field instead of a per-pane Display-menu entry.
    #[serde(default = "default_point_size")]
    pub point_size: f32,
}

fn default_point_size() -> f32 {
    DEFAULT_POINT_SIZE
}

fn default_hdri_intensity() -> f32 {
    DEFAULT_HDRI_INTENSITY
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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

/// A free pane's own rendering intent.
///
/// The counterpart to `scene::CameraLook`, and deliberately the smaller of
/// the two: a pane looking through a camera composites with that camera's
/// look, and a pane looking at nothing in particular gets this. It carries
/// the scalar half only, no lookup tables, because a table is a staged
/// document asset and a free pane is a viewport rather than a document
/// object. Load a table by pointing a camera at it.
///
/// Separate from [`PaneDisplaySettings`] rather than more fields on it,
/// because that struct is constructed as a literal in the golden harness
/// and mirrored field for field in the frontend; the look is a different
/// concern with a different lifetime and it reads better apart.
///
/// [`Default`] is neutral, and neutral is bit-identical: the renderer
/// skips the grade at these values rather than multiplying by one.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaneLook {
    #[serde(default = "default_exposure")]
    pub exposure: f32,
    #[serde(default)]
    pub tone_mode: ToneMode,
    #[serde(default)]
    pub lift: [f32; 3],
    #[serde(default = "unit_vec3")]
    pub gamma: [f32; 3],
    #[serde(default = "unit_vec3")]
    pub gain: [f32; 3],
}

fn default_exposure() -> f32 {
    1.0
}

/// For a serde default that has to be on rather than off. `#[serde(default)]`
/// on a `bool` gives false, which for a visibility flag means the feature
/// silently disappears for everyone with a saved layout.
fn default_true() -> bool {
    true
}

fn unit_vec3() -> [f32; 3] {
    [1.0; 3]
}

impl Default for PaneLook {
    fn default() -> Self {
        Self {
            exposure: default_exposure(),
            tone_mode: ToneMode::default(),
            lift: [0.0; 3],
            gamma: unit_vec3(),
            gain: unit_vec3(),
        }
    }
}

impl PaneLook {
    /// Seed from the host's global tone mapper and exposure, which is what
    /// the desktop shell's sidebar and its `E` / `Shift+T` keys drive.
    #[must_use]
    pub fn from_tone(tone_mode: ToneMode, exposure: f32) -> Self {
        Self {
            exposure,
            tone_mode,
            ..Self::default()
        }
    }
}

impl PaneDisplaySettings {
    /// The view a delivered still is drawn with: the scene, and nothing that
    /// exists to help someone work on it.
    ///
    /// No grid, no axis gizmo, no local axes, no bounds, no normals, no
    /// validation tint, no material override. A still is a photograph of the
    /// scene rather than a screenshot of the viewport, which is what the render
    /// node's own help promises, and it is the one view both a browser and a
    /// terminal can produce without agreeing about anything else first.
    ///
    /// Named rather than a `Default` impl, and the distinction is deliberate:
    /// neither this struct nor [`DisplaySettings`] carries a `Default`, because
    /// the two shells genuinely disagree about several fields and a default
    /// would quietly pick one shell's answer for both. This picks nobody's
    /// answer. It states what a *still* is.
    ///
    /// The background rides in rather than being fixed here, because a scene
    /// that authored a sky should be shot against it.
    #[must_use]
    pub fn for_still(background_mode: BackgroundMode) -> Self {
        Self {
            view_mode: ViewMode::Shaded,
            prev_non_ghosted_mode: ViewMode::Shaded,
            ghosted_wireframe: false,
            normals_mode: NormalsMode::Off,
            background_mode,
            uv_mode: UvMode::Off,
            bounds_mode: BoundsMode::Off,
            line_weight: LineWeight::Medium,
            show_grid: false,
            show_axis_gizmo: false,
            show_local_axes: false,
            inspection_mode: InspectionMode::Shaded,
            material_override: MaterialOverride::None,
            texel_density_target: 1.0,
            pane_mode: PaneMode::Scene3D,
            uv_bg: UvMapBackground::Dark,
            uv_offset: [0.0, 0.0],
            uv_zoom: 1.0,
            show_uv_overlap: false,
            show_validation: false,
            // A still is a photograph of the scene. A light marker is a thing
            // you aim with, not a thing in the scene, so it stays out for the
            // same reason the grid does.
            show_light_markers: false,
            turntable_active: false,
            // A still's engine is authored on the render node and carried
            // by the job's spec, never by these settings.
            pane_engine: PaneEngine::Raster,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
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
    /// Draw a screen-constant marker at every light, so a light can be found
    /// and aimed at without knowing where it already is.
    ///
    /// On by default, because a light with no marker is invisible and the
    /// alternative is hunting for it in the parameter panel. Distinct from a
    /// light's own `show_helper`, which draws the world-scaled wireframe
    /// describing that light's *extent*: the two answer different questions
    /// and coexist.
    ///
    /// Serde default so older pane blobs deserialize, and it defaults to
    /// **true** rather than to `bool`'s false, which is why it names a
    /// function rather than taking the bare attribute.
    #[serde(default = "default_true")]
    pub show_light_markers: bool,
    /// Live per-pane turntable spin. Session-temporary: the web host
    /// resets it on load, so it is never restored from a saved scene. Serde
    /// default so older pane blobs deserialize.
    #[serde(default)]
    pub turntable_active: bool,
    /// Which backend draws this pane's 3D content.
    ///
    /// A field on the display settings rather than a [`PaneMode`] variant,
    /// deliberately: a traced pane is still a 3D pane, with the same
    /// navigation, picking, review and toolbar semantics, and only the
    /// encode differs. A mode variant would change the meaning of every
    /// `pane_mode == Scene3D` comparison in both shells. Serde default so
    /// older pane blobs deserialize.
    #[serde(default)]
    pub pane_engine: PaneEngine,
}

/// Which renderer draws a 3D pane: the viewport rasterizer, or the path
/// tracer converging a preview while the pane is quiescent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PaneEngine {
    #[default]
    Raster,
    Traced,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::preferences::BackgroundMode;

    /// What a still is, asserted rather than left to the struct literal.
    ///
    /// The literal is exhaustive, so a new field cannot be forgotten; what it
    /// cannot catch is a field added with the wrong answer. Every flag here is
    /// a working aid, and a delivered still is a photograph of the scene.
    #[test]
    fn a_still_draws_the_scene_and_none_of_the_aids_for_working_on_it() {
        let pds = PaneDisplaySettings::for_still(BackgroundMode::GRADIENT);
        assert!(!pds.show_grid, "grid");
        assert!(!pds.show_axis_gizmo, "axis gizmo");
        assert!(!pds.show_local_axes, "local axes");
        assert!(!pds.show_validation, "validation overlay");
        assert!(!pds.show_light_markers, "light markers");
        assert_eq!(pds.bounds_mode, BoundsMode::Off);
        assert_eq!(pds.normals_mode, NormalsMode::Off);
        assert_eq!(pds.material_override, MaterialOverride::None);
    }

    /// A pane blob saved before markers existed has to come back with them on,
    /// or the feature silently disappears for everyone with a saved layout.
    #[test]
    fn an_older_pane_blob_deserializes_with_markers_on() {
        let mut v = serde_json::to_value(PaneDisplaySettings::for_still(BackgroundMode::GRADIENT))
            .expect("serializes");
        v.as_object_mut()
            .expect("an object")
            .remove("showLightMarkers")
            .expect("the field was there to remove");
        let back: PaneDisplaySettings = serde_json::from_value(v).expect("deserializes");
        assert!(
            back.show_light_markers,
            "a missing flag must default on, not to bool's false"
        );
    }
}
