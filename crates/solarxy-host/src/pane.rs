//! One pane's render: the uniform writes, the pass chain, and the composite.

use solarxy_core::AABB;
use solarxy_core::preferences::{InspectionMode, ResolvedBackground};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_renderer::camera::{Camera, CameraUniform};
use solarxy_renderer::camera_state::CameraState;
use solarxy_renderer::composite::CompositeLook;
use solarxy_renderer::environment::SceneEnvironment;
use solarxy_renderer::frame::{DrawObject, GradientUniform, Renderer, SelectionStyle, WireframeParams};
use solarxy_renderer::panes::PaneRect;
use solarxy_renderer::scene::{BackgroundModeExt, lights_from_camera};
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
    /// The environment holding the grid uniform.
    pub env: &'a SceneEnvironment,
    /// Bounds to fit the depth range against. Both shells always have some,
    /// falling back to the box their environment is fitted to; the `None`
    /// arm's fixed 0.01-to-100 range is retained only for callers of
    /// [`write_inspection_block`] that have no scene at all.
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

    let grid = u.background.grid_color();
    queue.write_buffer(
        &u.env.vis.grid_uniform_buf,
        GridUniform::COLOR_OFFSET,
        bytemuck::cast_slice(&grid),
    );
    if let Some(plane) = u.grid_plane {
        queue.write_buffer(
            &u.env.vis.grid_uniform_buf,
            GridUniform::PLANE_OFFSET,
            bytemuck::bytes_of(&plane),
        );
    }

    if let Some(cam) = u.camera {
        write_inspection_block(
            queue,
            cam,
            u.pds,
            u.bounds,
            u.display.roughness_scale,
            u.display.metallic_scale,
            u.display.hdri_rotation,
        );
    }
}

/// Write the camera uniform's inspection block alone: the mode, the texel
/// density target, the material override, the fitted depth range, and the
/// three scene-wide shading scalars.
///
/// Separate from [`write_pane_uniforms`] because the golden harness writes
/// exactly this and nothing else. It has no wireframe, gradient or grid state
/// of its own, so handing it the whole pane block would make it start writing
/// three buffers it never wrote and change what it captures.
///
/// The three scalars are passed individually rather than as a
/// [`DisplaySettings`] for the same reason: the harness has no such value, and
/// inventing one there would put a second set of defaults next to the shells'.
pub fn write_inspection_block(
    queue: &wgpu::Queue,
    camera: &CameraState,
    pds: &PaneDisplaySettings,
    bounds: Option<&AABB>,
    roughness_scale: f32,
    metallic_scale: f32,
    hdri_rotation: f32,
) {
    let (near, far) = bounds.map_or((0.01, 100.0), |b| {
        crate::cameras::depth_bounds(&camera.camera, b)
    });
    let data: [u32; 8] = [
        pds.inspection_mode.as_u32(),
        pds.texel_density_target.to_bits(),
        pds.material_override.as_u32(),
        near.to_bits(),
        far.to_bits(),
        roughness_scale.to_bits(),
        metallic_scale.to_bits(),
        hdri_rotation.to_bits(),
    ];
    queue.write_buffer(
        &camera.buffer,
        CameraUniform::INSPECTION_OFFSET,
        bytemuck::cast_slice(&data),
    );
}

/// One pane's 3D scene, as the pass chain sees it.
pub struct PaneScene<'a> {
    /// The pane's draw list, already assembled by the shell. The desktop
    /// shell puts its file-loaded model first and appends the multi-object
    /// entries; the web shell has only the latter.
    pub objects: &'a [DrawObject<'a>],
    pub env: &'a SceneEnvironment,
    pub cam_bg: &'a wgpu::BindGroup,
    pub cam_data: &'a Camera,
    pub pds: &'a PaneDisplaySettings,
    pub background: ResolvedBackground,
    /// Whether this pane re-renders the shadow map. Pane 0 always does;
    /// the others only when the light rig is following their camera.
    pub shadow: bool,
    /// Whether any object in `objects` is flagged selected, which is what
    /// decides if the outline stages run. Always `false` on a shell with no
    /// selection concept, and the outline is then never encoded.
    pub selected: bool,
}

/// Encode one pane's 3D passes: shadow, `GBuffer`, main, selection outline,
/// SSAO resolve and bloom, in that order.
///
/// `cam_bg` and `env` are not optional, and the reason is worth recording
/// because the code this replaced looked as though they were. Both shells
/// return from `render_pane` before reaching this point when the pane has no
/// camera, and both own their scene environment for the whole session rather
/// than for as long as a model is loaded. A pane that gets here therefore has
/// both. The draw list is the part that is allowed to be empty: a viewport
/// with nothing in it still renders its background, grid, floor and axes
/// through this chain.
pub fn render_3d_passes(
    renderer: &Renderer,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    s: &PaneScene<'_>,
) {
    if s.shadow {
        renderer.render_shadow_pass(encoder, s.env, s.objects);
    }

    if renderer.post.ssao_enabled {
        renderer.render_gbuffer_pass(encoder, s.objects, s.cam_bg);
    }
    renderer.render_main_pass(
        encoder,
        s.env,
        s.objects,
        s.cam_bg,
        s.cam_data,
        s.pds,
        s.background,
    );

    // The offscreen mask and jump-flood stages run here; the rim itself blits
    // onto the swapchain after the composite pass, so it never blooms and AO
    // never darkens it.
    if s.selected && renderer.selection_style == SelectionStyle::Outline {
        renderer.render_selection_outline(encoder, s.objects, s.cam_bg);
    }

    if renderer.post.ssao_enabled {
        renderer.render_ssao_passes(encoder, s.cam_bg);
    }
    if renderer.post.bloom_enabled {
        renderer.post.bloom.render(
            encoder,
            &renderer.pipelines,
            queue,
            renderer.target_width,
            renderer.target_height,
        );
    }
}

/// The overdraw inspection pass chain, which replaces the ordinary one.
pub fn render_overdraw_pane(
    renderer: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    objects: &[DrawObject<'_>],
    cam_bg: &wgpu::BindGroup,
    rect: PaneRect,
    is_split: bool,
) {
    let pane_viewport = is_split.then_some([rect.x, rect.y, rect.width, rect.height]);
    renderer.render_overdraw_passes(encoder, objects, cam_bg, pane_viewport);
}

/// What the composite pass needs to finish one pane.
pub struct PaneComposite {
    /// The pane's slot. Only pane 0 clears the surface; the rest composite
    /// into their own rect on top.
    pub index: usize,
    pub rect: PaneRect,
    /// The look this pane composites with, already resolved against whatever
    /// camera it is looking through.
    pub look: CompositeLook,
    pub inspection: InspectionMode,
    pub is_uv_map: bool,
    pub scene_present: bool,
    /// Whether to blit the selection rim after tone mapping. `false` on a
    /// shell with no selection concept.
    pub outline: bool,
}

/// Composite one pane into the surface and submit its encoder.
///
/// Takes the encoder by value because submitting it is the point: a pane owns
/// its encoder from the first pass to the queue.
pub fn composite_and_submit(
    queue: &wgpu::Queue,
    renderer: &Renderer,
    mut encoder: wgpu::CommandEncoder,
    surface_view: &wgpu::TextureView,
    c: &PaneComposite,
) {
    let pane_bloom = renderer.post.bloom_enabled && !c.is_uv_map && c.scene_present;
    let pane_ssao = renderer.post.ssao_enabled && !c.is_uv_map && c.scene_present;
    renderer.post.composite.write_params(
        queue,
        pane_bloom,
        pane_ssao,
        &c.look,
        &renderer.post.luts,
        c.inspection,
    );

    let viewport = Some([c.rect.x, c.rect.y, c.rect.width, c.rect.height]);
    renderer.post.composite.render(
        &mut encoder,
        &renderer.pipelines,
        surface_view,
        pane_ssao,
        &renderer.post.ssao,
        viewport,
        c.index == 0,
    );

    // The selection rim lands after tone mapping, so it never blooms and AO
    // never darkens it.
    if c.outline && c.scene_present && !c.is_uv_map && c.inspection != InspectionMode::Overdraw {
        renderer.composite_selection_outline(&mut encoder, surface_view, viewport);
    }

    queue.submit(std::iter::once(encoder.finish()));
}

/// Recompute the camera-relative light rig for a pane before it renders, so
/// each pane is lit from its own viewpoint.
///
/// The *body* only. Whether a given pane should get this at all is policy, and
/// the two shells answer it differently: both skip it when the lights are
/// locked, and the web shell additionally skips it whenever the document has
/// real light nodes, which are world-fixed and owe nothing to a camera. Those
/// guards stay with the shells; only the arithmetic is here.
pub fn setup_pane_lighting(
    queue: &wgpu::Queue,
    env: &mut SceneEnvironment,
    cam_data: &Camera,
    bounds: &AABB,
    ibl_avg: [f32; 3],
) {
    env.lights_uniform = lights_from_camera(cam_data, bounds, ibl_avg);
    queue.write_buffer(
        &env.light_buffer,
        0,
        bytemuck::cast_slice(&[env.lights_uniform]),
    );
    let key_pos = env.lights_uniform.lights[0].position;
    env.shadow.update_light_vp(
        queue,
        cgmath::Point3::new(key_pos[0], key_pos[1], key_pos[2]),
        bounds.center(),
        bounds.diagonal() / 2.0,
    );
}
