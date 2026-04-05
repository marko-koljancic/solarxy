use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::model::{self, Vertex};
use crate::cgi::pipeline_builder::PipelineBuilder;
use crate::cgi::texture;

pub(crate) struct Instance {
    pub(crate) position: cgmath::Vector3<f32>,
    pub(crate) rotation: cgmath::Quaternion<f32>,
}

impl Instance {
    pub(crate) fn to_raw(&self) -> InstanceRaw {
        let model =
            cgmath::Matrix4::from_translation(self.position) * cgmath::Matrix4::from(self.rotation);
        InstanceRaw {
            model: model.into(),
            normal: cgmath::Matrix3::from(self.rotation).into(),
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct InstanceRaw {
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

pub(crate) struct Pipelines {
    pub(crate) main: wgpu::RenderPipeline,
    pub(crate) alpha_blend: wgpu::RenderPipeline,
    pub(crate) shadow: wgpu::RenderPipeline,
    pub(crate) floor: wgpu::RenderPipeline,
    pub(crate) ghosted_fill: wgpu::RenderPipeline,
    pub(crate) edge_wire: wgpu::RenderPipeline,
    pub(crate) edge_wire_ghosted: wgpu::RenderPipeline,
    pub(crate) grid: wgpu::RenderPipeline,
    pub(crate) normals: wgpu::RenderPipeline,
    pub(crate) background: wgpu::RenderPipeline,
    pub(crate) uv_gradient: wgpu::RenderPipeline,
    pub(crate) uv_checker: wgpu::RenderPipeline,
    pub(crate) uv_no_uvs: wgpu::RenderPipeline,
    pub(crate) gizmo: wgpu::RenderPipeline,
    pub(crate) bloom_extract: wgpu::RenderPipeline,
    pub(crate) bloom_blur_h: wgpu::RenderPipeline,
    pub(crate) bloom_blur_v: wgpu::RenderPipeline,
    pub(crate) composite: wgpu::RenderPipeline,
    pub(crate) gbuffer: wgpu::RenderPipeline,
    pub(crate) ssao: wgpu::RenderPipeline,
    pub(crate) ssao_blur_h: wgpu::RenderPipeline,
    pub(crate) ssao_blur_v: wgpu::RenderPipeline,
}

fn model_instance_buffers() -> Vec<wgpu::VertexBufferLayout<'static>> {
    vec![
        model::ModelVertex::description(),
        InstanceRaw::description(),
    ]
}

impl Pipelines {
    pub(crate) fn new(
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

        let alpha_blend =
            PipelineBuilder::new(device, "Alpha Blend Pipeline", &main_layout, &main_shader)
                .buffers(model_instance_buffers())
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

        let grid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Grid Pipeline Layout"),
            bind_group_layouts: &[&layouts.grid],
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
            bind_group_layouts: &[&layouts.normals],
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
            bind_group_layouts: &[&layouts.ssao],
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
            bind_group_layouts: &[&layouts.ssao_blur],
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

        Pipelines {
            main,
            alpha_blend,
            shadow,
            floor,
            ghosted_fill,
            edge_wire,
            edge_wire_ghosted,
            grid,
            normals,
            background,
            uv_gradient,
            uv_checker,
            uv_no_uvs,
            gizmo,
            bloom_extract,
            bloom_blur_h,
            bloom_blur_v,
            composite,
            gbuffer,
            ssao,
            ssao_blur_h,
            ssao_blur_v,
        }
    }
}
