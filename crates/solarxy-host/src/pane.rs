//! Per-pane uniform writes, ahead of a pane's 3D passes.

use solarxy_core::AABB;
use solarxy_core::preferences::ResolvedBackground;
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_renderer::camera::CameraUniform;
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::frame::{GradientUniform, Renderer, WireframeParams};
use solarxy_renderer::scene::BackgroundModeExt;
use solarxy_renderer::visualization::GridUniform;

/// What a pane needs written before its passes encode.
///
/// Several of these are `Option` because one shell has a capability the other
/// does not, and in every case the `None` arm is what that shell already did.
pub struct PaneUniforms<'a> {
    /// The pane's background, already resolved against whatever registry of
    /// user backgrounds the shell keeps.
    pub background: ResolvedBackground,
    pub pds: &'a PaneDisplaySettings,
    pub display: &'a DisplaySettings,
    /// The pane's camera, or `None` for a slot that has none yet — in which
    /// case the camera-uniform block is not written at all, exactly as before.
    pub camera: Option<&'a CameraState>,
    /// The environment holding the grid uniform, or `None` where the shell has
    /// no scene yet and therefore no grid buffer to write.
    pub env: Option<&'a SceneEnvironment>,
    /// Bounds to fit the depth range against. `None` falls back to the fixed
    /// 0.01-to-100 range the desktop shell used when no model was loaded.
    pub bounds: Option<&'a AABB>,
    /// The grid plane this pane's camera wants, or `None` to leave the plane
    /// untouched.
    ///
    /// **`None` is not `Some(0)`.** The desktop shell has never written this
    /// offset, so writing a zero here would flip its orthographic panes from
    /// whatever the grid was initialised with to the ground plane. The shell
    /// that does not have the feature must leave the bytes alone, not write a
    /// value that looks neutral.
    pub grid_plane: Option<u32>,
}

/// Write the wireframe uniform alone.
///
/// Separate from [`write_pane_uniforms`] because the desktop shell also writes
/// it on its own, when a wireframe setting changes outside a pane's render.
pub fn write_wireframe_params(
    queue: &wgpu::Queue,
    renderer: &Renderer,
    background: ResolvedBackground,
    pds: &PaneDisplaySettings,
    display: &DisplaySettings,
) {
    let wire = WireframeParams {
        color: background.wireframe_color(),
        line_width: pds.line_weight.width_px(),
        screen_width: renderer.target_width as f32,
        screen_height: renderer.target_height as f32,
        point_size: display.point_size,
    };
    queue.write_buffer(
        &renderer.wire.wireframe_params_buffer,
        0,
        bytemuck::bytes_of(&wire),
    );
}

/// Write a pane's wireframe, gradient, grid and camera-inspection uniforms.
///
/// These share one buffer each across all panes, so they are rewritten before
/// every pane's passes rather than once per frame.
pub fn write_pane_uniforms(queue: &wgpu::Queue, renderer: &Renderer, u: &PaneUniforms<'_>) {
    write_wireframe_params(queue, renderer, u.background, u.pds, u.display);

    let (top, bottom) = u.background.sky_colors();
    let gradient = GradientUniform {
        top_color: [top[0], top[1], top[2], 1.0],
        bottom_color: [bottom[0], bottom[1], bottom[2], 1.0],
        uv_y_offset: 0.0,
        uv_y_scale: 1.0,
        _pad: [0.0; 2],
    };
    queue.write_buffer(
        &renderer.wire._gradient_buffer,
        0,
        bytemuck::bytes_of(&gradient),
    );

    if let Some(env) = u.env {
        let grid = u.background.grid_color();
        queue.write_buffer(
            &env.vis.grid_uniform_buf,
            GridUniform::COLOR_OFFSET,
            bytemuck::cast_slice(&grid),
        );
        if let Some(plane) = u.grid_plane {
            queue.write_buffer(
                &env.vis.grid_uniform_buf,
                GridUniform::PLANE_OFFSET,
                bytemuck::bytes_of(&plane),
            );
        }
    }

    if let Some(cam) = u.camera {
        let (near, far) = u.bounds.map_or((0.01, 100.0), |b| {
            crate::cameras::depth_bounds(&cam.camera, b)
        });
        let data: [u32; 8] = [
            u.pds.inspection_mode.as_u32(),
            u.pds.texel_density_target.to_bits(),
            u.pds.material_override.as_u32(),
            near.to_bits(),
            far.to_bits(),
            u.display.roughness_scale.to_bits(),
            u.display.metallic_scale.to_bits(),
            u.display.hdri_rotation.to_bits(),
        ];
        queue.write_buffer(
            &cam.buffer,
            CameraUniform::INSPECTION_OFFSET,
            bytemuck::cast_slice(&data),
        );
    }
}
