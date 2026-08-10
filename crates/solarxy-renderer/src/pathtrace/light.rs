//! The traced light record: one `LightDef` as the kernel reads it.
//!
//! # Why this is a storage array and not the raster path's uniform
//!
//! [`crate::light::LightsUniform`] holds eight entries because a uniform buffer
//! is sized once and bound to every draw, and eight was the number that fit
//! beside everything else the fragment stage needs. That is a raster
//! constraint, not a physical one. A compute kernel reads a runtime-sized
//! storage array, so a forty-light scene traces forty lights, and the two
//! consumers of the same `LightDef` disagree about how many of them arrive.
//! The disagreement is deliberate and is meant to be *stated*, which is what
//! `BackendCaps::max_lights` exists for; what must not happen is a user
//! inferring it from a picture that looks wrong.
//!
//! # Ambient and hemisphere are not here
//!
//! Both modulate the ambient term rather than occupying a slot, which
//! [`LightDef::consumes_slot`] already says. In a tracer that distinction
//! sharpens: next-event estimation samples a *place* light comes from, and
//! neither of those has one. They are excluded at build time rather than
//! carried with a flag, so nothing downstream has to remember that two of the
//! six kinds cannot be sampled.
//!
//! # The extent is sampled, the density is not
//!
//! A point or spot light with a non-zero radius samples its emitter's extent,
//! which is where a penumbra comes from, and still reports a probability of
//! one and is still weighted as a delta light. That is the source's treatment
//! and it is a deliberate inconsistency: the extent buys the soft shadow
//! without making the light a surface a scattered ray could find, which would
//! need the light to be intersectable and to carry its own solid-angle
//! density. Rect-area lights *are* intersectable, do carry one, and are the
//! only kind that multiple importance sampling has two estimators for.

use bytemuck::{Pod, Zeroable};
use solarxy_core::scene::{LightDef, LightKind};

/// [`TracedLight::kind`]: an omnidirectional emitter, optionally a sphere.
pub const LIGHT_POINT: u32 = 0;
/// [`TracedLight::kind`]: parallel rays from infinitely far away.
pub const LIGHT_DIRECTIONAL: u32 = 1;
/// [`TracedLight::kind`]: a cone, optionally with a disc emitter.
pub const LIGHT_SPOT: u32 = 2;
/// [`TracedLight::kind`]: an oriented rectangle, the one intersectable kind.
pub const LIGHT_RECT: u32 = 3;

/// Bit 0 of [`TracedLight::flags`]: a rectangle emits from both faces.
pub const LIGHT_TWO_SIDED: u32 = 1 << 0;

/// One light, 96 bytes, laid out as six 16-byte blocks.
///
/// Every block is a `vec3` plus a scalar, so the record needs no padding field
/// and no member straddles the 16-byte alignment WGSL gives a `vec3` in the
/// storage address space. `tests/uniform_layout.rs` is what holds the two
/// declarations to the same size; the field *order* within a block is held by
/// the probe reading a record back through the real binding, for the same
/// reason [`super::material::TracedMaterial`]'s is.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Pod, Zeroable)]
pub struct TracedLight {
    /// World-space position. Unused by a directional light, which has none.
    pub position: [f32; 3],
    /// One of [`LIGHT_POINT`], [`LIGHT_DIRECTIONAL`], [`LIGHT_SPOT`],
    /// [`LIGHT_RECT`].
    pub kind: u32,
    /// Linear RGB, as authored.
    ///
    /// Kept separate from `intensity` rather than premultiplied, so that a
    /// probe reading the record back sees the two numbers the user typed. The
    /// sampler multiplies them at the one place it needs the product.
    pub color: [f32; 3],
    pub intensity: f32,
    /// Rect: the full width edge in world space. Spot: a unit vector across
    /// the emitting disc. Unused otherwise.
    pub u: [f32; 3],
    /// Emitter size in meters. Zero is a mathematical point, and a hard shadow.
    pub radius: f32,
    /// Rect: the full height edge. Spot: the other unit vector across the disc.
    pub v: [f32; 3],
    /// Rect: the emitting area, which is the density's denominator. Spot: the
    /// disc's area, carried for the same reason and unused while the spot
    /// reports a delta density.
    pub area: f32,
    /// **The unit vector pointing from the scene back toward the light**, for
    /// a directional and a spot light, and the emitting face normal for a
    /// rectangle.
    ///
    /// Stated in that direction rather than as `LightDef::direction`'s
    /// travel direction because every consumer here compares it against a
    /// surface-to-light vector, and a convention that has to be negated at
    /// four call sites is a convention that will be negated at three of them.
    pub axis: [f32; 3],
    /// [`LIGHT_TWO_SIDED`].
    pub flags: u32,
    /// Point / spot cutoff distance; zero means unlimited.
    pub range: f32,
    /// Point / spot falloff exponent.
    pub decay: f32,
    /// Cosine of the spot's outer half-angle: the cone's edge.
    pub cone_cos: f32,
    /// Cosine of the spot's inner half-angle: where the falloff starts.
    pub penumbra_cos: f32,
}

const _: () = assert!(std::mem::size_of::<TracedLight>() == 96);

impl TracedLight {
    /// Builds the record for one light, or `None` for a kind the tracer
    /// cannot sample.
    ///
    /// Returns `None` for an invisible light and for ambient and hemisphere,
    /// which have no place to sample. A caller that wants a stable index per
    /// `LightDef` therefore cannot have one, which is deliberate: the kernel
    /// picks uniformly from what is in the array, so an unsamplable entry
    /// would be a light that steals probability and contributes nothing.
    #[must_use]
    pub fn from_def(def: &LightDef) -> Option<Self> {
        if !def.visible {
            return None;
        }
        let mut light = Self {
            position: def.position,
            color: def.color,
            intensity: def.intensity,
            radius: def.radius.max(0.0),
            range: def.range,
            decay: def.decay,
            ..Self::default()
        };
        match def.kind {
            LightKind::Ambient | LightKind::Hemisphere => return None,
            LightKind::Point => {
                light.kind = LIGHT_POINT;
                // No axis and no basis: a sphere looks the same from
                // everywhere, so the disc that stands in for it is built in
                // the kernel, square to whatever direction is asking.
            }
            LightKind::Directional => {
                light.kind = LIGHT_DIRECTIONAL;
                light.axis = negate(unit_or(def.direction, [0.0, -1.0, 0.0]));
            }
            LightKind::Spot => {
                light.kind = LIGHT_SPOT;
                light.axis = negate(unit_or(def.direction, [0.0, -1.0, 0.0]));
                let (u, v) = basis_from(light.axis);
                light.u = u;
                light.v = v;
                light.area = std::f32::consts::PI * light.radius * light.radius;
                light.cone_cos = def.outer_cone.cos();
                light.penumbra_cos = def.inner_cone.cos();
            }
            LightKind::RectArea => {
                light.kind = LIGHT_RECT;
                // The same basis the viewport helper draws, from the same
                // function, because a light integrated over one rectangle and
                // drawn as another is worse than one drawn not at all.
                let basis = def.rect_basis();
                light.u = scale(basis.half_x, 2.0);
                light.v = scale(basis.half_y, 2.0);
                // Negated, and this is the one place the convention costs
                // something. `RectBasis::normal` is the *emitting* face's
                // normal, pointing the way the light shines, because that is
                // what the raster shading and the viewport helper want. `axis`
                // is the other way round for every kind, so that a
                // surface-to-light vector can be compared against it without a
                // per-kind sign. Getting this backwards lights the wrong side
                // of the panel and looks like a rotation bug.
                light.axis = negate(basis.normal);
                light.area = (def.area_extent[0] * def.area_extent[1]).abs();
                if def.two_sided {
                    light.flags |= LIGHT_TWO_SIDED;
                }
            }
        }
        Some(light)
    }

    /// Every samplable light in a scene's light list, in document order.
    #[must_use]
    pub fn pool(defs: &[LightDef]) -> Vec<Self> {
        defs.iter().filter_map(Self::from_def).collect()
    }
}

fn negate(v: [f32; 3]) -> [f32; 3] {
    [-v[0], -v[1], -v[2]]
}

fn scale(v: [f32; 3], s: f32) -> [f32; 3] {
    [v[0] * s, v[1] * s, v[2] * s]
}

/// `v` normalized, or `fallback` when it is too short to have a direction.
fn unit_or(v: [f32; 3], fallback: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        fallback
    }
}

/// Two unit vectors orthogonal to `n` and to each other.
///
/// Frisvad's method, branching on the sign of `z` rather than on a magnitude:
/// the closed form is singular at `n.z == -1` and loses precision near it, and
/// the sign branch is the standard fix. The same construction is in the BSDF's
/// shading frame, and the two are deliberately separate because this one runs
/// once per light on the CPU where a branch costs nothing.
fn basis_from(n: [f32; 3]) -> ([f32; 3], [f32; 3]) {
    let sign = if n[2] >= 0.0 { 1.0_f32 } else { -1.0 };
    let a = -1.0 / (sign + n[2]);
    let b = n[0] * n[1] * a;
    (
        [1.0 + sign * n[0] * n[0] * a, sign * b, -sign * n[0]],
        [b, sign + n[1] * n[1] * a, -n[1]],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn def(kind: LightKind) -> LightDef {
        LightDef {
            kind,
            position: [1.0, 2.0, 3.0],
            direction: [0.0, -1.0, 0.0],
            color: [0.25, 0.5, 0.75],
            intensity: 2.0,
            range: 0.0,
            decay: 2.0,
            radius: 0.0,
            inner_cone: 0.3,
            outer_cone: 0.5,
            area_extent: [2.0, 4.0],
            rotate: [0.0; 3],
            two_sided: false,
            ground_color: [0.0; 3],
            cast_shadow: false,
            shadow_map_size: 1024,
            shadow_bias: 0.0,
            visible: true,
            show_helper: false,
            helper_size: 1.0,
        }
    }

    fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    #[test]
    fn ambient_and_hemisphere_have_no_record() {
        // The pool is what the kernel picks uniformly from, so a light with
        // nowhere to sample must not be in it: it would take probability mass
        // and return nothing, which reads as a scene that is too dark by a
        // factor of how many of them there are.
        assert!(TracedLight::from_def(&def(LightKind::Ambient)).is_none());
        assert!(TracedLight::from_def(&def(LightKind::Hemisphere)).is_none());
        for kind in [
            LightKind::Point,
            LightKind::Directional,
            LightKind::Spot,
            LightKind::RectArea,
        ] {
            assert!(TracedLight::from_def(&def(kind)).is_some(), "{kind:?}");
        }
    }

    #[test]
    fn an_invisible_light_has_no_record() {
        let mut d = def(LightKind::Point);
        d.visible = false;
        assert!(TracedLight::from_def(&d).is_none());
    }

    #[test]
    fn the_axis_points_from_the_scene_back_toward_the_light() {
        // The whole record's usefulness rests on this convention, and it is
        // the one a reader is most likely to flip.
        let d = def(LightKind::Directional);
        let light = TracedLight::from_def(&d).expect("directional");
        assert_eq!(light.axis, [0.0, 1.0, 0.0]);

        let light = TracedLight::from_def(&def(LightKind::Spot)).expect("spot");
        assert_eq!(light.axis, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn the_spot_basis_is_orthonormal_and_square_to_the_axis() {
        let light = TracedLight::from_def(&def(LightKind::Spot)).expect("spot");
        assert!(dot(light.u, light.v).abs() < 1e-5);
        assert!(dot(light.u, light.axis).abs() < 1e-5);
        assert!(dot(light.v, light.axis).abs() < 1e-5);
        assert!((dot(light.u, light.u) - 1.0).abs() < 1e-5);
        assert!((dot(light.v, light.v) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_basis_survives_an_axis_pointing_straight_down_negative_z() {
        // Frisvad's closed form is singular here, which is what the sign
        // branch exists for; without it this returns NaN and every shadow ray
        // from that light misses.
        let (u, v) = basis_from([0.0, 0.0, -1.0]);
        assert!(u.iter().chain(v.iter()).all(|c| c.is_finite()));
        assert!(dot(u, v).abs() < 1e-5);
        assert!((dot(u, u) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_rectangle_matches_the_helper_and_carries_full_edges() {
        let d = def(LightKind::RectArea);
        let light = TracedLight::from_def(&d).expect("rect");
        let basis = d.rect_basis();
        // Full edges, not halves: the kernel offsets from the centre by
        // `u * (r - 0.5)`, so a half edge would sample a quarter of the light
        // and report the whole area's density.
        assert_eq!(light.u, scale(basis.half_x, 2.0));
        assert_eq!(light.v, scale(basis.half_y, 2.0));
        assert_eq!(light.u, [2.0, 0.0, 0.0]);
        assert_eq!(light.v, [0.0, 0.0, 4.0]);
        assert!((light.area - 8.0).abs() < 1e-5);
    }

    #[test]
    fn the_rectangle_axis_is_the_opposite_of_the_face_it_emits_from() {
        // The one kind where the shared convention costs a negation, and the
        // one most likely to be "corrected" back by someone reading
        // `RectBasis::normal`'s own documentation. An unrotated panel emits
        // straight down, so a surface beneath it looks straight up to find it.
        let d = def(LightKind::RectArea);
        let light = TracedLight::from_def(&d).expect("rect");
        assert_eq!(d.rect_basis().normal, [0.0, -1.0, 0.0]);
        assert_eq!(light.axis, [0.0, 1.0, 0.0]);
    }

    #[test]
    fn two_sided_rides_the_flags() {
        let mut d = def(LightKind::RectArea);
        d.two_sided = true;
        let light = TracedLight::from_def(&d).expect("rect");
        assert_eq!(light.flags & LIGHT_TWO_SIDED, LIGHT_TWO_SIDED);
    }

    #[test]
    fn the_pool_drops_what_it_cannot_sample_and_keeps_document_order() {
        let defs = vec![
            def(LightKind::Ambient),
            def(LightKind::Point),
            def(LightKind::Hemisphere),
            def(LightKind::RectArea),
        ];
        let pool = TracedLight::pool(&defs);
        assert_eq!(pool.len(), 2);
        assert_eq!(pool[0].kind, LIGHT_POINT);
        assert_eq!(pool[1].kind, LIGHT_RECT);
    }

    #[test]
    fn forty_lights_all_reach_the_pool() {
        // The eight-slot ceiling is the raster path's uniform, and nothing
        // here may quietly acquire one.
        let defs: Vec<LightDef> = (0..40).map(|_| def(LightKind::Point)).collect();
        assert_eq!(TracedLight::pool(&defs).len(), 40);
    }

    #[test]
    fn a_negative_radius_cannot_reach_the_kernel() {
        // The param's hard range forbids it, but an expression resolves after
        // that and the record is what the kernel trusts.
        let mut d = def(LightKind::Point);
        d.radius = -1.0;
        assert_eq!(TracedLight::from_def(&d).expect("point").radius, 0.0);
    }
}
