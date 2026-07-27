//! Light helpers: the wireframe shapes that show where a light is and which way
//! it points.
//!
//! A SEPARATE overlay channel from the manipulator, deliberately. The
//! manipulator is one optional thing attached to the selection; helpers are
//! N-per-scene and shown whenever their light's `show_helper` param is set,
//! selection or no. Folding them into `ManipulatorState` would tie two unrelated
//! lifetimes together and make them share a vertex budget they have no reason to
//! share.
//!
//! Line geometry only, through the same [`GizmoVertex`] pipeline the manipulator
//! draws with. Each helper takes its own light's colour, so which helper belongs
//! to which light is obvious at a glance.
//!
//! Sizing is in WORLD units (the light's `helper_size` param), not screen units:
//! a spot cone's shape is meaningful information about the light, unlike a
//! gizmo handle, whose size is pure affordance.

use cgmath::{InnerSpace, Point3, Vector3};
use solarxy_core::scene::{CameraDef, LightDef, LightKind, SceneObjectId};

use crate::model::GizmoVertex;

/// The camera gizmo's neutral tint. Cameras have no color of their own (unlike
/// lights), so a single cool-gray reads as "camera" without competing with the
/// axis or light colors.
const CAMERA_COLOR: [f32; 3] = [0.82, 0.82, 0.9];

/// Builds the wireframe frustum gizmo for every camera that asks for one,
/// except `skip` (the camera the pane is currently looking through, hidden the
/// way Blender hides the camera you are inside). Line geometry through the same
/// [`GizmoVertex`] overlay pipeline as the light helpers; world-unit sizing
/// from each camera's `gizmo_size`.
#[must_use]
pub fn build_camera_helpers(
    cameras: &[CameraDef],
    skip: Option<SceneObjectId>,
) -> Vec<GizmoVertex> {
    let mut lines = Vec::new();
    for cam in cameras {
        if !cam.show_gizmo || Some(cam.id) == skip {
            continue;
        }
        push_camera(&mut lines, cam);
    }
    lines
}

/// A camera icon: the eye, a film-back rectangle at the camera's aspect with
/// four frustum edges converging on the eye, and a small "up" wedge on top so
/// the roll is unambiguous.
fn push_camera(lines: &mut Vec<GizmoVertex>, cam: &CameraDef) {
    let eye = Point3::from(cam.position);
    let target = Point3::from(cam.target);
    let mut forward = target - eye;
    if forward.magnitude2() < 1e-12 {
        forward = -Vector3::unit_z();
    }
    let forward = forward.normalize();
    let mut right = forward.cross(Vector3::unit_y());
    if right.magnitude2() < 1e-9 {
        right = Vector3::unit_x();
    }
    let right = right.normalize();
    let up = right.cross(forward).normalize();

    let s = cam.gizmo_size.max(0.05);
    let depth = s * 1.5;
    let hh = s * 0.5;
    let hw = hh * cam.aspect.clamp(0.2, 5.0);
    let center = eye + forward * depth;
    let color = CAMERA_COLOR;

    let tl = center + up * hh - right * hw;
    let tr = center + up * hh + right * hw;
    let br = center - up * hh + right * hw;
    let bl = center - up * hh - right * hw;
    // The film-back rectangle.
    push_line(lines, tl, tr, color);
    push_line(lines, tr, br, color);
    push_line(lines, br, bl, color);
    push_line(lines, bl, tl, color);
    // Frustum edges from the eye.
    for c in [tl, tr, br, bl] {
        push_line(lines, eye, c, color);
    }
    // The up wedge: a triangle above the top edge marks the camera's roll.
    let apex = center + up * (hh * 1.6);
    push_line(lines, tl, apex, color);
    push_line(lines, tr, apex, color);
    // A small cross at the eye marks the exact camera position.
    for axis in [right, up] {
        push_line(
            lines,
            eye - axis * (s * 0.12),
            eye + axis * (s * 0.12),
            color,
        );
    }
}

/// Circle resolution. Lower than the gizmo rings: a helper is a hint, not a
/// grab target, and there can be eight of them.
const SEGMENTS: usize = 32;

/// Builds the line geometry for every light that asks for a helper.
///
/// An invisible light draws nothing (it is not in the scene), and an ambient
/// light draws nothing regardless: it has no position and no direction, so there
/// is no honest shape for it.
#[must_use]
pub fn build_light_helpers(lights: &[LightDef]) -> Vec<GizmoVertex> {
    let mut lines = Vec::new();
    for light in lights {
        if !light.show_helper || !light.visible {
            continue;
        }
        // A zero or negative size would collapse the shape into the origin.
        let size = light.helper_size.max(0.01);
        let color = light.color;
        let origin = Point3::from(light.position);

        match light.kind {
            LightKind::Point => push_point(&mut lines, origin, size, color),
            LightKind::Directional => {
                push_arrow(&mut lines, origin, dir_of(light), size, color);
            }
            LightKind::Spot => push_spot(&mut lines, origin, dir_of(light), light, size, color),
            LightKind::RectArea => {
                // Drawn from the SAME basis the shading integrates over
                // (`LightDef::rect_basis`), not from a tangent frame
                // rebuilt off the normal. Those two agree on where the
                // rectangle is but not on how it is rolled, and since v3
                // restored Rotate, roll is now something the user sets and
                // expects to see.
                push_rect(&mut lines, origin, light.rect_basis(), color);
            }
            LightKind::Hemisphere => push_dome(&mut lines, origin, size, color),
            // No position, no direction, nothing honest to draw.
            LightKind::Ambient => {}
        }
    }
    lines
}

fn dir_of(light: &LightDef) -> Vector3<f32> {
    let d = Vector3::from(light.direction);
    if d.magnitude2() < 1e-12 {
        -Vector3::unit_y()
    } else {
        d.normalize()
    }
}

fn vertex(p: Point3<f32>, color: [f32; 3]) -> GizmoVertex {
    GizmoVertex {
        position: p.into(),
        color,
    }
}

fn push_line(lines: &mut Vec<GizmoVertex>, a: Point3<f32>, b: Point3<f32>, color: [f32; 3]) {
    lines.push(vertex(a, color));
    lines.push(vertex(b, color));
}

/// Two orthonormal vectors spanning the plane with this normal.
fn plane_basis(n: Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
    let seed = if n.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let u = n.cross(seed).normalize();
    (u, n.cross(u).normalize())
}

/// A circle, as a closed line loop.
fn push_circle(
    lines: &mut Vec<GizmoVertex>,
    center: Point3<f32>,
    normal: Vector3<f32>,
    radius: f32,
    color: [f32; 3],
) {
    let (u, v) = plane_basis(normal);
    let at = |i: usize| {
        let t = (i as f32) * std::f32::consts::TAU / (SEGMENTS as f32);
        center + (u * t.cos() + v * t.sin()) * radius
    };
    for i in 0..SEGMENTS {
        push_line(lines, at(i), at((i + 1) % SEGMENTS), color);
    }
}

/// A point light: a three-axis cross inside a wire sphere. The cross says
/// "here"; the sphere says "in every direction".
fn push_point(lines: &mut Vec<GizmoVertex>, o: Point3<f32>, size: f32, color: [f32; 3]) {
    for axis in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
        push_line(lines, o - axis * size, o + axis * size, color);
    }
    // Three great circles read as a sphere from any angle.
    for normal in [Vector3::unit_x(), Vector3::unit_y(), Vector3::unit_z()] {
        push_circle(lines, o, normal, size * 0.62, color);
    }
}

/// A directional light: an arrow showing which way the light travels. Its shaft
/// is drawn from the light's position, which is exactly the field that used to
/// go unfilled, leaving nowhere to draw this.
fn push_arrow(
    lines: &mut Vec<GizmoVertex>,
    o: Point3<f32>,
    dir: Vector3<f32>,
    size: f32,
    color: [f32; 3],
) {
    let tip = o + dir * size * 2.0;
    push_line(lines, o, tip, color);

    // A four-barb head, so it reads as an arrow from any viewing angle rather
    // than collapsing to a line when seen edge-on.
    let (u, v) = plane_basis(dir);
    let back = tip - dir * size * 0.35;
    let r = size * 0.16;
    for barb in [u * r, -u * r, v * r, -v * r] {
        push_line(lines, tip, back + barb, color);
    }

    // A small circle at the source, so the light's actual POSITION is visible
    // and not just its direction.
    push_circle(lines, o, dir, size * 0.25, color);
}

/// A spot light: the cone its outer angle actually describes. This is the helper
/// that earns its keep, because a spot's cone is genuinely hard to picture from
/// two numbers.
fn push_spot(
    lines: &mut Vec<GizmoVertex>,
    o: Point3<f32>,
    dir: Vector3<f32>,
    light: &LightDef,
    size: f32,
    color: [f32; 3],
) {
    // Draw the cone out to the light's range when it has one, so the helper shows
    // where the light actually stops. Range 0 means unlimited, so fall back to
    // the helper size.
    let length = if light.range > 0.0 {
        light.range
    } else {
        size * 4.0
    };
    let base = o + dir * length;
    let radius = length * light.outer_cone.tan().abs().max(1e-3);

    push_circle(lines, base, dir, radius, color);
    let (u, v) = plane_basis(dir);
    // Four edge lines: enough to read as a cone, few enough not to clutter.
    for edge in [u, -u, v, -v] {
        push_line(lines, o, base + edge * radius, color);
    }

    // The inner cone (the full-intensity core), dimmer, when penumbra opened a
    // gap worth seeing.
    if light.inner_cone > 1e-3 && (light.outer_cone - light.inner_cone).abs() > 1e-3 {
        let inner_r = length * light.inner_cone.tan().abs().max(1e-3);
        let dim = [color[0] * 0.45, color[1] * 0.45, color[2] * 0.45];
        push_circle(lines, base, dir, inner_r, dim);
    }
}

/// A rect-area light: the rectangle it emits from, plus a short normal stub so
/// the emitting SIDE is unambiguous.
fn push_rect(
    lines: &mut Vec<GizmoVertex>,
    o: Point3<f32>,
    basis: solarxy_core::scene::RectBasis,
    color: [f32; 3],
) {
    let u = Vector3::from(basis.half_x);
    let v = Vector3::from(basis.half_y);
    let corners = [o + u + v, o - u + v, o - u - v, o + u - v];
    for i in 0..4 {
        push_line(lines, corners[i], corners[(i + 1) % 4], color);
    }
    // Which way it emits. The stub length tracks the shorter edge so a long
    // thin strip does not sprout an arrow longer than the light.
    let stub = u.magnitude().min(v.magnitude()).max(0.1);
    push_line(lines, o, o + Vector3::from(basis.normal) * stub, color);
}

/// A hemisphere light: a dome. Two vertical arcs plus the horizon ring, which is
/// the least amount of line that still reads as "sky above, ground below".
fn push_dome(lines: &mut Vec<GizmoVertex>, o: Point3<f32>, size: f32, color: [f32; 3]) {
    const ARC: usize = 16;

    push_circle(lines, o, Vector3::unit_y(), size, color);
    for axis in [Vector3::unit_x(), Vector3::unit_z()] {
        let at = |i: usize| {
            let t = (i as f32) / (ARC as f32) * std::f32::consts::PI * 0.5;
            o + axis * (size * t.cos()) + Vector3::unit_y() * (size * t.sin())
        };
        for i in 0..ARC {
            push_line(lines, at(i), at(i + 1), color);
            // Mirror onto the other side, so the dome is whole.
            let mirror = |p: Point3<f32>| Point3::new(2.0 * o.x - p.x, p.y, 2.0 * o.z - p.z);
            push_line(lines, mirror(at(i)), mirror(at(i + 1)), color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(kind: LightKind) -> LightDef {
        LightDef {
            kind,
            position: [1.0, 2.0, 3.0],
            direction: [0.0, -1.0, 0.0],
            color: [1.0, 0.9, 0.8],
            intensity: 1.0,
            range: 0.0,
            decay: 2.0,
            inner_cone: 0.0,
            outer_cone: 0.5,
            area_extent: [4.0, 2.0],
            rotate: [0.0; 3],
            two_sided: false,
            ground_color: [0.2; 3],
            cast_shadow: false,
            shadow_map_size: 1024,
            shadow_bias: 0.0,
            visible: true,
            show_helper: true,
            helper_size: 1.0,
        }
    }

    /// The param has been declared on every light since and read by
    /// nothing. It reads now.
    #[test]
    fn show_helper_is_what_decides_whether_a_helper_is_drawn() {
        let mut l = light(LightKind::Point);
        assert!(!build_light_helpers(std::slice::from_ref(&l)).is_empty());

        l.show_helper = false;
        assert!(build_light_helpers(&[l]).is_empty());
    }

    /// A hidden light is not in the scene, so neither is its helper.
    #[test]
    fn an_invisible_light_draws_no_helper() {
        let mut l = light(LightKind::Point);
        l.visible = false;
        assert!(build_light_helpers(&[l]).is_empty());
    }

    /// Ambient light has no position and no direction. Any shape would be a lie
    /// about where it is, so it gets none.
    #[test]
    fn ambient_light_has_no_honest_shape_and_so_draws_nothing() {
        let l = light(LightKind::Ambient);
        assert!(build_light_helpers(&[l]).is_empty());
    }

    /// Every other type draws something, at the light's own position and in its
    /// own colour.
    #[test]
    fn every_positional_light_draws_at_its_own_position_in_its_own_colour() {
        for kind in [
            LightKind::Point,
            LightKind::Directional,
            LightKind::Spot,
            LightKind::RectArea,
            LightKind::Hemisphere,
        ] {
            let l = light(kind);
            let lines = build_light_helpers(std::slice::from_ref(&l));
            assert!(!lines.is_empty(), "{kind:?} must draw something");
            assert_eq!(lines.len() % 2, 0, "{kind:?}: lines come in pairs");

            // Every vertex takes the light's colour, so a helper is traceable
            // back to the light it belongs to. (The spot's inner cone dims it
            // deliberately, so compare hue rather than exact value there.)
            for v in &lines {
                let same_hue = v
                    .color
                    .iter()
                    .zip(l.color)
                    .all(|(a, b)| (a - b).abs() < 1e-6 || (a - b * 0.45).abs() < 1e-6);
                assert!(
                    same_hue,
                    "{kind:?}: {:?} is not the light's colour",
                    v.color
                );
            }
            // And the shape is built around where the light actually is.
            let near = lines.iter().any(|v| {
                let d = Vector3::new(
                    v.position[0] - l.position[0],
                    v.position[1] - l.position[1],
                    v.position[2] - l.position[2],
                );
                d.magnitude() < 6.0
            });
            assert!(near, "{kind:?} must be drawn near its light");
        }
    }

    /// The directional arrow's whole point: it has to be drawn SOMEWHERE, and
    /// until this phase `light_from_node` never filled `position`, so there was
    /// nowhere to put it.
    #[test]
    fn a_directional_arrow_starts_at_the_lights_position_and_follows_its_direction() {
        let mut l = light(LightKind::Directional);
        l.position = [0.0, 10.0, 0.0];
        l.direction = [0.0, -1.0, 0.0];
        let lines = build_light_helpers(&[l]);

        // The shaft: from the light, straight down.
        let shaft_start = lines[0].position;
        let shaft_end = lines[1].position;
        assert!((shaft_start[1] - 10.0).abs() < 1e-4, "starts at the light");
        assert!(
            shaft_end[1] < shaft_start[1],
            "and points the way it shines"
        );
    }

    /// A spot's cone should show where the light actually stops, which is its
    /// range when it has one.
    #[test]
    fn a_spot_cone_reaches_its_range() {
        let mut l = light(LightKind::Spot);
        l.position = [0.0, 0.0, 0.0];
        l.direction = [0.0, -1.0, 0.0];
        l.range = 20.0;
        let lines = build_light_helpers(&[l]);

        let deepest = lines
            .iter()
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        assert!(
            (deepest + 20.0).abs() < 1.0,
            "the cone should end at the range, got {deepest}"
        );
    }

    fn camera(id: u64) -> CameraDef {
        use solarxy_core::scene::CameraKind;
        CameraDef {
            id: SceneObjectId(id),
            kind: CameraKind::Perspective,
            position: [5.0, 3.0, 5.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y: 0.8,
            near: 0.1,
            far: 100.0,
            ortho_scale: 5.0,
            aspect: 16.0 / 9.0,
            show_gizmo: true,
            gizmo_size: 1.0,
        }
    }

    #[test]
    fn camera_gizmo_respects_show_gizmo_and_skip() {
        let c = camera(1);
        assert!(!build_camera_helpers(std::slice::from_ref(&c), None).is_empty());

        // Hidden gizmo draws nothing.
        let mut off = camera(1);
        off.show_gizmo = false;
        assert!(build_camera_helpers(&[off], None).is_empty());

        // The looked-through camera (skip) is not drawn.
        assert!(build_camera_helpers(&[camera(7)], Some(SceneObjectId(7))).is_empty());
        // A different camera still draws while another is skipped.
        assert!(!build_camera_helpers(&[camera(2)], Some(SceneObjectId(7))).is_empty());
    }

    /// Several lights at once, each with its own shape and colour: the common
    /// case, and the one that would break if the builder kept any state.
    #[test]
    fn helpers_accumulate_across_lights() {
        let a = light(LightKind::Point);
        let mut b = light(LightKind::Spot);
        b.color = [0.1, 0.2, 0.9];
        let both = build_light_helpers(&[a.clone(), b.clone()]);

        let mut a_off = a;
        a_off.show_helper = false;
        let only_b = build_light_helpers(&[a_off, b]);
        assert!(both.len() > only_b.len(), "dropping one drops its lines");
        assert!(!only_b.is_empty(), "and keeps the other's");
    }
}
