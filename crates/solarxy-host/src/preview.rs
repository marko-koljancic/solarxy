//! The asset preview: a throwaway scene holding one model, rendered on
//! demand into whatever target the shell hands over.
//!
//! Both shells preview a staged model the same way: parse it through the
//! path the import cooks use, upload it to its own [`SceneObjects`] with
//! its own camera, frame the camera on its bounds, and render only when
//! something moved (open, orbit, zoom, resize), never in the frame loop, so
//! an idle preview costs nothing. Each render borrows the shared render
//! chain at the preview's size; the next main frame's target sync restores
//! it. What differs between the shells is the target: the browser draws to
//! a second canvas's surface, the desktop into a texture egui samples.
//!
//! The camera rules are here, with their constants, so an orbit drags the
//! same distance per pixel in both shells.

use cgmath::{InnerSpace, Vector3};
use solarxy_core::AABB;
use solarxy_core::preferences::{BackgroundMode, ResolvedBackground};
use solarxy_core::scene::{SceneDelta, SceneObjectId, SceneOp};
use solarxy_core::view_config::PaneDisplaySettings;
pub use solarxy_kernel::GeometrySet;
use solarxy_renderer::bind_groups::BindGroupLayouts;
use solarxy_renderer::camera::Camera;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::error::RendererError;
use solarxy_renderer::frame::{DrawObject, Renderer};
use solarxy_renderer::scene_objects::SceneObjects;

/// A preview target is never smaller than this on either edge.
pub const MIN_EDGE: u32 = 16;
/// Radians of yaw per pixel of horizontal drag. Negative so dragging right
/// turns the model to the right.
pub const YAW_PER_PX: f32 = -0.008;
/// Radians of pitch per pixel of vertical drag.
pub const PITCH_PER_PX: f32 = 0.008;
/// The orbit never reaches the pole: pitch is held inside this, in radians.
pub const PITCH_LIMIT: f32 = 1.45;
/// A dolly of one unit scales the eye's distance by `exp(-0.1)`.
pub const DOLLY_PER_UNIT: f32 = 0.1;

/// The pane settings a preview renders with: what a still renders with, on
/// the default gradient. A still already draws no grid, no axis gizmo and
/// no light markers, which is what a preview of one model wants too, so
/// the rule is stated once, in `for_still`, and read here.
#[must_use]
pub fn preview_pane_settings() -> PaneDisplaySettings {
    PaneDisplaySettings::for_still(BackgroundMode::GRADIENT)
}

/// Orbit the eye around the target about the world-up axis by `yaw`
/// radians. Also the turntable rotation an export sweep applies.
pub fn orbit_yaw(cam: &mut Camera, yaw: f32) {
    let offset = cam.eye - cam.target;
    let (s, c) = yaw.sin_cos();
    let x = offset.x * c + offset.z * s;
    let z = -offset.x * s + offset.z * c;
    cam.eye = cam.target + Vector3::new(x, offset.y, z);
}

/// Tilt the eye about the target's horizontal axis by `delta` radians,
/// clamped so the orbit never flips over the pole.
pub fn orbit_pitch(cam: &mut Camera, delta: f32) {
    let offset = cam.eye - cam.target;
    let dist = offset.magnitude().max(1e-4);
    let pitch = (offset.y / dist).clamp(-1.0, 1.0).asin();
    let new_pitch = (pitch + delta).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    let horiz = (offset.x * offset.x + offset.z * offset.z).sqrt().max(1e-4);
    let scale = (dist * new_pitch.cos()) / horiz;
    cam.eye = cam.target + Vector3::new(offset.x * scale, dist * new_pitch.sin(), offset.z * scale);
}

/// Dolly the eye along its line to the target; positive moves in.
pub fn dolly(cam: &mut Camera, delta: f32) {
    let offset = cam.eye - cam.target;
    cam.eye = cam.target + offset * (-delta * DOLLY_PER_UNIT).exp();
}

/// One model, its camera, and the size it renders at.
pub struct PreviewScene {
    objects: SceneObjects,
    camera: CameraState,
    width: u32,
    height: u32,
}

impl PreviewScene {
    /// Upload a parsed set to a scene of its own and frame a camera on it.
    ///
    /// # Errors
    ///
    /// When the geometry cannot be uploaded.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        set: &GeometrySet,
        width: u32,
        height: u32,
    ) -> Result<Self, RendererError> {
        let cooked = std::sync::Arc::new(set.to_cooked());
        let width = width.max(MIN_EDGE);
        let height = height.max(MIN_EDGE);
        let mut objects = SceneObjects::new();
        let delta = SceneDelta {
            ops: vec![SceneOp::UpsertGeometry {
                id: SceneObjectId(0),
                geometry: cooked,
            }],
        };
        objects.apply(device, queue, layouts, &delta)?;
        let bounds = objects
            .visible_bounds()
            .unwrap_or_else(solarxy_renderer::environment::placeholder_bounds);
        let aspect = width as f32 / height as f32;
        let camera = CameraState::new(device, &layouts.camera, &bounds, aspect);
        Ok(Self {
            objects,
            camera,
            width,
            height,
        })
    }

    #[must_use]
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[must_use]
    pub fn camera(&self) -> &Camera {
        &self.camera.camera
    }

    /// The bounds the camera was framed on, for a caller that wants to
    /// frame again.
    #[must_use]
    pub fn bounds(&self) -> Option<AABB> {
        self.objects.visible_bounds()
    }

    /// Orbit by a drag of `dx`, `dy` pixels.
    pub fn orbit(&mut self, dx: f32, dy: f32) {
        orbit_yaw(&mut self.camera.camera, dx * YAW_PER_PX);
        orbit_pitch(&mut self.camera.camera, dy * PITCH_PER_PX);
    }

    /// Dolly by `delta`; positive moves in.
    pub fn zoom(&mut self, delta: f32) {
        dolly(&mut self.camera.camera, delta);
    }

    /// Adopt a new target size, floored at [`MIN_EDGE`]. Says whether it
    /// changed, so a caller can skip a render for a resize to the same size.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        let (w, h) = (width.max(MIN_EDGE), height.max(MIN_EDGE));
        let changed = (w, h) != (self.width, self.height);
        self.width = w;
        self.height = h;
        changed
    }

    /// Render one frame into `target`, which must be the preview's size and
    /// the format the renderer's composite was built for.
    ///
    /// Borrows the shared render chain at the preview's size, and leaves it
    /// there: the shell's next main frame syncs the targets back, as it does
    /// after a screenshot.
    pub fn render(
        &mut self,
        renderer: &mut Renderer,
        env: &SceneEnvironment,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        target: &wgpu::TextureView,
        background: ResolvedBackground,
    ) {
        let (w, h) = (self.width, self.height);
        renderer.resize_targets(device, w, h);
        let pds = preview_pane_settings();
        let aspect = w as f32 / h.max(1) as f32;
        self.camera.write_with_aspect(queue, aspect);

        let objects: Vec<DrawObject<'_>> = self.objects.draw_objects().collect();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Preview Encoder"),
        });
        // Shadow the preview content itself, so the map matches what is drawn.
        renderer.render_shadow_pass(&mut encoder, env, &objects);
        renderer.render_main_pass(
            &mut encoder,
            env,
            &objects,
            &self.camera.bind_group,
            &self.camera.camera,
            &pds,
            background,
        );
        // Composite without bloom or occlusion: a preview is a shaded look,
        // not a post-processed beauty frame.
        renderer.post.composite.write_params(
            queue,
            false,
            false,
            &CompositeLook::from_tone(renderer.post.tone_mode, renderer.post.exposure),
            &renderer.post.luts,
            pds.inspection_mode,
            false,
        );
        renderer.post.composite.render(
            &mut encoder,
            &renderer.pipelines,
            target,
            false,
            &renderer.post.ssao,
            Some([0.0, 0.0, w as f32, h as f32]),
            true,
            None,
        );
        queue.submit(std::iter::once(encoder.finish()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cgmath::Point3;
    use solarxy_core::preferences::ProjectionMode;

    fn camera() -> Camera {
        Camera {
            eye: Point3::new(0.0, 2.0, 8.0),
            target: Point3::new(0.0, 0.0, 0.0),
            up: Vector3::unit_y(),
            aspect: 1.0,
            fovy: 45.0,
            znear: 0.01,
            zfar: 100.0,
            projection: ProjectionMode::Perspective,
            ortho_scale: 1.0,
        }
    }

    fn distance(cam: &Camera) -> f32 {
        (cam.eye - cam.target).magnitude()
    }

    fn pitch(cam: &Camera) -> f32 {
        let offset = cam.eye - cam.target;
        (offset.y / offset.magnitude()).asin()
    }

    /// A yaw keeps the distance and the height, and a full turn comes back.
    #[test]
    fn a_yaw_keeps_distance_and_height_and_a_full_turn_returns() {
        let mut cam = camera();
        let before = distance(&cam);
        orbit_yaw(&mut cam, 1.0);
        assert!((distance(&cam) - before).abs() < 1e-4);
        assert!(
            (cam.eye.y - 2.0).abs() < 1e-5,
            "height is untouched by a yaw"
        );
        orbit_yaw(&mut cam, std::f32::consts::TAU - 1.0);
        assert!((cam.eye.z - 8.0).abs() < 1e-3 && cam.eye.x.abs() < 1e-3);
    }

    /// The pitch climbs by what was asked and stops inside the limit, so a
    /// drag past the pole never flips the model over.
    #[test]
    fn the_pitch_is_clamped_short_of_the_pole() {
        let mut cam = camera();
        let before = pitch(&cam);
        orbit_pitch(&mut cam, 0.3);
        assert!((pitch(&cam) - (before + 0.3)).abs() < 1e-4);
        assert!(
            (distance(&cam) - camera().eye.z.hypot(2.0)).abs() < 1e-3,
            "a pitch keeps distance"
        );
        orbit_pitch(&mut cam, 10.0);
        assert!(
            (pitch(&cam) - PITCH_LIMIT).abs() < 1e-4,
            "held at the limit going up"
        );
        orbit_pitch(&mut cam, -20.0);
        assert!((pitch(&cam) + PITCH_LIMIT).abs() < 1e-4, "and going down");
    }

    /// A positive dolly moves in by the browser's factor, a negative one
    /// out, and the line to the target does not turn.
    #[test]
    fn a_dolly_moves_along_the_line_to_the_target() {
        let mut cam = camera();
        let before = distance(&cam);
        dolly(&mut cam, 1.0);
        assert!((distance(&cam) - before * (-DOLLY_PER_UNIT).exp()).abs() < 1e-4);
        assert!(cam.eye.x.abs() < 1e-6, "still on the same line");
        dolly(&mut cam, -1.0);
        assert!(
            (distance(&cam) - before).abs() < 1e-4,
            "out by the same factor"
        );
    }

    /// A preview never renders with a grid, a gizmo or light markers: it
    /// is about one model, not a scene. Pinned here because the rule lives
    /// in the still's settings, which this reads rather than restates.
    #[test]
    fn a_preview_draws_no_furniture() {
        let pds = preview_pane_settings();
        assert!(!pds.show_grid);
        assert!(!pds.show_axis_gizmo);
        assert!(!pds.show_light_markers);
    }
}
