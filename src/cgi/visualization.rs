use crate::cgi::bind_groups::BindGroupLayouts;
use crate::cgi::model::{self, Model};
use crate::cgi::resources;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct NormalsColor {
    color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GridUniform {
    cell_size: f32,
    _pad: [f32; 3],
}

pub(crate) struct VisualizationState {
    pub(crate) grid_mesh: model::Mesh,
    pub(crate) grid_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    grid_uniform_buf: wgpu::Buffer,
    pub(crate) floor_mesh: model::Mesh,
    pub(crate) vertex_normals_buf: wgpu::Buffer,
    pub(crate) face_normals_buf: wgpu::Buffer,
    pub(crate) vertex_normals_count: u32,
    pub(crate) face_normals_count: u32,
    pub(crate) face_normals_bind_group: wgpu::BindGroup,
    pub(crate) vertex_normals_bind_group: wgpu::BindGroup,
    #[allow(dead_code)]
    face_normals_color_buf: wgpu::Buffer,
    #[allow(dead_code)]
    vertex_normals_color_buf: wgpu::Buffer,
}

impl VisualizationState {
    pub(crate) fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        model: &Model,
        normals_geo: &model::NormalsGeometry,
        camera_buffer: &wgpu::Buffer,
    ) -> Self {
        let floor_mesh = resources::create_floor_quad(device, &model.bounds);
        let (grid_mesh, cell_size) = resources::create_grid_quad(device, &model.bounds);

        let grid_uniform = GridUniform {
            cell_size,
            _pad: [0.0; 3],
        };
        let grid_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Grid Uniform Buffer"),
            contents: bytemuck::cast_slice(&[grid_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let grid_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grid Bind Group"),
            layout: &layouts.grid,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: grid_uniform_buf.as_entire_binding(),
                },
            ],
        });

        let (vertex_normals_buf, vertex_normals_count) =
            create_normals_buffer(device, &normals_geo.vertex_lines, "Vertex Normals Buffer");
        let (face_normals_buf, face_normals_count) =
            create_normals_buffer(device, &normals_geo.face_lines, "Face Normals Buffer");

        let face_normals_color_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Face Normals Color Buffer"),
            contents: bytemuck::cast_slice(&[NormalsColor {
                color: [0.2, 0.85, 0.2, 1.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let vertex_normals_color_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Vertex Normals Color Buffer"),
            contents: bytemuck::cast_slice(&[NormalsColor {
                color: [0.25, 0.55, 1.0, 1.0],
            }]),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let face_normals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Face Normals Bind Group"),
            layout: &layouts.normals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: face_normals_color_buf.as_entire_binding(),
                },
            ],
        });
        let vertex_normals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Vertex Normals Bind Group"),
            layout: &layouts.normals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: vertex_normals_color_buf.as_entire_binding(),
                },
            ],
        });

        VisualizationState {
            grid_mesh,
            grid_bind_group,
            grid_uniform_buf,
            floor_mesh,
            vertex_normals_buf,
            face_normals_buf,
            vertex_normals_count,
            face_normals_count,
            face_normals_bind_group,
            vertex_normals_bind_group,
            face_normals_color_buf,
            vertex_normals_color_buf,
        }
    }
}

fn create_normals_buffer(device: &wgpu::Device, lines: &[[f32; 3]], label: &str) -> (wgpu::Buffer, u32) {
    if lines.is_empty() {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &[0u8; 12],
                usage: wgpu::BufferUsages::VERTEX,
            }),
            0,
        )
    } else {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(lines),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            lines.len() as u32,
        )
    }
}
