//! GPU-side model representation: [`Model`], [`Mesh`], the [`Vertex`] trait
//! (defines vertex buffer layouts), and the `DrawModel`/`DrawMeshSimple`
//! draw-call helpers used by [`crate::frame::Renderer`].

use std::ops::Range;

pub use solarxy_core::AABB;
use solarxy_core::MeshTopology;

use super::material::Material;

pub trait Vertex {
    fn description() -> wgpu::VertexBufferLayout<'static>;
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct LineVertex {
    pub position: [f32; 3],
}

impl Vertex for LineVertex {
    fn description() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: 12,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x3,
            }],
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GizmoVertex {
    pub position: [f32; 3],
    pub color: [f32; 3],
}

impl Vertex for GizmoVertex {
    fn description() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: 24,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: 12,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

pub struct NormalsGeometry {
    pub vertex_lines: Vec<[f32; 3]>,
    pub face_lines: Vec<[f32; 3]>,
    /// Per-mesh vertex ranges into `vertex_lines`, parallel to
    /// `Model::meshes` (one entry per mesh, empty for meshes with no
    /// geometry). Lets the normals overlay skip Outliner-hidden meshes.
    pub vertex_segments: Vec<Range<u32>>,
    /// Per-mesh vertex ranges into `face_lines`, parallel to `Model::meshes`.
    pub face_segments: Vec<Range<u32>>,
}

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ModelVertex {
    pub position: [f32; 3],
    pub tex_coords: [f32; 2],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
    pub bitangent: [f32; 3],
}

/// A position-only view over the interleaved [`ModelVertex`] buffer: the
/// same stride, only location 0 declared. The line pipelines use it so a
/// polyline shares the mesh's regular vertex buffer.
#[must_use]
pub fn position_only_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        }],
    }
}

/// The per-vertex color buffer (linear RGBA f32) at shader location 12,
/// bound as an extra slot by the `*_colored` pipeline variants only.
#[must_use]
pub fn color_vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    wgpu::VertexBufferLayout {
        array_stride: 16,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[wgpu::VertexAttribute {
            offset: 0,
            shader_location: 12,
            format: wgpu::VertexFormat::Float32x4,
        }],
    }
}

impl Vertex for ModelVertex {
    fn description() -> wgpu::VertexBufferLayout<'static> {
        use std::mem;
        wgpu::VertexBufferLayout {
            array_stride: mem::size_of::<ModelVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 3]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x2,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 5]>() as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 8]>() as wgpu::BufferAddress,
                    shader_location: 3,
                    format: wgpu::VertexFormat::Float32x3,
                },
                wgpu::VertexAttribute {
                    offset: mem::size_of::<[f32; 11]>() as wgpu::BufferAddress,
                    shader_location: 4,
                    format: wgpu::VertexFormat::Float32x3,
                },
            ],
        }
    }
}

pub struct EdgeData {
    pub positions_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_edges: u32,
    pub bind_group: wgpu::BindGroup,
}

pub struct UvEdgeData {
    #[allow(dead_code)]
    pub uv_buffer: wgpu::Buffer,
    pub bind_group: wgpu::BindGroup,
}

#[allow(dead_code)]
pub struct Mesh {
    pub name: String,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub num_elements: u32,
    /// Vertex count; the point pipeline's draw count is `6 * num_vertices`
    /// (six expanded quad corners per point).
    pub num_vertices: u32,
    pub material: usize,
    /// How the index buffer connects vertices. Triangle meshes take the
    /// full PBR path; lines and points draw through their own unlit
    /// pipelines in the main pass only (decision M-15 keeps them in the
    /// selection-outline mask; shadow/gbuffer/overdraw skip them).
    pub topology: MeshTopology,
    /// Per-vertex linear RGBA colors at shader location 12; present only
    /// when the cooked mesh carried a color lane. Its presence selects the
    /// `*_colored` pipeline variants for triangle and line meshes.
    pub color_buffer: Option<wgpu::Buffer>,
    /// Outliner visibility. `false` skips the mesh in every draw pass
    /// (main / shadow / g-buffer / overlays / UV) — see `frame.rs`.
    pub visible: bool,
    pub edge_data: Option<EdgeData>,
    pub uv_edge_data: Option<UvEdgeData>,
    pub degen_index_buffer: Option<wgpu::Buffer>,
    pub degen_num_elements: u32,
}

/// CPU-side mirror of a GPU mesh's geometry. Kept around for picking
/// (review-mode click anchoring) and for the topology hashing used by
/// review-file stale detection ([`solarxy_core::review::hash_mesh`]).
///
/// Indexed identically to [`Model::meshes`] — `cpu_meshes[i]` corresponds
/// to `meshes[i]`. Empty raw meshes are filtered out symmetrically.
///
/// Memory cost: ~24 bytes per vertex + ~4 bytes per index. A typical 100K-
/// triangle, 50K-vertex model adds ~1.2 MB CPU-side — trivial vs. the GPU
/// resources.
pub struct CpuMesh {
    pub positions: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

/// CPU-side mirror of a material's source textures, kept alive so the
/// Material Inspector (`solarxy-app::gui::material_inspector`) can render
/// 128×128 thumbnails without re-decoding files or reading back from
/// the GPU. Indexed in lockstep with [`Model::materials`].
///
/// Five roles, one per slot in the source data ([`solarxy_core::RawMaterialData`]).
/// Roles whose source has no texture are `None`. The renderer combines
/// metallic-roughness + occlusion into a packed GPU "ORM" texture, but
/// the Inspector shows the source-side split that artists author against.
pub struct MaterialThumbnails {
    pub albedo: Option<TextureThumbnail>,
    pub normal: Option<TextureThumbnail>,
    pub metallic_roughness: Option<TextureThumbnail>,
    pub occlusion: Option<TextureThumbnail>,
    pub emissive: Option<TextureThumbnail>,
    pub base_color: [f32; 3],
}

pub struct TextureThumbnail {
    /// Decoded RGBA8 bytes + dimensions for the source texture. Shared
    /// via `Arc` so the Inspector can keep a reference without bloating
    /// per-material storage when the inspector window is closed.
    pub image: std::sync::Arc<solarxy_core::RawImageData>,
    /// On-disk source path when the texture was loaded from a file
    /// (OBJ MTL, external glTF reference). `None` means the texture was
    /// embedded in the model file (e.g. glTF binary) — the Inspector
    /// disables the "Open externally" button for these.
    pub source_path: Option<std::path::PathBuf>,
}

pub struct Model {
    pub meshes: Vec<Mesh>,
    pub materials: Vec<Material>,
    pub bounds: AABB,
    pub mesh_bounds: Vec<AABB>,
    pub cpu_meshes: Vec<CpuMesh>,
    pub material_thumbnails: Vec<MaterialThumbnails>,
    pub has_uvs: bool,
}

pub trait DrawMeshSimple<'a> {
    fn draw_model_simple(&mut self, model: &'a Model, instances: std::ops::Range<u32>);
}

impl<'a, 'b> DrawMeshSimple<'b> for wgpu::RenderPass<'a>
where
    'b: 'a,
{
    fn draw_model_simple(&mut self, model: &'b Model, instances: std::ops::Range<u32>) {
        for mesh in &model.meshes {
            // Triangle meshes only: every caller of this helper draws an
            // indexed triangle list; line and point meshes go through
            // their dedicated pipelines in the main pass.
            if !mesh.visible || mesh.topology != MeshTopology::Triangles {
                continue;
            }
            self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            self.draw_indexed(0..mesh.num_elements, 0, instances.clone());
        }
    }
}

pub trait DrawModel<'a> {
    fn draw_mesh(&mut self, mesh: &'a Mesh, material: &'a Material, instances: Range<u32>);
}

impl<'a, 'b> DrawModel<'b> for wgpu::RenderPass<'a>
where
    'b: 'a,
{
    fn draw_mesh(&mut self, mesh: &'b Mesh, material: &'b Material, instances: Range<u32>) {
        self.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        self.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        self.set_bind_group(0, &material.bind_group, &[]);
        self.draw_indexed(0..mesh.num_elements, 0, instances);
    }
}
