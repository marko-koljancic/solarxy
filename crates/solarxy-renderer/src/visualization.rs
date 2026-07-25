//! Debug visualizers: grid lines, axis gizmo, AABB bounds, and per-vertex /
//! per-face normal arrows. All CPU-side mesh construction with their own
//! pipelines registered in [`crate::pipelines::OverlayPipelines`].

use std::ops::Range;

use solarxy_core::AABB;
use crate::bind_groups::BindGroupLayouts;
use crate::model::{self, Model};
use crate::resources;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct NormalsColor {
    color: [f32; 4],
}
const _: () = assert!(std::mem::size_of::<NormalsColor>() == 16);

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GridUniform {
    pub cell_size: f32,
    pub color: [f32; 3],
    /// Which world plane the grid lies in: 0 = XZ ground (default), 1 = XY,
    /// 2 = YZ. Lets an orthographic elevation view show a view-plane grid
    /// instead of an edge-on hairline. Written per pane by the host.
    pub plane: u32,
    pub _pad: [u32; 3],
}
const _: () = assert!(std::mem::size_of::<GridUniform>() == 32);

impl GridUniform {
    pub const COLOR_OFFSET: u64 = std::mem::offset_of!(Self, color) as u64;
    pub const PLANE_OFFSET: u64 = std::mem::offset_of!(Self, plane) as u64;
}

/// The [`GridUniform::plane`] code a camera should see. Perspective always
/// keeps the XZ ground. Orthographic gets a face-on wall grid (XY or YZ) only
/// while the view direction is a true axis elevation, within ~2.5 degrees;
/// once the user orbits an axis view into a free ortho "user view" the ground
/// grid returns, so the plane cannot flip back and forth at the 45-degree
/// azimuth crossings of a drag. Callers with an animated camera should pass
/// the transition destination, not the mid-lerp camera.
pub fn grid_plane_for(cam: &crate::camera::Camera) -> u32 {
    use cgmath::InnerSpace;
    const AXIS_DOT: f32 = 0.999;
    if cam.projection != solarxy_core::preferences::ProjectionMode::Orthographic {
        return 0;
    }
    let f = cam.target - cam.eye;
    let m = f.magnitude();
    if m < 1e-6 {
        return 0;
    }
    let f = f / m;
    if f.y.abs() >= AXIS_DOT {
        0 // top / bottom -> XZ ground
    } else if f.z.abs() >= AXIS_DOT {
        1 // front / back -> XY
    } else if f.x.abs() >= AXIS_DOT {
        2 // left / right -> YZ
    } else {
        0 // free ortho user view -> ground, like perspective
    }
}

/// Stand-in for the normal-arrow line lists when no `NormalsGeometry` is
/// supplied (`new_from_parts` with `normals_geo: None`).
static EMPTY_LINES: &[[f32; 3]] = &[];

pub struct VisualizationState {
    pub grid_mesh: model::Mesh,
    pub grid_params_bind_group: wgpu::BindGroup,
    pub grid_uniform_buf: wgpu::Buffer,
    pub floor_mesh: model::Mesh,
    pub vertex_normals_buf: wgpu::Buffer,
    pub face_normals_buf: wgpu::Buffer,
    pub vertex_normals_count: u32,
    pub face_normals_count: u32,
    /// Per-mesh vertex ranges into `vertex_normals_buf` / `face_normals_buf`,
    /// parallel to `Model::meshes` — `draw_normals` skips hidden meshes.
    pub vertex_normals_segments: Vec<Range<u32>>,
    pub face_normals_segments: Vec<Range<u32>>,
    pub face_normals_params_bind_group: wgpu::BindGroup,
    pub vertex_normals_params_bind_group: wgpu::BindGroup,
    pub axes_vertex_buf: wgpu::Buffer,
    pub bounds_whole_buf: wgpu::Buffer,
    pub bounds_whole_count: u32,
    pub bounds_per_mesh_buf: wgpu::Buffer,
    pub bounds_per_mesh_count: u32,
    pub local_axes_vertex_buf: wgpu::Buffer,
    pub local_axes_vertex_count: u32,
    /// Per-point attribute-vector arrows (the web host's attribute
    /// visualization), as per-vertex-colored line vertices through the
    /// gizmo pipeline (which is what lets the host paint a uniform color
    /// or a magnitude ramp with no pipeline of its own). Always
    /// constructed empty; only [`VisualizationState::set_attr_lines`]
    /// populates it, so the desktop shell and the golden harness never
    /// draw this channel.
    pub attr_lines_buf: wgpu::Buffer,
    pub attr_lines_count: u32,
}

impl VisualizationState {
    pub fn new(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        model: &Model,
        normals_geo: &model::NormalsGeometry,
        initial_grid_color: [f32; 3],
    ) -> Self {
        Self::new_from_parts(
            device,
            layouts,
            &model.bounds,
            &model.mesh_bounds,
            Some(normals_geo),
            initial_grid_color,
        )
    }

    /// Build from raw parts, without a [`Model`]: `bounds` sizes the
    /// grid/floor/axes, `mesh_bounds` feeds the per-mesh bounds and local
    /// axes, and `normals_geo` (when present) supplies the normal-arrow
    /// line buffers. The web host passes `None` for normals until its
    /// per-object overlay rebuild lands; the buffers stay empty and the
    /// draw paths early-out on their zero counts.
    pub fn new_from_parts(
        device: &wgpu::Device,
        layouts: &BindGroupLayouts,
        bounds: &AABB,
        mesh_bounds: &[AABB],
        normals_geo: Option<&model::NormalsGeometry>,
        initial_grid_color: [f32; 3],
    ) -> Self {
        let floor_mesh = resources::create_floor_quad(device, bounds);
        let (grid_mesh, cell_size) = resources::create_grid_quad(device, bounds);

        let grid_uniform = GridUniform {
            cell_size,
            color: initial_grid_color,
            plane: 0,
            _pad: [0; 3],
        };
        let grid_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Grid Uniform Buffer"),
            contents: bytemuck::cast_slice(&[grid_uniform]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });

        let grid_params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Grid Params Bind Group"),
            layout: &layouts.grid_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: grid_uniform_buf.as_entire_binding(),
            }],
        });

        let (vertex_lines, face_lines) = normals_geo.map_or((EMPTY_LINES, EMPTY_LINES), |g| {
            (g.vertex_lines.as_slice(), g.face_lines.as_slice())
        });
        let (vertex_normals_buf, vertex_normals_count) =
            create_normals_buffer(device, vertex_lines, "Vertex Normals Buffer");
        let (face_normals_buf, face_normals_count) =
            create_normals_buffer(device, face_lines, "Face Normals Buffer");

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

        let face_normals_params_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Face Normals Params Bind Group"),
            layout: &layouts.normals_params,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: face_normals_color_buf.as_entire_binding(),
            }],
        });
        let vertex_normals_params_bind_group =
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Vertex Normals Params Bind Group"),
                layout: &layouts.normals_params,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: vertex_normals_color_buf.as_entire_binding(),
                }],
            });

        let axis_len = bounds.diagonal() * 0.5;
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
        let model_center = bounds.center();
        let model_axis_len = bounds.diagonal() * 0.3;
        local_axes_verts.extend(axes_at_center(
            [model_center.x, model_center.y, model_center.z],
            model_axis_len,
        ));
        if mesh_bounds.len() > 1 {
            for mb in mesh_bounds {
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

        let whole_verts = aabb_line_vertices(bounds, [1.0, 0.65, 0.0]);
        let bounds_whole_count = whole_verts.len() as u32;
        let bounds_whole_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Bounds Whole Buffer"),
            contents: bytemuck::cast_slice(&whole_verts),
            usage: wgpu::BufferUsages::VERTEX,
        });

        let palette = bounds_color_palette();
        let mut per_mesh_verts: Vec<model::GizmoVertex> = Vec::new();
        for (i, mesh_aabb) in mesh_bounds.iter().enumerate() {
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

        // The attribute-vector channel starts empty everywhere; only the
        // web host's set_attr_lines populates it.
        let (attr_lines_buf, attr_lines_count) =
            create_gizmo_lines_buffer(device, &[], "Attr Vectors Buffer");

        VisualizationState {
            grid_mesh,
            grid_params_bind_group,
            grid_uniform_buf,
            floor_mesh,
            vertex_normals_buf,
            face_normals_buf,
            vertex_normals_count,
            face_normals_count,
            vertex_normals_segments: normals_geo
                .map(|g| g.vertex_segments.clone())
                .unwrap_or_default(),
            face_normals_segments: normals_geo
                .map(|g| g.face_segments.clone())
                .unwrap_or_default(),
            face_normals_params_bind_group,
            vertex_normals_params_bind_group,
            axes_vertex_buf,
            bounds_whole_buf,
            bounds_whole_count,
            bounds_per_mesh_buf,
            bounds_per_mesh_count,
            local_axes_vertex_buf,
            local_axes_vertex_count,
            attr_lines_buf,
            attr_lines_count,
        }
    }

    /// Replaces the attribute-vector line list (world-space segment pairs,
    /// colored per vertex). An empty slice clears the channel; the draw
    /// early-outs on zero.
    pub fn set_attr_lines(&mut self, device: &wgpu::Device, verts: &[model::GizmoVertex]) {
        let (buf, count) = create_gizmo_lines_buffer(device, verts, "Attr Vectors Buffer");
        self.attr_lines_buf = buf;
        self.attr_lines_count = count;
    }
}

/// A gizmo-vertex line buffer with the empty-slice placeholder convention
/// of [`create_normals_buffer`].
fn create_gizmo_lines_buffer(
    device: &wgpu::Device,
    verts: &[model::GizmoVertex],
    label: &str,
) -> (wgpu::Buffer, u32) {
    if verts.is_empty() {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: &[0u8; std::mem::size_of::<model::GizmoVertex>()],
                usage: wgpu::BufferUsages::VERTEX,
            }),
            0,
        )
    } else {
        (
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(verts),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            verts.len() as u32,
        )
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

#[cfg(test)]
mod tests {
    use super::grid_plane_for;
    use crate::camera::Camera;
    use solarxy_core::preferences::ProjectionMode;

    fn cam_toward(eye: [f32; 3], projection: ProjectionMode) -> Camera {
        Camera {
            eye: cgmath::Point3::new(eye[0], eye[1], eye[2]),
            target: cgmath::Point3::new(0.0, 0.0, 0.0),
            up: cgmath::Vector3::unit_y(),
            aspect: 1.0,
            fovy: 45.0,
            znear: 0.1,
            zfar: 100.0,
            projection,
            ortho_scale: 5.0,
        }
    }

    #[test]
    fn grid_plane_table() {
        use ProjectionMode::{Orthographic, Perspective};
        // Perspective always grounds, whatever the angle.
        assert_eq!(grid_plane_for(&cam_toward([3.0, 2.0, 4.0], Perspective)), 0);
        assert_eq!(grid_plane_for(&cam_toward([0.0, 5.0, 0.0], Perspective)), 0);
        // Orthographic axis elevations get their wall planes.
        assert_eq!(
            grid_plane_for(&cam_toward([0.0, 5.0, 0.0], Orthographic)),
            0
        );
        assert_eq!(
            grid_plane_for(&cam_toward([0.0, -5.0, 0.0], Orthographic)),
            0
        );
        assert_eq!(
            grid_plane_for(&cam_toward([0.0, 0.0, 5.0], Orthographic)),
            1
        );
        assert_eq!(
            grid_plane_for(&cam_toward([0.0, 0.0, -5.0], Orthographic)),
            1
        );
        assert_eq!(
            grid_plane_for(&cam_toward([5.0, 0.0, 0.0], Orthographic)),
            2
        );
        assert_eq!(
            grid_plane_for(&cam_toward([-5.0, 0.0, 0.0], Orthographic)),
            2
        );
    }

    #[test]
    fn orbited_ortho_user_view_grounds() {
        use ProjectionMode::Orthographic;
        // 30 degrees of azimuth off the front axis: no longer an elevation,
        // the ground grid returns instead of snapping between walls.
        let eye = [5.0 * 0.5, 0.0, 5.0 * 0.866];
        assert_eq!(grid_plane_for(&cam_toward(eye, Orthographic)), 0);
        // But a hair off-axis (inside the ~2.5 degree tolerance) still walls.
        assert_eq!(
            grid_plane_for(&cam_toward([0.1, 0.0, 5.0], Orthographic)),
            1
        );
    }
}
