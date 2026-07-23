//! The attribute-visualization state and its pure color/scale math.
//!
//! Deliberately NOT wasm-gated (the gizmo-module convention): the state's
//! defaults, clamps, and the ramp are plain data and math, so keeping
//! them native-visible means native CI runs their tests instead of
//! leaving them to a wasm-only build. The wasm host (`app.rs`) owns the
//! only mutation path (`set_attr_viz`).

use serde::{Deserialize, Serialize};

/// How the vector arrows are colored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttrColorMode {
    /// One color for every arrow (`AttrVizState::color`).
    #[default]
    Uniform,
    /// A cold-to-warm ramp over the lane's magnitude range this frame.
    Ramp,
}

/// Host-owned attribute-visualization state (session-only, scene-wide):
/// the right strip's toggles, the picked point-lane name, the pin cap,
/// and the settings popover's vector controls. Deliberately NOT in
/// `PaneDisplaySettings` (which is `Copy`, desktop-shared, and serialized
/// into pane blobs) and never in `.slxy` or undo.
///
/// `Default` is manual and load-bearing: a derived default would zero
/// `vector_scale` and render every arrow invisible.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
// The strip really is four independent toggles; a state machine here
// would invent coupling the UI does not have.
#[allow(clippy::struct_excessive_bools)]
pub struct AttrVizState {
    pub labels: bool,
    pub vectors: bool,
    pub points: bool,
    pub name: Option<String>,
    /// Pin budget for labels/points; 0 means the default.
    pub cap: u32,
    /// Multiplier on the bounds-derived arrow length; 1.0 is the
    /// channel's historical look.
    pub vector_scale: f32,
    /// Unit-length each direction before scaling (magnitude stops
    /// affecting length; the ramp can carry it instead).
    pub normalize: bool,
    pub color_mode: AttrColorMode,
    /// The uniform arrow color, linear RGB. Defaults to the amber the
    /// channel has always drawn.
    pub color: [f32; 3],
}

impl Default for AttrVizState {
    fn default() -> Self {
        Self {
            labels: false,
            vectors: false,
            points: false,
            name: None,
            cap: 0,
            vector_scale: 1.0,
            normalize: false,
            color_mode: AttrColorMode::Uniform,
            color: Self::DEFAULT_COLOR,
        }
    }
}

impl AttrVizState {
    pub const DEFAULT_CAP: usize = 64;
    pub const MAX_CAP: usize = 256;
    /// The channel's historical amber.
    pub const DEFAULT_COLOR: [f32; 3] = [1.0, 0.62, 0.15];

    #[must_use]
    pub fn pins_wanted(&self) -> bool {
        self.labels || self.points
    }

    #[must_use]
    pub fn cap(&self) -> usize {
        if self.cap == 0 {
            Self::DEFAULT_CAP
        } else {
            (self.cap as usize).min(Self::MAX_CAP)
        }
    }

    /// The effective scale multiplier, clamped so a stray value can
    /// neither hide the arrows nor flood the scene.
    #[must_use]
    pub fn scale_multiplier(&self) -> f32 {
        if self.vector_scale.is_finite() {
            self.vector_scale.clamp(0.05, 10.0)
        } else {
            1.0
        }
    }
}

/// The magnitude ramp: cold blue through the channel's amber to warm red,
/// piecewise-linear over `t` in 0..=1.
#[must_use]
pub fn ramp_color(t: f32) -> [f32; 3] {
    const STOPS: [[f32; 3]; 3] = [
        [0.25, 0.45, 0.9],
        AttrVizState::DEFAULT_COLOR,
        [0.9, 0.15, 0.1],
    ];
    let t = t.clamp(0.0, 1.0) * 2.0;
    let (a, b, f) = if t <= 1.0 {
        (STOPS[0], STOPS[1], t)
    } else {
        (STOPS[1], STOPS[2], t - 1.0)
    };
    std::array::from_fn(|c| a[c] + (b[c] - a[c]) * f)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn the_default_scale_is_one_not_zero() {
        // The reason Default is hand-written: a derived default would
        // make every arrow zero-length.
        let viz = AttrVizState::default();
        assert_eq!(viz.vector_scale, 1.0);
        assert_eq!(viz.color, AttrVizState::DEFAULT_COLOR);
        assert_eq!(viz.color_mode, AttrColorMode::Uniform);
        assert!(!viz.normalize);
    }

    #[test]
    fn the_scale_multiplier_clamps_and_survives_non_finite_input() {
        let mut viz = AttrVizState::default();
        viz.vector_scale = 0.0;
        assert_eq!(viz.scale_multiplier(), 0.05);
        viz.vector_scale = 99.0;
        assert_eq!(viz.scale_multiplier(), 10.0);
        viz.vector_scale = f32::NAN;
        assert_eq!(viz.scale_multiplier(), 1.0);
    }

    #[test]
    fn an_old_payload_without_the_new_fields_deserializes_to_defaults() {
        // The TS mirror round-trips the host's own DTO, but serde(default)
        // keeps the boundary honest anyway.
        let viz: AttrVizState =
            serde_json::from_str(r#"{"labels":true,"vectors":true,"points":false,"name":"N","cap":0}"#)
                .unwrap();
        assert!(viz.labels && viz.vectors);
        assert_eq!(viz.vector_scale, 1.0);
        assert_eq!(viz.color_mode, AttrColorMode::Uniform);
    }

    #[test]
    fn the_boundary_shape_is_camel_case() {
        let v = serde_json::to_value(AttrVizState::default()).unwrap();
        assert!(v.get("vectorScale").is_some());
        assert!(v.get("colorMode").is_some());
        assert_eq!(v["colorMode"], "uniform");
    }

    #[test]
    fn the_ramp_runs_cold_to_warm_through_amber() {
        let close = |a: [f32; 3], b: [f32; 3]| {
            a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6)
        };
        assert!(close(ramp_color(0.0), [0.25, 0.45, 0.9]));
        assert!(close(ramp_color(0.5), AttrVizState::DEFAULT_COLOR));
        assert!(close(ramp_color(1.0), [0.9, 0.15, 0.1]));
        // Out-of-range input clamps instead of extrapolating.
        assert_eq!(ramp_color(-1.0), ramp_color(0.0));
        assert_eq!(ramp_color(2.0), ramp_color(1.0));
    }
}
