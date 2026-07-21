//! Per-check toggles and tunable thresholds for the validation pipeline.
//!
//! [`ValidationConfig`] gates individual checks on or off. [`ValidationThresholds`]
//! holds the numeric knobs that some checks read (e.g. the flipped-normal
//! dot-product cutoff). Both are always available — `serde` derives are
//! conditional on the `serialization` feature so the types remain usable in
//! pure-computation builds.

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ValidationConfig {
    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub normal_mismatch: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub flipped_normals: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub non_manifold_edges: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub triangle_budget: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_false"))]
    pub allow_open_mesh: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub degenerate_triangles: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub material_refs: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub uv_presence: bool,

    /// When set, the missing-UV check flags any mesh with no texture
    /// coordinates regardless of source format. Off by default so cooked,
    /// legitimately UV-less geometry stays quiet and the file-loading path
    /// keeps its format-gated behavior; the validate node's `require_uvs`
    /// param drives this.
    #[cfg_attr(feature = "serde", serde(default = "default_false"))]
    pub uv_presence_forced: bool,

    #[cfg_attr(feature = "serde", serde(default = "default_true"))]
    pub index_buffer: bool,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        Self {
            normal_mismatch: true,
            flipped_normals: true,
            non_manifold_edges: true,
            triangle_budget: true,
            allow_open_mesh: false,
            degenerate_triangles: true,
            material_refs: true,
            uv_presence: true,
            uv_presence_forced: false,
            index_buffer: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(deny_unknown_fields))]
#[cfg_attr(feature = "schemars-gen", derive(schemars::JsonSchema))]
pub struct ValidationThresholds {
    #[cfg_attr(feature = "serde", serde(default = "default_tolerance_percent"))]
    pub triangle_budget_tolerance_percent: f32,

    #[cfg_attr(feature = "serde", serde(default = "default_flipped_normal_dot"))]
    pub flipped_normal_dot: f32,
}

impl Default for ValidationThresholds {
    fn default() -> Self {
        Self {
            triangle_budget_tolerance_percent: default_tolerance_percent(),
            flipped_normal_dot: default_flipped_normal_dot(),
        }
    }
}

#[cfg(feature = "serde")]
fn default_true() -> bool {
    true
}
#[cfg(feature = "serde")]
fn default_false() -> bool {
    false
}
const fn default_tolerance_percent() -> f32 {
    20.0
}
const fn default_flipped_normal_dot() -> f32 {
    -0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_config_defaults_match_expected_policy() {
        let c = ValidationConfig::default();
        assert!(c.normal_mismatch);
        assert!(c.flipped_normals);
        assert!(c.non_manifold_edges);
        assert!(c.triangle_budget);
        assert!(!c.allow_open_mesh);
        assert!(c.degenerate_triangles);
        assert!(c.uv_presence);
        assert!(!c.uv_presence_forced);
    }

    #[test]
    fn thresholds_defaults() {
        let t = ValidationThresholds::default();
        assert!((t.triangle_budget_tolerance_percent - 20.0).abs() < f32::EPSILON);
        assert!((t.flipped_normal_dot - (-0.5)).abs() < f32::EPSILON);
    }
}
