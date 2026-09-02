//! Direct lights ([`LightEntry`]) plus the consolidated [`LightsUniform`]
//! pushed to the GPU. The CPU-side L0 SH ambient comes from `IblState` and is
//! merged here before upload; ambient and hemisphere [`LightDef`]s fold into
//! the hemisphere ambient rows instead of consuming light slots.
//!
//! Generalized from the fixed 3-entry viewer rig to
//! a capacity-[`MAX_LIGHTS`] array with per-kind fields. A scene with zero
//! light nodes still synthesizes the camera-relative key/fill/rim rig
//! (`scene:lights_from_camera`), whose entries use `range = 0`,
//! `decay = 0` so every generalized code path multiplies by exactly 1.0 —
//! desktop output is unchanged by construction.

use solarxy_core::scene::{LightDef, LightKind};

/// Light-slot capacity of the uniform (plan decision: 8; ambient and
/// hemisphere lights do not consume slots).
pub const MAX_LIGHTS: usize = 8;

/// `kind` discriminants shared with `shader.wgsl`.
pub const LIGHT_KIND_POINT: u32 = 0;
pub const LIGHT_KIND_DIRECTIONAL: u32 = 1;
pub const LIGHT_KIND_SPOT: u32 = 2;
/// Shaded through linearly transformed cosines against the rectangle's
/// four corners, rather than as a point at its centre.
pub const LIGHT_KIND_RECT_AREA: u32 = 3;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightEntry {
    pub position: [f32; 3],
    /// One of the `LIGHT_KIND_*` discriminants.
    pub kind: u32,
    /// Unit vector the light travels along (directional/spot).
    pub direction: [f32; 3],
    pub intensity: f32,
    pub color: [f32; 3],
    /// Cutoff distance; `0` disables distance attenuation entirely
    /// (the synthesized viewer rig relies on this).
    pub range: f32,
    /// Falloff exponent; `0` disables decay attenuation.
    pub decay: f32,
    /// Spot cone cosines (full intensity inside `cos_inner`, zero outside
    /// `cos_outer`).
    pub cos_inner: f32,
    pub cos_outer: f32,
    /// `1.0` when this entry is the exclusive shadow caster.
    pub shadowed: f32,
    /// Rect-area: half the width edge, world space. The shading derives the
    /// four corners from `position ± half_x ± half_y`.
    pub half_x: [f32; 3],
    /// Rect-area: `1.0` to emit from both faces.
    pub two_sided: f32,
    /// Rect-area: half the height edge, world space.
    pub half_y: [f32; 3],
    pub _pad_entry: f32,
}

impl LightEntry {
    /// A zeroed, disabled entry (kind point, black, zero intensity).
    #[must_use]
    pub fn disabled() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LightsUniform {
    pub lights: [LightEntry; MAX_LIGHTS],
    /// Number of populated `lights` entries.
    pub count: u32,
    pub sphere_scale: f32,
    /// L0 SH ambient from the active IBL (the Clay-mode ambient chokepoint).
    pub ibl_avg_r: f32,
    pub ibl_avg_g: f32,
    pub ibl_avg_b: f32,
    /// Hemisphere ambient: sky color x intensity, accumulated from ambient
    /// and hemisphere light defs (ambient contributes equally to both rows).
    /// All-zero when no such lights exist — the desktop-parity state.
    pub hemi_sky_r: f32,
    pub hemi_sky_g: f32,
    pub hemi_sky_b: f32,
    pub hemi_ground_r: f32,
    pub hemi_ground_g: f32,
    pub hemi_ground_b: f32,
    /// Multiplier on the image-based lighting contribution; `1.0` is the
    /// HDRI as authored, and is what every constructor here seeds, so a
    /// caller that never sets it renders exactly as it did before this
    /// field existed. Set it through
    /// [`LightsUniform::set_ibl_intensity`].
    ///
    /// This slot was `_pad_tail` alignment padding, so putting a real
    /// field in it changes no offset and no size: the asserts below and
    /// the WGSL pair are unchanged in shape.
    pub ibl_intensity: f32,
}

// 0.8.1 grew the entry from 64 bytes for the rect-area frame. Six vec4-sized
// rows, no padding wasted:
//
//   position  kind      | direction  intensity | color     range
//   decay cos_inner cos_outer shadowed         | half_x    two_sided
//   half_y    _pad
//
// The array STRIDE changes, so this is not a prefix-safe addition and the
// WGSL struct pair must move in lockstep. Only `shader.wgsl` declares them.
//
// (The 0.8.1 milestone doc predicted 80 bytes. That was arithmetic done in
// prose: two vec3s and a scalar are seven more floats, which cannot fit in
// the sixteen spare bytes an 80-byte entry would have. The cost of the
// extra row is 256 bytes of uniform across all eight slots.)
const _: () = assert!(std::mem::size_of::<LightEntry>() == 96);
const _: () = assert!(std::mem::size_of::<LightsUniform>() == 816);

impl LightsUniform {
    /// Scale the image-based lighting contribution, clamped to the usable
    /// range. Separate from [`Self::from_defs`] rather than a parameter on
    /// it because the constructor seeds the as-authored `1.0`: a host that
    /// never calls this lights the scene exactly as it did before the
    /// control existed, which is the failure mode worth having.
    pub fn set_ibl_intensity(&mut self, intensity: f32) {
        self.ibl_intensity = intensity.clamp(
            solarxy_core::view_config::MIN_HDRI_INTENSITY,
            solarxy_core::view_config::MAX_HDRI_INTENSITY,
        );
    }

    /// Build the uniform from resolved light definitions (document order).
    /// Slot-consuming lights beyond [`MAX_LIGHTS`] are dropped here; the
    /// engine is responsible for surfacing the "light limit reached"
    /// warning. Invisible lights are skipped entirely.
    #[must_use]
    pub fn from_defs(defs: &[LightDef], sphere_scale: f32, ibl_avg: [f32; 3]) -> Self {
        let mut uniform = LightsUniform {
            lights: [LightEntry::disabled(); MAX_LIGHTS],
            count: 0,
            sphere_scale,
            ibl_avg_r: ibl_avg[0],
            ibl_avg_g: ibl_avg[1],
            ibl_avg_b: ibl_avg[2],
            hemi_sky_r: 0.0,
            hemi_sky_g: 0.0,
            hemi_sky_b: 0.0,
            hemi_ground_r: 0.0,
            hemi_ground_g: 0.0,
            hemi_ground_b: 0.0,
            ibl_intensity: solarxy_core::view_config::DEFAULT_HDRI_INTENSITY,
        };

        for def in defs.iter().filter(|d| d.visible) {
            match def.kind {
                LightKind::Ambient => {
                    // Ambient raises both hemisphere rows equally.
                    for (sky, ground, c) in [
                        (
                            &mut uniform.hemi_sky_r,
                            &mut uniform.hemi_ground_r,
                            def.color[0],
                        ),
                        (
                            &mut uniform.hemi_sky_g,
                            &mut uniform.hemi_ground_g,
                            def.color[1],
                        ),
                        (
                            &mut uniform.hemi_sky_b,
                            &mut uniform.hemi_ground_b,
                            def.color[2],
                        ),
                    ] {
                        *sky += c * def.intensity;
                        *ground += c * def.intensity;
                    }
                }
                LightKind::Hemisphere => {
                    uniform.hemi_sky_r += def.color[0] * def.intensity;
                    uniform.hemi_sky_g += def.color[1] * def.intensity;
                    uniform.hemi_sky_b += def.color[2] * def.intensity;
                    uniform.hemi_ground_r += def.ground_color[0] * def.intensity;
                    uniform.hemi_ground_g += def.ground_color[1] * def.intensity;
                    uniform.hemi_ground_b += def.ground_color[2] * def.intensity;
                }
                LightKind::Point
                | LightKind::Directional
                | LightKind::Spot
                | LightKind::RectArea => {
                    let slot = uniform.count as usize;
                    if slot >= MAX_LIGHTS {
                        continue;
                    }
                    let kind = match def.kind {
                        LightKind::Directional => LIGHT_KIND_DIRECTIONAL,
                        LightKind::Spot => LIGHT_KIND_SPOT,
                        LightKind::RectArea => LIGHT_KIND_RECT_AREA,
                        _ => LIGHT_KIND_POINT,
                    };
                    // Only a rectangle has a frame. Leaving the others at
                    // zero means a stale half-vector can never leak into a
                    // point light's shading if a kind is ever mis-tagged.
                    let basis = (def.kind == LightKind::RectArea).then(|| def.rect_basis());
                    uniform.lights[slot] = LightEntry {
                        position: def.position,
                        kind,
                        // A rectangle's direction IS its face normal, so
                        // the helper's arrow and the shading's back-face
                        // test read the same value.
                        direction: basis.map_or(def.direction, |b| b.normal),
                        intensity: def.intensity,
                        color: def.color,
                        range: def.range,
                        decay: def.decay,
                        cos_inner: def.inner_cone.cos(),
                        cos_outer: def.outer_cone.cos(),
                        shadowed: if def.cast_shadow { 1.0 } else { 0.0 },
                        half_x: basis.map_or([0.0; 3], |b| b.half_x),
                        two_sided: f32::from(u8::from(def.two_sided)),
                        half_y: basis.map_or([0.0; 3], |b| b.half_y),
                        _pad_entry: 0.0,
                    };
                    uniform.count += 1;
                }
            }
        }

        uniform
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::float_cmp)] // exact values constructed by the tests

    use super::*;

    fn def(kind: LightKind) -> LightDef {
        LightDef {
            id: solarxy_core::scene::SceneObjectId::UNAUTHORED,
            kind,
            position: [1.0, 2.0, 3.0],
            direction: [0.0, -1.0, 0.0],
            color: [0.5, 0.6, 0.7],
            intensity: 2.0,
            range: 10.0,
            decay: 2.0,
            radius: 0.0,
            inner_cone: 0.3,
            outer_cone: 0.6,
            area_extent: [1.0, 1.0],
            rotate: [0.0; 3],
            two_sided: false,
            ground_color: [0.1, 0.2, 0.3],
            cast_shadow: false,
            shadow_map_size: 1024,
            shadow_bias: 0.0,
            visible: true,
            show_helper: false,
            helper_size: 1.0,
        }
    }

    #[test]
    fn slot_lights_fill_in_document_order_and_overflow_drops() {
        let defs: Vec<LightDef> = (0..10).map(|_| def(LightKind::Point)).collect();
        let uniform = LightsUniform::from_defs(&defs, 1.0, [0.0; 3]);
        assert_eq!(uniform.count, MAX_LIGHTS as u32);
    }

    #[test]
    fn invisible_lights_are_skipped() {
        let mut d = def(LightKind::Point);
        d.visible = false;
        let uniform = LightsUniform::from_defs(&[d], 1.0, [0.0; 3]);
        assert_eq!(uniform.count, 0);
    }

    #[test]
    fn ambient_and_hemisphere_fold_into_hemi_rows_not_slots() {
        let ambient = def(LightKind::Ambient);
        let hemi = def(LightKind::Hemisphere);
        let uniform = LightsUniform::from_defs(&[ambient, hemi], 1.0, [0.0; 3]);
        assert_eq!(uniform.count, 0);
        // Ambient contributed color*intensity to both rows; hemisphere added
        // color*intensity to sky and ground_color*intensity to ground.
        assert!((uniform.hemi_sky_r - (0.5 * 2.0 + 0.5 * 2.0)).abs() < 1e-6);
        assert!((uniform.hemi_ground_r - (0.5 * 2.0 + 0.1 * 2.0)).abs() < 1e-6);
    }

    /// Replaces `rect_area_approximates_as_point`, which asserted the v1
    /// downgrade this milestone exists to remove.
    #[test]
    fn rect_area_carries_its_own_kind_and_frame() {
        let mut light = def(LightKind::RectArea);
        light.area_extent = [4.0, 2.0];
        light.two_sided = true;
        let uniform = LightsUniform::from_defs(&[light], 1.0, [0.0; 3]);
        assert_eq!(uniform.count, 1);
        let entry = uniform.lights[0];
        assert_eq!(entry.kind, LIGHT_KIND_RECT_AREA);
        // Unrotated: width along +x, height along +z, emitting down -y.
        assert!((entry.half_x[0] - 2.0).abs() < 1e-6, "{:?}", entry.half_x);
        assert!((entry.half_y[2] - 1.0).abs() < 1e-6, "{:?}", entry.half_y);
        assert!(
            (entry.direction[1] + 1.0).abs() < 1e-6,
            "{:?}",
            entry.direction
        );
        assert!((entry.two_sided - 1.0).abs() < f32::EPSILON);
    }

    /// A rotation has to reach the stored frame, or Width and Height are
    /// back to being controls the renderer ignores.
    #[test]
    fn rect_area_rotation_turns_the_frame() {
        let mut light = def(LightKind::RectArea);
        light.area_extent = [4.0, 2.0];
        // A quarter turn about x tips the panel to face -z instead of -y.
        light.rotate = [std::f32::consts::FRAC_PI_2, 0.0, 0.0];
        let uniform = LightsUniform::from_defs(&[light], 1.0, [0.0; 3]);
        let entry = uniform.lights[0];
        assert!(
            (entry.direction[2] + 1.0).abs() < 1e-6,
            "{:?}",
            entry.direction
        );
        // Width is unaffected by a rotation about its own axis.
        assert!((entry.half_x[0] - 2.0).abs() < 1e-6, "{:?}", entry.half_x);
        // Height swings from +z to -y.
        assert!((entry.half_y[1] + 1.0).abs() < 1e-6, "{:?}", entry.half_y);
    }

    /// Every other kind must leave the frame zeroed, so a mis-tagged entry
    /// can never light through a stale rectangle.
    #[test]
    fn only_rect_area_lights_carry_a_frame() {
        for kind in [LightKind::Point, LightKind::Directional, LightKind::Spot] {
            let mut light = def(kind);
            light.area_extent = [9.0, 9.0];
            light.two_sided = true;
            let uniform = LightsUniform::from_defs(&[light], 1.0, [0.0; 3]);
            let entry = uniform.lights[0];
            assert_eq!(entry.half_x, [0.0; 3], "{kind:?}");
            assert_eq!(entry.half_y, [0.0; 3], "{kind:?}");
        }
    }

    #[test]
    fn shadow_flag_marks_the_caster_entry() {
        let mut caster = def(LightKind::Directional);
        caster.cast_shadow = true;
        let uniform = LightsUniform::from_defs(&[def(LightKind::Point), caster], 1.0, [0.0; 3]);
        assert!((uniform.lights[0].shadowed - 0.0).abs() < f32::EPSILON);
        assert!((uniform.lights[1].shadowed - 1.0).abs() < f32::EPSILON);
        assert_eq!(uniform.lights[1].kind, LIGHT_KIND_DIRECTIONAL);
    }

    #[test]
    fn spot_cones_are_stored_as_cosines() {
        let uniform = LightsUniform::from_defs(&[def(LightKind::Spot)], 1.0, [0.0; 3]);
        assert!((uniform.lights[0].cos_inner - 0.3f32.cos()).abs() < 1e-6);
        assert!((uniform.lights[0].cos_outer - 0.6f32.cos()).abs() < 1e-6);
    }
}
