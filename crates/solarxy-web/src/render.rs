//! A self-contained forward renderer for the Phase-4 web MVP.
//!
//! It consumes `solarxy_core::scene::SceneDelta` batches straight from the
//! engine (`take_scene_delta`) and draws them with simple Lambert-plus-
//! ambient shading and a depth buffer, single pane, no MSAA. This is a
//! deliberate MVP stopgap: it renders cooked geometry to pixels today
//! without depending on `solarxy_renderer`'s `ModelScene`-coupled PBR pass.
//! Full renderer parity (IBL, shadows, SSAO/bloom, floor) is a follow-up
//! once the main pass is decoupled from `ModelScene`.
//!
//! Geometry never leaves wasm: the engine hands over `Arc<CookedGeometry>`
//! as an in-memory pointer, and this module uploads it to GPU buffers.

use std::collections::BTreeMap;

use cgmath::{Matrix, Matrix4, SquareMatrix};
use solarxy_core::scene::{LightKind, SceneDelta, SceneOp};
use wgpu::util::DeviceExt;

use crate::camera::OrbitCamera;

/// The shared camera + lighting uniform (std140-friendly).
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct CameraUniform {
    view_proj: [[f32; 4]; 4],
    /// Direction toward the key light (xyz); w unused.
    light_dir: [f32; 4],
    /// Ambient RGB; w unused.
    ambient: [f32; 4],
}

/// The per-object model + normal matrix uniform.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct ModelUniform {
    model: [[f32; 4]; 4],
    normal: [[f32; 4]; 4],
}

/// One drawable object: interleaved position+normal vertices, an index
/// buffer, and its own model-matrix bind group.
struct WebObject {
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    visible: bool,
    model_buffer: wgpu::Buffer,
    model_bind_group: wgpu::BindGroup,
    transform: [[f32; 4]; 4],
}

/// The forward renderer state.
pub struct WebRenderer {
    pipeline: wgpu::RenderPipeline,
    model_layout: wgpu::BindGroupLayout,
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,
    depth_view: wgpu::TextureView,
    width: u32,
    height: u32,
    objects: BTreeMap<u64, WebObject>,
    light_dir: [f32; 3],
    ambient: [f32; 3],
}

const SHADER: &str = r"
struct Camera { view_proj: mat4x4<f32>, light_dir: vec4<f32>, ambient: vec4<f32> };
struct Model  { model: mat4x4<f32>, normal: mat4x4<f32> };
@group(0) @binding(0) var<uniform> cam: Camera;
@group(1) @binding(0) var<uniform> obj: Model;

struct VsOut { @builtin(position) clip: vec4<f32>, @location(0) world_n: vec3<f32> };

@vertex
fn vs(@location(0) pos: vec3<f32>, @location(1) nrm: vec3<f32>) -> VsOut {
    var out: VsOut;
    out.clip = cam.view_proj * (obj.model * vec4<f32>(pos, 1.0));
    out.world_n = (obj.normal * vec4<f32>(nrm, 0.0)).xyz;
    return out;
}

@fragment
fn fs(in: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(in.world_n);
    let l = normalize(cam.light_dir.xyz);
    // Two-sided so back faces (or inconsistent winding) still shade.
    let diff = max(abs(dot(n, l)), 0.0);
    let base = vec3<f32>(0.72, 0.74, 0.80);
    let col = base * (cam.ambient.rgb + diff * vec3<f32>(0.9, 0.9, 0.88));
    return vec4<f32>(col, 1.0);
}
";

impl WebRenderer {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
    ) -> Self {
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("web camera layout"),
            entries: &[uniform_entry(0)],
        });
        let model_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("web model layout"),
            entries: &[uniform_entry(0)],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("web forward shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("web forward pipeline layout"),
            bind_group_layouts: &[&camera_layout, &model_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("web forward pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: 24,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        wgpu::VertexAttribute {
                            format: wgpu::VertexFormat::Float32x3,
                            offset: 12,
                            shader_location: 1,
                        },
                    ],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: true,
                depth_compare: wgpu::CompareFunction::Less,
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("web camera uniform"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("web camera bind group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        Self {
            pipeline,
            model_layout,
            camera_buffer,
            camera_bind_group,
            depth_view: create_depth(device, width, height),
            width,
            height,
            objects: BTreeMap::new(),
            light_dir: normalize3([0.4, 0.85, 0.55]),
            ambient: [0.28, 0.29, 0.33],
        }
    }

    pub fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.width = width;
        self.height = height;
        self.depth_view = create_depth(device, width, height);
    }

    /// Applies a scene delta, uploading new geometry, updating transforms
    /// and visibility, removing objects, and picking a key light.
    pub fn apply_delta(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, delta: &SceneDelta) {
        for op in &delta.ops {
            match op {
                SceneOp::UpsertGeometry { id, geometry } => {
                    self.upsert(device, queue, id.0, geometry);
                }
                SceneOp::SetTransform { id, transform } => {
                    self.set_transform(queue, id.0, *transform);
                }
                SceneOp::SetVisible { id, visible } => {
                    if let Some(o) = self.objects.get_mut(&id.0) {
                        o.visible = *visible;
                    }
                }
                SceneOp::Remove { id } => {
                    self.objects.remove(&id.0);
                }
                SceneOp::SetLights { lights } => self.set_lights(lights),
                SceneOp::Clear => self.objects.clear(),
            }
        }
    }

    fn upsert(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        id: u64,
        geometry: &solarxy_core::scene::CookedGeometry,
    ) {
        // Interleave position + normal across all meshes into one buffer.
        let mut verts: Vec<f32> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        for mesh in &geometry.meshes {
            let base = (verts.len() / 6) as u32;
            let normals = mesh.normals.as_ref();
            for (vi, p) in mesh.positions.iter().enumerate() {
                let n = normals.map_or([0.0, 1.0, 0.0], |ns| ns[vi]);
                verts.extend_from_slice(&[p[0], p[1], p[2], n[0], n[1], n[2]]);
            }
            indices.extend(mesh.indices.iter().map(|i| i + base));
        }
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("web vertices"),
            contents: bytemuck::cast_slice(&verts),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("web indices"),
            contents: bytemuck::cast_slice(&indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        // Preserve an existing transform; otherwise start at identity.
        let transform = self
            .objects
            .get(&id)
            .map_or_else(col_major_identity, |o| o.transform);
        let model_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("web model uniform"),
            size: std::mem::size_of::<ModelUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let model_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("web model bind group"),
            layout: &self.model_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: model_buffer.as_entire_binding(),
            }],
        });
        // Write the (possibly preserved) transform into the fresh buffer.
        queue.write_buffer(
            &model_buffer,
            0,
            bytemuck::bytes_of(&model_uniform(transform)),
        );
        self.objects.insert(
            id,
            WebObject {
                vertex_buffer,
                index_buffer,
                index_count: indices.len() as u32,
                visible: true,
                model_buffer,
                model_bind_group,
                transform,
            },
        );
    }

    fn set_transform(&mut self, queue: &wgpu::Queue, id: u64, transform: [[f32; 4]; 4]) {
        if let Some(o) = self.objects.get_mut(&id) {
            o.transform = transform;
            queue.write_buffer(
                &o.model_buffer,
                0,
                bytemuck::bytes_of(&model_uniform(transform)),
            );
        }
    }

    fn set_lights(&mut self, lights: &[solarxy_core::scene::LightDef]) {
        let mut ambient = [0.18_f32, 0.19, 0.22];
        let mut key: Option<[f32; 3]> = None;
        for l in lights {
            if !l.visible {
                continue;
            }
            match l.kind {
                LightKind::Directional | LightKind::Spot => {
                    if key.is_none() {
                        key = Some(normalize3([
                            -l.direction[0],
                            -l.direction[1],
                            -l.direction[2],
                        ]));
                    }
                }
                LightKind::Point | LightKind::RectArea => {
                    if key.is_none() {
                        key = Some(normalize3(l.position));
                    }
                }
                LightKind::Ambient | LightKind::Hemisphere => {
                    let k = l.intensity.clamp(0.0, 1.0);
                    for (slot, c) in ambient.iter_mut().zip(l.color) {
                        *slot += c * k * 0.5;
                    }
                }
            }
        }
        if let Some(k) = key {
            self.light_dir = k;
        }
        self.ambient = [
            ambient[0].min(1.0),
            ambient[1].min(1.0),
            ambient[2].min(1.0),
        ];
    }

    /// Renders one frame into `surface_view` using the given camera. The
    /// caller owns surface acquire/present; this records and submits.
    pub fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        surface_view: &wgpu::TextureView,
        camera: &OrbitCamera,
    ) {
        let vp: [[f32; 4]; 4] = camera.view_proj().into();
        let cam = CameraUniform {
            view_proj: vp,
            light_dir: [self.light_dir[0], self.light_dir[1], self.light_dir[2], 0.0],
            ambient: [self.ambient[0], self.ambient[1], self.ambient[2], 0.0],
        };
        queue.write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&cam));

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("web frame"),
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("web forward pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: surface_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.05,
                            g: 0.06,
                            b: 0.09,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                occlusion_query_set: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.camera_bind_group, &[]);
            for obj in self.objects.values() {
                if !obj.visible || obj.index_count == 0 {
                    continue;
                }
                pass.set_bind_group(1, &obj.model_bind_group, &[]);
                pass.set_vertex_buffer(0, obj.vertex_buffer.slice(..));
                pass.set_index_buffer(obj.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..obj.index_count, 0, 0..1);
            }
        }
        queue.submit([encoder.finish()]);
    }

    /// Whether any visible geometry is loaded (a boot/smoke check).
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}

fn uniform_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

fn create_depth(device: &wgpu::Device, width: u32, height: u32) -> wgpu::TextureView {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("web depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    tex.create_view(&wgpu::TextureViewDescriptor::default())
}

fn col_major_identity() -> [[f32; 4]; 4] {
    Matrix4::identity().into()
}

fn model_uniform(transform: [[f32; 4]; 4]) -> ModelUniform {
    let m = Matrix4::from(transform);
    // Normal matrix = inverse-transpose of the model matrix (handles
    // non-uniform scale); identity if singular.
    let n = m
        .invert()
        .map_or_else(Matrix4::identity, |inv| inv.transpose());
    ModelUniform {
        model: transform,
        normal: n.into(),
    }
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len > 1e-6 {
        [v[0] / len, v[1] / len, v[2] / len]
    } else {
        [0.0, 1.0, 0.0]
    }
}
