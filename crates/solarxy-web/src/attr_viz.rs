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
    /// A preset ramp (`AttrVizState::ramp_preset`) over the lane's
    /// magnitude range this frame.
    Ramp,
}

/// The curated magnitude-ramp styles. `ColdWarm` is the channel's
/// historical look and stays the default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RampPreset {
    /// Cold blue through the channel amber to warm red.
    #[default]
    ColdWarm,
    /// Near-black through violet and orange to pale gold: a perceptual
    /// dark-to-bright read for dense magnitude fields.
    Ember,
    /// Deep water blue rising to pale cyan: a depth read.
    Ocean,
    /// Plain dark-to-light: for screenshots headed to print.
    Grayscale,
    /// Green through yellow to red: a pass / attention / fail read.
    Signal,
}

impl RampPreset {
    /// The preset's color stops, evenly spaced over `t` in 0..=1.
    #[must_use]
    pub fn stops(self) -> &'static [[f32; 3]] {
        match self {
            Self::ColdWarm => &[
                [0.25, 0.45, 0.9],
                AttrVizState::DEFAULT_COLOR,
                [0.9, 0.15, 0.1],
            ],
            Self::Ember => &[
                [0.05, 0.03, 0.12],
                [0.45, 0.10, 0.35],
                [0.90, 0.45, 0.12],
                [0.98, 0.92, 0.65],
            ],
            Self::Ocean => &[[0.03, 0.10, 0.25], [0.10, 0.45, 0.70], [0.85, 0.97, 1.0]],
            Self::Grayscale => &[[0.08, 0.08, 0.08], [0.95, 0.95, 0.95]],
            Self::Signal => &[[0.15, 0.65, 0.30], [0.95, 0.85, 0.20], [0.85, 0.15, 0.10]],
        }
    }
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
    /// Pin budget for labels/points; 0 (the default) means every point,
    /// up to [`Self::MAX_CAP`].
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
    /// Which curated ramp `AttrColorMode::Ramp` draws.
    pub ramp_preset: RampPreset,
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
            ramp_preset: RampPreset::ColdWarm,
        }
    }
}

impl AttrVizState {
    /// The hard ceiling on labels: the GPU channel draws 12 + 6-per-glyph
    /// vertices per label per pane, so this bounds vertex throughput (about
    /// 2M vertices per pane at the ceiling), not memory. The per-cook text
    /// assembly is the CPU side of the same budget.
    pub const MAX_CAP: usize = 16_384;
    /// The channel's historical amber.
    pub const DEFAULT_COLOR: [f32; 3] = [1.0, 0.62, 0.15];

    #[must_use]
    pub fn pins_wanted(&self) -> bool {
        self.labels || self.points
    }

    /// The pin budget against a scene of `total` displayed points: the
    /// 0 sentinel means every point (so meshes at or under the ceiling
    /// label completely, the Houdini read), an explicit cap is honored up
    /// to the same ceiling. Never 0, so a stride division is always safe.
    #[must_use]
    pub fn effective_cap(&self, total: usize) -> usize {
        if self.cap == 0 {
            total.clamp(1, Self::MAX_CAP)
        } else {
            (self.cap as usize).clamp(1, Self::MAX_CAP)
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

/// The magnitude ramp: the preset's stops, evenly spaced and
/// piecewise-linear over `t` in 0..=1.
#[must_use]
// Stop counts are tiny (2..=4), so the usize/f32 casts are exact; t is
// clamped non-negative before the floor.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
pub fn ramp_color(preset: RampPreset, t: f32) -> [f32; 3] {
    let stops = preset.stops();
    let segments = stops.len() - 1;
    let t = t.clamp(0.0, 1.0) * segments as f32;
    let idx = (t.floor() as usize).min(segments - 1);
    let frac = t - idx as f32;
    let (lo, hi) = (stops[idx], stops[idx + 1]);
    std::array::from_fn(|c| lo[c] + (hi[c] - lo[c]) * frac)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    #[test]
    fn the_zero_cap_sentinel_means_all_points_up_to_the_ceiling() {
        let viz = AttrVizState::default();
        assert_eq!(viz.cap, 0, "all-points is the out-of-the-box default");
        assert_eq!(
            viz.effective_cap(500),
            500,
            "small scenes label every point"
        );
        assert_eq!(
            viz.effective_cap(AttrVizState::MAX_CAP),
            AttrVizState::MAX_CAP
        );
        assert_eq!(
            viz.effective_cap(1_000_000),
            AttrVizState::MAX_CAP,
            "dense scenes clamp to the ceiling and sample"
        );
        assert_eq!(
            viz.effective_cap(0),
            1,
            "never zero: stride math divides by it"
        );
    }

    #[test]
    fn an_explicit_cap_is_honored_and_clamped() {
        let mut viz = AttrVizState::default();
        viz.cap = 64;
        assert_eq!(viz.effective_cap(1_000_000), 64);
        assert_eq!(
            viz.effective_cap(10),
            64,
            "an explicit cap does not shrink to the scene"
        );
        viz.cap = 999_999;
        assert_eq!(viz.effective_cap(1_000_000), AttrVizState::MAX_CAP);
    }

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
        // keeps the boundary honest anyway. `rampPreset` is deliberately
        // absent here: a pre-Stage-8 payload must keep the historical ramp.
        let viz: AttrVizState = serde_json::from_str(
            r#"{"labels":true,"vectors":true,"points":false,"name":"N","cap":0}"#,
        )
        .unwrap();
        assert!(viz.labels && viz.vectors);
        assert_eq!(viz.vector_scale, 1.0);
        assert_eq!(viz.color_mode, AttrColorMode::Uniform);
        assert_eq!(viz.ramp_preset, RampPreset::ColdWarm);
    }

    #[test]
    fn the_boundary_shape_is_camel_case() {
        let v = serde_json::to_value(AttrVizState::default()).unwrap();
        assert!(v.get("vectorScale").is_some());
        assert!(v.get("colorMode").is_some());
        assert_eq!(v["colorMode"], "uniform");
        assert_eq!(v["rampPreset"], "coldWarm");
    }

    #[test]
    fn the_default_ramp_runs_cold_to_warm_through_amber() {
        // The ColdWarm stops are pinned byte-identical to the pre-preset
        // constants: existing users must see no change.
        let close =
            |a: [f32; 3], b: [f32; 3]| a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(close(
            ramp_color(RampPreset::ColdWarm, 0.0),
            [0.25, 0.45, 0.9]
        ));
        assert!(close(
            ramp_color(RampPreset::ColdWarm, 0.5),
            AttrVizState::DEFAULT_COLOR
        ));
        assert!(close(
            ramp_color(RampPreset::ColdWarm, 1.0),
            [0.9, 0.15, 0.1]
        ));
        // Out-of-range input clamps instead of extrapolating.
        assert_eq!(
            ramp_color(RampPreset::ColdWarm, -1.0),
            ramp_color(RampPreset::ColdWarm, 0.0)
        );
        assert_eq!(
            ramp_color(RampPreset::ColdWarm, 2.0),
            ramp_color(RampPreset::ColdWarm, 1.0)
        );
    }

    #[test]
    fn every_preset_hits_its_endpoint_stops() {
        let close =
            |a: [f32; 3], b: [f32; 3]| a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6);
        let presets = [
            RampPreset::ColdWarm,
            RampPreset::Ember,
            RampPreset::Ocean,
            RampPreset::Grayscale,
            RampPreset::Signal,
        ];
        for preset in presets {
            let stops = preset.stops();
            assert!(stops.len() >= 2, "{preset:?} needs at least two stops");
            assert!(
                close(ramp_color(preset, 0.0), stops[0]),
                "{preset:?} low endpoint"
            );
            assert!(
                close(ramp_color(preset, 1.0), *stops.last().unwrap()),
                "{preset:?} high endpoint"
            );
        }
    }

    #[test]
    fn a_four_stop_ramp_interpolates_inside_interior_segments() {
        // Ember has 4 stops (3 segments): t = 1/3 lands exactly on the
        // second stop, t = 0.5 halfway between the second and third.
        let stops = RampPreset::Ember.stops();
        let close =
            |a: [f32; 3], b: [f32; 3]| a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-6);
        assert!(close(ramp_color(RampPreset::Ember, 1.0 / 3.0), stops[1]));
        let mid: [f32; 3] = std::array::from_fn(|c| (stops[1][c] + stops[2][c]) / 2.0);
        assert!(close(ramp_color(RampPreset::Ember, 0.5), mid));
    }
}
