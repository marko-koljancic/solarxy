use crate::texture;

pub struct PipelineBuilder<'a> {
    device: &'a wgpu::Device,
    label: &'a str,
    layout: &'a wgpu::PipelineLayout,
    shader: &'a wgpu::ShaderModule,
    vertex_entry: &'a str,
    fragment_entry: Option<&'a str>,
    vertex_buffers: Vec<wgpu::VertexBufferLayout<'a>>,
    color_format: Option<wgpu::TextureFormat>,
    blend: Option<wgpu::BlendState>,
    topology: wgpu::PrimitiveTopology,
    cull_mode: Option<wgpu::Face>,
    polygon_mode: wgpu::PolygonMode,
    depth_format: Option<wgpu::TextureFormat>,
    depth_write: bool,
    depth_compare: wgpu::CompareFunction,
    depth_bias: wgpu::DepthBiasState,
    sample_count: u32,
}

impl<'a> PipelineBuilder<'a> {
    pub fn new(
        device: &'a wgpu::Device,
        label: &'a str,
        layout: &'a wgpu::PipelineLayout,
        shader: &'a wgpu::ShaderModule,
    ) -> Self {
        Self {
            device,
            label,
            layout,
            shader,
            vertex_entry: "vs_main",
            fragment_entry: Some("fs_main"),
            vertex_buffers: Vec::new(),
            color_format: None,
            blend: Some(wgpu::BlendState {
                alpha: wgpu::BlendComponent::REPLACE,
                color: wgpu::BlendComponent::REPLACE,
            }),
            topology: wgpu::PrimitiveTopology::TriangleList,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            depth_format: Some(texture::Texture::DEPTH_FORMAT),
            depth_write: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            depth_bias: wgpu::DepthBiasState::default(),
            sample_count: 1,
        }
    }

    pub fn vertex_entry(mut self, entry: &'a str) -> Self {
        self.vertex_entry = entry;
        self
    }

    pub fn fragment_entry(mut self, entry: &'a str) -> Self {
        self.fragment_entry = Some(entry);
        self
    }

    pub fn buffers(mut self, buffers: Vec<wgpu::VertexBufferLayout<'a>>) -> Self {
        self.vertex_buffers = buffers;
        self
    }

    pub fn color_format(mut self, format: wgpu::TextureFormat) -> Self {
        self.color_format = Some(format);
        self
    }

    pub fn blend(mut self, state: wgpu::BlendState) -> Self {
        self.blend = Some(state);
        self
    }

    pub fn blend_alpha(mut self) -> Self {
        self.blend = Some(wgpu::BlendState::ALPHA_BLENDING);
        self
    }

    pub fn no_blend(mut self) -> Self {
        self.blend = None;
        self
    }

    pub fn topology(mut self, topology: wgpu::PrimitiveTopology) -> Self {
        self.topology = topology;
        self
    }

    pub fn cull_back(mut self) -> Self {
        self.cull_mode = Some(wgpu::Face::Back);
        self
    }

    pub fn no_depth(mut self) -> Self {
        self.depth_format = None;
        self
    }

    pub fn depth_format(mut self, format: wgpu::TextureFormat) -> Self {
        self.depth_format = Some(format);
        self
    }

    pub fn depth_write(mut self, enabled: bool) -> Self {
        self.depth_write = enabled;
        self
    }

    pub fn depth_compare(mut self, compare: wgpu::CompareFunction) -> Self {
        self.depth_compare = compare;
        self
    }

    pub fn depth_bias(mut self, bias: wgpu::DepthBiasState) -> Self {
        self.depth_bias = bias;
        self
    }

    pub fn sample_count(mut self, count: u32) -> Self {
        self.sample_count = count;
        self
    }

    pub fn build(self) -> wgpu::RenderPipeline {
        let fragment = self.fragment_entry.map(|entry| {
            let targets_storage;
            let targets: &[Option<wgpu::ColorTargetState>] = if let Some(format) = self.color_format
            {
                targets_storage = [Some(wgpu::ColorTargetState {
                    format,
                    blend: self.blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })];
                &targets_storage
            } else {
                &[]
            };

            (entry, targets.to_vec())
        });

        let targets_owned: Vec<Option<wgpu::ColorTargetState>>;
        let fragment_state;

        if let Some((entry, targets)) = fragment.as_ref() {
            targets_owned = targets.clone();
            fragment_state = Some(wgpu::FragmentState {
                module: self.shader,
                entry_point: Some(entry),
                targets: &targets_owned,
                compilation_options: Default::default(),
            });
        } else {
            targets_owned = Vec::new();
            fragment_state = None;
        }

        let depth_stencil = self.depth_format.map(|format| wgpu::DepthStencilState {
            format,
            depth_write_enabled: self.depth_write,
            depth_compare: self.depth_compare,
            stencil: wgpu::StencilState::default(),
            bias: self.depth_bias,
        });

        let multisample = if self.sample_count > 1 {
            wgpu::MultisampleState {
                count: self.sample_count,
                mask: !0,
                alpha_to_coverage_enabled: false,
            }
        } else {
            wgpu::MultisampleState::default()
        };

        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(self.label),
                layout: Some(self.layout),
                vertex: wgpu::VertexState {
                    module: self.shader,
                    entry_point: Some(self.vertex_entry),
                    buffers: &self.vertex_buffers,
                    compilation_options: Default::default(),
                },
                fragment: fragment_state.as_ref().map(|fs| wgpu::FragmentState {
                    module: fs.module,
                    entry_point: fs.entry_point,
                    targets: &targets_owned,
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: self.topology,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: self.cull_mode,
                    polygon_mode: self.polygon_mode,
                    ..Default::default()
                },
                depth_stencil,
                multisample,
                multiview: None,
                cache: None,
            })
    }
}
