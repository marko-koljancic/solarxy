//! Per-pane camera lifecycle and the depth-range fit.

use cgmath::Vector3;
use solarxy_core::AABB;
use solarxy_core::preferences::ProjectionMode;
use solarxy_renderer::camera::Camera;
use solarxy_renderer::camera_state::CameraState;

/// The near and far planes that put `bounds` exactly inside the Depth
/// inspection mode's visible range, for `camera`.
///
/// Existed three times before this crate: once in each shell and once more in
/// the golden harness, whose copy carried a comment saying it was the same
/// math. It is pure, so it is the one function here that moved without
/// reconciling anything.
#[must_use]
pub fn depth_bounds(camera: &Camera, bounds: &AABB) -> (f32, f32) {
    let view = camera.build_view_matrix();
    let mut z_min = f32::INFINITY;
    let mut z_max = f32::NEG_INFINITY;
    for corner in &bounds.corners() {
        let vp = view * corner.to_homogeneous();
        let z = -vp.z;
        z_min = z_min.min(z);
        z_max = z_max.max(z);
    }
    z_min = z_min.max(0.001);
    if z_max <= z_min {
        z_max = z_min + 1.0;
    }
    (z_min, z_max)
}

/// One of the six standard views a camera can be snapped to.
///
/// The direction and up pairs behind these existed three times before this
/// type: once in the desktop's key handler as eight lines of vectors spread
/// across four arms, once in the browser's camera command as a six-row match
/// on a string, and once more in [`ensure_pane_cameras`] below, which
/// open-coded three of them for the initial pane seed. A duplication census
/// that matched function names found the first two and missed the third,
/// which is the argument for a named type rather than a table: the pairs are
/// small enough to retype and easy enough to retype slightly wrong.
///
/// The framing arithmetic is not here and never was. Distance, target, near
/// and far planes and the orthographic scale all come from
/// `CameraState::reset_to_bounds_axis`, so what this type carries is only the
/// orientation each view is named for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StandardView {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
}

impl StandardView {
    /// The view a wire name asks for, or `None` for a name that is not one.
    ///
    /// The names are the ones the browser's camera command already accepted,
    /// so this parses exactly what that boundary parsed.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "top" => Some(Self::Top),
            "bottom" => Some(Self::Bottom),
            "front" => Some(Self::Front),
            "back" => Some(Self::Back),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            _ => None,
        }
    }

    /// The direction the camera looks from, and the up vector that keeps the
    /// view upright.
    ///
    /// Top and bottom cannot use the world up, because it is parallel to the
    /// view direction there and would leave the camera basis degenerate, so
    /// they take a horizontal axis instead. The other four take the world up.
    #[must_use]
    pub fn axis(self) -> (Vector3<f32>, Vector3<f32>) {
        match self {
            Self::Top => (Vector3::unit_y(), -Vector3::unit_z()),
            Self::Bottom => (-Vector3::unit_y(), Vector3::unit_z()),
            Self::Front => (Vector3::unit_z(), Vector3::unit_y()),
            Self::Back => (-Vector3::unit_z(), Vector3::unit_y()),
            Self::Left => (-Vector3::unit_x(), Vector3::unit_y()),
            Self::Right => (Vector3::unit_x(), Vector3::unit_y()),
        }
    }
}

/// Lazily create a [`CameraState`] for every pane slot the layout uses.
///
/// Idempotent: a slot that already holds a camera is skipped, so layout
/// toggles preserve per-slot cameras within a session. Slot 0 is the primary
/// perspective camera; slots 1 to 3 are cloned from it and reset to Top,
/// Front and Left as a one-time convenience the user re-orients afterwards.
///
/// `projection` is the shell's startup preference for slot 0, or `None` where
/// the shell has no such preference and takes the camera's own default. It is
/// deliberately not applied to slots 1 to 3: those are reset to an axis view
/// immediately afterwards, which sets their projection itself.
pub fn ensure_pane_cameras(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    cameras: &mut [Option<CameraState>; 4],
    bounds: &AABB,
    aspect: f32,
    count: usize,
    projection: Option<ProjectionMode>,
) {
    for i in 0..count.min(cameras.len()) {
        if cameras[i].is_some() {
            continue;
        }
        let mut cam = if i == 0 {
            CameraState::new(device, camera_layout, bounds, aspect)
        } else if let Some(src) = cameras[0].as_ref() {
            src.clone_with_new_resources(device, camera_layout)
        } else {
            continue;
        };
        match i {
            0 => {
                if let Some(mode) = projection {
                    cam.set_projection(mode);
                }
            }
            1 => reset_to_view(&mut cam, bounds, StandardView::Top),
            2 => reset_to_view(&mut cam, bounds, StandardView::Front),
            _ => reset_to_view(&mut cam, bounds, StandardView::Left),
        }
        cameras[i] = Some(cam);
    }
}

/// Writes a cooked camera node's definition onto a live camera.
///
/// Shared because two hosts now shoot through an authored camera, and a shot
/// taken from a slightly different place by each of them is the kind of
/// difference nobody notices until they are compared.
///
/// The guards are not defensive padding. A field-of-view or an orthographic
/// scale of zero is what a camera definition carries before its node has cooked,
/// and writing either through would give a camera that renders nothing at all
/// rather than one that renders the default.
pub fn apply_camera_def(cam: &mut Camera, def: &solarxy_core::scene::CameraDef) {
    use solarxy_core::preferences::ProjectionMode;
    use solarxy_core::scene::CameraKind;

    cam.eye = cgmath::Point3::new(def.position[0], def.position[1], def.position[2]);
    cam.target = cgmath::Point3::new(def.target[0], def.target[1], def.target[2]);
    cam.up = cgmath::Vector3::new(def.up[0], def.up[1], def.up[2]);
    if def.fov_y > 0.0 {
        cam.fovy = def.fov_y.to_degrees();
    }
    cam.projection = match def.kind {
        CameraKind::Orthographic => ProjectionMode::Orthographic,
        _ => ProjectionMode::Perspective,
    };
    if def.ortho_scale > 0.0 {
        cam.ortho_scale = def.ortho_scale;
    }
}

/// The lens a shot is taken through, with its one implicit value resolved.
///
/// Focus distance zero does not mean the focal plane is at the camera; it
/// means the camera focuses on what it is aimed at, which is what the node's
/// help promises and what makes aiming a camera also focus it. Resolving that
/// here rather than in a shader keeps the rule in one place and keeps it
/// readable, and every host that renders through a camera gets the same
/// answer.
///
/// A camera definition arrives from a node that may not have cooked, so a
/// negative or non-finite value is treated as unset rather than written
/// through to the sampler.
#[must_use]
pub fn lens_for(def: &solarxy_core::scene::CameraDef) -> solarxy_core::scene::CameraLens {
    let mut lens = def.lens;
    if !lens.aperture_radius.is_finite() || lens.aperture_radius < 0.0 {
        lens.aperture_radius = 0.0;
    }
    if !lens.focus_distance.is_finite() || lens.focus_distance <= 0.0 {
        let d = [
            def.target[0] - def.position[0],
            def.target[1] - def.position[1],
            def.target[2] - def.position[2],
        ];
        let dist = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        // A camera sitting on its own target focuses on nothing, so it stays a
        // pinhole rather than acquiring a focal plane at zero distance, which
        // would defocus the entire image.
        lens.focus_distance = dist;
        if dist <= 0.0 {
            lens.aperture_radius = 0.0;
        }
    }
    lens
}

#[cfg(test)]
mod tests {
    use solarxy_core::scene::{CameraDef, CameraLens};

    fn def(position: [f32; 3], target: [f32; 3], lens: CameraLens) -> CameraDef {
        CameraDef {
            id: solarxy_core::scene::SceneObjectId(0),
            kind: solarxy_core::scene::CameraKind::Perspective,
            position,
            target,
            up: [0.0, 1.0, 0.0],
            fov_y: std::f32::consts::FRAC_PI_4,
            near: 0.1,
            far: 100.0,
            ortho_scale: 1.0,
            aspect: 1.0,
            show_gizmo: false,
            gizmo_size: 1.0,
            look: solarxy_core::scene::CameraLook::default(),
            lens,
        }
    }

    #[test]
    fn every_standard_view_looks_down_its_own_axis_with_a_usable_up() {
        use super::StandardView::{Back, Bottom, Front, Left, Right, Top};
        use cgmath::{InnerSpace, Vector3};

        // The pairs were open-coded in three places before this type, so what
        // is worth pinning is not the literal vectors, which a reader can see,
        // but the two properties a wrongly retyped pair would break: the view
        // really points along the axis it is named for, and the up vector is
        // not parallel to it, which would leave the camera basis degenerate.
        let expected = [
            (Top, Vector3::unit_y()),
            (Bottom, -Vector3::unit_y()),
            (Front, Vector3::unit_z()),
            (Back, -Vector3::unit_z()),
            (Left, -Vector3::unit_x()),
            (Right, Vector3::unit_x()),
        ];
        for (view, axis) in expected {
            let (dir, up) = view.axis();
            assert_eq!(dir, axis, "{view:?} does not look down its own axis");
            assert!(
                dir.cross(up).magnitude() > 0.5,
                "{view:?} has an up vector parallel to its direction"
            );
        }
    }

    #[test]
    fn a_view_name_that_is_not_a_view_is_refused() {
        // The browser's camera command parses a wire string, and its previous
        // match arm ended in an error rather than a default. Silently falling
        // back to a view nobody asked for would be worse than the error.
        assert!(super::StandardView::from_name("top").is_some());
        assert!(super::StandardView::from_name("Top").is_none());
        assert!(super::StandardView::from_name("sideways").is_none());
    }

    #[test]
    fn an_unset_focus_distance_becomes_the_distance_to_the_target() {
        // The node's help promises that aiming a camera also focuses it, and
        // zero is what a camera carries until somebody types a number. Reading
        // it literally would put the focal plane on the lens and defocus the
        // whole image.
        let lens = super::lens_for(&def(
            [0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0],
            CameraLens {
                aperture_radius: 0.01,
                focus_distance: 0.0,
                blades: 0,
            },
        ));
        assert!((lens.focus_distance - 3.0).abs() < 1e-5);
        assert!((lens.aperture_radius - 0.01).abs() < f32::EPSILON);
    }

    #[test]
    fn an_authored_focus_distance_wins_over_the_target() {
        let lens = super::lens_for(&def(
            [0.0, 0.0, 4.0],
            [0.0, 0.0, 1.0],
            CameraLens {
                aperture_radius: 0.01,
                focus_distance: 12.5,
                blades: 6,
            },
        ));
        assert!((lens.focus_distance - 12.5).abs() < f32::EPSILON);
        assert_eq!(lens.blades, 6);
    }

    #[test]
    fn a_camera_sitting_on_its_target_stays_a_pinhole() {
        // Nothing to focus on and no distance to derive, so the aperture is
        // dropped rather than paired with a focal plane at zero, which would
        // blur every pixel of the image.
        let lens = super::lens_for(&def(
            [1.0, 2.0, 3.0],
            [1.0, 2.0, 3.0],
            CameraLens {
                aperture_radius: 0.02,
                focus_distance: 0.0,
                blades: 0,
            },
        ));
        assert!((lens.aperture_radius - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn a_nonsense_aperture_is_treated_as_unset() {
        // A camera definition can arrive from a node that has not cooked.
        for bad in [f32::NAN, f32::INFINITY, -1.0] {
            let lens = super::lens_for(&def(
                [0.0, 0.0, 4.0],
                [0.0, 0.0, 0.0],
                CameraLens {
                    aperture_radius: bad,
                    focus_distance: 2.0,
                    blades: 0,
                },
            ));
            assert!(
                (lens.aperture_radius - 0.0).abs() < f32::EPSILON,
                "{bad} survived into the sampler"
            );
        }
    }
}

/// Drive each bound pane's camera from its camera node's current pose.
///
/// Runs every frame in both shells, so a camera node moved by a scene edit,
/// a parameter change or a reload carries its panes with it. A binding whose
/// node no longer exists follows nothing and the pane keeps its last pose,
/// which is what makes deleting a camera harmless mid-session.
///
/// `suppressed` is the per-pane guard, and it is a mask rather than a
/// predicate on purpose. The two shells disagree about when a bound pane
/// should stop following: the browser holds the follow off while a pane is
/// mid-navigation and while a turntable spins its scratch camera, so the
/// follow never fights a live orbit; the desktop has neither state and
/// suppresses nothing. Passing that as data keeps the disagreement visible at
/// the call sites and keeps this signature free of anything a shell owns. A
/// closure here would let either shell hide policy inside the shared body,
/// which is the failure the crate's membership rule exists to prevent.
///
/// Takes the definitions and the pane cameras as separate borrows, which is
/// what lets a caller pass its scene and its view state in one expression
/// without cloning a definition to end a borrow first.
pub fn follow_camera_bindings(
    defs: &[solarxy_core::scene::CameraDef],
    bindings: &[Option<solarxy_core::scene::SceneObjectId>; 4],
    suppressed: &[bool; 4],
    cameras: &mut [Option<CameraState>; 4],
) {
    for (i, binding) in bindings.iter().enumerate() {
        let (Some(id), false) = (binding, suppressed[i]) else {
            continue;
        };
        let Some(def) = defs.iter().find(|c| c.id == *id) else {
            continue;
        };
        if let Some(state) = cameras[i].as_mut() {
            apply_camera_def(&mut state.camera, def);
        }
    }
}

/// Snap `cam` to a standard view fitted to `bounds`.
///
/// The one call site pairing [`StandardView::axis`] with the framing, so a
/// caller names the view it wants and never the vectors behind it.
pub fn reset_to_view(cam: &mut CameraState, bounds: &AABB, view: StandardView) {
    let (dir, up) = view.axis();
    cam.reset_to_bounds_axis(bounds, dir, up);
}

/// The look a pane's bound camera carries, if the pane is bound to one that
/// exists.
///
/// Both shells resolve a pane's grade by starting here. The lookup is small,
/// but it was written four times across the two shells and each copy had to
/// remember that a binding can name a camera the document no longer has.
#[must_use]
pub fn camera_look_for(
    defs: Option<&[solarxy_core::scene::CameraDef]>,
    binding: Option<solarxy_core::scene::SceneObjectId>,
) -> Option<&solarxy_core::scene::CameraLook> {
    let id = binding?;
    defs?.iter().find(|c| c.id == id).map(|c| &c.look)
}

/// Bind the pair of grading tables a look asks for, before whatever is about
/// to composite.
///
/// There is one pair of table textures for the whole renderer while a look is
/// per shot, so the pair follows whichever pane or still is next. `set_lut`
/// dedupes on content hash, so the common case, no tables at all or every
/// pane through one camera, costs two comparisons and rebuilds nothing.
///
/// A `None` look clears both slots rather than leaving them, which is the
/// behaviour that matters: an empty slot binds an identity table, so a pane
/// with no camera composites ungraded instead of inheriting the grade of
/// whichever pane drew before it.
///
/// This existed four times, three of them inside one shell, because the
/// viewport path, the still path and the still's look preparation each needed
/// it and none of them could reach the others.
pub fn bind_look_luts(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut solarxy_renderer::frame::Renderer,
    look: Option<&solarxy_core::scene::CameraLook>,
) {
    let (a, b) = look.map_or((None, None), |l| (l.lut_a.clone(), l.lut_b.clone()));
    renderer.set_lut(
        device,
        queue,
        solarxy_renderer::lut::LutSlot::A,
        a.as_deref(),
    );
    renderer.set_lut(
        device,
        queue,
        solarxy_renderer::lut::LutSlot::B,
        b.as_deref(),
    );
}
