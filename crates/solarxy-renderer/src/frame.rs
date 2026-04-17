use std::sync::Arc;

use cgmath::prelude::*;

use crate::bind_groups::BindGroupLayouts;
use crate::camera::Camera;
use crate::model::{DrawMeshSimple, DrawModel};
use crate::pipelines::Pipelines;
use crate::texture::SharedSamplers;
use crate::uv_camera::UvCameraState;
use solarxy_core::preferences::{BackgroundMode, NormalsMode, UvMapBackground, UvMode, ViewMode};

use crate::bloom::BloomState;
use crate::composite::CompositeState;
use crate::ibl::{BrdfLut, IblState};
use crate::ssao::SsaoState;
use crate::texture;
use solarxy_core::preferences::{IblMode, ToneMode};

use crate::scene::{BackgroundModeExt, ModelScene};
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

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct WireframeParams {
    pub color: [f32; 4],
    pub line_width: f32,
    pub screen_width: f32,
    pub screen_height: f32,
    pub _pad: f32,
}

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
    pub tone_mode: ToneMode,
    pub exposure: f32,
}

pub struct IblResources {
    pub ibl: IblState,
    pub ibl_fallback: IblState,
    pub brdf_lut: BrdfLut,
    pub ibl_mode: IblMode,
    pub last_active_ibl_mode: IblMode,
}

pub struct WireframeResources {
    pub _gradient_buffer: wgpu::Buffer,
    pub gradient_bind_group: wgpu::BindGroup,
    pub wireframe_params_buffer: wgpu::Buffer,
    pub wireframe_params_bind_group: wgpu::BindGroup,
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
}

pub struct ValidationColorResources {
    pub bind_groups: Vec<wgpu::BindGroup>,
    #[allow(dead_code)]
    pub buffers: Vec<wgpu::Buffer>,
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
    pub uv_overlap: UvOverlapResources,
    pub validation_colors: ValidationColorResources,
    #[allow(unused)]
    pub shared_samplers: SharedSamplers,
    pub msaa_sample_count: u32,
    pub target_width: u32,
    pub target_height: u32,
}

impl Renderer {
    pub fn draw_background_gradient<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipelines.background);
        pass.set_bind_group(0, &self.wire.gradient_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub fn render_empty_pass(&self, encoder: &mut wgpu::CommandEncoder, pds: &PaneDisplaySettings) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Empty Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(pds.background_mode.clear_color()),
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
        scene: &ModelScene,
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
        pass.set_pipeline(&self.pipelines.gbuffer);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode == 2 {
                continue;
            }
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
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
            pass.set_pipeline(&self.pipelines.ssao);
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
            pass.set_pipeline(&self.pipelines.ssao_blur_h);
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
            pass.set_pipeline(&self.pipelines.ssao_blur_v);
            pass.set_bind_group(0, &self.post.ssao.blur_v_bind_group, &[]);
            pass.set_bind_group(1, cam_bg, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    pub fn render_shadow_pass(&self, encoder: &mut wgpu::CommandEncoder, scene: &ModelScene) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Shadow Pass"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &scene.shadow.texture_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            occlusion_query_set: None,
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipelines.shadow);
        pass.set_bind_group(0, &scene.shadow.pass_bind_group, &[]);
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode == 2 {
                continue;
            }
            pass.set_bind_group(1, &material.bind_group, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
        }
    }

    pub fn render_main_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ModelScene,
        cam_bg: &wgpu::BindGroup,
        cam: &Camera,
        pds: &PaneDisplaySettings,
    ) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(pds.background_mode.clear_color()),
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

        if pds.background_mode == BackgroundMode::Gradient {
            self.draw_background_gradient(&mut pass);
        }

        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));

        if pds.uv_mode == UvMode::Off {
            match pds.view_mode {
                ViewMode::Shaded | ViewMode::ShadedWireframe => {
                    self.draw_opaque_meshes(&mut pass, scene, cam_bg);
                    self.draw_floor(&mut pass, scene, cam_bg);
                    if pds.view_mode == ViewMode::ShadedWireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire,
                            cam_bg,
                        );
                    }
                    self.draw_blend_meshes(&mut pass, scene, cam_bg, cam);
                }
                ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire, cam_bg);
                }
                ViewMode::Ghosted => {
                    pass.set_pipeline(&self.pipelines.ghosted_fill);
                    pass.set_bind_group(0, cam_bg, &[]);
                    pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
                    pass.draw_model_simple(&scene.model, 0..1);
                    if pds.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire_ghosted,
                            cam_bg,
                        );
                    }
                }
            }
        } else {
            pass.set_bind_group(0, cam_bg, &[]);
            if scene.model.has_uvs {
                match pds.uv_mode {
                    UvMode::Checker => {
                        pass.set_pipeline(&self.pipelines.uv_checker);
                        pass.set_bind_group(1, &self.wire.uv_checker_bind_group, &[]);
                    }
                    UvMode::Gradient | UvMode::Off => {
                        pass.set_pipeline(&self.pipelines.uv_gradient);
                    }
                }
            } else {
                pass.set_pipeline(&self.pipelines.uv_no_uvs);
            }
            pass.draw_model_simple(&scene.model, 0..1);

            match pds.view_mode {
                ViewMode::Shaded => {}
                ViewMode::ShadedWireframe | ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire, cam_bg);
                }
                ViewMode::Ghosted => {
                    if pds.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire_ghosted,
                            cam_bg,
                        );
                    }
                }
            }
        }

        if pds.show_grid {
            pass.set_pipeline(&self.pipelines.grid);
            pass.set_bind_group(0, cam_bg, &[]);
            pass.set_bind_group(1, &scene.vis.grid_params_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.grid_mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(
                scene.vis.grid_mesh.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..scene.vis.grid_mesh.num_elements, 0, 0..1);
        }
        self.draw_normals(&mut pass, scene, cam_bg, pds);
        self.draw_axes(&mut pass, scene, cam_bg, pds);
        self.draw_local_axes(&mut pass, scene, cam_bg, pds);
        self.draw_bounds(&mut pass, scene, cam_bg, pds);
        if pds.show_validation {
            self.draw_validation_overlay(&mut pass, scene, cam_bg);
        }
    }

    fn draw_opaque_meshes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.pipelines.main);
        pass.set_bind_group(1, cam_bg, &[]);
        pass.set_bind_group(2, &scene.light_bind_group, &[]);
        pass.set_bind_group(3, &scene.shadow.sample_bind_group, &[]);
        for mesh in &scene.model.meshes {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode == 2 {
                continue;
            }
            pass.draw_mesh(mesh, material, 0..1);
        }
    }

    fn draw_blend_meshes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
        cam: &Camera,
    ) {
        let forward = (cam.target - cam.eye).normalize();
        let eye = cam.eye;

        let mut blend_list: Vec<(usize, f32)> = Vec::new();
        for (i, mesh) in scene.model.meshes.iter().enumerate() {
            let material = &scene.model.materials[mesh.material];
            if material.uniform.alpha_mode != 2 {
                continue;
            }
            let center = scene.model.mesh_bounds[i].center();
            let to_center = center - eye;
            let depth = to_center.dot(forward);
            blend_list.push((i, depth));
        }

        if blend_list.is_empty() {
            return;
        }

        blend_list
            .sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        pass.set_pipeline(&self.pipelines.alpha_blend);
        pass.set_bind_group(1, cam_bg, &[]);
        pass.set_bind_group(2, &scene.light_bind_group, &[]);
        pass.set_bind_group(3, &scene.shadow.sample_bind_group, &[]);
        for (idx, _) in &blend_list {
            let mesh = &scene.model.meshes[*idx];
            let material = &scene.model.materials[mesh.material];
            pass.draw_mesh(mesh, material, 0..1);
        }
    }

    fn draw_edge_wireframe<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        pipeline: &'a wgpu::RenderPipeline,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            if let Some(edge) = &mesh.edge_data {
                pass.set_bind_group(2, &edge.bind_group, &[]);
                pass.draw(0..edge.num_edges * 6, 0..1);
            }
        }
    }

    fn draw_floor<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.pipelines.floor);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_bind_group(1, &scene.shadow.sample_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.floor_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(
            scene.vis.floor_mesh.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..scene.vis.floor_mesh.num_elements, 0, 0..1);
    }

    fn draw_axes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if !pds.show_axis_gizmo {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(0, scene.vis.axes_vertex_buf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_local_axes<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if !pds.show_local_axes || scene.vis.local_axes_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(0, scene.vis.local_axes_vertex_buf.slice(..));
        pass.draw(0..scene.vis.local_axes_vertex_count, 0..1);
    }

    fn draw_bounds<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if pds.bounds_mode == BoundsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, cam_bg, &[]);
        match pds.bounds_mode {
            BoundsMode::Off => {}
            BoundsMode::WholeModel => {
                pass.set_vertex_buffer(0, scene.vis.bounds_whole_buf.slice(..));
                pass.draw(0..scene.vis.bounds_whole_count, 0..1);
            }
            BoundsMode::PerMesh => {
                if scene.vis.bounds_per_mesh_count > 0 {
                    pass.set_vertex_buffer(0, scene.vis.bounds_per_mesh_buf.slice(..));
                    pass.draw(0..scene.vis.bounds_per_mesh_count, 0..1);
                }
            }
        }
    }

    fn draw_validation_overlay<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
    ) {
        use crate::validation::IssueCategory;

        pass.set_pipeline(&self.pipelines.validation_overlay);
        pass.set_bind_group(0, cam_bg, &[]);
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));

        for (i, mesh) in scene.model.meshes.iter().enumerate() {
            if let Some(cat_idx) = scene.validation_mesh_cat[i] {
                pass.set_bind_group(1, &self.validation_colors.bind_groups[cat_idx], &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
            }
        }

        let degen_idx = IssueCategory::ALL
            .iter()
            .position(|c| *c == IssueCategory::DegenerateTriangles)
            .unwrap_or(4);
        for mesh in &scene.model.meshes {
            if let Some(ref degen_buf) = mesh.degen_index_buffer {
                pass.set_bind_group(1, &self.validation_colors.bind_groups[degen_idx], &[]);
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_index_buffer(degen_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.degen_num_elements, 0, 0..1);
            }
        }
    }

    pub fn render_uv_overlap_count_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ModelScene,
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

        pass.set_pipeline(&self.pipelines.uv_overlap_count);
        pass.set_bind_group(0, uv_cam_bg, &[]);
        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
        }
    }

    pub fn render_uv_map_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ModelScene,
        uv_cam_bg: &wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        let clear_color = wgpu::Color {
            r: 0.10,
            g: 0.10,
            b: 0.10,
            a: 1.0,
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

        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));

        match pds.uv_bg {
            UvMapBackground::Dark => {
                self.draw_background_gradient(&mut pass);
            }
            UvMapBackground::Checker => {
                pass.set_pipeline(&self.pipelines.uv_map_checker);
                pass.set_bind_group(0, uv_cam_bg, &[]);
                pass.set_bind_group(1, &self.wire.uv_checker_bind_group, &[]);
                pass.draw_model_simple(&scene.model, 0..1);
            }
            UvMapBackground::Texture => {
                pass.set_pipeline(&self.pipelines.uv_map_texture);
                pass.set_bind_group(0, uv_cam_bg, &[]);
                for mesh in &scene.model.meshes {
                    let material = &scene.model.materials[mesh.material];
                    pass.set_bind_group(1, &material.bind_group, &[]);
                    pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.num_elements, 0, 0..1);
                }
            }
        }

        pass.set_pipeline(&self.pipelines.uv_map_wire);
        pass.set_bind_group(0, uv_cam_bg, &[]);
        pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
        for mesh in &scene.model.meshes {
            if let Some(uv_edge) = &mesh.uv_edge_data
                && let Some(edge) = &mesh.edge_data
            {
                pass.set_bind_group(2, &uv_edge.bind_group, &[]);
                pass.draw(0..edge.num_edges * 6, 0..1);
            }
        }

        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, uv_cam_bg, &[]);
        pass.set_vertex_buffer(0, self.uv_boundary_buf.slice(..));
        pass.draw(0..8, 0..1);

        if pds.show_uv_overlap {
            pass.set_pipeline(&self.pipelines.uv_overlap_overlay);
            pass.set_bind_group(0, &self.uv_overlap.overlay_bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
    }

    fn draw_normals<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        scene: &'a ModelScene,
        cam_bg: &'a wgpu::BindGroup,
        pds: &PaneDisplaySettings,
    ) {
        if pds.normals_mode == NormalsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.normals);
        pass.set_bind_group(0, cam_bg, &[]);
        if matches!(
            pds.normals_mode,
            NormalsMode::Face | NormalsMode::FaceAndVertex
        ) && scene.vis.face_normals_count > 0
        {
            pass.set_bind_group(1, &scene.vis.face_normals_params_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.face_normals_buf.slice(..));
            pass.draw(0..scene.vis.face_normals_count, 0..1);
        }
        if matches!(
            pds.normals_mode,
            NormalsMode::Vertex | NormalsMode::FaceAndVertex
        ) && scene.vis.vertex_normals_count > 0
        {
            pass.set_bind_group(1, &scene.vis.vertex_normals_params_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.vertex_normals_buf.slice(..));
            pass.draw(0..scene.vis.vertex_normals_count, 0..1);
        }
    }
}
