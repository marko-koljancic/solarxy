//! One pane's render: the uniform writes, the pass chain, and the composite.
//!
//! [`encode_pane_passes`] is the whole body, 3D and UV alike. It arrived after the
//! pieces below did, and collapsing the last two hand-rolled copies onto it
//! meant settling five places where the shells had quietly drifted apart.
//! Recording them here, because each one is a decision and four of them look
//! like accidents:
//!
//! - **Point size on the UV path.** One shell carried the compile-time default
//!   and the other the live setting, with comments making contradictory claims
//!   about the same shared uniform. The live setting wins: it is the only one
//!   that stays right after the value is changed, and the shell that never
//!   changes it reads the same number either way.
//! - **The aspect guard.** One shell divided by a raw pane height and could
//!   produce a non-finite aspect on a zero-height pane. The floor is kept.
//! - **The outline predicate.** One shell asked only whether something was
//!   selected, the other whether the selection resolves to a drawable object.
//!   The stricter one wins, and it is what the pass chain already gates on, so
//!   the looser one only ever submitted a blit that painted nothing.
//! - **The dark-background write's position** in the UV chain differed. It is
//!   a queue write, applied ahead of the encoder's commands at submit either
//!   way, so the two orders were always the same picture.
//! - **`scene_present` at the composite.** One shell computed it and the other
//!   passed a constant `true`, so a pane with a camera and an empty scene had
//!   bloom and ambient occlusion folded into its composite on one shell and not
//!   the other. The computed answer wins; see the scene-present field on the
//!   frame context.
//!
//! One thing deliberately **not** settled here: the UV overlap statistics pass
//! writes the identity UV camera, encodes, then writes the pane camera back,
//! all before a single submit. Queue writes are applied ahead of the command
//! buffer, so the last write wins and the statistic is measured at the pane's
//! zoom rather than at identity. Both shells had that, identically, before this
//! merge, and fixing it changes a number a user reads, which is not what a
//! collapse commit is for.

use solarxy_core::AABB;
use solarxy_core::preferences::{InspectionMode, ResolvedBackground, UvMapBackground};
use solarxy_core::view_config::{DisplaySettings, PaneDisplaySettings};
use solarxy_renderer::backend::{FrameCtx, PaneContent, UvSource};
use solarxy_renderer::scene_objects::SceneObjects;
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
        &renderer.wire.gradient_buffer,
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

/// What a pane's passes decided, and what its composite needs to know.
///
/// Distinct from [`solarxy_renderer::backend::FrameOutcome`], which answers a
/// different question: that one says whether a progressive backend has
/// converged, this one says how to parameterise the composite that follows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EncodedPane {
    /// Whether this pane rendered a UV layout rather than a 3D scene.
    pub is_uv_map: bool,
    /// Whether the pane had scene content. `false` for a slot with no camera,
    /// whatever [`FrameCtx::scene_present`] said.
    pub scene_present: bool,
}

/// Encode one pane's uniform writes and pass chain into the context's encoder,
/// without compositing or submitting.
///
/// This is the raster pass chain both shells drive, and the body the raster
/// backend adapter wraps. It stops short of the composite for two reasons: the
/// encoder has to be submitted by whoever owns it, and the web shell's
/// offscreen capture runs this identical chain and then composites differently,
/// always clearing, into a full-rect target, and without the selection rim.
pub fn encode_pane_passes(ctx: &mut FrameCtx<'_>, scene: &SceneObjects) -> EncodedPane {
    // Destructured rather than reached through `ctx.`, because the renderer,
    // the encoder and the camera are all borrowed mutably at once and only
    // field-level bindings let the compiler see they are disjoint.
    let FrameCtx {
        device,
        queue,
        renderer,
        encoder,
        rect,
        is_split,
        pds,
        display,
        background,
        camera,
        env,
        bounds,
        grid_plane,
        scene_present,
        content,
        ..
    } = ctx;

    // The 3D scene renders the full pane; the per-pane toolbar labels float on
    // top of it, so no strip is reserved. The floor guards a zero-height pane,
    // which the desktop shell used to let through as a division by zero.
    let pane_aspect = rect.width / rect.height.max(1.0);

    let (is_uv_map, present) = match (&*content, camera.as_deref_mut()) {
        (
            PaneContent::Scene {
                extra,
                selected,
                cam_data,
                shadow,
            },
            Some(camera),
        ) => {
            let objects = &build_draw_list(scene, *extra, *selected);
            camera.write_with_aspect(queue, pane_aspect);
            write_pane_uniforms(
                queue,
                renderer,
                &PaneUniforms {
                    background: *background,
                    pds,
                    display,
                    camera: Some(&*camera),
                    env,
                    bounds: *bounds,
                    grid_plane: *grid_plane,
                },
            );
            if pds.inspection_mode == InspectionMode::Overdraw {
                render_overdraw_pane(
                    renderer,
                    encoder,
                    objects,
                    &camera.bind_group,
                    *rect,
                    *is_split,
                );
            } else {
                render_3d_passes(
                    renderer,
                    queue,
                    encoder,
                    &PaneScene {
                        objects,
                        env,
                        cam_bg: &camera.bind_group,
                        cam_data,
                        pds,
                        background: *background,
                        shadow: *shadow,
                        // Asked of the list rather than of the shell's
                        // selection, because a selection that resolves to
                        // nothing drawable must not switch on the mask and
                        // jump-flood stages for an empty silhouette.
                        selected: objects.iter().any(|o| o.selected),
                    },
                );
            }
            (false, *scene_present)
        }
        (PaneContent::Uv { source }, Some(_)) => {
            let resolved = match source {
                UvSource::External(object) => Some(*object),
                UvSource::Scene { preferred } => preferred
                    .and_then(|id| scene.draw_object(id))
                    .or_else(|| scene.draw_objects().next()),
                UvSource::None => None,
            };
            render_uv_pane(
                &UvPane {
                    device,
                    queue,
                    pane_aspect,
                    pds,
                    display,
                    background: *background,
                    object: resolved.as_ref(),
                },
                renderer,
                encoder,
            );
            (true, *scene_present)
        }
        // No camera in this slot, whatever the pane mode says.
        _ => {
            renderer.render_empty_pass(encoder, *background);
            (false, false)
        }
    };

    EncodedPane {
        is_uv_map,
        scene_present: present,
    }
}

/// Assemble the raster draw list: the host's extra object first, then every
/// visible object the backend owns, with the selection flagged.
///
/// Order is load-bearing, not incidental. Overdraw counts fragments in
/// submission order, and the depth-equal overlays (edge wireframe, validation
/// lines) resolve against whatever landed first, so the host's own object
/// stays ahead of the delta-fed ones exactly as it did when it was the only
/// entry that could come first.
///
/// An empty list is a legitimate frame: the background, grid, floor and axes
/// come from the environment, not from this list.
///
/// This was written twice, once per shell, with the selection loop duplicated
/// character for character. It belongs to whichever component owns the scene,
/// and that is now the backend.
fn build_draw_list<'a>(
    scene: &'a SceneObjects,
    extra: Option<DrawObject<'a>>,
    selected: Option<solarxy_core::scene::SceneObjectId>,
) -> Vec<DrawObject<'a>> {
    let mut objects = Vec::with_capacity(usize::from(extra.is_some()) + scene.len());
    if let Some(extra) = extra {
        objects.push(extra);
    }
    objects.extend(scene.draw_objects());
    // `SceneObjects` hands out its draw objects unselected, so the flag is set
    // here by matching on model identity, which is also why the lookup filters
    // hidden objects: a hidden one is not in this list at all.
    if let Some(id) = selected
        && let Some(selected) = scene.draw_object(id)
    {
        for object in &mut objects {
            if std::ptr::eq(object.model, selected.model) {
                object.selected = true;
            }
        }
    }
    objects
}

/// The UV pane's chain: the UV camera and wireframe writes, the optional
/// overlap count pass and its one-shot statistics readback, then the layout
/// pass itself.
struct UvPane<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    pane_aspect: f32,
    pds: &'a PaneDisplaySettings,
    display: &'a DisplaySettings,
    background: ResolvedBackground,
    object: Option<&'a DrawObject<'a>>,
}

fn render_uv_pane(f: &UvPane<'_>, renderer: &mut Renderer, encoder: &mut wgpu::CommandEncoder) {
    let (device, queue, pane_aspect) = (f.device, f.queue, f.pane_aspect);
    renderer
        .uv_cam
        .write(queue, f.pds.uv_offset, f.pds.uv_zoom, pane_aspect);
    let uv_wire = WireframeParams {
        color: [0.8, 0.8, 0.8, 1.0],
        line_width: f.pds.line_weight.width_px(),
        screen_width: renderer.target_width as f32,
        screen_height: renderer.target_height as f32,
        // The UV pass draws no points, but this write clobbers the shared
        // uniform, so it carries the live size for the next 3D pass rather
        // than a constant that is only right while nothing has changed it.
        point_size: f.display.point_size,
    };
    queue.write_buffer(
        &renderer.wire.wireframe_params_buffer,
        0,
        bytemuck::bytes_of(&uv_wire),
    );
    if f.pds.uv_bg == UvMapBackground::Dark {
        let dark = GradientUniform {
            top_color: [0.10, 0.10, 0.10, 1.0],
            bottom_color: [0.10, 0.10, 0.10, 1.0],
            uv_y_offset: 0.0,
            uv_y_scale: 1.0,
            _pad: [0.0; 2],
        };
        queue.write_buffer(&renderer.wire.gradient_buffer, 0, bytemuck::bytes_of(&dark));
    }
    let stats_needed = f.pds.show_uv_overlap
        && renderer.uv_overlap.stats_dirty
        && !renderer.uv_overlap.readback_pending;

    let Some(object) = f.object.filter(|o| o.model.has_uvs) else {
        renderer.render_empty_pass(encoder, f.background);
        return;
    };

    if f.pds.show_uv_overlap {
        renderer.render_uv_overlap_count_pass(
            encoder,
            object,
            &renderer.uv_cam.bind_group,
            &renderer.uv_overlap.count_view,
        );
        if stats_needed {
            // One-shot statistics render at the identity UV camera, then
            // restore the pane view.
            renderer.uv_cam.write(queue, [0.0, 0.0], 1.0, 1.0);
            renderer.render_uv_overlap_count_pass(
                encoder,
                object,
                &renderer.uv_cam.bind_group,
                &renderer.uv_overlap.stats_view,
            );
            renderer.uv_overlap.request_readback(device, encoder);
            renderer
                .uv_cam
                .write(queue, f.pds.uv_offset, f.pds.uv_zoom, pane_aspect);
        }
    }
    renderer.render_uv_map_pass(encoder, object, &renderer.uv_cam.bind_group, f.pds);
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
