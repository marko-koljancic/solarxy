//! Per-frame draw orchestration: [`Renderer`] (the top-level handle),
//! [`RenderTargets`] (HDR + depth + bloom + SSAO targets), [`PostProcessing`]
//! (bloom/SSAO/tone settings), [`IblResources`], [`WireframeResources`],
//! [`UvOverlapResources`], [`ValidationColorResources`].
//!
//! `Renderer::render_pane` is the per-pane entry point called from
//! `solarxy-app/src/state/render.rs`.

use std::sync::Arc;

use cgmath::prelude::*;

use crate::bind_groups::BindGroupLayouts;
use crate::camera::Camera;

/// Vertex budget for the manipulator's two buffers. The translate gizmo is three
/// shafts, three cones and three quads -- a few hundred vertices -- so a fixed
/// allocation is honest here; `write_manipulator` refuses to overrun it.
const MANIPULATOR_BUF_BYTES: u64 = 64 * 1024;

/// Clips a gizmo vertex list to what [`MANIPULATOR_BUF_BYTES`] can hold, on a
/// whole-primitive boundary (`verts_per_prim` = 2 for lines, 3 for triangles),
/// so a truncated list never leaves a half-primitive behind.
///
/// Overflow means a developer added handles without raising the budget. Clipping
/// keeps the gizmo on screen and visibly wrong; the previous behaviour dropped it
/// entirely, behind a `debug_assert` that does nothing in release.
fn truncate_to_budget<V>(verts: &mut Vec<V>, verts_per_prim: usize, what: &str) {
    let cap = MANIPULATOR_BUF_BYTES as usize / std::mem::size_of::<V>();
    if verts.len() <= cap {
        return;
    }
    let keep = (cap / verts_per_prim) * verts_per_prim;
    tracing::error!(
        wanted = verts.len(),
        kept = keep,
        budget = MANIPULATOR_BUF_BYTES,
        "manipulator {what} vertices exceeded their buffer; clipping. \
         Raise MANIPULATOR_BUF_BYTES."
    );
    verts.truncate(keep);
}
/// Light helpers get their own budget: they are N-per-scene (eight lights, each
/// a wire sphere or a cone) rather than one selection-attached gizmo, so sharing
/// the manipulator's buffer would make one starve the other.
const LIGHT_HELPER_BUF_BYTES: u64 = 256 * 1024;
const CAMERA_HELPER_BUF_BYTES: u64 = 256 * 1024;
use crate::model::{DrawMeshSimple, DrawModel};
use crate::pipelines::Pipelines;
use crate::texture::SharedSamplers;
use crate::uv_camera::UvCameraState;
use solarxy_core::preferences::{
    BgKind, NormalsMode, ResolvedBackground, UvMapBackground, UvMode, ViewMode,
};

use crate::bloom::BloomState;
use crate::composite::CompositeState;
use crate::ibl::{BrdfLut, IblState};
use crate::ssao::SsaoState;
use crate::texture;
use solarxy_core::preferences::{IblMode, ToneMode};

use crate::environment::SceneEnvironment;
use crate::scene::BackgroundModeExt;
use solarxy_core::MeshTopology;
use solarxy_core::view_config::{BoundsMode, PaneDisplaySettings};

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GradientUniform {
    pub top_color: [f32; 4],
    pub bottom_color: [f32; 4],
    pub uv_y_offset: f32,
    pub uv_y_scale: f32,
    pub _pad: [f32; 2],
}

const _: () = assert!(std::mem::size_of::<GradientUniform>() == 48);

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireframeParams {
    pub color: [f32; 4],
    pub line_width: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    /// On-screen point size in pixels, for the shader-expanded point quads.
    ///
    /// Rides this uniform rather than getting one of its own because the
    /// points pipeline already binds it for the viewport size, and because
    /// the slot was a pad: the struct is the same 32 bytes it always was and
    /// the size assert below did not move.
    pub point_size: f32,
}

const _: () = assert!(std::mem::size_of::<WireframeParams>() == 32);

pub struct RenderTargets {
    pub depth_texture: texture::Texture,
    pub msaa_hdr_view: wgpu::TextureView,
    pub _hdr_resolve_texture: wgpu::Texture,
    pub hdr_resolve_view: wgpu::TextureView,
}

pub struct PostProcessing {
    pub bloom: BloomState,
    pub bloom_enabled: bool,
    pub ssao: SsaoState,
    pub ssao_enabled: bool,
    pub composite: CompositeState,
    /// The colour-grading tables the composite pass samples. They live
    /// beside `composite` because it is their only consumer.
    pub luts: crate::lut::LutSlots,
    pub tone_mode: ToneMode,
    pub exposure: f32,
}

pub struct IblResources {
    pub ibl: IblState,
    pub ibl_fallback: IblState,
    pub brdf_lut: BrdfLut,
    /// The rect-area light tables. Not image-based lighting, but they sit
    /// here because this is the bundle the light bind group is built from
    /// and both are shading lookup tables uploaded once at startup;
    /// splitting them out would mean threading a second reference through
    /// the same dozen call sites for no gain.
    pub ltc: crate::ltc::LtcLuts,
    pub ibl_mode: IblMode,
    pub last_active_ibl_mode: IblMode,
}

pub struct WireframeResources {
    /// The sky gradient the background pass reads. Written from outside this
    /// crate, by the shared host's per-pane uniform write, which is why it
    /// carries no underscore: the prefix said "kept alive, never touched" and
    /// that stopped being true when the pane path moved to `solarxy-host`.
    pub gradient_buffer: wgpu::Buffer,
    pub gradient_bind_group: wgpu::BindGroup,
    pub wireframe_params_buffer: wgpu::Buffer,
    pub wireframe_params_bind_group: wgpu::BindGroup,
    /// Genuinely a keep-alive: the bind group below borrows it and nothing
    /// else ever names it. The underscore stays for that reason.
    pub _checker_texture: texture::Texture,
    pub uv_checker_bind_group: wgpu::BindGroup,
}

pub struct UvOverlapResources {
    pub count_texture: wgpu::Texture,
    pub count_view: wgpu::TextureView,
    pub overlay_bind_group: wgpu::BindGroup,
    pub sampler: wgpu::Sampler,
    pub stats_texture: wgpu::Texture,
    pub stats_view: wgpu::TextureView,
    pub overlap_pct: Option<f32>,
    pub stats_dirty: bool,
    pub staging_buffer: Option<wgpu::Buffer>,
    pub readback_pending: bool,
    /// `Some` once `map_async` has been requested on `staging_buffer`;
    /// polled non-blocking each frame (blocking waits do not exist on
    /// WebGPU, and skipping them avoids a desktop frame hitch too).
    pub map_receiver: Option<std::sync::mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>>,
}

impl UvOverlapResources {
    /// Arms a GPU-to-CPU readback of the 512x512 overlap stats texture:
    /// copies it into a fresh staging buffer inside `encoder` and marks the
    /// readback pending. Poll completion with [`UvOverlapResources::poll_readback`]
    /// on later frames (blocking waits do not exist on WebGPU).
    pub fn request_readback(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder) {
        const STATS_SIZE: u32 = 512;
        let bytes_per_row = STATS_SIZE;
        let buffer_size = u64::from(bytes_per_row * STATS_SIZE);
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("UV Overlap Readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.stats_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(STATS_SIZE),
                },
            },
            wgpu::Extent3d {
                width: STATS_SIZE,
                height: STATS_SIZE,
                depth_or_array_layers: 1,
            },
        );
        self.staging_buffer = Some(staging);
        self.readback_pending = true;
        self.map_receiver = None;
        self.stats_dirty = false;
    }

    /// Pumps an armed readback without blocking: requests the async map
    /// once, then checks completion each call. Returns `true` when
    /// `overlap_pct` was updated this call (shared by the desktop frame
    /// tick and the web host, which forwards the change as a host event).
    pub fn poll_readback(&mut self, device: &wgpu::Device) -> bool {
        if !self.readback_pending {
            return false;
        }

        // Arm once: request the async map on the staged buffer.
        if self.map_receiver.is_none() {
            let Some(buf) = &self.staging_buffer else {
                self.readback_pending = false;
                return false;
            };
            let (tx, rx) = std::sync::mpsc::channel();
            buf.slice(..).map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
            self.map_receiver = Some(rx);
        }

        // Pump the device without blocking, then check for completion.
        let _ = device.poll(wgpu::PollType::Poll);
        let ready = match &self.map_receiver {
            Some(rx) => match rx.try_recv() {
                Ok(Ok(())) => true,
                // Not resolved yet; try again next frame.
                Err(std::sync::mpsc::TryRecvError::Empty) => return false,
                // Map failed or the sender vanished: abandon this readback.
                Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    tracing::error!("UV overlap readback map failed");
                    false
                }
            },
            None => false,
        };

        self.map_receiver = None;
        self.readback_pending = false;
        let Some(buf) = self.staging_buffer.take() else {
            return false;
        };
        if !ready {
            return false;
        }

        let slice = buf.slice(..);
        let data = slice.get_mapped_range();
        let mut total_nonzero = 0u64;
        let mut overlap = 0u64;
        for &byte in data.iter() {
            if byte > 0 {
                total_nonzero += 1;
            }
            if byte > 1 {
                overlap += 1;
            }
        }
        drop(data);
        buf.unmap();
        self.overlap_pct = if total_nonzero > 0 {
            Some(overlap as f32 / total_nonzero as f32 * 100.0)
        } else {
            Some(0.0)
        };
        true
    }
}

pub struct ValidationColorResources {
    pub bind_groups: Vec<wgpu::BindGroup>,
    #[allow(dead_code)]
    pub buffers: Vec<wgpu::Buffer>,
    /// The selection tint (accent blue, translucent) drawn over the picked
    /// object's meshes.
    pub selection_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    pub selection_buffer: wgpu::Buffer,
}

/// Per-object validation overlay GPU resources: the per-mesh issue
/// category (index into [`ValidationColorResources`]) and the
/// non-manifold edge index buffers, both parallel to the object's
/// `model.meshes`. Built by `LoadedModel::load` on desktop and by
/// `SceneObjects` from `SceneOp::SetValidation` on the web.
pub struct ObjectValidationGpu {
    pub mesh_cat: Vec<Option<usize>>,
    pub edge_buffers: Vec<Option<(wgpu::Buffer, u32)>>,
}

/// One drawable object as the geometry passes see it: a
/// [`crate::model::Model`] plus its per-object instance buffer (one
/// `InstanceRaw`) and optional validation overlay resources. The
/// multi-object draw path iterates slices of these; `ModelScene`
/// contributes itself as one entry and `scene_objects:SceneObjects`
/// entries append beside it.
#[derive(Clone, Copy)]
pub struct DrawObject<'a> {
    pub model: &'a crate::model::Model,
    pub instance_buffer: &'a wgpu::Buffer,
    /// Validation overlay resources, or `None` when the object has no
    /// report (the overlay pass skips it).
    pub validation: Option<&'a ObjectValidationGpu>,
    /// Whether the object carries the selection highlight (a translucent
    /// accent tint drawn at the end of the main pass; the web picking-sync
    /// Desktop passes `false`.
    pub selected: bool,
    /// Whether the object is drawn into the shadow map (per-object
    /// participation; which light owns the map is the light-side
    /// exclusive-caster rule). Desktop passes `true`.
    pub cast_shadow: bool,
}

impl<'a> DrawObject<'a> {
    /// Draw one of this object's meshes across every placement it carries.
    ///
    /// **Every per-object indexed draw in every pass goes through here.**
    /// The instance range used to be a literal `0..1` written out ten
    /// times across the shadow, gbuffer, main, outline, wireframe,
    /// selection and validation passes. Threading a count through ten
    /// literals means a scatter that draws but casts no shadow, or one
    /// the validation overlay cannot see, is one forgotten edit away.
    /// Here it is one edit, or none.
    ///
    /// Placements are per mesh, so this binds instance buffer slot 1 from
    /// the mesh's own offset. The passes that deliberately draw a single
    /// copy (UV layout, overlap counting, the ghosted fill) bind the whole
    /// buffer themselves and draw `0..1` from row zero.
    ///
    /// The caller still binds vertex buffer 0 and its own bind groups:
    /// those genuinely differ per pass. The index buffer and the draw do
    /// not, so they live here.
    ///
    /// A mesh with no placements draws nothing, and returning before the
    /// bind is what makes that safe rather than merely pointless: its
    /// offset is the end of the object's buffer whenever it is the last
    /// mesh, and slicing a buffer from its end is a wgpu panic, not an
    /// empty slice. Reachable by merging a copy onto a populated point
    /// cloud with a copy onto an empty one.
    pub fn draw_mesh(&self, pass: &mut wgpu::RenderPass<'a>, mesh: &'a crate::model::Mesh) {
        if mesh.instance_count == 0 {
            return;
        }
        pass.set_vertex_buffer(1, self.instance_buffer.slice(mesh.instance_offset..));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.num_elements, 0, 0..mesh.instance_count);
    }

    /// Draw one of this object's point meshes across every instance.
    ///
    /// Points expand to six quad corners each in the vertex shader rather
    /// than being indexed, so they take their own call; the instance
    /// range is the same question and gets the same answer.
    pub fn draw_points(&self, pass: &mut wgpu::RenderPass<'a>, mesh: &'a crate::model::Mesh) {
        if mesh.instance_count == 0 {
            return;
        }
        pass.set_vertex_buffer(1, self.instance_buffer.slice(mesh.instance_offset..));
        pass.draw(0..mesh.num_vertices * 6, 0..mesh.instance_count);
    }
}

pub struct Renderer {
    pub targets: RenderTargets,
    pub post: PostProcessing,
    pub ibl_res: IblResources,
    pub wire: WireframeResources,
    pub layouts: Arc<BindGroupLayouts>,
    pub pipelines: Pipelines,
    pub uv_cam: UvCameraState,
    pub uv_boundary_buf: wgpu::Buffer,
    /// The transform manipulator this frame, or `None` when no tool is active.
    /// Pull-based: the host sets it, the renderer draws it. `solarxy-app` never
    /// calls `set_manipulator`, so the desktop is unaffected.
    manipulator: Option<crate::manipulator::ManipulatorState>,
    /// Growable CPU-fed vertex buffers for the manipulator. They live HERE and
    /// not in `VisualizationState`, which is destroyed and rebuilt whenever the
    /// scene's bounds move materially -- a gizmo must survive that.
    manipulator_line_buf: wgpu::Buffer,
    manipulator_line_count: u32,
    light_helper_buf: wgpu::Buffer,
    light_helper_count: u32,
    /// Camera gizmos. Written PER PANE (not once per frame like the light
    /// helpers), because each pane hides the camera it is looking through.
    camera_helper_buf: wgpu::Buffer,
    camera_helper_count: u32,
    manipulator_tri_buf: wgpu::Buffer,
    manipulator_tri_count: u32,
    /// The GPU attribute-label channel (host-fed like the manipulator; the
    /// desktop never populates it, so it draws nothing there). Lives here
    /// and not in `VisualizationState` for the same reason the manipulator
    /// buffers do: the atlas and grown buffers must survive the
    /// bounds-driven visualization rebuilds.
    pub labels: crate::labels::LabelResources,
    pub uv_overlap: UvOverlapResources,
    pub validation_colors: ValidationColorResources,
    pub overdraw: crate::overdraw::OverdrawResources,
    /// Selection-outline resources; drawn only when
    /// `selection_style` is `Outline` and something is selected.
    pub outline: crate::outline::OutlineState,
    /// How selection presents in the viewport: the jump-flood rim
    /// (default), the legacy translucent tint, or nothing. A user
    /// preference plumbed by the host via `set_selection_highlight`.
    pub selection_style: SelectionStyle,
    /// Bind group for the HDRI skybox pass — `Some` only while an HDRI is
    /// loaded. Rebuilt through the app's `rebuild_light_bind_group`
    /// IBL chokepoint.
    pub skybox_bind_group: Option<wgpu::BindGroup>,
    #[allow(unused)]
    pub shared_samplers: SharedSamplers,
    pub msaa_sample_count: u32,
    pub target_width: u32,
    pub target_height: u32,
}

/// How a selected object is highlighted in the viewport (a user
/// preference).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionStyle {
    /// Constant-width jump-flood rim around the screen-space silhouette.
    #[default]
    Outline,
    /// The legacy translucent accent fill over the meshes, from before the
    /// jump-flood outline existed.
    Tint,
    /// No viewport highlight.
    None,
}

/// Startup inputs for [`Renderer::new`] the shell owns: preference-derived
/// toggles, background-derived colors, and the UV-checker texture bytes.
/// Everything else (targets, pipelines, IBL, post-FX state) is built inside.
pub struct RendererInit<'a> {
    pub msaa_sample_count: u32,
    /// Initial gradient-pass colors (the shell's background policy).
    pub gradient_top: [f32; 4],
    pub gradient_bottom: [f32; 4],
    /// Sky colors feeding the initial `IblState::from_sky_colors`.
    pub sky_top: [f32; 3],
    pub sky_bottom: [f32; 3],
    pub wireframe_color: [f32; 4],
    pub wireframe_line_width: f32,
    pub bloom_enabled: bool,
    pub ssao_enabled: bool,
    pub tone_mode: ToneMode,
    pub exposure: f32,
    pub ibl_mode: IblMode,
    /// PNG bytes for the UV-checker texture (an asset the shell ships).
    pub uv_checker_png: &'a [u8],
}

impl Renderer {
    /// Resizes every render target that follows the drawn area.
    ///
    /// The drawn area is not the surface: it is the largest pane of the current
    /// layout, and during a still render it is one tile. Both shells and the
    /// still job need this, and all three had their own copy of it until the
    /// tiled still made a third; what stays in a shell is the policy around it,
    /// such as the desktop marking its overlap statistics stale.
    ///
    /// Returns whether anything was reallocated, so a caller can skip the work
    /// that only matters when it was. A no-op resize is the common case: a job
    /// calls this once per tile and the tiles are almost all the same size.
    pub fn resize_targets(&mut self, device: &wgpu::Device, width: u32, height: u32) -> bool {
        if width == 0 || height == 0 {
            return false;
        }
        if width == self.target_width && height == self.target_height {
            return false;
        }
        self.target_width = width;
        self.target_height = height;
        self.targets.depth_texture = crate::texture::Texture::create_depth_texture(
            device,
            width,
            height,
            "depth_texture",
            self.msaa_sample_count,
        );
        self.targets.msaa_hdr_view =
            crate::texture::create_msaa_hdr_texture(device, width, height, self.msaa_sample_count);
        let (hdr_tex, hdr_view) = crate::texture::create_hdr_resolve_texture(device, width, height);
        self.targets._hdr_resolve_texture = hdr_tex;
        self.targets.hdr_resolve_view = hdr_view;
        self.post.bloom.resize(
            device,
            &self.layouts,
            &self.targets.hdr_resolve_view,
            width,
            height,
        );
        self.post.composite.rebuild_bind_group(
            device,
            &self.layouts,
            &self.targets.hdr_resolve_view,
            &self.post.bloom.ping_view,
            &self.post.bloom.sampler,
            &self.post.luts,
        );
        let (ct, cv) = crate::texture::create_overlap_count_texture(device, width, height, false);
        self.uv_overlap.count_texture = ct;
        self.uv_overlap.count_view = cv;
        self.uv_overlap.overlay_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UV Overlap Overlay Bind Group"),
            layout: &self.layouts.uv_overlap_read,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.uv_overlap.count_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.uv_overlap.sampler),
                },
            ],
        });
        self.post.ssao.resize(device, &self.layouts, width, height);
        self.overdraw.resize(device, &self.layouts, width, height);
        let layouts = Arc::clone(&self.layouts);
        self.outline.resize(device, &layouts, width, height);
        true
    }

    /// Build the full renderer: bind-group layouts, every pipeline, render
    /// targets, IBL (BRDF LUT + sky convolution), post-FX state, UV/overlap
    /// resources, validation colors, and overdraw resources. Pure code
    /// motion from the desktop shell's former field-by-field assembly.
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        config: &wgpu::SurfaceConfiguration,
        init: &RendererInit<'_>,
    ) -> Result<Self, crate::error::RendererError> {
        use wgpu::util::DeviceExt;

        let width = config.width;
        let height = config.height;
        let msaa_sample_count = init.msaa_sample_count;

        let depth_texture = texture::Texture::create_depth_texture(
            device,
            width,
            height,
            "depth_texture",
            msaa_sample_count,
        );
        let msaa_hdr_view =
            texture::create_msaa_hdr_texture(device, width, height, msaa_sample_count);
        let (hdr_resolve_texture, hdr_resolve_view) =
            texture::create_hdr_resolve_texture(device, width, height);
        let layouts = Arc::new(BindGroupLayouts::new(device));
        let pipelines = Pipelines::new(device, config, &layouts, msaa_sample_count);

        let gradient_uniform = GradientUniform {
            top_color: init.gradient_top,
            bottom_color: init.gradient_bottom,
            uv_y_offset: 0.0,
            uv_y_scale: 1.0,
            _pad: [0.0; 2],
        };
        let gradient_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Gradient Uniform"),
            contents: bytemuck::bytes_of(&gradient_uniform),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let gradient_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Gradient Bind Group"),
            layout: &layouts.background,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: gradient_buffer.as_entire_binding(),
            }],
        });

        let brdf_lut = BrdfLut::generate(device, queue);
        let ltc = crate::ltc::LtcLuts::load(device, queue);
        let ibl = IblState::from_sky_colors(device, queue, init.sky_top, init.sky_bottom);
        let ibl_fallback = IblState::fallback(device, queue);

        let wireframe_params_data = WireframeParams {
            color: init.wireframe_color,
            line_width: init.wireframe_line_width,
            screen_width: width as f32,
            screen_height: height as f32,
            point_size: solarxy_core::view_config::DEFAULT_POINT_SIZE,
        };
        let wireframe_params_buffer =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Wireframe Params Uniform"),
                contents: bytemuck::bytes_of(&wireframe_params_data),
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            });
        let wireframe_params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Wireframe Params Bind Group"),
            layout: &layouts.wireframe_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wireframe_params_buffer.as_entire_binding(),
            }],
        });

        let labels = crate::labels::LabelResources::new(device, queue, &layouts.labels);

        let shared_samplers = SharedSamplers::new(device);

        let checker_texture = texture::Texture::from_bytes(
            device,
            queue,
            init.uv_checker_png,
            "uv_checker_texture",
            texture::TextureOpts::flat(false),
        )?;
        let uv_checker_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UV Checker Bind Group"),
            layout: &layouts.uv_checker,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&checker_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&shared_samplers.linear_repeat),
                },
            ],
        });

        let bloom = BloomState::new(
            device,
            &layouts,
            &hdr_resolve_view,
            shared_samplers.linear_clamp.clone(),
            width,
            height,
        );

        let luts = crate::lut::LutSlots::new(device, queue, &shared_samplers.linear_clamp);

        let composite = CompositeState::new(
            device,
            &layouts,
            &hdr_resolve_view,
            &bloom.ping_view,
            &bloom.sampler,
            &luts,
            init.bloom_enabled,
            init.ssao_enabled,
            init.tone_mode,
            init.exposure,
        );

        let ssao = SsaoState::new(device, queue, &layouts, width, height);

        let uv_cam = UvCameraState::new(device, &layouts.camera);

        let yellow = [1.0, 0.85, 0.0];
        let boundary_verts: [crate::model::GizmoVertex; 8] = [
            crate::model::GizmoVertex {
                position: [0.0, 1.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [1.0, 1.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [1.0, 1.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [1.0, 0.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [1.0, 0.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: yellow,
            },
            crate::model::GizmoVertex {
                position: [0.0, 1.0, 0.0],
                color: yellow,
            },
        ];
        let uv_boundary_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("UV Boundary Buffer"),
            contents: bytemuck::cast_slice(&boundary_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let (count_tex, count_view) =
            texture::create_overlap_count_texture(device, width, height, false);
        let (stats_tex, stats_view) = texture::create_overlap_count_texture(device, 512, 512, true);
        let overlap_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("UV Overlap Sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });
        let overlap_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("UV Overlap Overlay Bind Group"),
            layout: &layouts.uv_overlap_read,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&count_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&overlap_sampler),
                },
            ],
        });

        let validation_colors = {
            use crate::validation::IssueCategory;
            let mut buffers = Vec::new();
            let mut bind_groups = Vec::new();
            for cat in IssueCategory::ALL {
                let color = cat.color();
                let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(&format!("Validation Color {cat:?}")),
                    contents: bytemuck::cast_slice(&color),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some(&format!("Validation Color BG {cat:?}")),
                    layout: &layouts.validation_color,
                    entries: &[wgpu::BindGroupEntry {
                        binding: 0,
                        resource: buf.as_entire_binding(),
                    }],
                });
                buffers.push(buf);
                bind_groups.push(bg);
            }
            let selection_color: [f32; 4] = [0.29, 0.565, 0.886, 0.35];
            let selection_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Selection Tint Color"),
                contents: bytemuck::cast_slice(&selection_color),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let selection_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Selection Tint BG"),
                layout: &layouts.validation_color,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: selection_buffer.as_entire_binding(),
                }],
            });
            ValidationColorResources {
                bind_groups,
                buffers,
                selection_bind_group,
                selection_buffer,
            }
        };

        let overdraw = crate::overdraw::OverdrawResources::new(device, &layouts, width, height);
        let outline = crate::outline::OutlineState::new(device, &layouts, width, height);

        Ok(Self {
            targets: RenderTargets {
                depth_texture,
                msaa_hdr_view,
                _hdr_resolve_texture: hdr_resolve_texture,
                hdr_resolve_view,
            },
            post: PostProcessing {
                bloom,
                bloom_enabled: init.bloom_enabled,
                ssao,
                ssao_enabled: init.ssao_enabled,
                composite,
                luts,
                tone_mode: init.tone_mode,
                exposure: init.exposure,
            },
            ibl_res: IblResources {
                ibl,
                ibl_fallback,
                brdf_lut,
                ltc,
                ibl_mode: init.ibl_mode,
                last_active_ibl_mode: match init.ibl_mode {
                    IblMode::Off => IblMode::Full,
                    other => other,
                },
            },
            wire: WireframeResources {
                gradient_buffer,
                gradient_bind_group,
                wireframe_params_buffer,
                wireframe_params_bind_group,
                _checker_texture: checker_texture,
                uv_checker_bind_group,
            },
            layouts,
            pipelines,
            uv_cam,
            uv_boundary_buf,
            manipulator: None,
            manipulator_line_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Manipulator Lines"),
                size: MANIPULATOR_BUF_BYTES,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            manipulator_line_count: 0,
            manipulator_tri_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Manipulator Tris"),
                size: MANIPULATOR_BUF_BYTES,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            manipulator_tri_count: 0,
            labels,
            light_helper_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Light Helpers"),
                size: LIGHT_HELPER_BUF_BYTES,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            light_helper_count: 0,
            camera_helper_buf: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Camera Helpers"),
                size: CAMERA_HELPER_BUF_BYTES,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            camera_helper_count: 0,
            uv_overlap: UvOverlapResources {
                count_texture: count_tex,
                count_view,
                overlay_bind_group: overlap_bind_group,
                sampler: overlap_sampler,
                stats_texture: stats_tex,
                stats_view,
                overlap_pct: None,
                stats_dirty: false,
                staging_buffer: None,
                readback_pending: false,
                map_receiver: None,
            },
            validation_colors,
            overdraw,
            outline,
            selection_style: SelectionStyle::default(),
            skybox_bind_group: None,
            shared_samplers,
            msaa_sample_count,
            target_width: width,
            target_height: height,
        })
    }

    pub fn draw_background_gradient<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipelines.overlay.background);
        pass.set_bind_group(0, &self.wire.gradient_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Draw the HDRI equirect as a fullscreen sky. No-op when no HDRI is
    /// loaded (`skybox_bind_group` is `None`) — the pass clear colour then
    /// shows through instead.
    pub fn draw_skybox<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, cam_bg: &'a wgpu::BindGroup) {
        let Some(skybox_bg) = &self.skybox_bind_group else {
            return;
        };
        pass.set_pipeline(&self.pipelines.overlay.skybox);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, skybox_bg, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn render_empty_pass(&self, encoder: &mut wgpu::CommandEncoder, bg: ResolvedBackground) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Empty Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(bg.clear_color()),
                    store: wgpu::StoreOp::Discard,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        self.draw_background_gradient(&mut pass);
    }

    pub fn render_gbuffer_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        objects: &[DrawObject<'_>],
        cam_bg: &wgpu::BindGroup,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("G-Buffer Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.post.ssao.gbuffer_normal_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.5,
                        g: 0.5,
                        b: 1.0,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.post.ssao.gbuffer_depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.scene.gbuffer);
        pass.set_bind_group(0, cam_bg, &[]);
        for obj in objects {
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for mesh in &obj.model.meshes {
                if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                    continue;
                }
                let material = &obj.model.materials[mesh.material];
                if material.uniform.alpha_mode == 2 {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                obj.draw_mesh(&mut pass, mesh);
            }
        }
    }

    pub fn render_ssao_passes(&self, encoder: &mut wgpu::CommandEncoder, cam_bg: &wgpu::BindGroup) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post.ssao.ssao_raw_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.post.ssao);
            pass.set_bind_group(0, &self.post.ssao.ssao_bind_group, &[]);
            pass.set_bind_group(1, cam_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Blur H Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post.ssao.ssao_blur_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.post.ssao_blur_h);
            pass.set_bind_group(0, &self.post.ssao.blur_h_bind_group, &[]);
            pass.set_bind_group(1, cam_bg, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SSAO Blur V Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.post.ssao.ssao_output_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.post.ssao_blur_v);
            pass.set_bind_group(0, &self.post.ssao.blur_v_bind_group, &[]);
            pass.set_bind_group(1, cam_bg, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Overdraw heat map — count + show. Replaces the main pass entirely for
    /// the active pane when `InspectionMode::Overdraw` is selected. Composite
    /// short-circuits with `inspection_mode == 4u` so the heatmap is presented
    /// untouched (no tone mapping, no bloom, no SSAO multiplication).
    ///
    /// `pane_viewport` is `[x, y, w, h]` in physical pixels, or `None` for
    /// single-pane mode (whole window).
    pub fn render_overdraw_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        objects: &[DrawObject<'_>],
        cam_bg: &wgpu::BindGroup,
        pane_viewport: Option<[f32; 4]>,
    ) {
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Overdraw Count Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.overdraw.count_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            if let Some([x, y, w, h]) = pane_viewport {
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
                pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
            }
            pass.set_pipeline(&self.pipelines.inspection.overdraw_count);
            pass.set_bind_group(0, cam_bg, &[]);
            for obj in objects {
                pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                for mesh in &obj.model.meshes {
                    if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                        continue;
                    }
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    // The UV passes draw the prototype's layout, never its
                    // placements: a UV map has one copy however many times
                    // the geometry is placed in the world, and the overlap
                    // counter would multiply every count by the instance
                    // number if this drew them all.
                    pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
                }
            }
        }

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Overdraw Show Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.targets.hdr_resolve_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            if let Some([x, y, w, h]) = pane_viewport {
                pass.set_viewport(x, y, w, h, 0.0, 1.0);
                pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
            }
            pass.set_pipeline(&self.pipelines.inspection.overdraw_show);
            pass.set_bind_group(0, &self.overdraw.show_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub fn render_shadow_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        env: &SceneEnvironment,
        objects: &[DrawObject<'_>],
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shadow Pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &env.shadow.texture_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.scene.shadow);
        pass.set_bind_group(0, &env.shadow.pass_bind_group, &[]);
        for obj in objects {
            if !obj.cast_shadow {
                continue;
            }
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for mesh in &obj.model.meshes {
                if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                    continue;
                }
                let material = &obj.model.materials[mesh.material];
                if material.uniform.alpha_mode == 2 {
                    continue;
                }
                pass.set_bind_group(1, &material.bind_group, &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                obj.draw_mesh(&mut pass, mesh);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_main_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        env: &SceneEnvironment,
        objects: &[DrawObject<'_>],
        cam_bg: &wgpu::BindGroup,
        cam: &Camera,
        pds: &PaneDisplaySettings,
        bg: ResolvedBackground,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(bg.clear_color()),
                    store: wgpu::StoreOp::Discard,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        match bg.kind {
            BgKind::Gradient => self.draw_background_gradient(&mut pass),
            BgKind::Hdri => self.draw_skybox(&mut pass, cam_bg),
            BgKind::Solid => {}
        }

        // Scene-level draws (floor, overlays) bind the environment's own
        // instance buffer; per-object loops rebind slot 1 as they go.
        pass.set_vertex_buffer(1, env.instance_buffer.slice(..));

        if pds.uv_mode == UvMode::Off {
            match pds.view_mode {
                ViewMode::Shaded | ViewMode::ShadedWireframe => {
                    self.draw_opaque_meshes(&mut pass, env, objects, cam_bg);
                    // Floor relies on slot 1 = the env instance buffer.
                    pass.set_vertex_buffer(1, env.instance_buffer.slice(..));
                    self.draw_floor(&mut pass, env, cam_bg);
                    if pds.view_mode == ViewMode::ShadedWireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            objects,
                            &self.pipelines.scene.edge_wire,
                            cam_bg,
                        );
                    }
                    self.draw_blend_meshes(&mut pass, env, objects, cam_bg, cam);
                }
                ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(
                        &mut pass,
                        objects,
                        &self.pipelines.scene.edge_wire,
                        cam_bg,
                    );
                }
                ViewMode::Ghosted => {
                    pass.set_pipeline(&self.pipelines.scene.ghosted_fill);
                    pass.set_bind_group(0, cam_bg, &[]);
                    for obj in objects {
                        pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                        pass.draw_model_simple(obj.model, 0..1);
                    }
                    if pds.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            objects,
                            &self.pipelines.scene.edge_wire_ghosted,
                            cam_bg,
                        );
                    }
                }
            }
        } else {
            pass.set_bind_group(0, cam_bg, &[]);
            for obj in objects {
                pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                if obj.model.has_uvs {
                    match pds.uv_mode {
                        UvMode::Checker => {
                            pass.set_pipeline(&self.pipelines.uv.uv_checker);
                            pass.set_bind_group(1, &self.wire.uv_checker_bind_group, &[]);
                        }
                        UvMode::Gradient | UvMode::Off => {
                            pass.set_pipeline(&self.pipelines.uv.uv_gradient);
                        }
                    }
                } else {
                    pass.set_pipeline(&self.pipelines.uv.uv_no_uvs);
                }
                pass.draw_model_simple(obj.model, 0..1);
            }

            match pds.view_mode {
                ViewMode::Shaded => {}
                ViewMode::ShadedWireframe | ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(
                        &mut pass,
                        objects,
                        &self.pipelines.scene.edge_wire,
                        cam_bg,
                    );
                }
                ViewMode::Ghosted => {
                    if pds.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            objects,
                            &self.pipelines.scene.edge_wire_ghosted,
                            cam_bg,
                        );
                    }
                }
            }
        }

        // Line and point meshes draw unlit through their own pipelines in
        // every view mode: they have no shaded, ghosted, or UV-inspection
        // variants in v1, and hiding them per mode would read as data loss.
        self.draw_topology_meshes(&mut pass, objects, cam_bg);

        // Overlays below rely on scene-level bindings.
        pass.set_vertex_buffer(1, env.instance_buffer.slice(..));

        if pds.show_grid {
            pass.set_pipeline(&self.pipelines.overlay.grid);
            pass.set_bind_group(0, cam_bg, &[]);
            pass.set_bind_group(1, &env.vis.grid_params_bind_group, &[]);
            pass.set_vertex_buffer(0, env.vis.grid_mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(
                env.vis.grid_mesh.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..env.vis.grid_mesh.num_elements, 0, 0..1);
        }
        self.draw_normals(&mut pass, env, objects, cam_bg, pds);
        self.draw_attr_vectors(&mut pass, env, cam_bg);
        self.draw_attr_labels(&mut pass, cam_bg, pds);
        self.draw_axes(&mut pass, env, cam_bg, pds);
        self.draw_local_axes(&mut pass, env, cam_bg, pds);
        self.draw_bounds(&mut pass, env, cam_bg, pds);
        if pds.show_validation {
            self.draw_validation_overlay(&mut pass, objects, cam_bg);
        }
        if self.selection_style == SelectionStyle::Tint && objects.iter().any(|o| o.selected) {
            self.draw_selection_tint(&mut pass, objects, cam_bg);
        }
        // Last, so it sits over everything; its pipelines ignore depth anyway.
        self.draw_light_helpers(&mut pass, cam_bg);
        self.draw_camera_helpers(&mut pass, cam_bg);
        self.draw_manipulator(&mut pass, cam_bg);
    }

    /// Draws the transform manipulator, if the host set one this frame.
    fn draw_manipulator<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        if self.manipulator.is_none() {
            return;
        }
        pass.set_bind_group(0, cam_bg, &[]);
        if self.manipulator_tri_count > 0 {
            pass.set_pipeline(&self.pipelines.overlay.manipulator_tris);
            pass.set_vertex_buffer(0, self.manipulator_tri_buf.slice(..));
            pass.draw(0..self.manipulator_tri_count, 0..1);
        }
        if self.manipulator_line_count > 0 {
            pass.set_pipeline(&self.pipelines.overlay.manipulator_lines);
            pass.set_vertex_buffer(0, self.manipulator_line_buf.slice(..));
            pass.draw(0..self.manipulator_line_count, 0..1);
        }
    }

    /// Draws the light helpers. Its own draw, on its own buffer, because helpers
    /// are shown per their light's `show_helper` param and have nothing to do
    /// with whether a manipulator is up.
    fn draw_light_helpers<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        if self.light_helper_count == 0 {
            return;
        }
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_pipeline(&self.pipelines.overlay.manipulator_lines);
        pass.set_vertex_buffer(0, self.light_helper_buf.slice(..));
        pass.draw(0..self.light_helper_count, 0..1);
    }

    /// Draws the camera gizmos. Its own draw/buffer, like the light helpers,
    /// but written per pane (each pane hides its own look-through camera).
    fn draw_camera_helpers<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        if self.camera_helper_count == 0 {
            return;
        }
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_pipeline(&self.pipelines.overlay.manipulator_lines);
        pass.set_vertex_buffer(0, self.camera_helper_buf.slice(..));
        pass.draw(0..self.camera_helper_count, 0..1);
    }

    /// Uploads this pane's camera gizmos. Written PER PANE with the pane's
    /// look-through camera as `skip`, so you never see the camera you are
    /// inside. Call before rendering each 3D pane.
    pub fn write_camera_helpers(
        &mut self,
        queue: &wgpu::Queue,
        cameras: &[solarxy_core::scene::CameraDef],
        skip: Option<solarxy_core::scene::SceneObjectId>,
    ) {
        let lines = crate::helpers::build_camera_helpers(cameras, skip);
        if lines.is_empty() {
            self.camera_helper_count = 0;
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&lines);
        if bytes.len() as u64 > CAMERA_HELPER_BUF_BYTES {
            tracing::warn!(
                bytes = bytes.len(),
                budget = CAMERA_HELPER_BUF_BYTES,
                "camera helpers exceeded their vertex buffer; not drawing"
            );
            self.camera_helper_count = 0;
            return;
        }
        queue.write_buffer(&self.camera_helper_buf, 0, bytes);
        self.camera_helper_count = u32::try_from(lines.len()).unwrap_or(0);
    }

    /// Uploads this frame's light helpers. Unlike the manipulator, these are
    /// sized in WORLD units, so they do NOT depend on the pane: one write per
    /// frame, not one per pane.
    pub fn write_light_helpers(
        &mut self,
        queue: &wgpu::Queue,
        lights: &[solarxy_core::scene::LightDef],
    ) {
        let lines = crate::helpers::build_light_helpers(lights);
        if lines.is_empty() {
            self.light_helper_count = 0;
            return;
        }
        let bytes: &[u8] = bytemuck::cast_slice(&lines);
        if bytes.len() as u64 > LIGHT_HELPER_BUF_BYTES {
            // Loud, not silent: a vanished helper with no explanation is exactly
            // the kind of thing that costs an afternoon.
            tracing::warn!(
                bytes = bytes.len(),
                budget = LIGHT_HELPER_BUF_BYTES,
                "light helpers exceeded their vertex buffer; not drawing"
            );
            self.light_helper_count = 0;
            return;
        }
        queue.write_buffer(&self.light_helper_buf, 0, bytes);
        self.light_helper_count = u32::try_from(lines.len()).unwrap_or(0);
    }

    /// Points a colour-grading slot at a table, or clears it with `None`.
    ///
    /// **The single chokepoint**, in the same spirit as the app's
    /// `rebuild_light_bind_group`: a bind group captures the texture views
    /// it was built from, so changing which table a slot binds without
    /// rebuilding leaves the previous one on screen. Uploading and
    /// rebuilding therefore happen here together rather than being two
    /// things a host has to remember to pair.
    ///
    /// Deduped on the table's content hash, so a host may call this every
    /// frame with the same table (which the scene delta encourages, since
    /// it replaces the whole camera list) for the cost of a comparison.
    pub fn set_lut(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        slot: crate::lut::LutSlot,
        cube: Option<&solarxy_core::LutCube>,
    ) {
        if self.post.luts.set(device, queue, slot, cube) {
            self.post.composite.rebuild_bind_group(
                device,
                &self.layouts,
                &self.targets.hdr_resolve_view,
                &self.post.bloom.ping_view,
                &self.post.bloom.sampler,
                &self.post.luts,
            );
        }
    }

    /// Sets (or clears) the manipulator for this frame. Pull-based, like every
    /// other overlay: the host decides, the renderer draws.
    pub fn set_manipulator(&mut self, state: Option<crate::manipulator::ManipulatorState>) {
        self.manipulator = state;
        if state.is_none() {
            self.manipulator_line_count = 0;
            self.manipulator_tri_count = 0;
        }
    }

    /// Regenerates the manipulator's vertices for ONE pane.
    ///
    /// Per pane, not per frame, because the screen-constant scale depends on the
    /// pane's camera and height: the same gizmo is a different world size in a
    /// wide perspective pane than in a small orthographic one. Call this
    /// immediately before that pane's main pass.
    pub fn write_manipulator(&mut self, queue: &wgpu::Queue, camera: &Camera, pane_height_px: f32) {
        let Some(mut state) = self.manipulator else {
            return;
        };
        state.scale =
            crate::manipulator::GIZMO_PX * camera.world_per_pixel(state.origin(), pane_height_px);
        // Per pane for the same reason the scale is: each pane has its own
        // camera, so the view-aligned ring faces a different way in each.
        state.view_dir = camera.forward();
        self.manipulator = Some(state);

        let (mut lines, mut tris) = state.build_vertices();
        // The gizmo is a bounded handful of vertices (the rotate tool's four
        // 64-segment rings are the worst case), so the buffers are sized once at
        // startup and an overflow means someone added handles without raising
        // the budget.
        //
        // On overflow, TRUNCATE to what fits rather than draw nothing. The old
        // code zeroed both counts, so the gizmo vanished entirely -- and it
        // guarded that with a `debug_assert`, which is a no-op in release, plus
        // a `tracing:warn` that reaches NO subscriber on web. A release web
        // build therefore lost the gizmo with zero diagnostics. A clipped gizmo
        // is still usable and still visibly wrong, which is what we want.
        truncate_to_budget(&mut lines, 2, "line");
        truncate_to_budget(&mut tris, 3, "triangle");

        let line_bytes = bytemuck::cast_slice(&lines);
        let tri_bytes = bytemuck::cast_slice(&tris);
        queue.write_buffer(&self.manipulator_line_buf, 0, line_bytes);
        queue.write_buffer(&self.manipulator_tri_buf, 0, tri_bytes);
        self.manipulator_line_count = u32::try_from(lines.len()).unwrap_or(0);
        self.manipulator_tri_count = u32::try_from(tris.len()).unwrap_or(0);
    }

    /// Renders the selection-outline offscreen stages: the
    /// silhouette mask of every selected object, the jump-flood init, and
    /// the fixed five-step ladder. Call after [`Self::render_main_pass`]
    /// when `selection_style` is `Outline` and something is selected; the
    /// rim itself lands on the swapchain via
    /// [`Self::composite_selection_outline`] after the composite pass.
    pub fn render_selection_outline(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        objects: &[DrawObject<'_>],
        cam_bg: &wgpu::BindGroup,
    ) {
        // Mask: selected silhouettes, depth-ignoring, white on black.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Outline Mask Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.outline.mask_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            // Triangles, then lines (same layout, different assembly),
            // then points (their own expansion pipeline): every topology
            // silhouettes into the mask.
            pass.set_pipeline(&self.pipelines.overlay.outline_mask);
            pass.set_bind_group(0, cam_bg, &[]);
            pass.set_bind_group(1, &self.outline.white_bind_group, &[]);
            for obj in objects.iter().filter(|o| o.selected) {
                pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                for mesh in &obj.model.meshes {
                    if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                        continue;
                    }
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    // The UV passes draw the prototype's layout, never its
                    // placements: a UV map has one copy however many times
                    // the geometry is placed in the world, and the overlap
                    // counter would multiply every count by the instance
                    // number if this drew them all.
                    pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
                }
            }
            let has_selected_topo = |topology: MeshTopology| {
                objects.iter().filter(|o| o.selected).any(|o| {
                    o.model
                        .meshes
                        .iter()
                        .any(|m| m.visible && m.topology == topology)
                })
            };
            if has_selected_topo(MeshTopology::Lines) {
                pass.set_pipeline(&self.pipelines.overlay.outline_mask_line);
                for obj in objects.iter().filter(|o| o.selected) {
                    pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                    for mesh in &obj.model.meshes {
                        if !mesh.visible || mesh.topology != MeshTopology::Lines {
                            continue;
                        }
                        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                        pass.set_index_buffer(
                            mesh.index_buffer.slice(..),
                            wgpu::IndexFormat::Uint32,
                        );
                        pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
                    }
                }
            }
            if has_selected_topo(MeshTopology::Points) {
                pass.set_pipeline(&self.pipelines.overlay.outline_mask_point);
                pass.set_bind_group(0, cam_bg, &[]);
                pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
                for obj in objects.iter().filter(|o| o.selected) {
                    pass.set_vertex_buffer(0, obj.instance_buffer.slice(..));
                    for mesh in &obj.model.meshes {
                        if !mesh.visible || mesh.topology != MeshTopology::Points {
                            continue;
                        }
                        let Some(edge) = &mesh.edge_data else {
                            continue;
                        };
                        pass.set_bind_group(2, &edge.bind_group, &[]);
                        obj.draw_points(&mut pass, mesh);
                    }
                }
            }
        }
        // Jump-flood init: seed the ping half from the mask.
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Outline JFA Init"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: self.outline.ping(),
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.overlay.outline_jfa_init);
            pass.set_bind_group(0, &self.outline.init_bind_group, &[]);
            pass.set_bind_group(1, &self.outline.params_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        // The fixed ladder (always five passes, so the final field always
        // lands in the pong half regardless of the preferred width).
        for i in 0..crate::outline::JFA_STEPS.len() {
            let (src, dst) = self.outline.step_io(i);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Outline JFA Step"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: dst,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipelines.overlay.outline_jfa_step);
            pass.set_bind_group(0, src, &[]);
            pass.set_bind_group(1, &self.outline.step_bind_groups[i], &[]);
            pass.draw(0..3, 0..1);
        }
    }

    /// Blits the outline rim onto the composited swapchain view (after
    /// tone mapping, so it never blooms and AO never darkens it). Pass
    /// the pane viewport exactly as the composite pass received it.
    pub fn composite_selection_outline(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        viewport: Option<[f32; 4]>,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Outline Blit Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        if let Some([x, y, w, h]) = viewport {
            pass.set_viewport(x, y, w, h, 0.0, 1.0);
            pass.set_scissor_rect(x as u32, y as u32, w as u32, h as u32);
        }
        pass.set_pipeline(&self.pipelines.overlay.outline_blit);
        pass.set_bind_group(0, self.outline.final_bind_group(), &[]);
        pass.set_bind_group(1, &self.outline.params_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    /// Applies the selection-highlight preference: the style, and the rim
    /// color/width (the legacy tint reuses the same color at a fixed
    /// 0.35 alpha, matching how it looked before the outline replaced it).
    pub fn set_selection_highlight(
        &mut self,
        queue: &wgpu::Queue,
        style: SelectionStyle,
        color: [f32; 4],
        width: f32,
    ) {
        self.selection_style = style;
        self.outline.write_params(queue, color, width);
        let tint = [color[0], color[1], color[2], 0.35f32];
        queue.write_buffer(
            &self.validation_colors.selection_buffer,
            0,
            bytemuck::cast_slice(&tint),
        );
    }

    /// Draws the translucent accent tint over selected objects' meshes,
    /// inside the main pass (after every scene draw).
    fn draw_selection_tint<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        objects: &[DrawObject<'a>],
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.pipelines.overlay.validation_overlay);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, &self.validation_colors.selection_bind_group, &[]);
        for obj in objects.iter().filter(|o| o.selected) {
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for mesh in &obj.model.meshes {
                if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                    continue;
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                obj.draw_mesh(pass, mesh);
            }
        }
    }

    fn draw_opaque_meshes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        objects: &[DrawObject<'a>],
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_bind_group(1, cam_bg, &[]);
        pass.set_bind_group(2, &env.light_bind_group, &[]);
        pass.set_bind_group(3, &env.shadow.sample_bind_group, &[]);
        // The colored variant binds the extra color slot; switch lazily so
        // runs of same-flavor meshes cost one pipeline set.
        let mut colored_bound: Option<bool> = None;
        for obj in objects {
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for mesh in &obj.model.meshes {
                if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                    continue;
                }
                let material = &obj.model.materials[mesh.material];
                if material.uniform.alpha_mode == 2 {
                    continue;
                }
                let colored = mesh.color_buffer.is_some();
                if colored_bound != Some(colored) {
                    pass.set_pipeline(if colored {
                        &self.pipelines.scene.main_colored
                    } else {
                        &self.pipelines.scene.main
                    });
                    colored_bound = Some(colored);
                }
                if let Some(colors) = &mesh.color_buffer {
                    pass.set_vertex_buffer(2, colors.slice(..));
                }
                pass.draw_mesh(mesh, material, 0..1);
            }
        }
    }

    fn draw_blend_meshes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        objects: &[DrawObject<'a>],
        cam_bg: &'a wgpu::BindGroup,
        cam: &Camera,
    ) {
        let forward = (cam.target - cam.eye).normalize();
        let eye = cam.eye;

        // Sort blended meshes back-to-front per object (cross-object
        // ordering stays draw-order; a global sort would need transformed
        // bounds and arrives with the engine work if it proves visible).
        let mut bound_pipeline = false;
        let mut colored_bound: Option<bool> = None;
        for obj in objects {
            let mut blend_list: Vec<(usize, f32)> = Vec::new();
            for (i, mesh) in obj.model.meshes.iter().enumerate() {
                if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                    continue;
                }
                let material = &obj.model.materials[mesh.material];
                if material.uniform.alpha_mode != 2 {
                    continue;
                }
                let center = obj.model.mesh_bounds[i].center();
                let to_center = center - eye;
                let depth = to_center.dot(forward);
                blend_list.push((i, depth));
            }

            if blend_list.is_empty() {
                continue;
            }

            blend_list.sort_unstable_by(|a, b| {
                b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal)
            });

            if !bound_pipeline {
                pass.set_bind_group(1, cam_bg, &[]);
                pass.set_bind_group(2, &env.light_bind_group, &[]);
                pass.set_bind_group(3, &env.shadow.sample_bind_group, &[]);
                bound_pipeline = true;
            }
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for (idx, _) in &blend_list {
                let mesh = &obj.model.meshes[*idx];
                let material = &obj.model.materials[mesh.material];
                let colored = mesh.color_buffer.is_some();
                if colored_bound != Some(colored) {
                    pass.set_pipeline(if colored {
                        &self.pipelines.scene.alpha_blend_colored
                    } else {
                        &self.pipelines.scene.alpha_blend
                    });
                    colored_bound = Some(colored);
                }
                if let Some(colors) = &mesh.color_buffer {
                    pass.set_vertex_buffer(2, colors.slice(..));
                }
                pass.draw_mesh(mesh, material, 0..1);
            }
        }
    }

    /// Draws every visible line and point mesh: 1 px unlit line lists and
    /// camera-facing point quads (expanded from the edge-geometry storage
    /// buffer; see `points_lines.wgsl`). Called once per main pass,
    /// independent of view mode.
    fn draw_topology_meshes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        objects: &[DrawObject<'a>],
        cam_bg: &'a wgpu::BindGroup,
    ) {
        let mut line_bound: Option<bool> = None;
        for obj in objects {
            for mesh in &obj.model.meshes {
                if !mesh.visible || mesh.topology != MeshTopology::Lines {
                    continue;
                }
                let colored = mesh.color_buffer.is_some();
                if line_bound != Some(colored) {
                    pass.set_pipeline(if colored {
                        &self.pipelines.scene.line_colored
                    } else {
                        &self.pipelines.scene.line
                    });
                    pass.set_bind_group(0, cam_bg, &[]);
                    line_bound = Some(colored);
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                if let Some(colors) = &mesh.color_buffer {
                    pass.set_vertex_buffer(2, colors.slice(..));
                }
                obj.draw_mesh(pass, mesh);
            }
        }

        let mut point_bound = false;
        for obj in objects {
            for mesh in &obj.model.meshes {
                if !mesh.visible || mesh.topology != MeshTopology::Points {
                    continue;
                }
                let Some(edge) = &mesh.edge_data else {
                    continue;
                };
                if !point_bound {
                    pass.set_pipeline(&self.pipelines.scene.point);
                    pass.set_bind_group(0, cam_bg, &[]);
                    pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
                    point_bound = true;
                }
                pass.set_bind_group(2, &edge.bind_group, &[]);
                pass.set_vertex_buffer(0, obj.instance_buffer.slice(..));
                obj.draw_points(pass, mesh);
            }
        }
    }

    fn draw_edge_wireframe<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        objects: &[DrawObject<'a>],
        pipeline: &'a wgpu::RenderPipeline,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
        for obj in objects {
            pass.set_vertex_buffer(0, obj.instance_buffer.slice(..));
            for mesh in &obj.model.meshes {
                if !mesh.visible {
                    continue;
                }
                if let Some(edge) = &mesh.edge_data {
                    pass.set_bind_group(2, &edge.bind_group, &[]);
                    pass.draw(0..edge.num_edges * 6, 0..1);
                }
            }
        }
    }

    fn draw_floor<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.pipelines.scene.floor);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, &env.shadow.sample_bind_group, &[]);
        pass.set_vertex_buffer(0, env.vis.floor_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(
            env.vis.floor_mesh.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..env.vis.floor_mesh.num_elements, 0, 0..1);
    }

    fn draw_axes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if !pds.show_axis_gizmo {
            return;
        }
        pass.set_pipeline(&self.pipelines.overlay.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(0, env.vis.axes_vertex_buf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_local_axes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if !pds.show_local_axes || env.vis.local_axes_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipelines.overlay.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(0, env.vis.local_axes_vertex_buf.slice(..));
        pass.draw(0..env.vis.local_axes_vertex_count, 0..1);
    }

    fn draw_bounds<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if pds.bounds_mode == BoundsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.overlay.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        match pds.bounds_mode {
            BoundsMode::Off => {}
            BoundsMode::WholeModel => {
                pass.set_vertex_buffer(0, env.vis.bounds_whole_buf.slice(..));
                pass.draw(0..env.vis.bounds_whole_count, 0..1);
            }
            BoundsMode::PerMesh => {
                if env.vis.bounds_per_mesh_count > 0 {
                    pass.set_vertex_buffer(0, env.vis.bounds_per_mesh_buf.slice(..));
                    pass.draw(0..env.vis.bounds_per_mesh_count, 0..1);
                }
            }
        }
    }

    fn draw_validation_overlay<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        objects: &[DrawObject<'a>],
        cam_bg: &'a wgpu::BindGroup,
    ) {
        use crate::validation::IssueCategory;

        pass.set_pipeline(&self.pipelines.overlay.validation_overlay);
        pass.set_bind_group(0, cam_bg, &[]);

        for obj in objects {
            let Some(validation) = obj.validation else {
                continue;
            };
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for (i, mesh) in obj.model.meshes.iter().enumerate() {
                if !mesh.visible {
                    continue;
                }
                if let Some(cat_idx) = validation.mesh_cat.get(i).copied().flatten() {
                    pass.set_bind_group(1, &self.validation_colors.bind_groups[cat_idx], &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    // The UV passes draw the prototype's layout, never its
                    // placements: a UV map has one copy however many times
                    // the geometry is placed in the world, and the overlap
                    // counter would multiply every count by the instance
                    // number if this drew them all.
                    pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
                }
            }
        }

        let degen_idx = IssueCategory::ALL
            .iter()
            .position(|c| *c == IssueCategory::DegenerateTriangles)
            .unwrap_or(4);
        for obj in objects {
            if obj.validation.is_none() {
                continue;
            }
            pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
            for mesh in &obj.model.meshes {
                if !mesh.visible {
                    continue;
                }
                if let Some(ref degen_buf) = mesh.degen_index_buffer {
                    pass.set_bind_group(1, &self.validation_colors.bind_groups[degen_idx], &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(degen_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.degen_num_elements, 0, 0..1);
                }
            }
        }

        let edge_idx = IssueCategory::ALL
            .iter()
            .position(|c| *c == IssueCategory::NonManifoldEdge)
            .unwrap_or(5);
        let mut switched_to_edge = false;
        for obj in objects {
            let Some(validation) = obj.validation else {
                continue;
            };
            let mut bound_instance = false;
            for (mi, mesh) in obj.model.meshes.iter().enumerate() {
                if !mesh.visible {
                    continue;
                }
                if let Some(Some((edge_buf, num))) =
                    validation.edge_buffers.get(mi).map(|o| o.as_ref())
                {
                    if !switched_to_edge {
                        pass.set_pipeline(&self.pipelines.overlay.validation_edge);
                        pass.set_bind_group(0, cam_bg, &[]);
                        switched_to_edge = true;
                    }
                    if !bound_instance {
                        pass.set_vertex_buffer(1, obj.instance_buffer.slice(..));
                        bound_instance = true;
                    }
                    pass.set_bind_group(1, &self.validation_colors.bind_groups[edge_idx], &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(edge_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..*num, 0, 0..1);
                }
            }
        }
    }

    pub fn render_uv_overlap_count_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        object: &DrawObject<'_>,
        uv_cam_bg: &wgpu::BindGroup,
        count_view: &wgpu::TextureView,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("UV Overlap Count Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: count_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: None,
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        pass.set_pipeline(&self.pipelines.uv.uv_overlap_count);
        pass.set_bind_group(0, uv_cam_bg, &[]);
        pass.set_vertex_buffer(1, object.instance_buffer.slice(..));
        for mesh in &object.model.meshes {
            if !mesh.visible {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            // The UV passes draw the prototype's layout, never its
            // placements: a UV map has one copy however many times the
            // geometry is placed in the world, and the overlap counter
            // would multiply every count by the instance number.
            pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
        }
    }

    pub fn render_uv_map_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        object: &DrawObject<'_>,
        uv_cam_bg: &wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        // Flat-solid variants paint the pane purely via the clear colour.
        // `Charcoal` carries a faint blue tint to sit with the Ayu Mirage
        // theme; `Dark` clears the same as before and overdraws a gradient.
        let clear_color = match pds.uv_bg {
            UvMapBackground::Charcoal => wgpu::Color {
                r: 0.045,
                g: 0.050,
                b: 0.072,
                a: 1.0,
            },
            UvMapBackground::Gray => wgpu::Color {
                r: 0.300,
                g: 0.300,
                b: 0.320,
                a: 1.0,
            },
            UvMapBackground::Dark | UvMapBackground::Checker | UvMapBackground::Texture => {
                wgpu::Color {
                    r: 0.10,
                    g: 0.10,
                    b: 0.10,
                    a: 1.0,
                }
            }
        };

        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("UV Map Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear_color),
                    store: wgpu::StoreOp::Discard,
                },
                depth_slice: None,
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.targets.depth_texture.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });

        pass.set_vertex_buffer(1, object.instance_buffer.slice(..));

        match pds.uv_bg {
            UvMapBackground::Dark => {
                self.draw_background_gradient(&mut pass);
            }
            UvMapBackground::Charcoal | UvMapBackground::Gray => {
                // Flat solid — the clear colour above is the background.
            }
            UvMapBackground::Checker => {
                pass.set_pipeline(&self.pipelines.uv.uv_map_checker);
                pass.set_bind_group(0, uv_cam_bg, &[]);
                pass.set_bind_group(1, &self.wire.uv_checker_bind_group, &[]);
                pass.draw_model_simple(object.model, 0..1);
            }
            UvMapBackground::Texture => {
                pass.set_pipeline(&self.pipelines.uv.uv_map_texture);
                pass.set_bind_group(0, uv_cam_bg, &[]);
                for mesh in &object.model.meshes {
                    if !mesh.visible {
                        continue;
                    }
                    let material = &object.model.materials[mesh.material];
                    pass.set_bind_group(1, &material.bind_group, &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    // The UV passes draw the prototype's layout, never its
                    // placements: a UV map has one copy however many times
                    // the geometry is placed in the world, and the overlap
                    // counter would multiply every count by the instance
                    // number if this drew them all.
                    pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
                }
            }
        }

        pass.set_pipeline(&self.pipelines.uv.uv_map_wire);
        pass.set_bind_group(0, uv_cam_bg, &[]);
        pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
        for mesh in &object.model.meshes {
            if !mesh.visible {
                continue;
            }
            if let Some(uv_edge) = &mesh.uv_edge_data
                && let Some(edge) = &mesh.edge_data
            {
                pass.set_bind_group(2, &uv_edge.bind_group, &[]);
                pass.draw(0..edge.num_edges * 6, 0..1);
            }
        }

        pass.set_pipeline(&self.pipelines.overlay.gizmo);
        pass.set_bind_group(0, uv_cam_bg, &[]);
        pass.set_vertex_buffer(0, self.uv_boundary_buf.slice(..));
        pass.draw(0..8, 0..1);

        if pds.show_uv_overlap {
            pass.set_pipeline(&self.pipelines.uv.uv_overlap_overlay);
            pass.set_bind_group(0, &self.uv_overlap.overlay_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn draw_normals<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        objects: &[DrawObject<'a>],
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if pds.normals_mode == NormalsMode::Off {
            return;
        }
        if objects.is_empty() {
            return;
        }
        // The normal-arrow segments are parallel to the drawn objects'
        // meshes FLATTENED in draw order. The desktop builds them from its
        // file-loaded model, which it always draws first, so the segments
        // line up with the head of the list and any cooked objects behind
        // it simply run past the end; the web host aggregates every
        // displayed object in the same order. Visibility flags gate the
        // per-mesh draws.
        let flat_meshes = || objects.iter().flat_map(|o| o.model.meshes.iter());
        pass.set_pipeline(&self.pipelines.overlay.normals);
        pass.set_bind_group(0, cam_bg, &[]);
        if matches!(
            pds.normals_mode,
            NormalsMode::Face | NormalsMode::FaceAndVertex
        ) && env.vis.face_normals_count > 0
        {
            // Segments zip against the flattened meshes; fewer segments
            // than meshes is legitimate (desktop builds vis from the
            // primary model while extra objects draw), but MORE segments
            // than meshes means the aggregate desynced from the scene.
            debug_assert!(
                env.vis.face_normals_segments.len() <= flat_meshes().count(),
                "normals segments exceed the flattened meshes"
            );
            pass.set_bind_group(1, &env.vis.face_normals_params_bind_group, &[]);
            pass.set_vertex_buffer(0, env.vis.face_normals_buf.slice(..));
            // One draw per visible mesh — a hidden mesh's normals are
            // skipped.
            for (mesh, seg) in flat_meshes().zip(&env.vis.face_normals_segments) {
                if mesh.visible && !seg.is_empty() {
                    pass.draw(seg.clone(), 0..1);
                }
            }
        }
        if matches!(
            pds.normals_mode,
            NormalsMode::Vertex | NormalsMode::FaceAndVertex
        ) && env.vis.vertex_normals_count > 0
        {
            pass.set_bind_group(1, &env.vis.vertex_normals_params_bind_group, &[]);
            pass.set_vertex_buffer(0, env.vis.vertex_normals_buf.slice(..));
            for (mesh, seg) in flat_meshes().zip(&env.vis.vertex_normals_segments) {
                if mesh.visible && !seg.is_empty() {
                    pass.draw(seg.clone(), 0..1);
                }
            }
        }
    }

    /// Per-point attribute-vector arrows (the web attribute visualization),
    /// through the gizmo line pipeline: the host CPU-colors each vertex
    /// (uniform color or a magnitude ramp), so no params bind group is
    /// needed. Gated purely on the buffer count: only the web host
    /// populates the channel, so the desktop shell and the golden harness
    /// draw nothing here by construction.
    fn draw_attr_vectors<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        env: &'a SceneEnvironment,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        if env.vis.attr_lines_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipelines.overlay.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(0, env.vis.attr_lines_buf.slice(..));
        pass.draw(0..env.vis.attr_lines_count, 0..1);
    }

    /// The GPU attribute labels: one draw over the whole set (chips, dots,
    /// glyph quads decoded from `vertex_index` ranges), expanded per pane
    /// against its camera so orbiting costs no CPU work. Gated purely on
    /// the count: only the web host populates the channel.
    ///
    /// A shaded pane depth-tests, so a label anchored on the far side of an
    /// object is hidden by the near side; a wireframe pane draws every
    /// label, because there is no surface there to hide behind and every
    /// point is genuinely visible to the user.
    fn draw_attr_labels<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        let verts = self.labels.vertex_count();
        if verts == 0 {
            return;
        }
        let occlude = !matches!(pds.view_mode, ViewMode::WireframeOnly);
        pass.set_pipeline(if occlude {
            &self.pipelines.overlay.attr_labels_occluded
        } else {
            &self.pipelines.overlay.attr_labels
        });
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
        pass.set_bind_group(2, &self.labels.bind_group, &[]);
        pass.draw(0..verts, 0..1);
    }

    /// Replaces the label set (host-driven, event-paced; see
    /// [`crate::labels::LabelResources::set_labels`]).
    pub fn set_attr_labels(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        instances: &[crate::labels::LabelInstance],
        glyph_words: &[u32],
    ) {
        self.labels
            .set_labels(device, queue, &self.layouts.labels, instances, glyph_words);
    }

    /// Pushes the label theme colors / device pixel ratio.
    pub fn write_label_style(&mut self, queue: &wgpu::Queue, style: &crate::labels::LabelStyle) {
        self.labels.write_style(queue, style);
    }

    /// Updates only the labels' device pixel ratio (kept honest across
    /// browser zoom without the host re-reading theme colors).
    pub fn write_label_dpr(&mut self, queue: &wgpu::Queue, dpr: f32) {
        self.labels.write_dpr(queue, dpr);
    }
}
