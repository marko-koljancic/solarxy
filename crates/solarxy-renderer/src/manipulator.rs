//! The transform manipulator overlay: translate arrows, rotate rings, scale
//! cubes.
//!
//! Pull-based, exactly like the rest of the renderer's overlay state: the host
//! hands over a [`ManipulatorState`] (or `None`) and the renderer draws it. The
//! desktop shell never calls it, so the desktop is unaffected until it wants the
//! same feature, at which point it wires the same three pieces.
//!
//! Geometry is generated on the CPU into the existing [`GizmoVertex`] format and
//! drawn through the existing gizmo shader, so this adds no shader and no bind
//! group -- only two pipeline variants that draw ON TOP of the scene
//! (`depth_compare: Always`), because a handle buried inside the mesh it is
//! meant to move is useless.
//!
//! Sizing is the caller's job: `scale` is world units per gizmo unit, obtained
//! from [`crate::camera::Camera::world_per_pixel`], which the hit-tester also
//! uses. That shared helper is what keeps the grab zones aligned with the
//! visuals: every accessor below (`axis_segment`, `plane_quad`, `ring`,
//! `scale_cube`) is read by BOTH the vertex generator and the host's hit test,
//! so a grab zone cannot drift from the thing it grabs.

use cgmath::{InnerSpace, Matrix3, Matrix4, Point3, Transform, Vector3};

use crate::model::GizmoVertex;

/// One grabbable part of the manipulator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    AxisX,
    AxisY,
    AxisZ,
    /// The little square between two axes: drags in that plane.
    PlaneXY,
    PlaneYZ,
    PlaneZX,
    /// A rotation ring about one axis.
    RingX,
    RingY,
    RingZ,
    /// The camera-facing outer ring: rotates about the view direction. The
    /// handle people reach for to tumble something to a rough angle.
    RingView,
    /// A cube at an axis tip: scales that lane.
    ScaleX,
    ScaleY,
    ScaleZ,
    /// The centre cube: scales all three lanes together (`uniform_scale`).
    ScaleUniform,
}

impl Handle {
    /// The two axis indices a plane handle spans; `None` for anything else.
    #[must_use]
    pub fn plane_axes(self) -> Option<(usize, usize)> {
        match self {
            Handle::PlaneXY => Some((0, 1)),
            Handle::PlaneYZ => Some((1, 2)),
            Handle::PlaneZX => Some((2, 0)),
            _ => None,
        }
    }

    /// The axis index a translate handle runs along; `None` for anything else.
    #[must_use]
    pub fn axis(self) -> Option<usize> {
        match self {
            Handle::AxisX => Some(0),
            Handle::AxisY => Some(1),
            Handle::AxisZ => Some(2),
            _ => None,
        }
    }

    /// The axis index a rotation ring turns about. `None` for the view ring,
    /// whose axis is the camera direction rather than one of the object's.
    #[must_use]
    pub fn ring_axis(self) -> Option<usize> {
        match self {
            Handle::RingX => Some(0),
            Handle::RingY => Some(1),
            Handle::RingZ => Some(2),
            _ => None,
        }
    }

    /// The axis index a scale cube scales along; `None` for anything else
    /// (including the uniform centre cube, which scales no single axis).
    #[must_use]
    pub fn scale_axis(self) -> Option<usize> {
        match self {
            Handle::ScaleX => Some(0),
            Handle::ScaleY => Some(1),
            Handle::ScaleZ => Some(2),
            _ => None,
        }
    }

    /// Every handle this tool offers, in hit-test priority order (first hit of
    /// equal depth wins). Plane handles lead the translate set so the small
    /// square between two axes stays grabbable where it overlaps their shafts;
    /// the axis rings lead the rotate set so they win against the outer view
    /// ring wherever the two cross at a grazing angle.
    #[must_use]
    pub fn for_tool(tool: ManipulatorTool) -> &'static [Handle] {
        match tool {
            ManipulatorTool::Translate => &[
                Handle::PlaneXY,
                Handle::PlaneYZ,
                Handle::PlaneZX,
                Handle::AxisX,
                Handle::AxisY,
                Handle::AxisZ,
            ],
            ManipulatorTool::Rotate => &[
                Handle::RingX,
                Handle::RingY,
                Handle::RingZ,
                Handle::RingView,
            ],
            ManipulatorTool::Scale => &[
                Handle::ScaleUniform,
                Handle::ScaleX,
                Handle::ScaleY,
                Handle::ScaleZ,
            ],
        }
    }
}

/// A scale cube's oriented box: centre, orthonormal axes, half-extents.
pub type ScaleCube = (Point3<f32>, [Vector3<f32>; 3], [f32; 3]);

/// Which tool the manipulator is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManipulatorTool {
    Translate,
    Rotate,
    Scale,
}

/// Everything the renderer needs to draw the manipulator this frame.
#[derive(Debug, Clone, Copy)]
pub struct ManipulatorState {
    /// World frame of the thing being manipulated: positioned at the point it
    /// actually rotates and scales about (its pivot), oriented by its basis, and
    /// carrying NO scale (a scaled frame would stretch the handles with the
    /// object, defeating the screen-constant sizing).
    pub anchor: Matrix4<f32>,
    /// The handle basis. Identity for world orientation; the object's own
    /// orthonormal basis for local. Kept separate from `anchor` so the world/
    /// local switch is one field rather than a rebuilt matrix.
    pub basis: Matrix3<f32>,
    /// The camera's forward direction at the anchor: the view ring's axis.
    pub view_dir: Vector3<f32>,
    pub tool: ManipulatorTool,
    pub hovered: Option<Handle>,
    pub active: Option<Handle>,
    /// World units per gizmo unit: `GIZMO_PX * world_per_pixel(...)`. Recomputed
    /// per pane, since a pane's camera and height decide it.
    pub scale: f32,
}

// ---- geometry constants (in gizmo units; multiplied by `scale`) ----

/// Arrow length, in pixels once scaled. The rest of the gizmo is proportional.
pub const GIZMO_PX: f32 = 90.0;
/// Click tolerance around an axis shaft or a ring band, in pixels.
pub const HIT_PX: f32 = 9.0;

/// The radius of a light's viewport marker, in pixels.
///
/// Lives here beside the gizmo's own sizes rather than with the marker
/// geometry, because it is read twice and by two crates that must agree: the
/// renderer draws a marker this big, and the engine's pick tests a disc this
/// big in screen space. They cannot disagree, because the host reads this one
/// constant and hands it to both, the same way it already hands
/// `GIZMO_PX * world_per_pixel` to the manipulator.
///
/// Smaller than a gizmo handle on purpose. A marker is a thing to find and
/// click, not a thing to drag, and at gizmo size six of them would crowd a
/// scene they exist to help you read.
pub const MARKER_PX: f32 = 11.0;

/// Where the shaft stops and the arrowhead begins.
const HEAD_START: f32 = 0.78;
const HEAD_RADIUS: f32 = 0.055;
/// Plane handles sit out at this fraction, spanning this much.
const PLANE_OFFSET: f32 = 0.30;
const PLANE_SIZE: f32 = 0.22;
/// The shaft starts slightly out from the origin so the three axes do not
/// smear into one blob at the centre.
const SHAFT_START: f32 = 0.12;

/// The axis rings sit just inside the arrow length; the view ring rides outside
/// them, where it cannot be confused for one.
const RING_RADIUS: f32 = 0.92;
const VIEW_RING_RADIUS: f32 = 1.12;
/// Enough that a ring reads as a circle rather than a polygon at any zoom.
const RING_SEGMENTS: usize = 64;

/// How wide a shaft or ring is drawn, in pixels.
///
/// Not a `LineList`: WebGPU has no line-width control, so a `LineList` is always
/// one physical pixel. At one pixel a ring is anti-aliased down to partial
/// coverage and blends halfway into the background, which turns a saturated blue
/// into a barely-there grey against a light viewport. So every shaft and ring is
/// drawn as a camera-facing RIBBON instead: two triangles per segment, widened in
/// screen space. That is what Blender and Maya both do, and it is the difference
/// between a gizmo you can see and one you have to hunt for.
const LINE_PX: f32 = 2.6;

/// The scale cubes sit at the axis tips; the uniform one is a little fatter so
/// it reads as the odd one out at the centre.
const CUBE_HALF: f32 = 0.06;
const UNIFORM_CUBE_HALF: f32 = 0.075;

/// The colours, chosen to survive the ACES tone mapping the overlay rides
/// through (it draws inside the main HDR pass, like the axis gizmo) and to stay
/// below the bloom threshold.
///
/// Deliberately DEEP rather than bright. The viewport background is a light
/// gradient, so a pale handle disappears into it; saturated dark ink holds its
/// contrast against both a light background and a light-coloured mesh, and still
/// reads against a dark one.
const X_COLOR: [f32; 3] = [0.80, 0.10, 0.16];
const Y_COLOR: [f32; 3] = [0.16, 0.62, 0.14];
const Z_COLOR: [f32; 3] = [0.13, 0.35, 0.85];
/// The view ring: neutral, because it belongs to the camera rather than to any
/// of the object's axes.
const VIEW_COLOR: [f32; 3] = [0.42, 0.44, 0.48];
/// The uniform-scale cube, likewise axis-less.
const UNIFORM_COLOR: [f32; 3] = [0.45, 0.47, 0.50];
/// Hover and grab both go amber, matching the app's accent.
const HILIGHT: [f32; 3] = [1.0, 0.78, 0.28];

/// How far an axis fades once it points at the camera, 0 = no fade,
/// 1 = fully neutral.
///
/// Not all the way to the background: a fully invisible handle reads as
/// broken rather than as unusable, and the axis is still pickable.
const VIEW_PARALLEL_FADE: f32 = 0.72;

/// Where the fade starts, as `|dot(axis, view)|`. Below this the axis has
/// enough screen-space extent to drag along and draws at full strength;
/// above it the projected axis collapses toward a point and a drag stops
/// meaning anything.
const FADE_ONSET: f32 = 0.86;

impl ManipulatorState {
    /// The colour a handle draws in, accounting for hover and grab.
    fn color_for(&self, handle: Handle) -> [f32; 3] {
        // While a drag is live the grabbed handle wins and hover is ignored: the
        // user is committed, and a stray hover highlight would be a lie.
        if self.active == Some(handle) || (self.active.is_none() && self.hovered == Some(handle)) {
            return HILIGHT;
        }
        let base = Self::base_color(handle);
        // 3ds Max's cue: an axis pointing at the camera has almost no
        // screen-space extent, so dragging along it is guesswork. Fading it
        // says so before the user tries. Purely visual: picking is
        // untouched, and the hover highlight above still wins, so
        // a faded axis you do manage to grab reads as grabbed.
        let fade = self.view_parallel_fade(handle);
        if fade <= 0.0 {
            return base;
        }
        let mut out = base;
        for (c, n) in out.iter_mut().zip(VIEW_COLOR) {
            *c += (n - *c) * fade;
        }
        out
    }

    /// How much `handle` should fade, from its angle to the view direction.
    /// Zero for handles with no single axis (the planes read fine head-on,
    /// and the view ring is defined BY the view direction).
    fn view_parallel_fade(&self, handle: Handle) -> f32 {
        let Some(axis) = handle
            .axis()
            .or_else(|| handle.scale_axis())
            .or_else(|| handle.ring_axis())
        else {
            return 0.0;
        };
        // A rotation ring is edge-on when its AXIS faces the camera, which is
        // the opposite of a translate arrow: the ring is then a line. So the
        // ring fades on the same measure for the opposite reason, and both
        // are unusable at the same angle.
        let alignment = self.axis_dir(axis).dot(self.view_dir).abs();
        let ring = handle.ring_axis().is_some();
        let measure = if ring { 1.0 - alignment } else { alignment };
        if measure <= FADE_ONSET {
            return 0.0;
        }
        // Ramps 0 to 1 across the remaining span, so the cue arrives
        // gradually rather than snapping on.
        ((measure - FADE_ONSET) / (1.0 - FADE_ONSET)).clamp(0.0, 1.0) * VIEW_PARALLEL_FADE
    }

    /// The handle's own colour, before hover and fade.
    fn base_color(handle: Handle) -> [f32; 3] {
        match handle {
            // A plane handle takes the colour of the axis it is NORMAL to, the
            // way Blender and Maya both do it, so the YZ square reads as "the X
            // plane".
            Handle::AxisX | Handle::PlaneYZ | Handle::RingX | Handle::ScaleX => X_COLOR,
            Handle::AxisY | Handle::PlaneZX | Handle::RingY | Handle::ScaleY => Y_COLOR,
            Handle::AxisZ | Handle::PlaneXY | Handle::RingZ | Handle::ScaleZ => Z_COLOR,
            Handle::RingView => VIEW_COLOR,
            Handle::ScaleUniform => UNIFORM_COLOR,
        }
    }

    /// A handle axis in world space. This is where world-versus-local
    /// orientation actually happens: the basis is identity in world mode and the
    /// object's own rotation in local mode, and EVERY handle accessor routes
    /// through here, so the whole gizmo swings together.
    fn axis_dir(&self, axis: usize) -> Vector3<f32> {
        let v = match axis {
            0 => self.basis.x,
            1 => self.basis.y,
            _ => self.basis.z,
        };
        // Defensive: a degenerate basis would otherwise produce NaN geometry.
        if v.magnitude2() < 1e-12 {
            match axis {
                0 => Vector3::unit_x(),
                1 => Vector3::unit_y(),
                _ => Vector3::unit_z(),
            }
        } else {
            v.normalize()
        }
    }

    /// The world-space origin of the manipulator: the point the object actually
    /// rotates and scales about.
    #[must_use]
    pub fn origin(&self) -> Point3<f32> {
        Point3::new(self.anchor.w.x, self.anchor.w.y, self.anchor.w.z)
    }

    /// The world-space endpoints of an axis shaft: what the ray-capsule hit test
    /// runs against, and what the drag re-parametrizes along.
    ///
    /// Shared with the hit tester on purpose -- the grab zone IS the drawn shaft.
    #[must_use]
    pub fn axis_segment(&self, axis: usize) -> (Point3<f32>, Point3<f32>) {
        let o = self.origin();
        let d = self.axis_dir(axis) * self.scale;
        (o + d * SHAFT_START, o + d)
    }

    /// A plane handle's quad: origin corner plus the two edge vectors.
    #[must_use]
    pub fn plane_quad(&self, a: usize, b: usize) -> (Point3<f32>, Vector3<f32>, Vector3<f32>) {
        let o = self.origin();
        let (ua, ub) = (self.axis_dir(a) * self.scale, self.axis_dir(b) * self.scale);
        let corner = o + ua * PLANE_OFFSET + ub * PLANE_OFFSET;
        (corner, ua * PLANE_SIZE, ub * PLANE_SIZE)
    }

    /// A rotation ring as `(centre, normal, radius)`: what both the ring-band
    /// hit test and the drag's plane projection run against.
    ///
    /// The view ring turns about the camera direction, so it is the one handle
    /// whose axis is not the object's.
    #[must_use]
    pub fn ring(&self, handle: Handle) -> Option<(Point3<f32>, Vector3<f32>, f32)> {
        let o = self.origin();
        if handle == Handle::RingView {
            let n = self.view_dir;
            return (n.magnitude2() > 1e-12)
                .then(|| (o, n.normalize(), VIEW_RING_RADIUS * self.scale));
        }
        let axis = handle.ring_axis()?;
        Some((o, self.axis_dir(axis), RING_RADIUS * self.scale))
    }

    /// A scale cube as `(centre, orthonormal axes, half-extents)`: an oriented
    /// box, because under local orientation the cubes ride the object's axes.
    #[must_use]
    pub fn scale_cube(&self, handle: Handle) -> Option<ScaleCube> {
        let axes = [self.axis_dir(0), self.axis_dir(1), self.axis_dir(2)];
        let o = self.origin();
        if handle == Handle::ScaleUniform {
            let h = UNIFORM_CUBE_HALF * self.scale;
            return Some((o, axes, [h; 3]));
        }
        let axis = handle.scale_axis()?;
        let h = CUBE_HALF * self.scale;
        Some((o + axes[axis] * self.scale, axes, [h; 3]))
    }

    /// Builds this frame's overlay geometry: `(lines, triangles)`.
    ///
    /// Two lists because they need different primitive topologies, not because
    /// they are conceptually different.
    #[must_use]
    pub fn build_vertices(&self) -> (Vec<GizmoVertex>, Vec<GizmoVertex>) {
        match self.tool {
            ManipulatorTool::Translate => self.build_translate(),
            ManipulatorTool::Rotate => self.build_rotate(),
            ManipulatorTool::Scale => self.build_scale(),
        }
    }

    fn build_translate(&self) -> (Vec<GizmoVertex>, Vec<GizmoVertex>) {
        let mut tris = Vec::with_capacity(3 * 8 * 2 * 3 + 3 * 2 * 3 + 3 * 6);

        for axis in 0..3 {
            let handle = [Handle::AxisX, Handle::AxisY, Handle::AxisZ][axis];
            let color = self.color_for(handle);
            let (start, end) = self.axis_segment(axis);
            self.push_ribbon(&mut tris, start, end, color);
            self.push_arrowhead(&mut tris, axis, color);
        }

        for (a, b, handle) in [
            (0usize, 1usize, Handle::PlaneXY),
            (1, 2, Handle::PlaneYZ),
            (2, 0, Handle::PlaneZX),
        ] {
            let color = self.color_for(handle);
            let (corner, u, v) = self.plane_quad(a, b);
            // Two triangles, wound both ways: the quad must read from either
            // side, and the pipeline does not cull.
            let (p0, p1, p2, p3) = (corner, corner + u, corner + u + v, corner + v);
            for p in [p0, p1, p2, p0, p2, p3] {
                tris.push(vertex(p, color));
            }
        }

        (Vec::new(), tris)
    }

    fn build_rotate(&self) -> (Vec<GizmoVertex>, Vec<GizmoVertex>) {
        let mut tris = Vec::with_capacity(4 * RING_SEGMENTS * 6);
        for handle in Handle::for_tool(ManipulatorTool::Rotate) {
            let Some((center, normal, radius)) = self.ring(*handle) else {
                continue;
            };
            self.push_ring(&mut tris, center, normal, radius, self.color_for(*handle));
        }
        (Vec::new(), tris)
    }

    fn build_scale(&self) -> (Vec<GizmoVertex>, Vec<GizmoVertex>) {
        let mut tris = Vec::with_capacity(4 * 36 + 3 * 6);

        for axis in 0..3 {
            let handle = [Handle::ScaleX, Handle::ScaleY, Handle::ScaleZ][axis];
            let color = self.color_for(handle);
            // The shaft runs out to the cube, exactly like the translate arrow's
            // does, so the two tools read as the same gizmo wearing a different
            // tip.
            let (start, end) = self.axis_segment(axis);
            self.push_ribbon(&mut tris, start, end, color);
            if let Some((c, axes, half)) = self.scale_cube(handle) {
                push_cube(&mut tris, c, axes, half, color);
            }
        }

        if let Some((c, axes, half)) = self.scale_cube(Handle::ScaleUniform) {
            push_cube(
                &mut tris,
                c,
                axes,
                half,
                self.color_for(Handle::ScaleUniform),
            );
        }

        (Vec::new(), tris)
    }

    /// Half the ribbon width, in world units at the gizmo's depth. `scale` is
    /// `GIZMO_PX * world_per_pixel`, so dividing it back out recovers the world
    /// size of one pixel.
    fn half_width(&self) -> f32 {
        LINE_PX * 0.5 * self.scale / GIZMO_PX
    }

    /// One segment of a shaft or ring, as a quad that always faces the camera.
    ///
    /// Widened perpendicular to BOTH the segment and the view direction, so the
    /// ribbon keeps its width from any angle instead of vanishing when seen
    /// edge-on (which is exactly what a flat quad in the ring's own plane would
    /// do, and it is the failure the axis rings would hit constantly).
    fn push_ribbon(
        &self,
        tris: &mut Vec<GizmoVertex>,
        a: Point3<f32>,
        b: Point3<f32>,
        color: [f32; 3],
    ) {
        let along = b - a;
        if along.magnitude2() < 1e-12 {
            return;
        }
        let side = along.cross(self.view_dir);
        // Segment parallel to the view axis: it projects to a point, so any width
        // is arbitrary. Fall back to any perpendicular rather than emit NaNs.
        let side = if side.magnitude2() < 1e-12 {
            basis_for_plane(along.normalize()).0
        } else {
            side.normalize()
        } * self.half_width();

        let (p0, p1, p2, p3) = (a - side, a + side, b + side, b - side);
        for p in [p0, p1, p2, p0, p2, p3] {
            tris.push(vertex(p, color));
        }
    }

    /// A ring as a closed ribbon loop in the plane through `center` with `normal`.
    fn push_ring(
        &self,
        tris: &mut Vec<GizmoVertex>,
        center: Point3<f32>,
        normal: Vector3<f32>,
        radius: f32,
        color: [f32; 3],
    ) {
        let (u, v) = basis_for_plane(normal);
        let point = |i: usize| {
            let theta = (i as f32) * std::f32::consts::TAU / (RING_SEGMENTS as f32);
            center + (u * theta.cos() + v * theta.sin()) * radius
        };
        for i in 0..RING_SEGMENTS {
            self.push_ribbon(tris, point(i), point((i + 1) % RING_SEGMENTS), color);
        }
    }

    /// A cone at the tip of an axis, as a triangle fan around its base plus a
    /// cap, so it reads as solid from any angle.
    fn push_arrowhead(&self, tris: &mut Vec<GizmoVertex>, axis: usize, color: [f32; 3]) {
        const SEGMENTS: usize = 8;
        let o = self.origin();
        let dir = self.axis_dir(axis);
        let base_center = o + dir * (self.scale * HEAD_START);
        let tip = o + dir * self.scale;
        let radius = self.scale * HEAD_RADIUS;

        // Any two vectors perpendicular to the axis span its base circle.
        let u = self.axis_dir((axis + 1) % 3) * radius;
        let v = self.axis_dir((axis + 2) % 3) * radius;

        let ring: Vec<Point3<f32>> = (0..SEGMENTS)
            .map(|i| {
                let theta = (i as f32) * std::f32::consts::TAU / (SEGMENTS as f32);
                base_center + u * theta.cos() + v * theta.sin()
            })
            .collect();

        for i in 0..SEGMENTS {
            let a = ring[i];
            let b = ring[(i + 1) % SEGMENTS];
            for p in [a, b, tip] {
                tris.push(vertex(p, color));
            }
            // Cap, so the cone is not hollow when seen from behind.
            for p in [b, a, base_center] {
                tris.push(vertex(p, color));
            }
        }
    }
}

fn vertex(p: Point3<f32>, color: [f32; 3]) -> GizmoVertex {
    GizmoVertex {
        position: p.into(),
        color,
    }
}

/// Two orthonormal vectors spanning the plane with this normal. The seed is
/// chosen away from the normal so the cross product never degenerates.
fn basis_for_plane(normal: Vector3<f32>) -> (Vector3<f32>, Vector3<f32>) {
    let n = normal.normalize();
    let seed = if n.x.abs() < 0.9 {
        Vector3::unit_x()
    } else {
        Vector3::unit_y()
    };
    let u = n.cross(seed).normalize();
    (u, n.cross(u).normalize())
}

/// A solid oriented box: 12 triangles, wound outward.
fn push_cube(
    tris: &mut Vec<GizmoVertex>,
    center: Point3<f32>,
    axes: [Vector3<f32>; 3],
    half: [f32; 3],
    color: [f32; 3],
) {
    // Six faces, two triangles each.
    const FACES: [[usize; 4]; 6] = [
        [0, 3, 2, 1], // -Z
        [4, 5, 6, 7], // +Z
        [0, 1, 5, 4], // -Y
        [3, 7, 6, 2], // +Y
        [0, 4, 7, 3], // -X
        [1, 2, 6, 5], // +X
    ];
    let (x, y, z) = (axes[0] * half[0], axes[1] * half[1], axes[2] * half[2]);
    // The eight corners, indexed by the sign of each axis.
    let corner = |sx: f32, sy: f32, sz: f32| center + x * sx + y * sy + z * sz;
    let c = [
        corner(-1.0, -1.0, -1.0),
        corner(1.0, -1.0, -1.0),
        corner(1.0, 1.0, -1.0),
        corner(-1.0, 1.0, -1.0),
        corner(-1.0, -1.0, 1.0),
        corner(1.0, -1.0, 1.0),
        corner(1.0, 1.0, 1.0),
        corner(-1.0, 1.0, 1.0),
    ];
    for f in FACES {
        for i in [f[0], f[1], f[2], f[0], f[2], f[3]] {
            tris.push(vertex(c[i], color));
        }
    }
}

/// Transforms a world point by a matrix. Small helper so callers do not have to
/// import cgmath's `Transform` just to place a handle.
#[must_use]
pub fn transform_point(m: &Matrix4<f32>, p: Point3<f32>) -> Point3<f32> {
    m.transform_point(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::SquareMatrix;

    /// Colours are f32 arrays; clippy (rightly) forbids `==` on those.
    fn same(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
    }

    fn state(scale: f32) -> ManipulatorState {
        ManipulatorState {
            anchor: Matrix4::from_translation(Vector3::new(1.0, 2.0, 3.0)),
            basis: Matrix3::identity(),
            view_dir: -Vector3::unit_z(),
            tool: ManipulatorTool::Translate,
            hovered: None,
            active: None,
            scale,
        }
    }

    #[test]
    fn the_gizmo_sits_at_its_anchor() {
        let s = state(1.0);
        assert_eq!(s.origin(), Point3::new(1.0, 2.0, 3.0));

        let identity = ManipulatorState {
            anchor: Matrix4::identity(),
            ..s
        };
        assert_eq!(identity.origin(), Point3::new(0.0, 0.0, 0.0));
    }

    #[test]
    fn axis_segments_scale_with_the_camera_distance() {
        // `scale` is world-units-per-gizmo-unit, so doubling it doubles the
        // shaft: that is what keeps the gizmo the same SIZE on screen as the
        // camera pulls back.
        let near = state(1.0);
        let far = state(2.0);
        let (_, near_end) = near.axis_segment(0);
        let (_, far_end) = far.axis_segment(0);
        assert!((near_end.x - near.origin().x - 1.0).abs() < 1e-6);
        assert!((far_end.x - far.origin().x - 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_three_axes_run_along_the_three_world_axes() {
        let s = state(1.0);
        let o = s.origin();
        for (axis, expected) in [
            (0, Vector3::unit_x()),
            (1, Vector3::unit_y()),
            (2, Vector3::unit_z()),
        ] {
            let (start, end) = s.axis_segment(axis);
            let along = end - start;
            assert!(along.dot(expected) > 0.0);
            // Starting off the origin, so the three shafts do not smear together.
            assert!((start - o).magnitude() > 0.0);
        }
    }

    /// The whole world-versus-local switch lives in one field, so proving the
    /// basis swings the handles proves it for every tool at once.
    #[test]
    fn a_local_basis_swings_every_handle_onto_the_objects_axes() {
        // A 90-degree turn about Y sends the object's local +X onto world -Z.
        let s = ManipulatorState {
            anchor: Matrix4::identity(),
            basis: Matrix3::from_angle_y(cgmath::Deg(90.0)),
            ..state(1.0)
        };

        let (start, end) = s.axis_segment(0);
        let along = (end - start).normalize();
        assert!(
            along.z < -0.99 && along.x.abs() < 1e-5,
            "the X arrow must follow the object, got {along:?}"
        );

        // And so must the X ring's normal, which is what makes a local rotate
        // turn about the object's axis rather than the world's.
        let (_, normal, _) = s.ring(Handle::RingX).unwrap();
        assert!(
            normal.z < -0.99,
            "the X ring's axis follows too: {normal:?}"
        );

        // And the X scale cube rides out along the same direction.
        let (center, _, _) = s.scale_cube(Handle::ScaleX).unwrap();
        assert!(center.z < -0.99 && center.x.abs() < 1e-5, "{center:?}");
    }

    #[test]
    fn a_plane_handle_spans_its_two_axes_and_sits_off_the_origin() {
        let s = state(1.0);
        let (corner, u, v) = s.plane_quad(0, 1);
        assert!(corner.x > s.origin().x && corner.y > s.origin().y);
        assert!((corner.z - s.origin().z).abs() < 1e-6);
        // Spanned by X and Y, so its normal is Z.
        assert!(u.x > 0.0 && u.y.abs() < 1e-6);
        assert!(v.y > 0.0 && v.x.abs() < 1e-6);
    }

    #[test]
    fn hover_and_grab_light_the_handle_up() {
        let mut s = state(1.0);
        assert!(same(s.color_for(Handle::AxisX), X_COLOR));

        s.hovered = Some(Handle::AxisX);
        assert!(same(s.color_for(Handle::AxisX), HILIGHT));
        assert!(
            same(s.color_for(Handle::AxisY), Y_COLOR),
            "only the hovered one"
        );

        // While dragging, the grabbed handle wins and hover is ignored: the user
        // is committed, and a stray hover highlight would be a lie.
        s.active = Some(Handle::AxisZ);
        assert!(same(s.color_for(Handle::AxisZ), HILIGHT));
        assert!(same(s.color_for(Handle::AxisX), X_COLOR));
    }

    /// Everything is triangles now, not a `LineList`: WebGPU cannot widen a line,
    /// and a one-pixel handle washes out into the background.
    #[test]
    fn build_vertices_emits_ribbons_and_solid_heads() {
        let (lines, tris) = state(1.0).build_vertices();
        assert!(lines.is_empty(), "nothing is a hairline any more");
        // Three cones (8 segments x 2 tris x 3 verts), three plane quads (2 tris),
        // and three shaft ribbons (2 tris).
        assert_eq!(tris.len(), 3 * 8 * 2 * 3 + 3 * 2 * 3 + 3 * 2 * 3);
    }

    /// A ribbon is widened across the VIEW direction, so it keeps its width from
    /// any angle. Widening it inside the ring's own plane instead would make the
    /// ring vanish exactly when it is seen edge-on, which is most of the time.
    #[test]
    fn a_ribbon_faces_the_camera_rather_than_lying_in_its_own_plane() {
        let s = ManipulatorState {
            tool: ManipulatorTool::Rotate,
            view_dir: -Vector3::unit_z(),
            ..state(1.0)
        };
        // The Z ring lies in the XY plane, so it is seen FACE-on from -Z: its
        // ribbon should be widened radially, staying in that plane.
        let (_, tris) = s.build_vertices();
        assert!(!tris.is_empty());

        // The X ring lies in the YZ plane, seen EDGE-on. A ribbon in its own
        // plane would collapse; a camera-facing one keeps its width, so the ring
        // must still span a measurable range in X.
        //
        // Found by the colour the state ACTUALLY assigns rather than by the
        // raw X_COLOR: an edge-on ring is view-parallel-faded, so a literal
        // comparison would match nothing and silently measure an empty set.
        let ring_color = s.color_for(Handle::RingX);
        let x_ring: Vec<_> = tris
            .iter()
            .filter(|v| same(v.color, ring_color))
            .map(|v| v.position[0])
            .collect();
        assert!(
            !x_ring.is_empty(),
            "the X ring must be in the vertex stream"
        );
        let spread = x_ring.iter().fold(f32::MIN, |a, b| a.max(*b))
            - x_ring.iter().fold(f32::MAX, |a, b| a.min(*b));
        assert!(
            spread > 1e-4,
            "the edge-on X ring must still have width, got {spread}"
        );
    }

    #[test]
    fn a_plane_handle_takes_the_colour_of_the_axis_it_faces() {
        // Blender/Maya convention: the YZ plane reads as "the X plane".
        let s = state(1.0);
        assert!(same(s.color_for(Handle::PlaneYZ), X_COLOR));
        assert!(same(s.color_for(Handle::PlaneZX), Y_COLOR));
        assert!(same(s.color_for(Handle::PlaneXY), Z_COLOR));
    }

    /// The view ring rides outside the axis rings, so it can never be mistaken
    /// for one of them, and it turns about the camera rather than the object.
    #[test]
    fn the_view_ring_faces_the_camera_and_sits_outside_the_axis_rings() {
        let s = ManipulatorState {
            tool: ManipulatorTool::Rotate,
            view_dir: Vector3::new(0.0, 0.0, -1.0),
            ..state(1.0)
        };
        let (_, view_normal, view_radius) = s.ring(Handle::RingView).unwrap();
        let (_, _, axis_radius) = s.ring(Handle::RingX).unwrap();

        assert!(
            (view_normal.z + 1.0).abs() < 1e-6,
            "turns about the view axis"
        );
        assert!(
            view_radius > axis_radius,
            "and rides outside the axis rings"
        );
    }

    #[test]
    fn scale_draws_three_axis_cubes_plus_a_uniform_one() {
        let s = ManipulatorState {
            tool: ManipulatorTool::Scale,
            ..state(1.0)
        };
        let (lines, tris) = s.build_vertices();
        assert!(lines.is_empty());
        // Four solid cubes (12 triangles each) plus three shaft ribbons.
        assert_eq!(tris.len(), 4 * 36 + 3 * 6);

        // The uniform cube sits AT the centre (it scales everything, so it
        // belongs to no axis) and is fatter, so it reads as the odd one out.
        let (center, _, half) = s.scale_cube(Handle::ScaleUniform).unwrap();
        assert_eq!(center, s.origin());
        let (_, _, axis_half) = s.scale_cube(Handle::ScaleX).unwrap();
        assert!(half[0] > axis_half[0]);
    }

    /// Each tool offers only its own handles: a rotate ring must never be
    /// grabbable while the Move tool is up.
    #[test]
    fn each_tool_offers_only_its_own_handles() {
        let translate = Handle::for_tool(ManipulatorTool::Translate);
        let rotate = Handle::for_tool(ManipulatorTool::Rotate);
        let scale = Handle::for_tool(ManipulatorTool::Scale);

        assert!(translate.contains(&Handle::AxisX) && !translate.contains(&Handle::RingX));
        assert!(rotate.contains(&Handle::RingView) && !rotate.contains(&Handle::AxisX));
        assert!(scale.contains(&Handle::ScaleUniform) && !scale.contains(&Handle::RingX));

        // Plane handles lead translate (they overlap the shafts and would be
        // unusable otherwise); axis rings lead rotate (they must win against the
        // outer view ring where the two cross).
        assert_eq!(translate[0], Handle::PlaneXY);
        assert_eq!(rotate[0], Handle::RingX);
    }
}

#[cfg(test)]
mod view_fade_tests {
    // Exact comparison is the assertion: the fade's early-return path yields
    // a literal 0.0, and an unfaded handle must come back byte-identical to
    // its own constant rather than merely close to it.
    #![allow(clippy::float_cmp)]

    use super::*;
    use cgmath::{Matrix4, SquareMatrix, Vector3};

    /// A translate manipulator at the origin, viewed down `view_dir`.
    fn state(view_dir: Vector3<f32>) -> ManipulatorState {
        ManipulatorState {
            anchor: Matrix4::identity(),
            basis: cgmath::Matrix3::identity(),
            view_dir: view_dir.normalize(),
            tool: ManipulatorTool::Translate,
            hovered: None,
            active: None,
            scale: 1.0,
        }
    }

    #[test]
    fn an_axis_across_the_view_does_not_fade() {
        // Looking down -Z, the X axis is fully across the screen.
        let s = state(Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(s.view_parallel_fade(Handle::AxisX), 0.0);
        assert_eq!(s.color_for(Handle::AxisX), X_COLOR);
    }

    #[test]
    fn an_axis_pointing_at_the_camera_fades() {
        // Looking down -Z, the Z axis points at the viewer.
        let s = state(Vector3::new(0.0, 0.0, -1.0));
        let fade = s.view_parallel_fade(Handle::AxisZ);
        assert!(fade > 0.0, "a view-parallel axis must fade");
        let c = s.color_for(Handle::AxisZ);
        assert_ne!(c, Z_COLOR, "and its colour must actually move");
        // Toward neutral, never past it.
        for (got, (from, to)) in c.iter().zip(Z_COLOR.iter().zip(VIEW_COLOR.iter())) {
            let lo = from.min(*to);
            let hi = from.max(*to);
            assert!(
                *got >= lo - 1e-6 && *got <= hi + 1e-6,
                "{got} outside [{lo}, {hi}]"
            );
        }
    }

    #[test]
    fn the_fade_never_reaches_the_neutral_colour() {
        // A fully invisible handle reads as broken, not as unusable.
        let s = state(Vector3::new(0.0, 0.0, -1.0));
        assert!(s.view_parallel_fade(Handle::AxisZ) <= VIEW_PARALLEL_FADE);
        assert_ne!(s.color_for(Handle::AxisZ), VIEW_COLOR);
    }

    #[test]
    fn hover_still_wins_over_the_fade() {
        // Whatever the angle, a hovered handle has to read as hovered.
        let mut s = state(Vector3::new(0.0, 0.0, -1.0));
        s.hovered = Some(Handle::AxisZ);
        assert_eq!(s.color_for(Handle::AxisZ), HILIGHT);
    }

    #[test]
    fn a_grabbed_handle_stays_highlighted_even_view_parallel() {
        let mut s = state(Vector3::new(0.0, 0.0, -1.0));
        s.active = Some(Handle::AxisZ);
        assert_eq!(s.color_for(Handle::AxisZ), HILIGHT);
    }

    #[test]
    fn a_rotation_ring_fades_when_it_is_edge_on_not_when_it_faces_us() {
        // The opposite measure to an arrow: a ring whose axis faces the
        // camera is a full circle (usable); one whose axis lies across the
        // view is a line (not).
        let s = state(Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(
            s.view_parallel_fade(Handle::RingZ),
            0.0,
            "facing us, usable"
        );
        assert!(
            s.view_parallel_fade(Handle::RingX) > 0.0,
            "edge-on, unusable"
        );
    }

    #[test]
    fn a_plane_handle_never_fades() {
        // A plane reads fine at any angle its normal happens to take.
        let s = state(Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(s.view_parallel_fade(Handle::PlaneXY), 0.0);
        assert_eq!(s.view_parallel_fade(Handle::PlaneYZ), 0.0);
    }

    #[test]
    fn the_view_ring_and_uniform_cube_are_never_faded() {
        // Neither belongs to an object axis, so neither has an angle to be
        // wrong at.
        let s = state(Vector3::new(0.0, 0.0, -1.0));
        assert_eq!(s.view_parallel_fade(Handle::RingView), 0.0);
        assert_eq!(s.view_parallel_fade(Handle::ScaleUniform), 0.0);
    }
}
