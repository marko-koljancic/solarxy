//! All `wgpu::RenderPipeline`s used by the renderer, grouped by purpose:
//! [`ScenePipelines`] (PBR, shadow, floor, wireframes, gbuffer),
//! [`PostProcessingPipelines`] (bloom, SSAO, composite),
//! [`OverlayPipelines`] (grid, normals, background, gizmo, validation),
//! [`UvPipelines`] (UV map / overlap / debug), and
//! [`InspectionPipelines`] (reserved for inspection-mode-specific pipelines
//! — empty in 0.6.0 Stream A; populated by the Overdraw work in Stream D).
//!
//! Built once at startup via [`Pipelines::new`] and reused. The fluent
//! builder in [`crate::pipeline_builder`] cuts boilerplate.

use crate::bind_groups::BindGroupLayouts;
use crate::model::{self, Vertex};
use crate::pipeline_builder::PipelineBuilder;
use crate::texture;

pub struct Instance {
    pub position: cgmath::Vector3<f32>,
    pub rotation: cgmath::Quaternion<f32>,
}

impl Instance {
    pub fn to_raw(&self) -> InstanceRaw {
        let model =
            cgmath::Matrix4::from_translation(self.position) * cgmath::Matrix4::from(self.rotation);
        InstanceRaw {
            model: model.into(),
            normal: cgmath::Matrix3::from(self.rotation).into(),
        }
    }
}

impl InstanceRaw {
    /// Build from an arbitrary world matrix (the `SceneOp::SetTransform`
    /// path). The normal matrix is the inverse-transpose of the upper 3x3,
    /// correct under non-uniform scale; a singular matrix falls back to
    /// identity.
    pub fn from_matrix(model: cgmath::Matrix4<f32>) -> Self {
        use cgmath::{Matrix, SquareMatrix};
        let upper = cgmath::Matrix3::new(
            model.x.x, model.x.y, model.x.z, model.y.x, model.y.y, model.y.z, model.z.x, model.z.y,
            model.z.z,
        );
        let normal = upper
            .invert()
            .unwrap_or_else(cgmath::Matrix3::identity)
            .transpose();
        Self {
            model: model.into(),
            normal: normal.into(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct InstanceRaw {
    model: [[f32; 4]; 4],
    normal: [[f32; 3]; 3],
}

impl model::Vertex for InstanceRaw {
    fn description() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<InstanceRaw>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 5,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 6,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 7,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 12]>() as wgpu::BufferAddress,
                    shader_location: 8,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 16]>() as wgpu::BufferAddress,
                    shader_location: 9,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 19]>() as wgpu::BufferAddress,
                    shader_location: 10,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 22]>() as wgpu::BufferAddress,
                    shader_location: 11,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

pub struct ScenePipelines {
    pub main: wgpu::RenderPipeline,
    /// `main` with the color vertex buffer at slot 2 (`vs_main_colored`);
    /// selected per mesh when a color lane is present.
    pub main_colored: wgpu::RenderPipeline,
    pub alpha_blend: wgpu::RenderPipeline,
    pub alpha_blend_colored: wgpu::RenderPipeline,
    pub shadow: wgpu::RenderPipeline,
    pub floor: wgpu::RenderPipeline,
    pub ghosted_fill: wgpu::RenderPipeline,
    pub edge_wire: wgpu::RenderPipeline,
    pub edge_wire_ghosted: wgpu::RenderPipeline,
    pub gbuffer: wgpu::RenderPipeline,
    /// Lines-topology scene meshes: 1 px hardware line list, unlit white.
    pub line: wgpu::RenderPipeline,
    /// `line` with the color vertex buffer (per-vertex color).
    pub line_colored: wgpu::RenderPipeline,
    /// Points-topology scene meshes: camera-facing quads expanded in the
    /// vertex shader from the edge-geometry storage buffer (M-6).
    pub point: wgpu::RenderPipeline,
}

pub struct PostProcessingPipelines {
    pub bloom_extract: wgpu::RenderPipeline,
    pub bloom_blur_h: wgpu::RenderPipeline,
    pub bloom_blur_v: wgpu::RenderPipeline,
    pub composite: wgpu::RenderPipeline,
    pub ssao: wgpu::RenderPipeline,
    pub ssao_blur_h: wgpu::RenderPipeline,
    pub ssao_blur_v: wgpu::RenderPipeline,
}

pub struct OverlayPipelines {
    pub grid: wgpu::RenderPipeline,
    pub normals: wgpu::RenderPipeline,
    pub background: wgpu::RenderPipeline,
    pub skybox: wgpu::RenderPipeline,
    pub gizmo: wgpu::RenderPipeline,
    /// The transform manipulator, drawn ON TOP of the scene (`depth_compare:
    /// Always`): a handle buried inside the mesh it is meant to move is useless.
    /// Same shader, same vertex format, same bind group as `gizmo` -- only the
    /// depth test and the topology differ.
    pub manipulator_lines: wgpu::RenderPipeline,
    pub manipulator_tris: wgpu::RenderPipeline,
    pub validation_overlay: wgpu::RenderPipeline,
    pub validation_edge: wgpu::RenderPipeline,
    /// Selection outline: the silhouette mask (validation.wgsl's
    /// transform-only stages into an R8 target, no depth), the jump-flood
    /// init and step passes (`Rg32Float` ping-pong), and the rim blit onto
    /// the composited swapchain view.
    pub outline_mask: wgpu::RenderPipeline,
    /// M-15: line and point meshes silhouette into the same mask.
    pub outline_mask_line: wgpu::RenderPipeline,
    pub outline_mask_point: wgpu::RenderPipeline,
    pub outline_jfa_init: wgpu::RenderPipeline,
    pub outline_jfa_step: wgpu::RenderPipeline,
    pub outline_blit: wgpu::RenderPipeline,
}

pub struct UvPipelines {
    pub uv_gradient: wgpu::RenderPipeline,
    pub uv_checker: wgpu::RenderPipeline,
    pub uv_no_uvs: wgpu::RenderPipeline,
    pub uv_map_checker: wgpu::RenderPipeline,
    pub uv_map_texture: wgpu::RenderPipeline,
    pub uv_map_wire: wgpu::RenderPipeline,
    pub uv_overlap_count: wgpu::RenderPipeline,
    pub uv_overlap_overlay: wgpu::RenderPipeline,
}

pub struct InspectionPipelines {
    pub overdraw_count: wgpu::RenderPipeline,
    pub overdraw_show: wgpu::RenderPipeline,
}

pub struct Pipelines {
    pub scene: ScenePipelines,
    pub post: PostProcessingPipelines,
    pub overlay: OverlayPipelines,
    pub uv: UvPipelines,
    pub inspection: InspectionPipelines,
}

fn model_instance_buffers() -> Vec<wgpu::VertexBufferLayout<'static>> {
    vec![
        model::ModelVertex::description(),
        InstanceRaw::description(),
    ]
}

/// The colored-variant slot set: the regular pair plus the per-vertex
/// color buffer at slot 2 (shader location 12).
fn model_instance_color_buffers() -> Vec<wgpu::VertexBufferLayout<'static>> {
    vec![
        model::ModelVertex::description(),
        InstanceRaw::description(),
        model::color_vertex_layout(),
    ]
}

impl Pipelines {
    /// Builds every render pipeline once at startup; returns them grouped
    /// into [`ScenePipelines`], [`PostProcessingPipelines`], [`OverlayPipelines`],
    /// [`UvPipelines`], and [`InspectionPipelines`]. Each pipeline reuses
    /// layouts from `layouts` (which itself is the single source of truth
    /// for bind-group layouts) and a shared `sample_count` for MSAA-aware
    /// pipelines.
    pub fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        layouts: &BindGroupLayouts,
        sample_count: u32,
    ) -> Self {
        let hdr_format = texture::Texture::HDR_FORMAT;

        let shadow_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Shadow Pipeline Layout"),
            bind_group_layouts: &[&layouts.shadow_pass, &layouts.texture],
            push_constant_ranges: &[],
        });
        let shadow_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Shadow Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
        });
        let shadow =
            PipelineBuilder::new(device, "Shadow Pipeline", &shadow_layout, &shadow_shader)
                .vertex_entry("vs_shadow")
                .fragment_entry("fs_shadow")
                .buffers(model_instance_buffers())
                .cull_back()
                .depth_format(wgpu::TextureFormat::Depth32Float)
                .depth_compare(wgpu::CompareFunction::Less)
                .depth_bias(wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                })
                .build();

        let main_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Rendering Pipeline Layout"),
            bind_group_layouts: &[
                &layouts.texture,
                &layouts.camera,
                &layouts.light,
                &layouts.shadow_read,
            ],
            push_constant_ranges: &[],
        });
        let main_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Normal Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shader.wgsl").into()),
        });
        let main = PipelineBuilder::new(device, "Render Pipeline", &main_layout, &main_shader)
            .buffers(model_instance_buffers())
            .color_format(hdr_format)
            .cull_back()
            .depth_compare(wgpu::CompareFunction::Less)
            .sample_count(sample_count)
            .build();

        let main_colored = PipelineBuilder::new(
            device,
            "Render Pipeline (colored)",
            &main_layout,
            &main_shader,
        )
        .vertex_entry("vs_main_colored")
        .buffers(model_instance_color_buffers())
        .color_format(hdr_format)
        .cull_back()
        .depth_compare(wgpu::CompareFunction::Less)
        .sample_count(sample_count)
        .build();

        let alpha_blend =
            PipelineBuilder::new(device, "Alpha Blend Pipeline", &main_layout, &main_shader)
                .buffers(model_instance_buffers())
                .color_format(hdr_format)
                .blend_alpha()
                .depth_write(false)
                .sample_count(sample_count)
                .build();

        let alpha_blend_colored = PipelineBuilder::new(
            device,
            "Alpha Blend Pipeline (colored)",
            &main_layout,
            &main_shader,
        )
        .vertex_entry("vs_main_colored")
        .buffers(model_instance_color_buffers())
        .color_format(hdr_format)
        .blend_alpha()
        .depth_write(false)
        .sample_count(sample_count)
        .build();

        let floor_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Floor Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.shadow_read],
            push_constant_ranges: &[],
        });
        let floor_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Floor Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/floor.wgsl").into()),
        });
        let floor = PipelineBuilder::new(device, "Floor Pipeline", &floor_layout, &floor_shader)
            .vertex_entry("vs_floor")
            .fragment_entry("fs_floor")
            .buffers(vec![model::ModelVertex::description()])
            .color_format(hdr_format)
            .blend_alpha()
            .depth_write(false)
            .depth_compare(wgpu::CompareFunction::Less)
            .sample_count(sample_count)
            .build();

        let ghosted_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ghosted Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let ghosted_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ghosted Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ghosted.wgsl").into()),
        });
        let ghosted_fill =
            PipelineBuilder::new(device, "fs_ghosted_fill", &ghosted_layout, &ghosted_shader)
                .vertex_entry("vs_ghosted")
                .fragment_entry("fs_ghosted_fill")
                .buffers(model_instance_buffers())
                .color_format(hdr_format)
                .blend_alpha()
                .depth_write(false)
                .depth_compare(wgpu::CompareFunction::Less)
                .sample_count(sample_count)
                .build();

        let validation_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Validation Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.validation_color],
            push_constant_ranges: &[],
        });
        let validation_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Validation Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/validation.wgsl").into()),
        });

        let validation_overlay = PipelineBuilder::new(
            device,
            "Validation Overlay",
            &validation_layout,
            &validation_shader,
        )
        .vertex_entry("vs_validation")
        .fragment_entry("fs_validation")
        .buffers(model_instance_buffers())
        .color_format(hdr_format)
        .blend_alpha()
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::LessEqual)
        .depth_bias(wgpu::DepthBiasState {
            constant: -4,
            slope_scale: -1.0,
            clamp: 0.0,
        })
        .sample_count(sample_count)
        .build();

        // Selection outline. The mask reuses validation.wgsl's
        // transform-only vertex stage with a white color uniform; it
        // ignores depth entirely (the rim marks the full screen-space
        // silhouette of the selection, occluded or not).
        let outline_mask = PipelineBuilder::new(
            device,
            "Outline Mask",
            &validation_layout,
            &validation_shader,
        )
        .vertex_entry("vs_validation")
        .fragment_entry("fs_validation")
        .buffers(model_instance_buffers())
        .color_format(wgpu::TextureFormat::R8Unorm)
        .no_blend()
        .no_depth()
        .build();

        // Per decision M-15, line and point meshes join the selection
        // outline like triangles: the same mask target, assembled per
        // their topology (the point variant is built later, after the
        // points/lines shader exists; see `outline_mask_point`).
        let outline_mask_line = PipelineBuilder::new(
            device,
            "Outline Mask (lines)",
            &validation_layout,
            &validation_shader,
        )
        .vertex_entry("vs_validation")
        .fragment_entry("fs_validation")
        .buffers(model_instance_buffers())
        .topology(wgpu::PrimitiveTopology::LineList)
        .color_format(wgpu::TextureFormat::R8Unorm)
        .no_blend()
        .no_depth()
        .build();

        let outline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Outline JFA Pipeline Layout"),
            bind_group_layouts: &[&layouts.outline_texture, &layouts.outline_params],
            push_constant_ranges: &[],
        });
        let outline_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Outline Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/outline.wgsl").into()),
        });
        let outline_jfa_init =
            PipelineBuilder::new(device, "Outline JFA Init", &outline_layout, &outline_shader)
                .vertex_entry("vs_fullscreen")
                .fragment_entry("fs_jfa_init")
                .color_format(wgpu::TextureFormat::Rg32Float)
                .no_blend()
                .no_depth()
                .build();
        let outline_jfa_step =
            PipelineBuilder::new(device, "Outline JFA Step", &outline_layout, &outline_shader)
                .vertex_entry("vs_fullscreen")
                .fragment_entry("fs_jfa_step")
                .color_format(wgpu::TextureFormat::Rg32Float)
                .no_blend()
                .no_depth()
                .build();
        let outline_blit =
            PipelineBuilder::new(device, "Outline Blit", &outline_layout, &outline_shader)
                .vertex_entry("vs_fullscreen")
                .fragment_entry("fs_outline")
                .color_format(config.format)
                .blend_alpha()
                .no_depth()
                .build();

        let validation_edge = PipelineBuilder::new(
            device,
            "Validation Edge Lines",
            &validation_layout,
            &validation_shader,
        )
        .vertex_entry("vs_validation")
        .fragment_entry("fs_validation")
        .buffers(model_instance_buffers())
        .color_format(hdr_format)
        .blend_alpha()
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::LessEqual)
        // No depth bias: WebGPU requires depthBias == 0 for line topologies
        // (native wgpu tolerated the -8 nudge). LessEqual with depth-write
        // off already lets on-edge lines pass; if desktop QA shows edge
        // z-fighting, the offset moves into vs_validation as a clip-space
        // nudge instead.
        .topology(wgpu::PrimitiveTopology::LineList)
        .sample_count(sample_count)
        .build();

        let edge_wire_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Edge Wire Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/edge_wire.wgsl").into()),
        });
        let edge_wire_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Edge Wire Pipeline Layout"),
            bind_group_layouts: &[
                &layouts.camera,
                &layouts.wireframe_params,
                &layouts.edge_geometry,
            ],
            push_constant_ranges: &[],
        });
        let edge_wire =
            PipelineBuilder::new(device, "fs_edge_wire", &edge_wire_layout, &edge_wire_shader)
                .vertex_entry("vs_edge_quad")
                .fragment_entry("fs_edge_wire")
                .buffers(vec![InstanceRaw::description()])
                .color_format(hdr_format)
                .depth_bias(wgpu::DepthBiasState {
                    constant: -2,
                    slope_scale: -2.0,
                    clamp: 0.0,
                })
                .sample_count(sample_count)
                .build();
        let edge_wire_ghosted = PipelineBuilder::new(
            device,
            "fs_edge_wire_ghosted",
            &edge_wire_layout,
            &edge_wire_shader,
        )
        .vertex_entry("vs_edge_quad")
        .fragment_entry("fs_edge_wire_ghosted")
        .buffers(vec![InstanceRaw::description()])
        .color_format(hdr_format)
        .blend_alpha()
        .depth_write(false)
        .sample_count(sample_count)
        .build();

        // Non-triangle scene topologies (0.8.0): 1 px unlit lines over the
        // regular vertex buffer, and camera-facing point quads pulled from
        // the edge-geometry storage buffer (see points_lines.wgsl).
        let points_lines_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Points/Lines Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/points_lines.wgsl").into()),
        });
        let line_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Line Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let line =
            PipelineBuilder::new(device, "Line Pipeline", &line_layout, &points_lines_shader)
                .vertex_entry("vs_line")
                .fragment_entry("fs_unlit")
                .buffers(vec![
                    model::position_only_layout(),
                    InstanceRaw::description(),
                ])
                .topology(wgpu::PrimitiveTopology::LineList)
                .color_format(hdr_format)
                .depth_compare(wgpu::CompareFunction::Less)
                .sample_count(sample_count)
                .build();
        let line_colored = PipelineBuilder::new(
            device,
            "Line Pipeline (colored)",
            &line_layout,
            &points_lines_shader,
        )
        .vertex_entry("vs_line_colored")
        .fragment_entry("fs_unlit")
        .buffers(vec![
            model::position_only_layout(),
            InstanceRaw::description(),
            model::color_vertex_layout(),
        ])
        .topology(wgpu::PrimitiveTopology::LineList)
        .color_format(hdr_format)
        .depth_compare(wgpu::CompareFunction::Less)
        .sample_count(sample_count)
        .build();

        let point_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Point Pipeline Layout"),
            bind_group_layouts: &[
                &layouts.camera,
                &layouts.wireframe_params,
                &layouts.edge_geometry,
            ],
            push_constant_ranges: &[],
        });
        let point = PipelineBuilder::new(
            device,
            "Point Pipeline",
            &point_layout,
            &points_lines_shader,
        )
        .vertex_entry("vs_point")
        .fragment_entry("fs_unlit")
        .buffers(vec![InstanceRaw::description()])
        .color_format(hdr_format)
        .depth_compare(wgpu::CompareFunction::Less)
        .sample_count(sample_count)
        .build();

        // The point half of M-15: the same quad expansion into the
        // outline mask's R8 target (single-sampled, depth-ignoring, like
        // `outline_mask`).
        let outline_mask_point = PipelineBuilder::new(
            device,
            "Outline Mask (points)",
            &point_layout,
            &points_lines_shader,
        )
        .vertex_entry("vs_point")
        .fragment_entry("fs_mask")
        .buffers(vec![InstanceRaw::description()])
        .color_format(wgpu::TextureFormat::R8Unorm)
        .no_blend()
        .no_depth()
        .build();

        let grid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.grid_params],
            push_constant_ranges: &[],
        });
        let grid_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Grid Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/grid.wgsl").into()),
        });
        let grid = PipelineBuilder::new(device, "Grid Pipeline", &grid_layout, &grid_shader)
            .vertex_entry("vs_grid")
            .fragment_entry("fs_grid")
            .buffers(vec![model::LineVertex::description()])
            .color_format(hdr_format)
            .blend_alpha()
            .depth_write(false)
            .depth_compare(wgpu::CompareFunction::Less)
            .sample_count(sample_count)
            .build();

        let normals_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Normals Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.normals_params],
            push_constant_ranges: &[],
        });
        let normals_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Normals Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/normals.wgsl").into()),
        });
        let normals =
            PipelineBuilder::new(device, "Normals Pipeline", &normals_layout, &normals_shader)
                .vertex_entry("vs_normals")
                .fragment_entry("fs_normals")
                .buffers(vec![model::LineVertex::description()])
                .color_format(hdr_format)
                .topology(wgpu::PrimitiveTopology::LineList)
                .depth_write(false)
                .sample_count(sample_count)
                .build();

        let bg_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Background Pipeline Layout"),
            bind_group_layouts: &[&layouts.background],
            push_constant_ranges: &[],
        });
        let bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Background Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/background.wgsl").into()),
        });
        let background =
            PipelineBuilder::new(device, "Background Pipeline", &bg_layout, &bg_shader)
                .vertex_entry("vs_background")
                .fragment_entry("fs_background")
                .color_format(hdr_format)
                .depth_compare(wgpu::CompareFunction::Always)
                .sample_count(sample_count)
                .build();

        let skybox_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Skybox Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.skybox],
            push_constant_ranges: &[],
        });
        let skybox_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Skybox Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/skybox.wgsl").into()),
        });
        let skybox = PipelineBuilder::new(device, "Skybox Pipeline", &skybox_layout, &skybox_shader)
                .vertex_entry("vs_skybox")
                .fragment_entry("fs_skybox")
                .color_format(hdr_format)
                .depth_compare(wgpu::CompareFunction::Always)
                // Vertex z is the far plane; the depth buffer is already
                // cleared to 1.0 and the skybox draws first, so writing
                // depth is a redundant no-op — disable it explicitly.
                .depth_write(false)
                .sample_count(sample_count)
                .build();

        let uv_debug_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("UV Debug Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/uv_debug.wgsl").into()),
        });
        let uv_camera_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UV Camera-Only Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let uv_gradient = PipelineBuilder::new(
            device,
            "fs_uv_gradient",
            &uv_camera_layout,
            &uv_debug_shader,
        )
        .vertex_entry("vs_uv_debug")
        .fragment_entry("fs_uv_gradient")
        .buffers(model_instance_buffers())
        .color_format(hdr_format)
        .cull_back()
        .depth_compare(wgpu::CompareFunction::Less)
        .sample_count(sample_count)
        .build();

        let uv_no_uvs =
            PipelineBuilder::new(device, "fs_uv_no_uvs", &uv_camera_layout, &uv_debug_shader)
                .vertex_entry("vs_uv_debug")
                .fragment_entry("fs_uv_no_uvs")
                .buffers(model_instance_buffers())
                .color_format(hdr_format)
                .cull_back()
                .depth_compare(wgpu::CompareFunction::Less)
                .sample_count(sample_count)
                .build();

        let uv_checker_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UV Checker Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.uv_checker],
            push_constant_ranges: &[],
        });
        let uv_checker = PipelineBuilder::new(
            device,
            "fs_uv_checker",
            &uv_checker_layout,
            &uv_debug_shader,
        )
        .vertex_entry("vs_uv_debug")
        .fragment_entry("fs_uv_checker")
        .buffers(model_instance_buffers())
        .color_format(hdr_format)
        .cull_back()
        .depth_compare(wgpu::CompareFunction::Less)
        .sample_count(sample_count)
        .build();

        let gizmo_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Gizmo Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let gizmo_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Gizmo Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/gizmo.wgsl").into()),
        });
        let gizmo = PipelineBuilder::new(device, "Gizmo Pipeline", &gizmo_layout, &gizmo_shader)
            .vertex_entry("vs_gizmo")
            .fragment_entry("fs_gizmo")
            .buffers(vec![model::GizmoVertex::description()])
            .color_format(hdr_format)
            .topology(wgpu::PrimitiveTopology::LineList)
            .depth_write(false)
            .sample_count(sample_count)
            .build();

        // The manipulator rides the gizmo shader unchanged; it only needs to
        // ignore depth (so handles are never swallowed by geometry) and to draw
        // solid arrowheads and plane quads as well as line shafts.
        let manipulator_lines = PipelineBuilder::new(
            device,
            "Manipulator Lines Pipeline",
            &gizmo_layout,
            &gizmo_shader,
        )
        .vertex_entry("vs_gizmo")
        .fragment_entry("fs_gizmo")
        .buffers(vec![model::GizmoVertex::description()])
        .color_format(hdr_format)
        .topology(wgpu::PrimitiveTopology::LineList)
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::Always)
        .sample_count(sample_count)
        .build();

        let manipulator_tris = PipelineBuilder::new(
            device,
            "Manipulator Tris Pipeline",
            &gizmo_layout,
            &gizmo_shader,
        )
        .vertex_entry("vs_gizmo")
        .fragment_entry("fs_gizmo")
        .buffers(vec![model::GizmoVertex::description()])
        .color_format(hdr_format)
        .topology(wgpu::PrimitiveTopology::TriangleList)
        // No culling: a plane handle must read from either side.
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::Always)
        .sample_count(sample_count)
        .build();

        let uv_map_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("UV Map Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/uv_map.wgsl").into()),
        });
        let uv_map_checker = PipelineBuilder::new(
            device,
            "UV Map Checker Pipeline",
            &uv_checker_layout,
            &uv_map_shader,
        )
        .vertex_entry("vs_uv_fill")
        .fragment_entry("fs_uv_checker")
        .buffers(model_instance_buffers())
        .color_format(hdr_format)
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::Always)
        .sample_count(sample_count)
        .build();

        let uv_map_texture_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("UV Map Texture Pipeline Layout"),
                bind_group_layouts: &[&layouts.camera, &layouts.texture],
                push_constant_ranges: &[],
            });
        let uv_map_texture = PipelineBuilder::new(
            device,
            "UV Map Texture Pipeline",
            &uv_map_texture_layout,
            &uv_map_shader,
        )
        .vertex_entry("vs_uv_fill")
        .fragment_entry("fs_uv_texture")
        .buffers(model_instance_buffers())
        .color_format(hdr_format)
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::Always)
        .sample_count(sample_count)
        .build();

        let uv_map_wire = PipelineBuilder::new(
            device,
            "UV Map Wire Pipeline",
            &edge_wire_layout,
            &edge_wire_shader,
        )
        .vertex_entry("vs_uv_edge_quad")
        .fragment_entry("fs_edge_wire")
        .color_format(hdr_format)
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::Always)
        .sample_count(sample_count)
        .build();

        let uv_overlap_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("UV Overlap Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/uv_overlap.wgsl").into()),
        });
        let additive_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
        };
        let uv_overlap_count = PipelineBuilder::new(
            device,
            "UV Overlap Count Pipeline",
            &uv_camera_layout,
            &uv_overlap_shader,
        )
        .vertex_entry("vs_uv_count")
        .fragment_entry("fs_uv_count")
        .buffers(model_instance_buffers())
        .color_format(wgpu::TextureFormat::R8Unorm)
        .blend(additive_blend)
        .no_depth()
        .build();

        let uv_overlap_overlay_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("UV Overlap Overlay Pipeline Layout"),
                bind_group_layouts: &[&layouts.uv_overlap_read],
                push_constant_ranges: &[],
            });
        let uv_overlap_overlay = PipelineBuilder::new(
            device,
            "UV Overlap Overlay Pipeline",
            &uv_overlap_overlay_layout,
            &uv_overlap_shader,
        )
        .vertex_entry("vs_overlap_fullscreen")
        .fragment_entry("fs_uv_overlap")
        .color_format(hdr_format)
        .blend_alpha()
        .depth_write(false)
        .depth_compare(wgpu::CompareFunction::Always)
        .sample_count(sample_count)
        .build();

        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bloom Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bloom.wgsl").into()),
        });
        let bloom_extract_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom Extract Pipeline Layout"),
            bind_group_layouts: &[&layouts.bloom_texture, &layouts.bloom_params],
            push_constant_ranges: &[],
        });
        let bloom_extract = PipelineBuilder::new(
            device,
            "Bloom Extract Pipeline",
            &bloom_extract_layout,
            &bloom_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_brightness_extract")
        .color_format(hdr_format)
        .no_blend()
        .no_depth()
        .build();

        let bloom_blur_h_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom Blur H Pipeline Layout"),
            bind_group_layouts: &[&layouts.bloom_texture, &layouts.bloom_params],
            push_constant_ranges: &[],
        });
        let bloom_blur_h = PipelineBuilder::new(
            device,
            "Bloom Blur H Pipeline",
            &bloom_blur_h_layout,
            &bloom_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_blur_horizontal")
        .color_format(hdr_format)
        .no_blend()
        .no_depth()
        .build();

        let bloom_blur_v_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Bloom Blur V Pipeline Layout"),
            bind_group_layouts: &[&layouts.bloom_texture, &layouts.bloom_params],
            push_constant_ranges: &[],
        });
        let bloom_blur_v = PipelineBuilder::new(
            device,
            "Bloom Blur V Pipeline",
            &bloom_blur_v_layout,
            &bloom_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_blur_vertical")
        .color_format(hdr_format)
        .no_blend()
        .no_depth()
        .build();

        let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Composite Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
        });
        let composite_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Composite Pipeline Layout"),
            bind_group_layouts: &[
                &layouts.composite,
                &layouts.composite_params,
                &layouts.ssao_read,
            ],
            push_constant_ranges: &[],
        });
        let composite = PipelineBuilder::new(
            device,
            "Composite Pipeline",
            &composite_layout,
            &composite_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_composite")
        .color_format(config.format)
        .no_blend()
        .no_depth()
        .build();

        let gbuffer_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("G-Buffer Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let gbuffer_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("G-Buffer Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/gbuffer.wgsl").into()),
        });
        let gbuffer = PipelineBuilder::new(
            device,
            "G-Buffer Pipeline",
            &gbuffer_layout,
            &gbuffer_shader,
        )
        .vertex_entry("vs_gbuffer")
        .fragment_entry("fs_gbuffer")
        .buffers(model_instance_buffers())
        .color_format(texture::Texture::HDR_FORMAT)
        .no_blend()
        .cull_back()
        .depth_compare(wgpu::CompareFunction::Less)
        .build();

        let ssao_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SSAO Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ssao.wgsl").into()),
        });
        let ssao_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SSAO Pipeline Layout"),
            bind_group_layouts: &[&layouts.ssao, &layouts.camera],
            push_constant_ranges: &[],
        });
        let ssao = PipelineBuilder::new(device, "SSAO Pipeline", &ssao_layout, &ssao_shader)
            .vertex_entry("vs_fullscreen")
            .fragment_entry("fs_ssao")
            .color_format(wgpu::TextureFormat::R8Unorm)
            .no_blend()
            .no_depth()
            .build();

        let ssao_blur_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SSAO Blur Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ssao_blur.wgsl").into()),
        });
        let ssao_blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SSAO Blur Pipeline Layout"),
            bind_group_layouts: &[&layouts.ssao_blur, &layouts.camera],
            push_constant_ranges: &[],
        });
        let ssao_blur_h = PipelineBuilder::new(
            device,
            "SSAO Blur H Pipeline",
            &ssao_blur_layout,
            &ssao_blur_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_blur_h")
        .color_format(wgpu::TextureFormat::R8Unorm)
        .no_blend()
        .no_depth()
        .build();
        let ssao_blur_v = PipelineBuilder::new(
            device,
            "SSAO Blur V Pipeline",
            &ssao_blur_layout,
            &ssao_blur_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_blur_v")
        .color_format(wgpu::TextureFormat::R8Unorm)
        .no_blend()
        .no_depth()
        .build();

        let overdraw_count_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Overdraw Count Pipeline Layout"),
                bind_group_layouts: &[&layouts.camera],
                push_constant_ranges: &[],
            });
        let overdraw_count_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Overdraw Count Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/overdraw_count.wgsl").into()),
        });
        let overdraw_count_blend = wgpu::BlendState {
            color: wgpu::BlendComponent {
                src_factor: wgpu::BlendFactor::One,
                dst_factor: wgpu::BlendFactor::One,
                operation: wgpu::BlendOperation::Add,
            },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let overdraw_count = PipelineBuilder::new(
            device,
            "Overdraw Count Pipeline",
            &overdraw_count_layout,
            &overdraw_count_shader,
        )
        .vertex_entry("vs_count")
        .fragment_entry("fs_count")
        .buffers(model_instance_buffers())
        .color_format(crate::overdraw::COUNT_FORMAT)
        .blend(overdraw_count_blend)
        .no_depth()
        .build();

        let overdraw_show_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Overdraw Show Pipeline Layout"),
            bind_group_layouts: &[&layouts.overdraw_show],
            push_constant_ranges: &[],
        });
        let overdraw_show_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Overdraw Show Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/overdraw_show.wgsl").into()),
        });
        let overdraw_show = PipelineBuilder::new(
            device,
            "Overdraw Show Pipeline",
            &overdraw_show_layout,
            &overdraw_show_shader,
        )
        .vertex_entry("vs_fullscreen")
        .fragment_entry("fs_show")
        .color_format(hdr_format)
        .no_blend()
        .no_depth()
        .build();

        Pipelines {
            scene: ScenePipelines {
                main,
                main_colored,
                alpha_blend,
                alpha_blend_colored,
                shadow,
                floor,
                ghosted_fill,
                edge_wire,
                edge_wire_ghosted,
                gbuffer,
                line,
                line_colored,
                point,
            },
            post: PostProcessingPipelines {
                bloom_extract,
                bloom_blur_h,
                bloom_blur_v,
                composite,
                ssao,
                ssao_blur_h,
                ssao_blur_v,
            },
            overlay: OverlayPipelines {
                grid,
                normals,
                background,
                skybox,
                gizmo,
                manipulator_lines,
                manipulator_tris,
                validation_overlay,
                validation_edge,
                outline_mask,
                outline_mask_line,
                outline_mask_point,
                outline_jfa_init,
                outline_jfa_step,
                outline_blit,
            },
            uv: UvPipelines {
                uv_gradient,
                uv_checker,
                uv_no_uvs,
                uv_map_checker,
                uv_map_texture,
                uv_map_wire,
                uv_overlap_count,
                uv_overlap_overlay,
            },
            inspection: InspectionPipelines {
                overdraw_count,
                overdraw_show,
            },
        }
    }
}
