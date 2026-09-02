//! The viewport gizmo: tool state, hit-testing and drag solving.
//!
//! All of it lives in Rust, on purpose. The drag loop runs at pointer rate, and
//! routing it through JavaScript would mean a boundary crossing per mouse move;
//! instead `pointer_move` solves the drag and streams the result straight into
//! the engine's preview lane, so a drag costs ZERO JS traffic. The only JS
//! surface is `set_tool` and `set_gizmo_settings`.
//!
//! The POLICY (which node a drag writes, whether one must be appended first)
//! lives in the engine (`Engine::gizmo_target`); this module does routing and
//! arithmetic only.
//!
//! A drag writes exactly the params its target names for the handle being
//! dragged, asked once. That is not incidental: preview, commit, no-op
//! rollback and Escape-cancel all resolve from the same place, so none of them
//! can forget a param another one touched. The translate-only version of this
//! module hardcoded `"translate"` at four separate call sites, which was one
//! careless copy-paste away from a cancel that left a rotation stranded in the
//! preview lane.
//!
//! It is "the params" rather than "the param" because a target sized by two
//! edge lengths writes both when its size is dragged uniformly. Everything
//! else writes one. The names themselves are never chosen here: they come from
//! the target's own [`solarxy_core::gizmo::TransformParams`], because a
//! parameter's role and its name are different facts and only the node knows
//! the second one.

use cgmath::{InnerSpace, Matrix, Matrix3, Matrix4, Point3, Rad, SquareMatrix, Vector3, Vector4};
use solarxy_core::gizmo::{ScaleParams, TransformParams};
use solarxy_core::raycast::{
    Ray, closest_point_ray_line, closest_points_ray_segment, intersect_obb, intersect_plane,
    intersect_quad, intersect_ring_band,
};
use solarxy_kernel::transform::{RotateOrder, decompose_rotation, rotation_matrix};
use solarxy_renderer::manipulator::{HIT_PX, Handle, ManipulatorState, ManipulatorTool};

/// Everything the drag solver knows about what it is dragging.
///
/// Deliberately a pose and nothing more. The engine's own target type also
/// carries *where* a result is addressed (which graph context, which node) and
/// whether a transform node still has to be appended; none of that is
/// arithmetic, and carrying it here would mean this crate — which sits under
/// both shells, one of which has no engine — depending on the engine to
/// multiply matrices. The shell that owns the document holds the addressing
/// and builds one of these to ask a question.
///
/// The pivot is absent because it is already inside `anchor`: rotation and
/// scale happen about `translate + pivot`, which is where the anchor is
/// placed, so the solver never needs it separately.
#[derive(Debug, Clone, Copy)]
pub struct GizmoPose {
    /// The target's current translate, previews included, so a handle tracks
    /// the object mid-drag.
    pub translate: [f32; 3],
    /// The current rotate, in **degrees**, matching what a param write stores.
    /// Deliberately not radians: handing back radians here would put a silent
    /// 57x error one careless assignment away.
    pub rotate: [f32; 3],
    /// The order the target composes its rotation in, so a rotate drag can
    /// decompose its result back into the angles this node actually means.
    pub rotate_order: RotateOrder,
    /// The current per-axis scale. Identity on a target sized by extent.
    pub scale: [f32; 3],
    /// The current uniform-scale factor (the centre handle's param).
    pub uniform_scale: f32,
    /// The current edge lengths, in metres, of a target sized by extent
    /// rather than by scale. Zero on every other kind.
    pub extent: [f32; 2],
    /// What the target declares and what it calls it. The solver asks this
    /// which param a handle writes rather than deciding, which is the whole
    /// reason a light can be dragged at all.
    pub params: TransformParams,
    /// World matrix placing the manipulator, pivot included.
    pub anchor: [[f32; 4]; 4],
    /// The target's own orthonormal orientation basis, for local-space handles.
    pub basis: [[f32; 3]; 3],
    /// The parent's orientation basis, for resolving a world-space drag into
    /// the parent space the params are expressed in.
    pub parent_basis: [[f32; 3]; 3],
    /// The full parent transform, for the same reason.
    pub parent: [[f32; 4]; 4],
}

/// The active viewport tool.
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

    /// The manipulator this tool draws, or `None` for Select (which draws none,
    /// grabs nothing, and leaves every click to the camera and the picker).
    #[must_use]
    pub fn manipulator_tool(self) -> Option<ManipulatorTool> {
        match self {
            ToolMode::Select => None,
            ToolMode::Move => Some(ManipulatorTool::Translate),
            ToolMode::Rotate => Some(ManipulatorTool::Rotate),
            ToolMode::Scale => Some(ManipulatorTool::Scale),
        }
    }

    /// Whether this tool draws and grabs a manipulator at all.
    #[must_use]
    pub fn manipulates(self) -> bool {
        self.manipulator_tool().is_some()
    }

    /// The label a drag's undo step carries, so the history reads "rotate"
    /// rather than "move" for a ring drag.
    #[must_use]
    pub fn undo_label(self) -> &'static str {
        match self {
            ToolMode::Select => "select",
            ToolMode::Move => "move",
            ToolMode::Rotate => "rotate",
            ToolMode::Scale => "scale",
        }
    }
}

/// Which frame the handles align to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    World,
    Local,
}

impl Orientation {
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "local" => Orientation::Local,
            _ => Orientation::World,
        }
    }
}

/// The drag ergonomics the user tunes in Preferences. Pushed into the host once
/// and cached, because the drag loop never crosses back into JS to ask.
#[derive(Debug, Clone, Copy)]
pub struct GizmoSettings {
    pub orientation: Orientation,
    /// World units the translate drag snaps to while Ctrl is held.
    pub snap_translate: f32,
    /// Degrees the rotate drag snaps to.
    pub snap_rotate: f32,
    /// The scale drag's snap increment.
    pub snap_scale: f32,
}

impl Default for GizmoSettings {
    fn default() -> Self {
        Self {
            orientation: Orientation::World,
            snap_translate: 0.5,
            snap_rotate: 15.0,
            snap_scale: 0.1,
        }
    }
}

/// The pointer modifiers a drag cares about. A bitflag rather than a bool so
/// shift-for-precision can land later without changing the wasm signature.
pub const MOD_SNAP: u8 = 1 << 0;

/// Which role a drag writes. Stored on the [`Drag`] so preview, commit,
/// rollback and cancel all resolve from the same place.
///
/// A role, not a name: what the target actually calls it is
/// [`DragParam::keys`]' answer, read off the target's own declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragParam {
    Translate,
    Rotate,
    Scale,
    UniformScale,
}

impl DragParam {
    /// The params this drag writes on this target, or `None` when the target
    /// declares nothing for this role, in which case the handle must neither
    /// draw nor grab.
    ///
    /// The single authority on which key a drag touches. Writing a key a
    /// descriptor never declared is not a benign miss: the parameter write
    /// refuses it, and the resolver would have debug-asserted on the way
    /// there, so guessing panics a debug build rather than doing nothing.
    #[must_use]
    pub fn keys(self, params: &TransformParams) -> Option<ParamKeys> {
        match self {
            DragParam::Translate => params.translate.map(ParamKeys::one),
            DragParam::Rotate => params.rotate.map(ParamKeys::one),
            DragParam::Scale => match params.scale {
                ScaleParams::Vec3 { scale, .. } => Some(ParamKeys::one(scale)),
                // An extent is written one edge at a time by its own handles,
                // which arrive with the per-axis mask; the three-lane scale
                // this role means does not exist on such a target.
                ScaleParams::None | ScaleParams::Extent2 { .. } => None,
            },
            DragParam::UniformScale => match params.scale {
                ScaleParams::Vec3 { uniform, .. } => Some(ParamKeys::one(uniform)),
                // The one drag that writes two: scaling a panel uniformly
                // moves both of its edge lengths together.
                ScaleParams::Extent2 { x, z } => Some(ParamKeys::two(x, z)),
                ScaleParams::None => None,
            },
        }
    }

    /// The handle's role. Every handle drives exactly one, though a role may
    /// resolve to more than one param on a given target.
    #[must_use]
    pub fn for_handle(handle: Handle) -> Self {
        match handle {
            Handle::AxisX
            | Handle::AxisY
            | Handle::AxisZ
            | Handle::PlaneXY
            | Handle::PlaneYZ
            | Handle::PlaneZX => DragParam::Translate,
            Handle::RingX | Handle::RingY | Handle::RingZ | Handle::RingView => DragParam::Rotate,
            Handle::ScaleX | Handle::ScaleY | Handle::ScaleZ => DragParam::Scale,
            Handle::ScaleUniform => DragParam::UniformScale,
        }
    }

    /// The target's current value for this param, previews included. This is how
    /// the commit reads back whatever the drag last streamed.
    #[must_use]
    pub fn read(self, t: &GizmoPose) -> DragValue {
        match self {
            DragParam::Translate => DragValue::Translate(t.translate),
            DragParam::Rotate => DragValue::Rotate(t.rotate),
            DragParam::Scale => DragValue::Scale(t.scale),
            // A panel's uniform size is its two edges, so that is what the
            // drag scales and what the commit reads back.
            DragParam::UniformScale => match t.params.scale {
                ScaleParams::Extent2 { .. } => DragValue::Extent(t.extent),
                ScaleParams::None | ScaleParams::Vec3 { .. } => {
                    DragValue::UniformScale(t.uniform_scale)
                }
            },
        }
    }
}

/// The one or two params a single drag writes.
///
/// Two only when a target's size is a pair of edge lengths and the drag scales
/// both. Deliberately not a `Vec`: the count is bounded by the design, and an
/// allocation per pointer move on the zero-JS hot path would be a poor trade
/// for generality nothing asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParamKeys([Option<&'static str>; 2]);

impl ParamKeys {
    #[must_use]
    fn one(key: &'static str) -> Self {
        Self([Some(key), None])
    }

    #[must_use]
    fn two(first: &'static str, second: &'static str) -> Self {
        Self([Some(first), Some(second)])
    }

    /// The keys, in the order the matching values come out of
    /// [`DragValue::values`].
    pub fn iter(self) -> impl Iterator<Item = &'static str> {
        self.0.into_iter().flatten()
    }
}

/// A value a drag streams into the preview lane and finally commits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragValue {
    Translate([f32; 3]),
    /// Degrees, matching what a `SetParam` writes.
    Rotate([f32; 3]),
    Scale([f32; 3]),
    UniformScale(f32),
    /// Two edge lengths in metres, for a target whose size is an extent
    /// rather than a scale. Absolute lengths rather than a factor, because
    /// that is what the params store.
    Extent([f32; 2]),
}

impl DragValue {
    #[must_use]
    pub fn param(self) -> DragParam {
        match self {
            DragValue::Translate(_) => DragParam::Translate,
            DragValue::Rotate(_) => DragParam::Rotate,
            DragValue::Scale(_) => DragParam::Scale,
            DragValue::UniformScale(_) | DragValue::Extent(_) => DragParam::UniformScale,
        }
    }

    /// The values this drag writes, positionally matching the keys
    /// [`DragParam::keys`] hands back for the same target.
    ///
    /// Every value is either three floats or one, which is why this returns
    /// the pair rather than a typed union: the caller lowers each into the
    /// parameter source its own crate owns. A vec3 lane carries `None` for
    /// its scalar and vice versa, so a caller cannot read the wrong one.
    #[must_use]
    pub fn values(self) -> [Option<DragScalarOrVec3>; 2] {
        match self {
            DragValue::Translate(v) | DragValue::Rotate(v) | DragValue::Scale(v) => {
                [Some(DragScalarOrVec3::Vec3(v)), None]
            }
            DragValue::UniformScale(f) => [Some(DragScalarOrVec3::Scalar(f)), None],
            DragValue::Extent([x, z]) => [
                Some(DragScalarOrVec3::Scalar(x)),
                Some(DragScalarOrVec3::Scalar(z)),
            ],
        }
    }

    /// Whether the drag actually moved anything. A click on a handle that never
    /// moved is not an edit: committing it would push an undo step that visibly
    /// does nothing and, on the append path, would leave a transform node behind
    /// for a mere click.
    #[must_use]
    pub fn differs_from(self, other: Self) -> bool {
        const EPS: f32 = 1e-6;
        match (self, other) {
            (DragValue::Translate(a), DragValue::Translate(b))
            | (DragValue::Rotate(a), DragValue::Rotate(b))
            | (DragValue::Scale(a), DragValue::Scale(b)) => {
                a.iter().zip(b).any(|(x, y)| (x - y).abs() > EPS)
            }
            (DragValue::Extent(a), DragValue::Extent(b)) => {
                a.iter().zip(b).any(|(x, y)| (x - y).abs() > EPS)
            }
            (DragValue::UniformScale(a), DragValue::UniformScale(b)) => (a - b).abs() > EPS,
            // Different params entirely: that IS a difference.
            _ => true,
        }
    }

    /// A short human-readable delta for the viewport readout, given where the
    /// drag started. What makes a professional gizmo feel precise is being able
    /// to see the number without opening a panel.
    #[must_use]
    pub fn readout(self, start: Self) -> Option<String> {
        const AXES: [&str; 3] = ["X", "Y", "Z"];
        match (self, start) {
            (DragValue::Translate(now), DragValue::Translate(was)) => {
                Some(lane_readout(now, was, |d| format!("{d:+.3} m")))
            }
            (DragValue::Rotate(now), DragValue::Rotate(was)) => {
                Some(lane_readout(now, was, |d| format!("{d:+.1}\u{b0}")))
            }
            (DragValue::Scale(now), DragValue::Scale(was)) => {
                // A ratio, not a difference: that is what scale means.
                let parts: Vec<String> = (0..3)
                    .filter(|&i| (now[i] - was[i]).abs() > 1e-5 && was[i].abs() > 1e-9)
                    .map(|i| format!("{} {:.3}x", AXES[i], now[i] / was[i]))
                    .collect();
                Some(if parts.is_empty() {
                    "1.000x".to_string()
                } else {
                    parts.join("  ")
                })
            }
            (DragValue::UniformScale(now), DragValue::UniformScale(was)) => {
                let ratio = if was.abs() > 1e-9 { now / was } else { 1.0 };
                Some(format!("{ratio:.3}x"))
            }
            // Metres, not a ratio: an extent is a length, and a panel is
            // authored by the size you want it to be.
            (DragValue::Extent(now), DragValue::Extent(_)) => {
                Some(format!("{:.3} x {:.3} m", now[0], now[1]))
            }
            _ => None,
        }
    }
}

/// One value a drag writes: a vec3 lane or a single float. Named rather than a
/// bare tuple because the two are not interchangeable at the parameter write,
/// and mixing them up would store a length in a rotation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragScalarOrVec3 {
    Vec3([f32; 3]),
    Scalar(f32),
}

/// Formats only the lanes that actually moved, so an X drag reads "X +1.250 m"
/// rather than burying it in two zeroes.
fn lane_readout(now: [f32; 3], was: [f32; 3], fmt: impl Fn(f32) -> String) -> String {
    const AXES: [&str; 3] = ["X", "Y", "Z"];
    let parts: Vec<String> = (0..3)
        .filter(|&i| (now[i] - was[i]).abs() > 1e-5)
        .map(|i| format!("{} {}", AXES[i], fmt(now[i] - was[i])))
        .collect();
    if parts.is_empty() {
        fmt(0.0)
    } else {
        parts.join("  ")
    }
}

/// How a live drag tracks the pointer, per tool.
#[derive(Debug, Clone, Copy)]
pub enum DragGrab {
    /// The world point under the cursor when the drag began, so the object moves
    /// WITH the cursor instead of snapping its origin to it.
    Translate { grab_world: Point3<f32> },
    Rotate {
        /// The world axis being turned about.
        axis: Vector3<f32>,
        /// The grab direction in the ring plane, normalized.
        start_vec: Vector3<f32>,
        /// The last raw angle in `(-pi, pi]`, and how many full turns have been
        /// crossed. Together these let a drag sweep past 180 degrees and keep
        /// going, instead of snapping back the other way.
        last_raw: f32,
        turns: i32,
    },
    Scale {
        /// The distance from the pivot at grab time; the drag is its ratio.
        start_distance: f32,
        /// The axis to measure along, or `None` for the uniform handle (which
        /// measures radially on the view plane).
        axis: Option<Vector3<f32>>,
    },
}

/// A drag in flight.
#[derive(Debug, Clone, Copy)]
pub struct Drag {
    pub handle: Handle,
    /// The target as resolved at drag START, with `node` already replaced by the
    /// real target on the append path (the engine minted it via
    /// `EnsureTransformTarget`).
    pub target: GizmoPose,
    /// Which param this drag writes, asked once and honoured everywhere.
    pub param: DragParam,
    /// That param's value when the drag began.
    pub start: DragValue,
    pub grab: DragGrab,
}

/// Tool + settings + hover + drag. One per app.
#[derive(Debug, Default)]
pub struct GizmoState {
    pub tool: ToolMode,
    pub settings: GizmoSettings,
    pub hovered: Option<Handle>,
    pub drag: Option<Drag>,
}

impl GizmoState {
    /// The manipulator to draw (and hit-test) this frame, or `None`.
    ///
    /// `scale` and `view_dir` are per-pane, so the renderer overwrites them in
    /// `write_manipulator`; the hit test passes the real ones itself.
    #[must_use]
    pub fn manipulator(
        &self,
        target: &GizmoPose,
        view_dir: Vector3<f32>,
        scale: f32,
    ) -> Option<ManipulatorState> {
        let tool = self.tool.manipulator_tool()?;
        Some(ManipulatorState {
            anchor: Matrix4::from(target.anchor),
            basis: self.handle_basis(target, tool),
            view_dir,
            tool,
            hovered: self.hovered,
            active: self.drag.map(|d| d.handle),
            scale,
        })
    }

    /// Which frame the handles align to.
    ///
    /// Scale is ALWAYS local, whatever the orientation setting says, and that is
    /// not a shortcut: a scale along a world axis is simply not representable in
    /// the node's params. Scaling a rotated object along world X would shear it,
    /// and there is no shear param (nor should there be). Maya makes the same
    /// call for the same reason. When the object is unrotated the two frames
    /// coincide, so the distinction is invisible in the common case.
    fn handle_basis(&self, target: &GizmoPose, tool: ManipulatorTool) -> Matrix3<f32> {
        let local = Matrix3::from(target.basis);
        match tool {
            ManipulatorTool::Scale => local,
            _ => match self.settings.orientation {
                Orientation::World => Matrix3::identity(),
                Orientation::Local => local,
            },
        }
    }
}

/// Which handle a ray grabs, if any.
///
/// `world_per_px` converts the pixel tolerance into world units through the SAME
/// helper the vertex generator uses, so the grab zone is exactly the drawn
/// handle. Handles are tested in `Handle::for_tool` order, and the nearest hit
/// wins, so the small square between two axes stays clickable where it overlaps
/// their shafts.
#[must_use]
pub fn hit_test(ray: &Ray, state: &ManipulatorState, world_per_px: f32) -> Option<Handle> {
    let tolerance = HIT_PX * world_per_px;
    let mut best: Option<(f32, Handle)> = None;

    for &handle in Handle::for_tool(state.tool) {
        let t = hit_handle(ray, state, handle, tolerance);
        if let Some(t) = t
            && best.is_none_or(|(best_t, _)| t < best_t)
        {
            best = Some((t, handle));
        }
    }
    best.map(|(_, handle)| handle)
}

fn hit_handle(ray: &Ray, state: &ManipulatorState, handle: Handle, tolerance: f32) -> Option<f32> {
    if let Some((a, b)) = handle.plane_axes() {
        let (corner, u, v) = state.plane_quad(a, b);
        return intersect_quad(ray, corner, u, v);
    }
    if let Some(axis) = handle.axis() {
        let (start, end) = state.axis_segment(axis);
        let (t_ray, _, distance) = closest_points_ray_segment(ray, start, end);
        // A capsule: the shaft plus a pixel-sized skin around it.
        return (distance <= tolerance).then_some(t_ray);
    }
    if handle.ring_axis().is_some() || handle == Handle::RingView {
        let (center, normal, radius) = state.ring(handle)?;
        return intersect_ring_band(ray, center, normal, radius, tolerance);
    }
    if handle.scale_axis().is_some() || handle == Handle::ScaleUniform {
        let (center, axes, half) = state.scale_cube(handle)?;
        // The cube, fattened by the pixel tolerance so a small handle stays as
        // easy to click as a big one.
        let grown = [
            half[0] + tolerance,
            half[1] + tolerance,
            half[2] + tolerance,
        ];
        let cube = intersect_obb(ray, center, axes, grown);
        // The shaft leading to it is grabbable too, the way Maya's is: the whole
        // arm scales that axis, not just the tip.
        let shaft = handle.scale_axis().and_then(|axis| {
            let (start, end) = state.axis_segment(axis);
            let (t_ray, _, distance) = closest_points_ray_segment(ray, start, end);
            (distance <= tolerance).then_some(t_ray)
        });
        return match (cube, shaft) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (hit, None) | (None, hit) => hit,
        };
    }
    None
}

/// Where the pointer ray lands on a TRANSLATE handle's constraint, in world
/// space.
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
) -> Option<Point3<f32>> {
    if let Some(axis) = handle.axis() {
        let (start, end) = state.axis_segment(axis);
        let dir = (end - start).normalize();
        // The axis as an INFINITE line: a drag must be able to run past the end
        // of the drawn arrow, so this deliberately does not clamp the way the
        // hit test does. Returns None when the ray is near-parallel to the axis,
        // where the solution is unstable and the object would jump.
        return closest_point_ray_line(ray, state.origin(), dir);
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

/// The translate drag's new value: where the object's translate lands, in the
/// target's own space.
///
/// The world delta is mapped through the inverse of `parent`: identity at root
/// (the geo's translate IS world), and the geo's world matrix inside a subflow,
/// where a rotated or scaled container means a world drag of one metre is NOT
/// one metre of local translate.
#[must_use]
pub fn solve_translate(
    ray: &Ray,
    state: &ManipulatorState,
    drag: &Drag,
    settings: &GizmoSettings,
    mods: u8,
) -> Option<DragValue> {
    let DragGrab::Translate { grab_world } = drag.grab else {
        return None;
    };
    let DragValue::Translate(start) = drag.start else {
        return None;
    };

    let world_now = solve_drag_point(ray, state, drag.handle)?;
    let world_delta = world_now - grab_world;

    let parent = Matrix4::from(drag.target.parent);
    let inv = parent.invert()?;
    // A direction, not a point: w = 0, so the parent's translation drops out and
    // only its rotation and scale apply.
    let local = inv * Vector4::new(world_delta.x, world_delta.y, world_delta.z, 0.0);

    let mut next = [start[0] + local.x, start[1] + local.y, start[2] + local.z];
    if mods & MOD_SNAP != 0 {
        snap_changed_lanes(&mut next, start, settings.snap_translate);
    }
    Some(DragValue::Translate(next))
}

/// The rotate drag's new value, in degrees.
///
/// Composes the swept rotation onto the orientation captured at drag START and
/// decomposes ONCE, rather than accumulating onto the previous frame's euler
/// angles. That is what keeps a drag free of both float drift and gimbal churn:
/// the euler representation is only ever an output.
///
/// Returns the value and the updated wrap state, which the caller stores back on
/// the drag.
#[must_use]
pub fn solve_rotate(
    ray: &Ray,
    state: &ManipulatorState,
    drag: &Drag,
    settings: &GizmoSettings,
    mods: u8,
) -> Option<(DragValue, f32, i32)> {
    let DragGrab::Rotate {
        axis,
        start_vec,
        last_raw,
        turns,
    } = drag.grab
    else {
        return None;
    };
    let DragValue::Rotate(start_deg) = drag.start else {
        return None;
    };

    let now_vec = ring_vector(ray, state.origin(), axis)?;

    // Signed angle from the grab direction to the current one, about the axis.
    let raw = Rad(axis
        .dot(start_vec.cross(now_vec))
        .atan2(start_vec.dot(now_vec)));

    // Cross the +/- pi seam and keep counting, so a full 360-degree sweep in one
    // drag keeps turning instead of snapping back the other way.
    let mut turns = turns;
    let delta = raw.0 - last_raw;
    if delta > std::f32::consts::PI {
        turns -= 1;
    } else if delta < -std::f32::consts::PI {
        turns += 1;
    }
    let total = raw.0 + (turns as f32) * std::f32::consts::TAU;

    let mut degrees = total.to_degrees();
    if mods & MOD_SNAP != 0 && settings.snap_rotate > 0.0 {
        degrees = snap_to(degrees, settings.snap_rotate);
    }

    // The delta is a WORLD-axis turn, but the param lives in the parent's frame,
    // so conjugate it into that frame before composing.
    let delta_world = Matrix3::from_axis_angle(axis, Rad(degrees.to_radians()));
    let parent = Matrix3::from(drag.target.parent_basis);
    let delta_local = parent.transpose() * delta_world * parent;

    let order = drag.target.rotate_order;
    let start_basis = rotation_matrix(start_deg.map(f32::to_radians), order);
    let next_basis = delta_local * start_basis;

    let next = decompose_rotation(next_basis, order).map(f32::to_degrees);
    Some((DragValue::Rotate(next), raw.0, turns))
}

/// The scale drag's new value: the ratio of the pointer's distance from the
/// pivot now to what it was at grab time.
#[must_use]
pub fn solve_scale(
    ray: &Ray,
    state: &ManipulatorState,
    drag: &Drag,
    settings: &GizmoSettings,
    mods: u8,
) -> Option<DragValue> {
    let DragGrab::Scale {
        start_distance,
        axis,
    } = drag.grab
    else {
        return None;
    };
    // A grab right on the pivot has no lever arm; any ratio would be infinite.
    if start_distance.abs() < 1e-5 {
        return None;
    }

    let distance = scale_distance(ray, state, axis)?;
    // Through the pivot and out the other side would mirror the object. The
    // params clamp at their hard minimum anyway, so stop at zero rather than
    // hand the resolver a negative to swallow.
    let factor = (distance / start_distance).max(0.0);

    match drag.start {
        DragValue::Scale(start) => {
            let index = drag.handle.scale_axis()?;
            let mut next = start;
            next[index] = start[index] * factor;
            if mods & MOD_SNAP != 0 {
                snap_changed_lanes(&mut next, start, settings.snap_scale);
            }
            Some(DragValue::Scale(next))
        }
        DragValue::UniformScale(start) => {
            let mut next = start * factor;
            if mods & MOD_SNAP != 0 && settings.snap_scale > 0.0 {
                next = snap_to(next, settings.snap_scale);
            }
            Some(DragValue::UniformScale(next))
        }
        // Both edges together, in metres. Snapping is on the LENGTHS rather
        // than on the factor, because that is the number a person is trying
        // to land on when they size a panel.
        DragValue::Extent(start) => {
            let mut next = [start[0] * factor, start[1] * factor];
            if mods & MOD_SNAP != 0 && settings.snap_translate > 0.0 {
                next = [
                    snap_to(next[0], settings.snap_translate),
                    snap_to(next[1], settings.snap_translate),
                ];
            }
            Some(DragValue::Extent(next))
        }
        _ => None,
    }
}

/// How far along its measuring line the pointer currently sits.
///
/// An axis cube measures along that axis; the uniform cube measures radially on
/// the plane facing the camera, so dragging out in any direction grows it.
fn scale_distance(ray: &Ray, state: &ManipulatorState, axis: Option<Vector3<f32>>) -> Option<f32> {
    let origin = state.origin();
    let Some(axis) = axis else {
        // The uniform handle: radial distance on the plane facing the camera, so
        // dragging out in ANY direction grows the object.
        let t = intersect_plane(ray, origin, state.view_dir)?;
        return Some((ray.origin + ray.direction * t - origin).magnitude());
    };
    // Looking straight down the axis returns None: unstable.
    let point = closest_point_ray_line(ray, origin, axis)?;
    // Signed, so dragging back through the pivot shrinks toward zero rather than
    // growing again on the far side.
    Some((point - origin).dot(axis))
}

/// The grab direction in a ring's plane: from the ring centre out to where the
/// pointer ray crosses it, normalized.
fn ring_vector(ray: &Ray, center: Point3<f32>, axis: Vector3<f32>) -> Option<Vector3<f32>> {
    // Edge-on to the ring, the intersection slides wildly for a sub-pixel move.
    if axis.dot(ray.direction).abs() < 1e-3 {
        return None;
    }
    let t = intersect_plane(ray, center, axis)?;
    let v = ray.origin + ray.direction * t - center;
    // Project out any component along the axis (float slop) and reject a grab
    // right at the centre, which has no direction.
    let planar = v - axis * v.dot(axis);
    (planar.magnitude() > 1e-5).then(|| planar.normalize())
}

/// Rounds to the nearest multiple. Absolute snapping (the VALUE lands on the
/// grid), Blender-style, not delta-relative: what the user wants is a translate
/// of exactly 1.5, not a translate that moved by exactly 1.5.
fn snap_to(value: f32, step: f32) -> f32 {
    if step <= 0.0 {
        return value;
    }
    (value / step).round() * step
}

/// Snaps only the lanes the drag actually moved.
///
/// Snapping every lane would drag untouched ones onto the grid too: an object
/// sitting at y = 0.3 would jump to y = 0.5 the moment you nudged it along X,
/// which is not what "snap X" means.
fn snap_changed_lanes(next: &mut [f32; 3], start: [f32; 3], step: f32) {
    if step <= 0.0 {
        return;
    }
    for i in 0..3 {
        if (next[i] - start[i]).abs() > 1e-6 {
            next[i] = snap_to(next[i], step);
        }
    }
}

/// Opens a drag on a handle: captures everything the solve will need so that no
/// later frame has to re-resolve the target (which would move the gizmo's own
/// origin under the maths and make the object accelerate away from the cursor).
#[must_use]
pub fn begin_drag(
    ray: &Ray,
    state: &ManipulatorState,
    target: GizmoPose,
    handle: Handle,
) -> Option<Drag> {
    let param = DragParam::for_handle(handle);
    // A handle whose role this target does not declare grabs nothing, so the
    // press falls through to the camera rather than opening a drag that would
    // have nowhere to write. This is what keeps an armed tool honest on a
    // target that cannot use it.
    param.keys(&target.params)?;
    let start = param.read(&target);

    let grab = if handle.axis().is_some() || handle.plane_axes().is_some() {
        DragGrab::Translate {
            grab_world: solve_drag_point(ray, state, handle)?,
        }
    } else if handle.ring_axis().is_some() || handle == Handle::RingView {
        let (center, axis, _) = state.ring(handle)?;
        let start_vec = ring_vector(ray, center, axis)?;
        DragGrab::Rotate {
            axis,
            start_vec,
            last_raw: 0.0,
            turns: 0,
        }
    } else if handle.scale_axis().is_some() || handle == Handle::ScaleUniform {
        let axis = handle.scale_axis().map(|i| {
            let (start, end) = state.axis_segment(i);
            (end - start).normalize()
        });
        DragGrab::Scale {
            start_distance: scale_distance(ray, state, axis)?,
            axis,
        }
    } else {
        return None;
    };

    Some(Drag {
        handle,
        target,
        param,
        start,
        grab,
    })
}

/// Solves one pointer move against a live drag, returning the value to preview.
///
/// The rotate case also hands back its updated wrap state, which the caller
/// stores on the drag.
#[must_use]
pub fn solve_drag(
    ray: &Ray,
    state: &ManipulatorState,
    drag: &Drag,
    settings: &GizmoSettings,
    mods: u8,
) -> Option<(DragValue, Option<(f32, i32)>)> {
    match drag.grab {
        DragGrab::Translate { .. } => {
            solve_translate(ray, state, drag, settings, mods).map(|v| (v, None))
        }
        DragGrab::Rotate { .. } => solve_rotate(ray, state, drag, settings, mods)
            .map(|(v, last_raw, turns)| (v, Some((last_raw, turns)))),
        DragGrab::Scale { .. } => solve_scale(ray, state, drag, settings, mods).map(|v| (v, None)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_for(tool: ManipulatorTool, basis: Matrix3<f32>, scale: f32) -> ManipulatorState {
        ManipulatorState {
            anchor: Matrix4::identity(),
            basis,
            view_dir: -Vector3::unit_z(),
            tool,
            hovered: None,
            active: None,
            scale,
        }
    }

    fn state(scale: f32) -> ManipulatorState {
        state_for(ManipulatorTool::Translate, Matrix3::identity(), scale)
    }

    fn ray(origin: [f32; 3], dir: [f32; 3]) -> Ray {
        Ray {
            origin: Point3::from(origin),
            direction: Vector3::from(dir).normalize(),
        }
    }

    /// The full transform vocabulary, which is what a `geo` and a `transform`
    /// declare. Named for what it is so the light-shaped cases below read as
    /// the deliberate contrast they are.
    const FULL_TRS: TransformParams = TransformParams {
        translate: Some("translate"),
        rotate: Some("rotate"),
        rotate_order: Some("rotate_order"),
        scale: ScaleParams::Vec3 {
            scale: "scale",
            uniform: "uniform_scale",
        },
        pivot: None,
        aim: None,
    };

    fn target(parent: Matrix4<f32>) -> GizmoPose {
        target_with(parent, FULL_TRS)
    }

    fn target_with(parent: Matrix4<f32>, params: TransformParams) -> GizmoPose {
        GizmoPose {
            translate: [0.0; 3],
            rotate: [0.0; 3],
            rotate_order: RotateOrder::Xyz,
            scale: [1.0; 3],
            uniform_scale: 1.0,
            extent: [0.0; 2],
            params,
            anchor: Matrix4::identity().into(),
            basis: mat3(Matrix3::identity()),
            parent_basis: mat3(Matrix3::identity()),
            parent: parent.into(),
        }
    }

    fn mat3(m: Matrix3<f32>) -> [[f32; 3]; 3] {
        [m.x.into(), m.y.into(), m.z.into()]
    }

    // ---- translate (behaviour, preserved) ----

    #[test]
    fn the_x_arrow_is_grabbable_and_the_empty_space_beside_it_is_not() {
        let s = state(1.0);
        let on = ray([0.5, 0.0, 5.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit_test(&on, &s, 0.001), Some(Handle::AxisX));

        let off = ray([0.5, -0.6, 5.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit_test(&off, &s, 0.001), None);
    }

    #[test]
    fn a_plane_handle_wins_where_it_overlaps_the_shafts() {
        let s = state(1.0);
        let r = ray([0.40, 0.40, 5.0], [0.0, 0.0, -1.0]);
        assert_eq!(hit_test(&r, &s, 0.02), Some(Handle::PlaneXY));
    }

    #[test]
    fn an_axis_drag_slides_along_that_axis_only() {
        let s = state(1.0);
        let r = ray([3.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let p = solve_drag_point(&r, &s, Handle::AxisX).unwrap();
        assert!(
            (p.x - 3.0).abs() < 1e-3,
            "followed the cursor along X: {p:?}"
        );
        assert!(p.y.abs() < 1e-3 && p.z.abs() < 1e-3, "and nowhere else");
    }

    #[test]
    fn an_axis_drag_refuses_to_solve_looking_straight_down_the_axis() {
        let s = state(1.0);
        let down_x = ray([5.0, 0.0, 0.0], [-1.0, 0.0, 0.0]);
        assert!(solve_drag_point(&down_x, &s, Handle::AxisX).is_none());
    }

    #[test]
    fn the_object_moves_with_the_cursor_not_to_it() {
        // Grabbing the arrow 3 units out and dragging 1 more must move the object
        // by 1, not teleport its origin to 4.
        let s = state(1.0);
        let grab = ray([3.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, target(Matrix4::identity()), Handle::AxisX).unwrap();

        let now = ray([4.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (value, _) = solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap();
        let DragValue::Translate(v) = value else {
            panic!("a translate handle must write translate")
        };
        assert!((v[0] - 1.0).abs() < 1e-4, "moved by the delta: {v:?}");
    }

    #[test]
    fn a_subflow_drag_maps_the_world_delta_through_the_container() {
        // The geo is scaled 2x: dragging one world metre must move the SOP-level
        // translate by half a metre, or the object would run away from the cursor
        // at twice the speed.
        let s = state(1.0);
        let t = target(Matrix4::from_scale(2.0));
        let grab = ray([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, t, Handle::AxisX).unwrap();

        let now = ray([2.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (DragValue::Translate(v), _) =
            solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap()
        else {
            panic!()
        };
        assert!((v[0] - 1.0).abs() < 1e-4, "halved by the container: {v:?}");
    }

    // ---- rotate ----

    #[test]
    fn a_ring_drag_turns_about_its_axis_by_the_swept_angle() {
        // The Z ring lies in the XY plane; grab it at +X and sweep to +Y, which
        // is a clean +90 degrees about Z.
        let s = state_for(ManipulatorTool::Rotate, Matrix3::identity(), 1.0);
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, target(Matrix4::identity()), Handle::RingZ).unwrap();

        let now = ray([0.0, 1.0, 5.0], [0.0, 0.0, -1.0]);
        let (DragValue::Rotate(deg), _) =
            solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap()
        else {
            panic!("a ring must write rotate")
        };
        assert!(
            (deg[2] - 90.0).abs() < 0.5,
            "90 degrees about Z, got {deg:?}"
        );
        assert!(deg[0].abs() < 0.5 && deg[1].abs() < 0.5, "and nothing else");
    }

    /// The wrap seam: a sweep past 180 degrees must keep going, not snap back to
    /// the negative side. This is the bug that makes a naive `atan2` gizmo
    /// unusable for anything but small turns.
    #[test]
    fn a_rotate_drag_sweeps_past_180_degrees_without_flipping() {
        let s = state_for(ManipulatorTool::Rotate, Matrix3::identity(), 1.0);
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let mut drag = begin_drag(&grab, &s, target(Matrix4::identity()), Handle::RingZ).unwrap();

        // Walk the pointer around in steps, feeding the wrap state back the way
        // the host does, and confirm the angle keeps climbing past 180.
        let settings = GizmoSettings::default();
        let mut last_deg = 0.0_f32;
        for step in 1..=8 {
            let theta = (step as f32) * 40.0_f32.to_radians(); // 40 deg per step, to 320
            let r = ray(
                [theta.cos() * 1.0, theta.sin() * 1.0, 5.0],
                [0.0, 0.0, -1.0],
            );
            let (value, wrap) = solve_drag(&r, &s, &drag, &settings, 0).unwrap();
            if let Some((last_raw, turns)) = wrap
                && let DragGrab::Rotate {
                    axis, start_vec, ..
                } = drag.grab
            {
                drag.grab = DragGrab::Rotate {
                    axis,
                    start_vec,
                    last_raw,
                    turns,
                };
            }
            let DragValue::Rotate(deg) = value else {
                panic!()
            };
            // The euler output wraps at +/-180 by construction, so compare the
            // ORIENTATION, not the raw number: rebuild the matrix and check it
            // matches the angle we actually swept.
            let want = (step as f32) * 40.0;
            let got = rotation_matrix(deg.map(f32::to_radians), RotateOrder::Xyz);
            let expect = Matrix3::from_angle_z(cgmath::Deg(want));
            for c in 0..3 {
                for r_ in 0..3 {
                    assert!(
                        (got[c][r_] - expect[c][r_]).abs() < 1e-3,
                        "step {step} ({want} deg): orientation drifted at [{c}][{r_}]"
                    );
                }
            }
            last_deg = want;
        }
        assert!(last_deg > 180.0, "the sweep really did pass the seam");
    }

    #[test]
    fn ctrl_snaps_a_rotation_to_the_configured_step() {
        let s = state_for(ManipulatorTool::Rotate, Matrix3::identity(), 1.0);
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, target(Matrix4::identity()), Handle::RingZ).unwrap();

        // Sweep to ~40 degrees; with a 15-degree snap that must land on 45.
        let theta = 40.0_f32.to_radians();
        let now = ray([theta.cos(), theta.sin(), 5.0], [0.0, 0.0, -1.0]);
        let settings = GizmoSettings::default();
        let (DragValue::Rotate(deg), _) = solve_drag(&now, &s, &drag, &settings, MOD_SNAP).unwrap()
        else {
            panic!()
        };
        assert!((deg[2] - 45.0).abs() < 0.5, "snapped to 45, got {deg:?}");
    }

    /// A world-axis turn on a node inside a ROTATED container is not the same
    /// angle in the node's own params: it has to be conjugated into the parent's
    /// frame first, or the object would spin about the wrong axis.
    #[test]
    fn a_rotate_inside_a_rotated_container_is_expressed_in_the_parents_frame() {
        let s = state_for(ManipulatorTool::Rotate, Matrix3::identity(), 1.0);
        // Container turned 90 degrees about X: its local +Y points along world +Z.
        let parent_basis = Matrix3::from_angle_x(cgmath::Deg(90.0));
        let mut t = target(Matrix4::identity());
        t.parent_basis = mat3(parent_basis);

        // Turn 90 degrees about the WORLD Z ring.
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, t, Handle::RingZ).unwrap();
        let now = ray([0.0, 1.0, 5.0], [0.0, 0.0, -1.0]);
        let (DragValue::Rotate(deg), _) =
            solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap()
        else {
            panic!()
        };

        // Composing the node's new params inside the container must reproduce a
        // world-Z turn. That is the invariant; the raw euler numbers are just
        // one encoding of it.
        let node = rotation_matrix(deg.map(f32::to_radians), RotateOrder::Xyz);
        let world = parent_basis * node * parent_basis.transpose();
        let expect = Matrix3::from_angle_z(cgmath::Deg(90.0));
        for c in 0..3 {
            for r in 0..3 {
                assert!(
                    (world[c][r] - expect[c][r]).abs() < 1e-3,
                    "the world turn must survive the frame change at [{c}][{r}]"
                );
            }
        }
    }

    // ---- scale ----

    #[test]
    fn dragging_a_scale_cube_outward_grows_that_lane_only() {
        let s = state_for(ManipulatorTool::Scale, Matrix3::identity(), 1.0);
        // Grab the X cube at its tip (x = 1) and drag out to x = 2: a 2x scale.
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, target(Matrix4::identity()), Handle::ScaleX).unwrap();

        let now = ray([2.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (DragValue::Scale(v), _) =
            solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap()
        else {
            panic!("an axis cube must write scale")
        };
        assert!((v[0] - 2.0).abs() < 1e-3, "X doubled: {v:?}");
        assert!(
            (v[1] - 1.0).abs() < 1e-6 && (v[2] - 1.0).abs() < 1e-6,
            "Y and Z untouched"
        );
    }

    #[test]
    fn the_centre_cube_writes_uniform_scale_not_the_three_lanes() {
        let s = state_for(ManipulatorTool::Scale, Matrix3::identity(), 1.0);
        // The uniform handle measures radially on the view plane (z = 0 here).
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag =
            begin_drag(&grab, &s, target(Matrix4::identity()), Handle::ScaleUniform).unwrap();

        let now = ray([3.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (value, _) = solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap();
        let DragValue::UniformScale(f) = value else {
            panic!("the centre cube must write uniform_scale, got {value:?}")
        };
        assert!((f - 3.0).abs() < 1e-3, "3x uniform, got {f}");
    }

    #[test]
    fn a_scale_drag_never_goes_negative_through_the_pivot() {
        // Dragging back past the pivot would mirror the object; the params clamp
        // at their hard minimum, so stop at zero instead of handing the resolver
        // a negative to swallow.
        let s = state_for(ManipulatorTool::Scale, Matrix3::identity(), 1.0);
        let grab = ray([1.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let drag = begin_drag(&grab, &s, target(Matrix4::identity()), Handle::ScaleX).unwrap();

        let now = ray([-4.0, 0.0, 5.0], [0.0, 0.0, -1.0]);
        let (DragValue::Scale(v), _) =
            solve_drag(&now, &s, &drag, &GizmoSettings::default(), 0).unwrap()
        else {
            panic!()
        };
        assert!(v[0] >= 0.0, "never negative, got {v:?}");
    }

    // ---- the declared-param contract ----

    /// The structural guarantee: every handle names one role, that role
    /// resolves to the names the TARGET declares, and the value a drag
    /// produces writes that same role. This is what makes preview, commit,
    /// rollback and cancel agree by construction rather than by four copies of
    /// the same string literal.
    #[test]
    fn every_handle_writes_the_params_its_target_declares() {
        let cases = [
            (Handle::AxisX, DragParam::Translate, "translate"),
            (Handle::PlaneYZ, DragParam::Translate, "translate"),
            (Handle::RingX, DragParam::Rotate, "rotate"),
            (Handle::RingView, DragParam::Rotate, "rotate"),
            (Handle::ScaleZ, DragParam::Scale, "scale"),
            (
                Handle::ScaleUniform,
                DragParam::UniformScale,
                "uniform_scale",
            ),
        ];
        let t = target(Matrix4::identity());
        for (handle, param, key) in cases {
            assert_eq!(DragParam::for_handle(handle), param, "{handle:?}");
            let keys: Vec<&str> = param.keys(&t.params).expect("declared").iter().collect();
            assert_eq!(keys, vec![key]);
            // And the value the target reports for that param round-trips back to
            // the same param, so a commit cannot write the wrong key.
            assert_eq!(param.read(&t).param(), param);
        }
    }

    /// The same handles against a target that names its position differently
    /// and declares no rotation or scale at all. Nothing about the handle
    /// changed; the answer did, which is the whole point of asking the target.
    #[test]
    fn a_position_only_target_writes_its_own_name_and_refuses_the_rest() {
        let point_light = TransformParams {
            translate: Some("position"),
            ..TransformParams::default()
        };
        let t = target_with(Matrix4::identity(), point_light);

        let keys: Vec<&str> = DragParam::Translate
            .keys(&t.params)
            .expect("a position is declared")
            .iter()
            .collect();
        assert_eq!(keys, vec!["position"]);

        for role in [DragParam::Rotate, DragParam::Scale, DragParam::UniformScale] {
            assert!(
                role.keys(&t.params).is_none(),
                "{role:?} must write nothing on a target that declares none"
            );
        }
    }

    /// A panel is sized by two edge lengths, so its three-lane scale role has
    /// nothing to write while its uniform one writes both edges together.
    #[test]
    fn an_extent_sized_target_writes_both_edges_from_one_uniform_drag() {
        let rect_area = TransformParams {
            translate: Some("translate"),
            rotate: Some("rotate"),
            rotate_order: None,
            scale: ScaleParams::Extent2 {
                x: "width",
                z: "height",
            },
            pivot: None,
            aim: None,
        };
        let mut t = target_with(Matrix4::identity(), rect_area);
        t.extent = [4.0, 2.0];

        assert!(
            DragParam::Scale.keys(&t.params).is_none(),
            "a panel has no scale lanes"
        );
        let keys: Vec<&str> = DragParam::UniformScale
            .keys(&t.params)
            .expect("a size is declared")
            .iter()
            .collect();
        assert_eq!(keys, vec!["width", "height"]);

        // And the value read back carries both edges, positionally matching.
        let value = DragParam::UniformScale.read(&t);
        assert_eq!(value, DragValue::Extent([4.0, 2.0]));
        let values = value.values();
        assert_eq!(values[0], Some(DragScalarOrVec3::Scalar(4.0)));
        assert_eq!(values[1], Some(DragScalarOrVec3::Scalar(2.0)));
    }

    /// The keys and the values must always be the same length, or a commit
    /// would pair a name with the wrong number. Checked over every role
    /// against every shape a target can take.
    #[test]
    fn the_keys_and_the_values_of_a_drag_are_always_the_same_length() {
        let shapes = [
            FULL_TRS,
            TransformParams {
                translate: Some("position"),
                ..TransformParams::default()
            },
            TransformParams {
                translate: Some("translate"),
                rotate: Some("rotate"),
                scale: ScaleParams::Extent2 {
                    x: "width",
                    z: "height",
                },
                ..TransformParams::default()
            },
            TransformParams::default(),
        ];
        for shape in shapes {
            let mut t = target_with(Matrix4::identity(), shape);
            t.extent = [3.0, 5.0];
            for role in [
                DragParam::Translate,
                DragParam::Rotate,
                DragParam::Scale,
                DragParam::UniformScale,
            ] {
                let Some(keys) = role.keys(&t.params) else {
                    continue;
                };
                let values = role.read(&t).values();
                assert_eq!(
                    keys.iter().count(),
                    values.iter().flatten().count(),
                    "{role:?} on {shape:?}"
                );
            }
        }
    }

    #[test]
    fn a_drag_that_never_moved_is_not_an_edit() {
        assert!(
            !DragValue::Translate([1.0, 2.0, 3.0])
                .differs_from(DragValue::Translate([1.0, 2.0, 3.0]))
        );
        assert!(
            DragValue::Translate([1.0, 2.0, 3.0])
                .differs_from(DragValue::Translate([1.0, 2.0, 3.5]))
        );
        assert!(!DragValue::UniformScale(2.0).differs_from(DragValue::UniformScale(2.0)));
    }

    #[test]
    fn snapping_leaves_the_lanes_the_drag_never_touched_alone() {
        // An object sitting at y = 0.3 must not jump to y = 0.5 just because you
        // nudged it along X with Ctrl held.
        let mut next = [1.23, 0.3, 0.0];
        snap_changed_lanes(&mut next, [0.0, 0.3, 0.0], 0.5);
        assert!((next[0] - 1.0).abs() < 1e-6, "X snapped to the grid");
        assert!((next[1] - 0.3).abs() < 1e-6, "Y left exactly where it was");
    }

    #[test]
    fn only_select_refuses_to_manipulate() {
        assert!(!ToolMode::Select.manipulates());
        assert!(ToolMode::Move.manipulates());
        assert!(ToolMode::Rotate.manipulates());
        assert!(ToolMode::Scale.manipulates());
        assert_eq!(ToolMode::parse("rotate"), ToolMode::Rotate);
        assert_eq!(ToolMode::parse("nonsense"), ToolMode::Select);
    }

    /// Scale is always local, whatever the orientation says: a world-axis scale
    /// on a rotated object would need a shear param, and there is none.
    #[test]
    fn the_scale_tool_ignores_world_orientation() {
        let turned = Matrix3::from_angle_y(cgmath::Deg(90.0));
        let mut t = target(Matrix4::identity());
        t.basis = mat3(turned);

        let mut g = GizmoState {
            tool: ToolMode::Scale,
            ..Default::default()
        };
        g.settings.orientation = Orientation::World;
        let m = g.manipulator(&t, -Vector3::unit_z(), 1.0).unwrap();
        assert!(
            (m.basis.x.z + 1.0).abs() < 1e-5,
            "scale handles stay on the object's axes even in world mode"
        );

        // Translate, by contrast, honours it.
        let mut g = GizmoState {
            tool: ToolMode::Move,
            ..Default::default()
        };
        g.settings.orientation = Orientation::World;
        let m = g.manipulator(&t, -Vector3::unit_z(), 1.0).unwrap();
        assert!(
            (m.basis.x.x - 1.0).abs() < 1e-5,
            "world mode keeps the move handles on the world axes"
        );
        g.settings.orientation = Orientation::Local;
        let m = g.manipulator(&t, -Vector3::unit_z(), 1.0).unwrap();
        assert!((m.basis.x.z + 1.0).abs() < 1e-5, "local mode swings them");
    }
}
