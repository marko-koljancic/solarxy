//! Multi-object dynamic scene: the renderer half of the engine-renderer
//! contract. Consumes [`SceneDelta`] batches (built by the node engine, a
//! shell, or the dev harness) and maintains per-object GPU state.
//!
//! Buffer strategy (plan decision): per-object vertex/index/edge buffers
//! are created with `COPY_DST` and 1.5x headroom. A cook output that fits
//! rewrites in place via `queue.write_buffer` (the param-drag case);
//! capacity overflow recreates at 1.5x the new size. No slab allocator or
//! pooling in v1 — `apply` is the seam where a pool would slot in later.
//!
//! Object storage is a `BTreeMap` keyed by [`SceneObjectId`] (deliberate
//! deviation from the plan sketch's SlotMap): iteration order is
//! deterministic, which the golden-image comparisons rely on, and scenes
//! hold dozens of objects, not thousands.

use std::collections::BTreeMap;
use std::sync::Arc;

use cgmath::{Matrix4, SquareMatrix};
use wgpu::util::DeviceExt;

use solarxy_core::AABB;
use solarxy_core::geometry::{
    compute_bounds, compute_normals, compute_tangent_basis, compute_tangent_from_normal,
    extract_edges,
};
use solarxy_core::scene::{CookedGeometry, CookedMesh, LightDef, SceneDelta, SceneObjectId, SceneOp};

use crate::bind_groups::BindGroupLayouts;
use crate::error::RendererError;
use crate::model::{CpuMesh, EdgeData, Mesh, Model, ModelVertex};
use crate::pipelines::InstanceRaw;
use crate::resources;

/// Extra capacity factor for growable buffers (1.5x).
fn with_headroom(bytes: u64) -> u64 {
    bytes + bytes / 2
}

/// Recorded capacities of one mesh's growable buffers, parallel to
/// `Model::meshes`.
struct MeshCaps {
    vertex_bytes: u64,
    index_bytes: u64,
    edge_pos_bytes: u64,
    edge_idx_bytes: u64,
}

/// One renderable object: an owned [`Model`] (the same struct the
/// single-model path draws), its world transform on the instance path,
/// and growable-buffer bookkeeping.
pub struct SceneObject {
    pub model: Model,
    pub transform: Matrix4<f32>,
    pub visible: bool,
    /// One `InstanceRaw` per object — a transform-only change is a single
    /// small buffer write, never a geometry re-upload.
    pub instance_buffer: wgpu::Buffer,
    caps: Vec<MeshCaps>,
}

/// The dynamic scene: objects in deterministic id order plus the last
/// received light list.
#[derive(Default)]
pub struct SceneObjects {
    objects: BTreeMap<SceneObjectId, SceneObject>,
    lights: Option<Vec<LightDef>>,
    lights_dirty: bool,
}

impl SceneObjects {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    /// Objects in deterministic id order (visibility is the draw loop's
    /// concern — hidden objects still exist).
    pub fn iter(&self) -> impl Iterator<Item = (&SceneObjectId, &SceneObject)> {
        self.objects.iter()
    }

    #[must_use]
    pub fn get(&self, id: SceneObjectId) -> Option<&SceneObject> {
        self.objects.get(&id)
    }

    /// Visible objects as [`crate::frame::DrawObject`]s, in deterministic
    /// id order — appended to the draw loop beside the `ModelScene` entry
    /// (or standing alone once a shell feeds only deltas).
    pub fn draw_objects(&self) -> impl Iterator<Item = crate::frame::DrawObject<'_>> {
        self.objects
            .values()
            .filter(|o| o.visible)
            .map(|o| crate::frame::DrawObject {
                model: &o.model,
                instance_buffer: &o.instance_buffer,
            })
    }

    /// The engine-provided light list; `None` means no `SetLights` has
    /// arrived and the shell keeps its synthesized viewer rig.
    #[must_use]
    pub fn lights(&self) -> Option<&[LightDef]> {
        self.lights.as_deref()
    }

    /// True once after each `SetLights`, so the shell knows to rebuild
    /// its lights uniform (`LightsUniform::from_defs`).
    pub fn take_lights_dirty(&mut self) -> bool {
        std::mem::take(&mut self.lights_dirty)
    }

    /// Union world-space-ish bounds over visible objects (object bounds
    /// only; transforms are not applied in v1 — good enough for the
    /// camera-framing and shadow-fit callers, refined with the engine).
    #[must_use]
    pub fn visible_bounds(&self) -> Option<AABB> {
        let mut acc: Option<AABB> = None;
        for obj in self.objects.values().filter(|o| o.visible) {
            let b = obj.model.bounds;
            acc = Some(match acc {
                None => b,
                Some(a) => AABB {
                    min: cgmath::Point3::new(
                        a.min.x.min(b.min.x),
                        a.min.y.min(b.min.y),
                        a.min.z.min(b.min.z),
                    ),
                    max: cgmath::Point3::new(
                        a.max.x.max(b.max.x),
                        a.max.y.max(b.max.y),
                        a.max.z.max(b.max.z),
                    ),
                },
            });
        }
        acc
    }

    /// Apply one delta batch in order.
    pub fn apply(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        delta: &SceneDelta,
    ) -> Result<(), RendererError> {
        for op in &delta.ops {
            match op {
                SceneOp::UpsertGeometry { id, geometry } => {
                    self.upsert_geometry(device, queue, layouts, *id, geometry)?;
                }
                SceneOp::SetTransform { id, transform } => {
                    if let Some(obj) = self.objects.get_mut(id) {
                        obj.transform = (*transform).into();
                        queue.write_buffer(
                            &obj.instance_buffer,
                            0,
                            bytemuck::bytes_of(&InstanceRaw::from_matrix(obj.transform)),
                        );
                    }
                }
                SceneOp::SetVisible { id, visible } => {
                    if let Some(obj) = self.objects.get_mut(id) {
                        obj.visible = *visible;
                    }
                }
                SceneOp::Remove { id } => {
                    self.objects.remove(id);
                }
                SceneOp::SetLights { lights } => {
                    self.lights = Some(lights.clone());
                    self.lights_dirty = true;
                }
                SceneOp::Clear => {
                    self.objects.clear();
                    self.lights = None;
                    self.lights_dirty = true;
                }
            }
        }
        Ok(())
    }

    fn upsert_geometry(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: &BindGroupLayouts,
        id: SceneObjectId,
        cooked: &CookedGeometry,
    ) -> Result<(), RendererError> {
        let built = build_meshes(cooked);

        // In-place fast path: same mesh count, same material count, and
        // every new buffer fits its recorded capacity.
        if let Some(obj) = self.objects.get_mut(&id)
            && obj.model.meshes.len() == built.len()
            && obj.model.materials.len() == cooked.materials.len().max(1)
            && built.iter().zip(&obj.caps).all(|(b, caps)| {
                b.vertex_bytes() <= caps.vertex_bytes
                    && b.index_bytes() <= caps.index_bytes
                    && b.edge_pos_bytes() <= caps.edge_pos_bytes
                    && b.edge_idx_bytes() <= caps.edge_idx_bytes
            })
        {
            for (mesh, b) in obj.model.meshes.iter_mut().zip(&built) {
                queue.write_buffer(&mesh.vertex_buffer, 0, bytemuck::cast_slice(&b.vertices));
                queue.write_buffer(&mesh.index_buffer, 0, bytemuck::cast_slice(&b.indices));
                mesh.num_elements = b.indices.len() as u32;
                mesh.material = b.material_index;
                if let Some(edge) = &mesh.edge_data {
                    queue.write_buffer(
                        &edge.positions_buffer,
                        0,
                        bytemuck::cast_slice(&b.padded_positions),
                    );
                    queue.write_buffer(
                        &edge.index_buffer,
                        0,
                        bytemuck::cast_slice(&b.edge_indices),
                    );
                }
            }
            for (edge_opt, b) in obj
                .model
                .meshes
                .iter_mut()
                .map(|m| &mut m.edge_data)
                .zip(&built)
            {
                if let Some(edge) = edge_opt {
                    edge.num_edges = (b.edge_indices.len() / 2) as u32;
                }
            }
            obj.model.cpu_meshes = built
                .iter()
                .map(|b| CpuMesh {
                    positions: b.cpu_positions.clone(),
                    indices: b.indices.clone(),
                })
                .collect();
            obj.model.mesh_bounds = built.iter().map(|b| b.bounds).collect();
            obj.model.bounds = cooked.bounds;
            obj.model.has_uvs = built.iter().any(|b| b.has_uvs);
            return Ok(());
        }

        // Full (re)build: new buffers with headroom; transform and
        // visibility survive a rebuild.
        let materials =
            resources::upload_cooked_materials(&cooked.materials, device, queue, &layouts.texture)?;
        let material_thumbnails = cooked
            .materials
            .iter()
            .map(|m| crate::model::MaterialThumbnails {
                albedo: None,
                normal: None,
                metallic_roughness: None,
                occlusion: None,
                emissive: None,
                base_color: m.diffuse.unwrap_or([0.8, 0.8, 0.8]),
            })
            .chain(
                cooked
                    .materials
                    .is_empty()
                    .then_some(crate::model::MaterialThumbnails {
                        albedo: None,
                        normal: None,
                        metallic_roughness: None,
                        occlusion: None,
                        emissive: None,
                        base_color: [0.8, 0.8, 0.8],
                    }),
            )
            .collect();

        let mut meshes = Vec::with_capacity(built.len());
        let mut caps = Vec::with_capacity(built.len());
        for b in &built {
            let vertex_cap = with_headroom(b.vertex_bytes());
            let index_cap = with_headroom(b.index_bytes());
            let edge_pos_cap = with_headroom(b.edge_pos_bytes());
            let edge_idx_cap = with_headroom(b.edge_idx_bytes());

            let vertex_buffer = create_with_capacity(
                device,
                queue,
                "SceneObject Vertex Buffer",
                vertex_cap,
                bytemuck::cast_slice(&b.vertices),
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            );
            let index_buffer = create_with_capacity(
                device,
                queue,
                "SceneObject Index Buffer",
                index_cap,
                bytemuck::cast_slice(&b.indices),
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            );
            let edge_positions = create_with_capacity(
                device,
                queue,
                "SceneObject Edge Positions",
                edge_pos_cap,
                bytemuck::cast_slice(&b.padded_positions),
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            let edge_index = create_with_capacity(
                device,
                queue,
                "SceneObject Edge Indices",
                edge_idx_cap,
                bytemuck::cast_slice(&b.edge_indices),
                wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            );
            let edge_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("SceneObject Edge Bind Group"),
                layout: &layouts.edge_geometry,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: edge_positions.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: edge_index.as_entire_binding(),
                    },
                ],
            });

            meshes.push(Mesh {
                name: b.name.clone(),
                vertex_buffer,
                index_buffer,
                num_elements: b.indices.len() as u32,
                material: b.material_index,
                visible: true,
                edge_data: Some(EdgeData {
                    positions_buffer: edge_positions,
                    index_buffer: edge_index,
                    num_edges: (b.edge_indices.len() / 2) as u32,
                    bind_group: edge_bind_group,
                }),
                uv_edge_data: None,
                degen_index_buffer: None,
                degen_num_elements: 0,
            });
            caps.push(MeshCaps {
                vertex_bytes: vertex_cap,
                index_bytes: index_cap,
                edge_pos_bytes: edge_pos_cap,
                edge_idx_bytes: edge_idx_cap,
            });
        }

        let model = Model {
            meshes,
            materials,
            bounds: cooked.bounds,
            mesh_bounds: built.iter().map(|b| b.bounds).collect(),
            cpu_meshes: built
                .iter()
                .map(|b| CpuMesh {
                    positions: b.cpu_positions.clone(),
                    indices: b.indices.clone(),
                })
                .collect(),
            material_thumbnails,
            has_uvs: built.iter().any(|b| b.has_uvs),
        };

        if let Some(existing) = self.objects.get_mut(&id) {
            existing.model = model;
            existing.caps = caps;
        } else {
            let transform = Matrix4::identity();
            let instance_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("SceneObject Instance Buffer"),
                contents: bytemuck::bytes_of(&InstanceRaw::from_matrix(transform)),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            self.objects.insert(
                id,
                SceneObject {
                    model,
                    transform,
                    visible: true,
                    instance_buffer,
                    caps,
                },
            );
        }
        Ok(())
    }
}

/// Create a buffer of `capacity` bytes and write `contents` into its head.
fn create_with_capacity(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    capacity: u64,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    // wgpu requires COPY_BUFFER_ALIGNMENT (4) alignment on buffer sizes.
    let capacity = capacity.max(4).div_ceil(4) * 4;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: capacity,
        usage,
        mapped_at_creation: false,
    });
    if !contents.is_empty() {
        queue.write_buffer(&buffer, 0, contents);
    }
    buffer
}

/// CPU-side per-mesh build products shared by the in-place and rebuild
/// paths.
struct BuiltMesh {
    name: String,
    vertices: Vec<ModelVertex>,
    indices: Vec<u32>,
    padded_positions: Vec<[f32; 4]>,
    edge_indices: Vec<u32>,
    cpu_positions: Vec<[f32; 3]>,
    bounds: AABB,
    material_index: usize,
    has_uvs: bool,
}

impl BuiltMesh {
    fn vertex_bytes(&self) -> u64 {
        (self.vertices.len() * std::mem::size_of::<ModelVertex>()) as u64
    }
    fn index_bytes(&self) -> u64 {
        (self.indices.len() * 4) as u64
    }
    fn edge_pos_bytes(&self) -> u64 {
        (self.padded_positions.len() * 16) as u64
    }
    fn edge_idx_bytes(&self) -> u64 {
        (self.edge_indices.len() * 4) as u64
    }
}

/// Interleave cooked meshes into `ModelVertex` data — the same normals /
/// UV-default / tangent pipeline as `geometry::process_raw_model`, reading
/// the `Arc`-shared attribute buffers without cloning them.
fn build_meshes(cooked: &CookedGeometry) -> Vec<BuiltMesh> {
    let mut out = Vec::with_capacity(cooked.meshes.len());
    for mesh in &cooked.meshes {
        if mesh.positions.is_empty() || mesh.indices.is_empty() {
            continue;
        }
        out.push(build_mesh(mesh));
    }
    out
}

fn build_mesh(mesh: &CookedMesh) -> BuiltMesh {
    let positions: &[[f32; 3]] = &mesh.positions;
    let indices: &[u32] = &mesh.indices;

    let computed_normals;
    let normals: &[[f32; 3]] = if let Some(n) = &mesh.normals {
        n
    } else {
        computed_normals = compute_normals(positions, indices);
        &computed_normals
    };

    let default_uvs;
    let has_uvs = mesh.tex_coords.is_some();
    let tex_coords: &[[f32; 2]] = if let Some(tc) = &mesh.tex_coords {
        tc
    } else {
        default_uvs = vec![[0.0, 0.0]; positions.len()];
        &default_uvs
    };

    let (tangents, bitangents) = if has_uvs {
        compute_tangent_basis(positions, normals, tex_coords, indices)
    } else {
        compute_tangent_from_normal(normals)
    };

    let vertices: Vec<ModelVertex> = positions
        .iter()
        .enumerate()
        .map(|(i, pos)| ModelVertex {
            position: *pos,
            tex_coords: tex_coords[i],
            normal: normals[i],
            tangent: tangents[i],
            bitangent: bitangents[i],
        })
        .collect();

    let padded_positions: Vec<[f32; 4]> =
        positions.iter().map(|p| [p[0], p[1], p[2], 0.0]).collect();
    let edge_indices = extract_edges(indices);
    let bounds = compute_bounds(positions);

    BuiltMesh {
        name: mesh.name.clone(),
        vertices,
        indices: mesh.indices.to_vec(),
        padded_positions,
        edge_indices,
        cpu_positions: positions.to_vec(),
        bounds,
        material_index: mesh.material_index.unwrap_or(0),
        has_uvs,
    }
}

/// Convenience for tests and the dev harness: wrap plain buffers into a
/// one-mesh [`CookedGeometry`].
#[must_use]
pub fn cooked_from_parts(
    name: &str,
    positions: Vec<[f32; 3]>,
    indices: Vec<u32>,
    normals: Option<Vec<[f32; 3]>>,
) -> CookedGeometry {
    let bounds = compute_bounds(&positions);
    CookedGeometry {
        meshes: vec![CookedMesh {
            name: name.to_string(),
            positions: Arc::new(positions),
            normals: normals.map(Arc::new),
            tex_coords: None,
            indices: Arc::new(indices),
            material_index: None,
        }],
        materials: Vec::new(),
        bounds,
    }
}
