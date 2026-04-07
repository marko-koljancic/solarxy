use crate::aabb::AABB;
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
pub(crate) struct GridUniform {
    pub(crate) cell_size: f32,
    pub(crate) color: [f32; 3],
}

impl GridUniform {
    pub const COLOR_OFFSET: u64 = std::mem::offset_of!(Self, color) as u64;
}

pub(crate) struct VisualizationState {
    pub(crate) grid_mesh: model::Mesh,
    pub(crate) grid_bind_group: wgpu::BindGroup,
    pub(crate) grid_uniform_buf: wgpu::Buffer,
    pub(crate) floor_mesh: model::Mesh,
    pub(crate) vertex_normals_buf: wgpu::Buffer,
    pub(crate) face_normals_buf: wgpu::Buffer,
    pub(crate) vertex_normals_count: u32,
    pub(crate) face_normals_count: u32,
    pub(crate) face_normals_bind_group: wgpu::BindGroup,
    pub(crate) vertex_normals_bind_group: wgpu::BindGroup,
    face_normals_color_buf: wgpu::Buffer,
    vertex_normals_color_buf: wgpu::Buffer,
    pub(crate) axes_vertex_buf: wgpu::Buffer,
    pub(crate) bounds_whole_buf: wgpu::Buffer,
    pub(crate) bounds_whole_count: u32,
    pub(crate) bounds_per_mesh_buf: wgpu::Buffer,
    pub(crate) bounds_per_mesh_count: u32,
    pub(crate) local_axes_vertex_buf: wgpu::Buffer,
    pub(crate) local_axes_vertex_count: u32,
}

impl VisualizationState {
    pub(crate) fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        model: &Model,
        normals_geo: &model::NormalsGeometry,
        camera_buffer: &wgpu::Buffer,
        initial_grid_color: [f32; 3],
    ) -> Self {
        let floor_mesh = resources::create_floor_quad(device, &model.bounds);
        let (grid_mesh, cell_size) = resources::create_grid_quad(device, &model.bounds);

        let grid_uniform = GridUniform {
            cell_size,
            color: initial_grid_color,
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
        let vertex_normals_color_buf =
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
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

        let axis_len = model.bounds.diagonal() * 0.5;
        let axes_vertices: [model::GizmoVertex; 6] = [
            model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: [1.0, 0.2, 0.2],
            },
            model::GizmoVertex {
                position: [axis_len, 0.0, 0.0],
                color: [1.0, 0.2, 0.2],
            },
            model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: [0.2, 1.0, 0.2],
            },
            model::GizmoVertex {
                position: [0.0, axis_len, 0.0],
                color: [0.2, 1.0, 0.2],
            },
            model::GizmoVertex {
                position: [0.0, 0.0, 0.0],
                color: [0.3, 0.5, 1.0],
            },
            model::GizmoVertex {
                position: [0.0, 0.0, axis_len],
                color: [0.3, 0.5, 1.0],
            },
        ];
        let axes_vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Axes Vertex Buffer"),
            contents: bytemuck::cast_slice(&axes_vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let mut local_axes_verts: Vec<model::GizmoVertex> = Vec::new();
        let model_center = model.bounds.center();
        let model_axis_len = model.bounds.diagonal() * 0.3;
        local_axes_verts.extend(axes_at_center(
            [model_center.x, model_center.y, model_center.z],
            model_axis_len,
        ));
        if model.mesh_bounds.len() > 1 {
            for mb in &model.mesh_bounds {
                let c = mb.center();
                let len = mb.diagonal() * 0.3;
                local_axes_verts.extend(axes_at_center([c.x, c.y, c.z], len));
            }
        }
        let local_axes_vertex_count = local_axes_verts.len() as u32;
        let local_axes_vertex_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Local Axes Buffer"),
            contents: bytemuck::cast_slice(&local_axes_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let whole_verts = aabb_line_vertices(&model.bounds, [1.0, 0.65, 0.0]);
        let bounds_whole_count = whole_verts.len() as u32;
        let bounds_whole_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bounds Whole Buffer"),
            contents: bytemuck::cast_slice(&whole_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let palette = bounds_color_palette();
        let mut per_mesh_verts: Vec<model::GizmoVertex> = Vec::new();
        for (i, mesh_aabb) in model.mesh_bounds.iter().enumerate() {
            per_mesh_verts.extend(aabb_line_vertices(mesh_aabb, palette[i % palette.len()]));
        }
        let bounds_per_mesh_count = per_mesh_verts.len() as u32;
        let bounds_per_mesh_buf = if per_mesh_verts.is_empty() {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Bounds Per Mesh Buffer"),
                contents: &[0u8; 24],
                usage: wgpu::BufferUsages::VERTEX,
            })
        } else {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("Bounds Per Mesh Buffer"),
                contents: bytemuck::cast_slice(&per_mesh_verts),
                usage: wgpu::BufferUsages::VERTEX,
            })
        };

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
            axes_vertex_buf,
            bounds_whole_buf,
            bounds_whole_count,
            bounds_per_mesh_buf,
            bounds_per_mesh_count,
            local_axes_vertex_buf,
            local_axes_vertex_count,
        }
    }

    pub(crate) fn rebuild_camera_bind_groups(
        &mut self,
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        camera_buffer: &wgpu::Buffer,
    ) {
        self.grid_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grid Bind Group"),
            layout: &layouts.grid,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.grid_uniform_buf.as_entire_binding(),
                },
            ],
        });
        self.face_normals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Face Normals Bind Group"),
            layout: &layouts.normals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.face_normals_color_buf.as_entire_binding(),
                },
            ],
        });
        self.vertex_normals_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Vertex Normals Bind Group"),
            layout: &layouts.normals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: camera_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: self.vertex_normals_color_buf.as_entire_binding(),
                },
            ],
        });
    }
}

fn axes_at_center(center: [f32; 3], length: f32) -> [model::GizmoVertex; 6] {
    let [cx, cy, cz] = center;
    [
        model::GizmoVertex {
            position: [cx, cy, cz],
            color: [1.0, 0.2, 0.2],
        },
        model::GizmoVertex {
            position: [cx + length, cy, cz],
            color: [1.0, 0.2, 0.2],
        },
        model::GizmoVertex {
            position: [cx, cy, cz],
            color: [0.2, 1.0, 0.2],
        },
        model::GizmoVertex {
            position: [cx, cy + length, cz],
            color: [0.2, 1.0, 0.2],
        },
        model::GizmoVertex {
            position: [cx, cy, cz],
            color: [0.3, 0.5, 1.0],
        },
        model::GizmoVertex {
            position: [cx, cy, cz + length],
            color: [0.3, 0.5, 1.0],
        },
    ]
}

fn aabb_line_vertices(aabb: &AABB, color: [f32; 3]) -> Vec<model::GizmoVertex> {
    let mn = [aabb.min.x, aabb.min.y, aabb.min.z];
    let mx = [aabb.max.x, aabb.max.y, aabb.max.z];

    let corners: [[f32; 3]; 8] = [
        [mn[0], mn[1], mn[2]],
        [mx[0], mn[1], mn[2]],
        [mx[0], mn[1], mx[2]],
        [mn[0], mn[1], mx[2]],
        [mn[0], mx[1], mn[2]],
        [mx[0], mx[1], mn[2]],
        [mx[0], mx[1], mx[2]],
        [mn[0], mx[1], mx[2]],
    ];

    let edges: [(usize, usize); 12] = [
        (0, 1),
        (1, 2),
        (2, 3),
        (3, 0),
        (4, 5),
        (5, 6),
        (6, 7),
        (7, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ];

    let mut verts = Vec::with_capacity(24);
    for (a, b) in edges {
        verts.push(model::GizmoVertex {
            position: corners[a],
            color,
        });
        verts.push(model::GizmoVertex {
            position: corners[b],
            color,
        });
    }
    verts
}

fn bounds_color_palette() -> [[f32; 3]; 8] {
    [
        [1.0, 0.4, 0.4],
        [0.3, 0.85, 0.4],
        [0.4, 0.6, 1.0],
        [1.0, 0.85, 0.2],
        [0.85, 0.4, 1.0],
        [0.2, 0.9, 0.9],
        [1.0, 0.55, 0.75],
        [0.7, 0.9, 0.3],
    ]
}

fn create_normals_buffer(
    device: &wgpu::Device,
    lines: &[[f32; 3]],
    label: &str,
) -> (wgpu::Buffer, u32) {
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
