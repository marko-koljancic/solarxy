use cgmath::prelude::*;

use crate::cgi::model::{DrawMeshSimple, DrawModel};
use crate::preferences::{BackgroundMode, NormalsMode, UvMode, ViewMode};

use super::BoundsMode;

use super::{ModelScene, State};

impl State {
    pub(super) fn draw_background_gradient<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>) {
        pass.set_pipeline(&self.pipelines.background);
        pass.set_bind_group(0, &self.wire.gradient_bind_group, &[]);
        pass.draw(0..3, 0..1);
    }

    pub(super) fn render_empty_pass(&self, encoder: &mut wgpu::CommandEncoder) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Empty Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.display.background_mode.clear_color()),
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
        if self.display.background_mode == BackgroundMode::Gradient {
            self.draw_background_gradient(&mut pass);
        }
    }

    pub(super) fn render_gbuffer_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ModelScene,
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
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
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

    pub(super) fn render_ssao_passes(&self, encoder: &mut wgpu::CommandEncoder) {
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
            pass.draw(0..3, 0..1);
        }
    }

    pub(super) fn render_shadow_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        scene: &ModelScene,
    ) {
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

    pub(super) fn render_main_pass(&self, encoder: &mut wgpu::CommandEncoder, scene: &ModelScene) {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Render Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.targets.msaa_hdr_view,
                resolve_target: Some(&self.targets.hdr_resolve_view),
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.display.background_mode.clear_color()),
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

        if self.display.background_mode == BackgroundMode::Gradient {
            self.draw_background_gradient(&mut pass);
        }

        pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));

        if self.display.uv_mode == UvMode::Off {
            match self.display.view_mode {
                ViewMode::Shaded | ViewMode::ShadedWireframe => {
                    self.draw_opaque_meshes(&mut pass, scene);
                    self.draw_floor(&mut pass, scene);
                    if self.display.view_mode == ViewMode::ShadedWireframe {
                        self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire);
                    }
                    self.draw_blend_meshes(&mut pass, scene);
                }
                ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire);
                }
                ViewMode::Ghosted => {
                    pass.set_pipeline(&self.pipelines.ghosted_fill);
                    pass.set_bind_group(0, &scene.cam.bind_group, &[]);
                    pass.set_vertex_buffer(1, scene.instance_buffer.slice(..));
                    pass.draw_model_simple(&scene.model, 0..1);
                    if self.display.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire_ghosted,
                        );
                    }
                }
            }
        } else {
            pass.set_bind_group(0, &scene.cam.bind_group, &[]);
            if scene.model.has_uvs {
                match self.display.uv_mode {
                    UvMode::Gradient => {
                        pass.set_pipeline(&self.pipelines.uv_gradient);
                    }
                    UvMode::Checker => {
                        pass.set_pipeline(&self.pipelines.uv_checker);
                        pass.set_bind_group(1, &self.wire.uv_checker_bind_group, &[]);
                    }
                    UvMode::Off => unreachable!(),
                }
            } else {
                pass.set_pipeline(&self.pipelines.uv_no_uvs);
            }
            pass.draw_model_simple(&scene.model, 0..1);

            match self.display.view_mode {
                ViewMode::Shaded => {}
                ViewMode::ShadedWireframe | ViewMode::WireframeOnly => {
                    self.draw_edge_wireframe(&mut pass, scene, &self.pipelines.edge_wire);
                }
                ViewMode::Ghosted => {
                    if self.display.ghosted_wireframe {
                        self.draw_edge_wireframe(
                            &mut pass,
                            scene,
                            &self.pipelines.edge_wire_ghosted,
                        );
                    }
                }
            }
        }

        if self.display.show_grid {
            pass.set_pipeline(&self.pipelines.grid);
            pass.set_bind_group(0, &scene.vis.grid_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.grid_mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(
                scene.vis.grid_mesh.index_buffer.slice(..),
                wgpu::IndexFormat::Uint32,
            );
            pass.draw_indexed(0..scene.vis.grid_mesh.num_elements, 0, 0..1);
        }
        self.draw_normals(&mut pass, scene);
        self.draw_axes(&mut pass, scene);
        self.draw_local_axes(&mut pass, scene);
        self.draw_bounds(&mut pass, scene);
    }

    fn draw_opaque_meshes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        pass.set_pipeline(&self.pipelines.main);
        pass.set_bind_group(1, &scene.cam.bind_group, &[]);
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

    fn draw_blend_meshes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        let forward = (scene.cam.camera.target - scene.cam.camera.eye).normalize();
        let eye = scene.cam.camera.eye;

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
        pass.set_bind_group(1, &scene.cam.bind_group, &[]);
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
    ) {
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_bind_group(1, &self.wire.wireframe_params_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.instance_buffer.slice(..));
        for mesh in &scene.model.meshes {
            if let Some(edge) = &mesh.edge_data {
                pass.set_bind_group(2, &edge.bind_group, &[]);
                pass.draw(0..edge.num_edges * 6, 0..1);
            }
        }
    }

    fn draw_floor<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        pass.set_pipeline(&self.pipelines.floor);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_bind_group(1, &scene.shadow.sample_bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.floor_mesh.vertex_buffer.slice(..));
        pass.set_index_buffer(
            scene.vis.floor_mesh.index_buffer.slice(..),
            wgpu::IndexFormat::Uint32,
        );
        pass.draw_indexed(0..scene.vis.floor_mesh.num_elements, 0, 0..1);
    }

    fn draw_axes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if !self.display.show_axis_gizmo {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.axes_vertex_buf.slice(..));
        pass.draw(0..6, 0..1);
    }

    fn draw_local_axes<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if !self.display.show_local_axes || scene.vis.local_axes_vertex_count == 0 {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        pass.set_vertex_buffer(0, scene.vis.local_axes_vertex_buf.slice(..));
        pass.draw(0..scene.vis.local_axes_vertex_count, 0..1);
    }

    fn draw_bounds<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if self.display.bounds_mode == BoundsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.gizmo);
        pass.set_bind_group(0, &scene.cam.bind_group, &[]);
        match self.display.bounds_mode {
            BoundsMode::Off => unreachable!(),
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

    fn draw_normals<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, scene: &'a ModelScene) {
        if self.display.normals_mode == NormalsMode::Off {
            return;
        }
        pass.set_pipeline(&self.pipelines.normals);
        if matches!(
            self.display.normals_mode,
            NormalsMode::Face | NormalsMode::FaceAndVertex
        ) && scene.vis.face_normals_count > 0
        {
            pass.set_bind_group(0, &scene.vis.face_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.face_normals_buf.slice(..));
            pass.draw(0..scene.vis.face_normals_count, 0..1);
        }
        if matches!(
            self.display.normals_mode,
            NormalsMode::Vertex | NormalsMode::FaceAndVertex
        ) && scene.vis.vertex_normals_count > 0
        {
            pass.set_bind_group(0, &scene.vis.vertex_normals_bind_group, &[]);
            pass.set_vertex_buffer(0, scene.vis.vertex_normals_buf.slice(..));
            pass.draw(0..scene.vis.vertex_normals_count, 0..1);
        }
    }
}
