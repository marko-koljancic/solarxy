//! The viewport gizmo: tool state, hit-testing and drag solving (phase 11).
//!
//! All of it lives in Rust, on purpose. The drag loop runs at pointer rate, and
//! routing it through JavaScript would mean a boundary crossing per mouse move;
//! instead `pointer_move` solves the drag and streams the result straight into
//! the engine's preview lane, so a drag costs ZERO JS traffic. The only new JS
//! surface is `set_tool`.
//!
//! The POLICY (which node a drag writes, whether one must be appended first)
//! lives in the engine (`Engine::gizmo_target`); this module does routing and
//! arithmetic only.

use solarxy_core::raycast::{Ray, closest_points_ray_segment, intersect_plane, intersect_quad};
use solarxy_graph::engine::GizmoTarget;
use solarxy_renderer::manipulator::{HIT_PX, Handle, ManipulatorState};

/// The active viewport tool. Rotate and Scale are Phase 12: the modes exist so
/// the column can select them, but they resolve no handles and so never grab.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolMode {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}

impl ToolMode {
    /// Parses the tool id the frontend sends. Not `FromStr`: an unknown id is a
    /// harmless fall-back to Select, not an error worth a `Result`.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "move" => ToolMode::Move,
            "rotate" => ToolMode::Rotate,
            "scale" => ToolMode::Scale,
            _ => ToolMode::Select,
        }
    }

    /// Whether this tool draws and grabs a manipulator at all.
    #[must_use]
    pub fn manipulates(self) -> bool {
        // Phase 11 ships translate only; Rotate and Scale render no gizmo, which
        // is exactly why their buttons are disabled in the tool column.
        matches!(self, ToolMode::Move)
    }
}

/// A drag in flight.
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub handle: Handle,
    /// The target as resolved at drag START, with `node` already replaced by the
    /// real target on the append path (the engine minted it via
    /// `EnsureTransformTarget`).
    pub target: GizmoTarget,
    /// The target's translate when the drag began, in the target's own space.
    pub start_local: [f32; 3],
    /// The world point under the cursor when the drag began, so the object moves
    /// WITH the cursor instead of snapping its origin to it.
    pub grab_world: cgmath::Point3<f32>,
}

/// Tool + hover + drag. One per app.
#[derive(Debug, Default)]
pub struct GizmoState {
    pub tool: ToolMode,
    pub hovered: Option<Handle>,
    pub drag: Option<Drag>,
}

impl GizmoState {
    /// The manipulator to draw this frame, or `None`.
    #[must_use]
    pub fn manipulator(&self, target: &GizmoTarget) -> Option<ManipulatorState> {
        if !self.tool.manipulates() {
            return None;
        }
        Some(ManipulatorState {
            anchor: cgmath::Matrix4::from(target.anchor),
            tool: solarxy_renderer::manipulator::ManipulatorTool::Translate,
            hovered: self.hovered,
            active: self.drag.map(|d| d.handle),
            // Overwritten per pane by `Renderer::write_manipulator`, which knows
            // that pane's camera and height.
            scale: 1.0,
        })
    }
}

/// Which handle a ray grabs, if any.
///
/// `world_per_px` converts the pixel tolerance into world units through the SAME
/// helper the vertex generator uses, so the grab zone is exactly the drawn
/// handle. Plane handles are tested first (see `Handle::all`), so the little
/// square between two axes stays clickable where it overlaps their shafts.
#[must_use]
pub fn hit_test(ray: &Ray, state: &ManipulatorState, world_per_px: f32) -> Option<Handle> {
    let tolerance = HIT_PX * world_per_px;
    let mut best: Option<(f32, Handle)> = None;

    for handle in Handle::all() {
        let t = if let Some((a, b)) = handle.plane_axes() {
            let (corner, u, v) = state.plane_quad(a, b);
            intersect_quad(ray, corner, u, v)
        } else if let Some(axis) = handle.axis() {
            let (start, end) = state.axis_segment(axis);
            let (t_ray, _, distance) = closest_points_ray_segment(ray, start, end);
            // A capsule: the shaft plus a pixel-sized skin around it.
            (distance <= tolerance).then_some(t_ray)
        } else {
            None
        };

        if let Some(t) = t
            && best.is_none_or(|(best_t, _)| t < best_t)
        {
            best = Some((t, handle));
        }
    }
    best.map(|(_, handle)| handle)
}

/// Where the pointer ray lands on the handle's constraint, in world space.
///
/// An axis handle constrains to its line (the ray is re-parametrized against
/// it); a plane handle constrains to its plane. Returns `None` when the ray is
/// degenerate against that constraint (looking straight down an axis, or edge-on
/// at a plane), in which case the caller holds the previous value rather than
/// letting the object shoot off to infinity.
#[must_use]
pub fn solve_drag_point(
    ray: &Ray,
    state: &ManipulatorState,
    handle: Handle,
) -> Option<cgmath::Point3<f32>> {
    use cgmath::InnerSpace;

    if let Some(axis) = handle.axis() {
        let (start, end) = state.axis_segment(axis);
        let dir = (end - start).normalize();
        // The axis as an INFINITE line: a drag must be able to run past the end
        // of the drawn arrow, so this deliberately does not clamp the way the
        // hit test does.
        let far = start + dir * 1.0e5;
        let near = start - dir * 1.0e5;
        let (_, s, _) = closest_points_ray_segment(ray, near, far);
        // Reject a ray nearly parallel to the axis: the solution is unstable and
        // the object would jump.
        if ray.direction.cross(dir).magnitude() < 1e-3 {
            return None;
        }
        return Some(near + (far - near) * s);
    }

    let (axis_a, axis_b) = handle.plane_axes()?;
    let (corner, edge_u, edge_v) = state.plane_quad(axis_a, axis_b);
    let normal = edge_u.cross(edge_v).normalize();
    // The plane through the gizmo ORIGIN, not through the little square: the
    // square is just the grab affordance, the drag happens on the whole plane.
    let t = intersect_plane(ray, state.origin(), normal)
        .or_else(|| intersect_plane(ray, corner, normal))?;
    Some(ray.origin + ray.direction * t)
}

/// The target's new translate, given where the drag started and where the
/// pointer is now.
///
/// The world delta is mapped into the target's own space through the inverse of
/// `parent`: identity at root (the geo's translate IS world), and the geo's world
/// matrix inside a subflow, where a rotated or scaled container means a world
/// drag of one metre is NOT one metre of local translate.
#[must_use]
pub fn drag_to_local(drag: &Drag, world_now: cgmath::Point3<f32>) -> Option<[f32; 3]> {
    use cgmath::SquareMatrix;

    let world_delta = world_now - drag.grab_world;
    let parent = cgmath::Matrix4::from(drag.target.parent);
    let inv = parent.invert()?;
    // A direction, not a point: w = 0, so the parent's translation drops out and
    // only its rotation and scale apply.
    let local_delta = inv * cgmath::Vector4::new(world_delta.x, world_delta.y, world_delta.z, 0.0);

    Some([
        drag.start_local[0] + local_delta.x,
        drag.start_local[1] + local_delta.y,
        drag.start_local[2] + local_delta.z,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::{InnerSpace, Matrix4, Point3, SquareMatrix, Vector3};
    use solarxy_renderer::manipulator::ManipulatorTool;

    fn state_at(origin: [f32; 3], scale: f32) -> ManipulatorState {
        ManipulatorState {
            anchor: Matrix4::from_translation(Vector3::from(origin)),
            tool: ManipulatorTool::Translate,
            hovered: None,
            active: None,
            scale,
        }
    }

    fn ray(origin: [f32; 3], dir: [f32; 3]) -> Ray {
        Ray {
            origin: Point3::from(origin),
            direction: Vector3::from(dir).normalize(),
        }
    }

    fn target(parent: Matrix4<f32>) -> GizmoTarget {
        GizmoTarget {
            ctx: solarxy_graph::document::GraphContext::Root,
            node: solarxy_graph::document::NodeId(1),
            current: [0.0; 3],
            anchor: Matrix4::identity().into(),
            parent: parent.into(),
            append_pending: false,
        }
    }

    #[test]
    fn the_x_arrow_is_grabbable_and_the_empty_space_beside_it_is_not() {
        let s = state_at([0.0, 0.0, 0.0], 1.0);
        // Down -Z, through the middle of the +X shaft.
        let on = ray([0.5, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit_test(&on, &s, 0.001), Some(Handle::AxisX));

        // Well off the shaft, and well off the plane handles.
        let off = ray([0.5, -0.6, 5.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit_test(&off, &s, 0.001), None);
    }

    #[test]
    fn the_grab_zone_grows_with_the_pixel_tolerance() {
        // The whole point of routing the tolerance through world_per_pixel: a
        // gizmo far from the camera must still be as easy to click.
        let s = state_at([0.0, 0.0, 0.0], 1.0);
        let near_miss = ray([0.5, 0.05, 5.0], [0.0, 0.0, -1.0]);
        // A tight tolerance (a big pane / a close camera) misses...
        assert_eq!(hit_test(&near_miss, &s, 0.0001), None);
        // ...and a looser one (a distant gizmo) grabs.
        assert_eq!(hit_test(&near_miss, &s, 0.01), Some(Handle::AxisX));
    }

    #[test]
    fn a_plane_handle_wins_where_it_overlaps_the_shafts() {
        // The XY square sits out along +X and +Y, where it can overlap both
        // shafts' skins; it must take priority or it would be unusable.
        let s = state_at([0.0, 0.0, 0.0], 1.0);
        let r = ray([0.40, 0.40, 5.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit_test(&r, &s, 0.02), Some(Handle::PlaneXY));
    }

    #[test]
    fn an_axis_drag_slides_along_that_axis_only() {
        let s = state_at([0.0, 0.0, 0.0], 1.0);
        let r = ray([3.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let p = solve_drag_point(&r, &s, Handle::AxisX).unwrap();
        // The X arrow is only 1 unit long, but a drag runs PAST it: the axis is
        // an infinite line here, unlike in the hit test.
        assert!(
            (p.x - 3.0).abs() < 1e-3,
            "followed the cursor along X: {p:?}"
        );
        assert!(p.y.abs() < 1e-3 && p.z.abs() < 1e-3, "and nowhere else");
    }

    #[test]
    fn an_axis_drag_refuses_to_solve_looking_straight_down_the_axis() {
        // The solution is unstable here, and a naive solve makes the object jump
        // to infinity. Better to hold still.
        let s = state_at([0.0, 0.0, 0.0], 1.0);
        let down_x = ray([5.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        assert!(solve_drag_point(&down_x, &s, Handle::AxisX).is_none());
    }

    #[test]
    fn a_plane_drag_lands_on_the_gizmo_plane() {
        let s = state_at([0.0, 0.0, 0.0], 1.0);
        // The XY plane handle: its plane is z = 0 (through the ORIGIN, not
        // through the little square).
        let r = ray([2.0, 3.0, 5.0], [0.0, 0.0, -1.0]);
        let p = solve_drag_point(&r, &s, Handle::PlaneXY).unwrap();
        assert!((p.x - 2.0).abs() < 1e-3 && (p.y - 3.0).abs() < 1e-3);
        assert!(p.z.abs() < 1e-3, "on the plane: {p:?}");
    }

    #[test]
    fn a_root_drag_maps_world_straight_onto_translate() {
        let drag = Drag {
            handle: Handle::AxisX,
            target: target(Matrix4::identity()),
            start_local: [1.0, 0.0, 0.0],
            grab_world: Point3::new(0.0, 0.0, 0.0),
        };
        let next = drag_to_local(&drag, Point3::new(3.0, 0.0, 0.0)).unwrap();
        assert!((next[0] - 4.0).abs() < 1e-5, "start + delta: {next:?}");
    }

    #[test]
    fn a_subflow_drag_maps_the_world_delta_through_the_container() {
        // The geo is scaled 2x: dragging one world metre must move the SOP-level
        // translate by half a metre, or the object would run away from the
        // cursor at twice the speed.
        let parent = Matrix4::from_scale(2.0);
        let drag = Drag {
            handle: Handle::AxisX,
            target: target(parent),
            start_local: [0.0; 3],
            grab_world: Point3::new(0.0, 0.0, 0.0),
        };
        let next = drag_to_local(&drag, Point3::new(2.0, 0.0, 0.0)).unwrap();
        assert!(
            (next[0] - 1.0).abs() < 1e-5,
            "halved by the container: {next:?}"
        );

        // And a rotated container redirects the axis. Under a 90-degree Y
        // rotation the container maps its local +Z onto world +X, so dragging
        // the object one metre along world X is a local +Z move -- which is
        // exactly the correction that stops the object sliding sideways away
        // from the cursor inside a rotated geo.
        let rot = Matrix4::from_angle_y(cgmath::Deg(90.0));
        let drag = Drag {
            handle: Handle::AxisX,
            target: target(rot),
            start_local: [0.0; 3],
            grab_world: Point3::new(0.0, 0.0, 0.0),
        };
        let next = drag_to_local(&drag, Point3::new(1.0, 0.0, 0.0)).unwrap();
        assert!(next[0].abs() < 1e-5, "not local X: {next:?}");
        assert!((next[2] - 1.0).abs() < 1e-5, "local +Z: {next:?}");
    }

    #[test]
    fn the_object_moves_with_the_cursor_not_to_it() {
        // Grabbing the arrow 3 units out and dragging 1 more must move the object
        // by 1, not teleport its origin to 4.
        let drag = Drag {
            handle: Handle::AxisX,
            target: target(Matrix4::identity()),
            start_local: [0.0; 3],
            grab_world: Point3::new(3.0, 0.0, 0.0),
        };
        let next = drag_to_local(&drag, Point3::new(4.0, 0.0, 0.0)).unwrap();
        assert!((next[0] - 1.0).abs() < 1e-5, "moved by the delta: {next:?}");
    }

    #[test]
    fn only_the_move_tool_manipulates() {
        assert!(!ToolMode::Select.manipulates());
        assert!(ToolMode::Move.manipulates());
        // Phase 12: these draw nothing yet, which is why their buttons ship
        // disabled rather than dead.
        assert!(!ToolMode::Rotate.manipulates());
        assert!(!ToolMode::Scale.manipulates());
        assert_eq!(ToolMode::parse("move"), ToolMode::Move);
        assert_eq!(ToolMode::parse("nonsense"), ToolMode::Select);
    }
}
