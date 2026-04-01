use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::model::{self, Vertex};
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
}

impl Pipelines {
    pub(crate) fn new(
        device: &wgpu::Device,
        config: &wgpu::SurfaceConfiguration,
        layouts: &BindGroupLayouts,
        sample_count: u32,
    ) -> Self {
        let hdr_format = texture::Texture::HDR_FORMAT;

        let shadow_pipeline = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Shadow Pipeline Layout"),
                bind_group_layouts: &[&layouts.shadow_pass, &layouts.texture],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Shadow Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shadow.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Shadow Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_shadow"),
                    buffers: &[
                        model::ModelVertex::description(),
                        InstanceRaw::description(),
                    ],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_shadow"),
                    targets: &[],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: Some(wgpu::Face::Back),
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState {
                        constant: 2,
                        slope_scale: 2.0,
                        clamp: 0.0,
                    },
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
                cache: None,
            })
        };

        let main = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Rendering Pipeline Layout"),
                bind_group_layouts: &[
                    &layouts.texture,
                    &layouts.camera,
                    &layouts.light,
                    &layouts.shadow_read,
                ],
                push_constant_ranges: &[],
            });
            create_render_pipeline(
                device,
                &layout,
                hdr_format,
                Some(texture::Texture::DEPTH_FORMAT),
                &[
                    model::ModelVertex::description(),
                    InstanceRaw::description(),
                ],
                wgpu::ShaderModuleDescriptor {
                    label: Some("Normal Shader"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("shaders/shader.wgsl").into()),
                },
                sample_count,
            )
        };

        let floor = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Floor Pipeline Layout"),
                bind_group_layouts: &[&layouts.camera, &layouts.shadow_read],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Floor Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/floor.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Floor Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_floor"),
                    buffers: &[model::ModelVertex::description()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_floor"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let ghosted_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Ghosted Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let ghosted_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Ghosted Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/ghosted.wgsl").into()),
        });

        let ghosted_fill = create_ghosted_pipeline(
            device,
            &ghosted_layout,
            &ghosted_shader,
            hdr_format,
            "vs_ghosted",
            "fs_ghosted_fill",
            wgpu::PolygonMode::Fill,
            false,
            None,
            sample_count,
        );
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
        let edge_wire = create_edge_wire_pipeline(
            device,
            &edge_wire_layout,
            &edge_wire_shader,
            hdr_format,
            "fs_edge_wire",
            true,
            Some(wgpu::DepthBiasState {
                constant: -2,
                slope_scale: -2.0,
                clamp: 0.0,
            }),
            sample_count,
        );
        let edge_wire_ghosted = create_edge_wire_pipeline(
            device,
            &edge_wire_layout,
            &edge_wire_shader,
            hdr_format,
            "fs_edge_wire_ghosted",
            false,
            None,
            sample_count,
        );

        let grid = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Grid Pipeline Layout"),
                bind_group_layouts: &[&layouts.grid],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Grid Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/grid.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Grid Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_grid"),
                    buffers: &[model::LineVertex::description()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_grid"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::Less,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let normals = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Normals Pipeline Layout"),
                bind_group_layouts: &[&layouts.normals],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Normals Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/normals.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Normals Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_normals"),
                    buffers: &[model::LineVertex::description()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_normals"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(wgpu::BlendState {
                            alpha: wgpu::BlendComponent::REPLACE,
                            color: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let background = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Background Pipeline Layout"),
                bind_group_layouts: &[&layouts.background],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Background Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/background.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Background Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_background"),
                    buffers: &[],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_background"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(wgpu::BlendState {
                            alpha: wgpu::BlendComponent::REPLACE,
                            color: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: true,
                    depth_compare: wgpu::CompareFunction::Always,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let uv_debug_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("UV Debug Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/uv_debug.wgsl").into()),
        });
        let uv_camera_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UV Camera-Only Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera],
            push_constant_ranges: &[],
        });
        let uv_gradient = create_ghosted_pipeline(
            device,
            &uv_camera_layout,
            &uv_debug_shader,
            hdr_format,
            "vs_uv_debug",
            "fs_uv_gradient",
            wgpu::PolygonMode::Fill,
            true,
            None,
            sample_count,
        );
        let uv_no_uvs = create_ghosted_pipeline(
            device,
            &uv_camera_layout,
            &uv_debug_shader,
            hdr_format,
            "vs_uv_debug",
            "fs_uv_no_uvs",
            wgpu::PolygonMode::Fill,
            true,
            None,
            sample_count,
        );
        let uv_checker_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("UV Checker Pipeline Layout"),
            bind_group_layouts: &[&layouts.camera, &layouts.uv_checker],
            push_constant_ranges: &[],
        });
        let uv_checker = create_ghosted_pipeline(
            device,
            &uv_checker_layout,
            &uv_debug_shader,
            hdr_format,
            "vs_uv_debug",
            "fs_uv_checker",
            wgpu::PolygonMode::Fill,
            true,
            None,
            sample_count,
        );

        let gizmo = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Gizmo Pipeline Layout"),
                bind_group_layouts: &[&layouts.camera],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Gizmo Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/gizmo.wgsl").into()),
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Gizmo Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_gizmo"),
                    buffers: &[model::GizmoVertex::description()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_gizmo"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: hdr_format,
                        blend: Some(wgpu::BlendState {
                            alpha: wgpu::BlendComponent::REPLACE,
                            color: wgpu::BlendComponent::REPLACE,
                        }),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::LineList,
                    front_face: wgpu::FrontFace::Ccw,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: texture::Texture::DEPTH_FORMAT,
                    depth_write_enabled: false,
                    depth_compare: wgpu::CompareFunction::LessEqual,
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: sample_count,
                    mask: !0,
                    alpha_to_coverage_enabled: false,
                },
                multiview: None,
                cache: None,
            })
        };

        let bloom_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Bloom Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/bloom.wgsl").into()),
        });

        let bloom_extract = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Bloom Extract Pipeline Layout"),
                bind_group_layouts: &[&layouts.bloom_texture, &layouts.bloom_params],
                push_constant_ranges: &[],
            });
            create_fullscreen_pipeline(
                device,
                &layout,
                &bloom_shader,
                "fs_brightness_extract",
                hdr_format,
                "Bloom Extract Pipeline",
            )
        };

        let bloom_blur_h = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Bloom Blur H Pipeline Layout"),
                bind_group_layouts: &[&layouts.bloom_texture, &layouts.bloom_params],
                push_constant_ranges: &[],
            });
            create_fullscreen_pipeline(
                device,
                &layout,
                &bloom_shader,
                "fs_blur_horizontal",
                hdr_format,
                "Bloom Blur H Pipeline",
            )
        };

        let bloom_blur_v = {
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Bloom Blur V Pipeline Layout"),
                bind_group_layouts: &[&layouts.bloom_texture, &layouts.bloom_params],
                push_constant_ranges: &[],
            });
            create_fullscreen_pipeline(
                device,
                &layout,
                &bloom_shader,
                "fs_blur_vertical",
                hdr_format,
                "Bloom Blur V Pipeline",
            )
        };

        let composite = {
            let composite_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Composite Shader"),
                source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite.wgsl").into()),
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Composite Pipeline Layout"),
                bind_group_layouts: &[&layouts.composite, &layouts.composite_params],
                push_constant_ranges: &[],
            });
            create_fullscreen_pipeline(
                device,
                &layout,
                &composite_shader,
                "fs_composite",
                config.format,
                "Composite Pipeline",
            )
        };

        Pipelines {
            main,
            shadow: shadow_pipeline,
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
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn create_ghosted_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    vertex_entry: &str,
    fragment_entry: &str,
    polygon_mode: wgpu::PolygonMode,
    depth_write: bool,
    depth_bias: Option<wgpu::DepthBiasState>,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let cull_mode = if depth_write {
        Some(wgpu::Face::Back)
    } else {
        None
    };
    let blend = if depth_write {
        wgpu::BlendState {
            alpha: wgpu::BlendComponent::REPLACE,
            color: wgpu::BlendComponent::REPLACE,
        }
    } else {
        wgpu::BlendState::ALPHA_BLENDING
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some(vertex_entry),
            buffers: &[
                model::ModelVertex::description(),
                InstanceRaw::description(),
            ],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode,
            polygon_mode,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: texture::Texture::DEPTH_FORMAT,
            depth_write_enabled: depth_write,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: depth_bias.unwrap_or_default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn create_edge_wire_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    format: wgpu::TextureFormat,
    fragment_entry: &str,
    depth_write: bool,
    depth_bias: Option<wgpu::DepthBiasState>,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let blend = if depth_write {
        wgpu::BlendState {
            alpha: wgpu::BlendComponent::REPLACE,
            color: wgpu::BlendComponent::REPLACE,
        }
    } else {
        wgpu::BlendState::ALPHA_BLENDING
    };

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(fragment_entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_edge_quad"),
            buffers: &[InstanceRaw::description()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(blend),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: texture::Texture::DEPTH_FORMAT,
            depth_write_enabled: depth_write,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: depth_bias.unwrap_or_default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}

fn create_fullscreen_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    fragment_entry: &str,
    format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: Some("vs_fullscreen"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: Some(fragment_entry),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
        cache: None,
    })
}

fn create_render_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    color_format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    vertex_layouts: &[wgpu::VertexBufferLayout],
    shader: wgpu::ShaderModuleDescriptor,
    sample_count: u32,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(shader);
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("Render Pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            buffers: vertex_layouts,
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format: color_format,
                blend: Some(wgpu::BlendState {
                    alpha: wgpu::BlendComponent::REPLACE,
                    color: wgpu::BlendComponent::REPLACE,
                }),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState {
            count: sample_count,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
        cache: None,
    })
}
