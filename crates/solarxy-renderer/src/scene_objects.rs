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
    MeshTopology, compute_bounds, compute_normals, compute_tangent_basis,
    compute_tangent_from_normal, extract_edges,
};
use solarxy_core::scene::{
    CameraDef, CookedGeometry, CookedMesh, LightDef, SceneDelta, SceneObjectId, SceneOp,
};
use solarxy_core::validation::ValidationResult;

use crate::bind_groups::BindGroupLayouts;
use crate::error::RendererError;
use crate::frame::ObjectValidationGpu;
use crate::model::{CpuMesh, EdgeData, Mesh, Model, ModelVertex};
use crate::pipelines::InstanceRaw;
use crate::resources;
use crate::validation::{build_mesh_category_map, build_mesh_edge_indices};

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
    color_bytes: u64,
}

/// One renderable object: an owned [`Model`] (the same struct the
/// single-model path draws), its world transform on the instance path,
/// and growable-buffer bookkeeping.
pub struct SceneObject {
    pub model: Model,
    pub transform: Matrix4<f32>,
    pub visible: bool,
    /// Whether the object is drawn into the shadow map (`SetCastShadow`);
    /// orthogonal to which light owns the map.
    pub cast_shadow: bool,
    /// One `InstanceRaw` per object — a transform-only change is a single
    /// small buffer write, never a geometry re-upload.
    pub instance_buffer: wgpu::Buffer,
    caps: Vec<MeshCaps>,
    /// The last-applied cooked geometry, for pointer-identity dedupe: the
    /// engine re-lowers the full delta each frame, so an upsert whose
    /// attribute `Arc`s are unchanged is skipped entirely.
    geometry: Arc<CookedGeometry>,
    /// Cooked-mesh index to GPU-mesh index (empty meshes are skipped at
    /// build). Validation issue scopes carry raw indices; this remaps them.
    raw_to_gpu: Vec<Option<usize>>,
    /// The object's effective validation result (`SetValidation`), deduped
    /// by `Arc` identity.
    validation: Option<Arc<ValidationResult>>,
    /// GPU overlay resources derived from `validation` against the current
    /// meshes (category tints + issue edge index buffers).
    validation_gpu: Option<ObjectValidationGpu>,
}

/// The dynamic scene: objects in deterministic id order plus the last
/// received light list.
#[derive(Default)]
pub struct SceneObjects {
    objects: BTreeMap<SceneObjectId, SceneObject>,
    lights: Option<Vec<LightDef>>,
    lights_dirty: bool,
    /// Engine-provided cameras (from `camera` nodes). Non-drawn scene objects
    /// the host reads back to drive a pane's look-through view and to draw the
    /// wireframe camera gizmos.
    cameras: Option<Vec<CameraDef>>,
    /// Content-addressed GPU textures shared across materials and cooks;
    /// swept when objects leave the scene.
    texture_cache: resources::TextureCache,
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

    /// One visible object as a [`crate::frame::DrawObject`], or `None` if
    /// absent or hidden (the selection-tint lookup).
    pub fn draw_object(&self, id: SceneObjectId) -> Option<crate::frame::DrawObject<'_>> {
        self.objects
            .get(&id)
            .filter(|o| o.visible)
            .map(|o| crate::frame::DrawObject {
                model: &o.model,
                instance_buffer: &o.instance_buffer,
                validation: o.validation_gpu.as_ref(),
                selected: false,
                cast_shadow: o.cast_shadow,
            })
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
                validation: o.validation_gpu.as_ref(),
                selected: false,
                cast_shadow: o.cast_shadow,
            })
    }

    /// The object's effective validation result (set by `SetValidation`),
    /// for issue lists and camera fly-to.
    #[must_use]
    pub fn validation(&self, id: SceneObjectId) -> Option<&Arc<ValidationResult>> {
        self.objects.get(&id)?.validation.as_ref()
    }

    /// The object's cooked-to-GPU mesh index remap (validation issue
    /// scopes carry raw indices).
    #[must_use]
    pub fn raw_to_gpu(&self, id: SceneObjectId) -> Option<&[Option<usize>]> {
        self.objects.get(&id).map(|o| o.raw_to_gpu.as_slice())
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

    /// The engine-provided cameras (from `camera` nodes); `None` means no
    /// `SetCameras` has arrived. The host reads these to drive a pane's
    /// look-through view and to draw the wireframe camera gizmos.
    #[must_use]
    pub fn cameras(&self) -> Option<&[CameraDef]> {
        self.cameras.as_deref()
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
                    self.upsert_geometry(device, queue, layouts, *id, Arc::clone(geometry))?;
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
                SceneOp::SetCastShadow { id, cast_shadow } => {
                    if let Some(obj) = self.objects.get_mut(id) {
                        obj.cast_shadow = *cast_shadow;
                    }
                }
                SceneOp::SetValidation { id, validation } => {
                    self.set_validation(device, *id, validation.as_ref());
                }
                SceneOp::Remove { id } => {
                    self.objects.remove(id);
                    self.texture_cache.sweep();
                }
                SceneOp::SetLights { lights } => {
                    self.lights = Some(lights.clone());
                    self.lights_dirty = true;
                }
                SceneOp::SetCameras { cameras } => {
                    self.cameras = Some(cameras.clone());
                }
                SceneOp::Clear => {
                    self.objects.clear();
                    self.texture_cache.sweep();
                    self.lights = None;
                    self.lights_dirty = true;
                    self.cameras = None;
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
        cooked_arc: Arc<CookedGeometry>,
    ) -> Result<(), RendererError> {
        // Identity dedupe: the engine re-lowers the full delta each frame,
        // so an upsert whose attribute/material `Arc`s all match the
        // last-applied geometry is a no-op (no re-upload, and the derived
        // validation resources stay valid).
        if let Some(obj) = self.objects.get(&id)
            && same_geometry(&obj.geometry, &cooked_arc)
        {
            return Ok(());
        }
        let cooked: &CookedGeometry = &cooked_arc;
        let (built, raw_to_gpu) = build_meshes(cooked);

        // In-place fast path: same mesh count, same material count, and
        // every new buffer fits its recorded capacity. A changed material
        // TABLE (new `Arc`s: the material node's factor drags produce
        // exactly this) re-uploads materials only, through the content
        // cache, so unchanged textures cost nothing and the geometry
        // buffers still rewrite in place.
        if let Some(obj) = self.objects.get_mut(&id)
            && obj.model.meshes.len() == built.len()
            && obj.model.materials.len() == cooked.materials.len().max(1)
            && built
                .iter()
                .zip(&obj.model.meshes)
                .all(|(b, m)| b.padded_uvs.is_empty() != m.uv_edge_data.is_some())
            && built
                .iter()
                .zip(&obj.model.meshes)
                .all(|(b, m)| b.wants_color_buffer() == m.color_buffer.is_some())
            && built.iter().zip(&obj.caps).all(|(b, caps)| {
                b.vertex_bytes() <= caps.vertex_bytes
                    && b.index_bytes() <= caps.index_bytes
                    && b.edge_pos_bytes() <= caps.edge_pos_bytes
                    && b.edge_idx_bytes() <= caps.edge_idx_bytes
                    && b.color_bytes() <= caps.color_bytes
            })
        {
            let materials_unchanged = obj.geometry.materials.len() == cooked.materials.len()
                && obj
                    .geometry
                    .materials
                    .iter()
                    .zip(&cooked.materials)
                    .all(|(a, b)| Arc::ptr_eq(a, b));
            if !materials_unchanged {
                obj.model.materials = resources::upload_cooked_materials(
                    &cooked.materials,
                    device,
                    queue,
                    &layouts.texture,
                    &mut self.texture_cache,
                )?;
            }
            for (mesh, b) in obj.model.meshes.iter_mut().zip(&built) {
                queue.write_buffer(&mesh.vertex_buffer, 0, bytemuck::cast_slice(&b.vertices));
                queue.write_buffer(&mesh.index_buffer, 0, bytemuck::cast_slice(&b.indices));
                mesh.num_elements = b.indices.len() as u32;
                mesh.num_vertices = b.vertices.len() as u32;
                mesh.topology = b.topology;
                mesh.material = b.material_index;
                if let (Some(buffer), Some(colors)) = (&mesh.color_buffer, &b.colors)
                    && b.wants_color_buffer()
                {
                    queue.write_buffer(buffer, 0, bytemuck::cast_slice(colors));
                }
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
                if let Some(uv) = &mesh.uv_edge_data
                    && !b.padded_uvs.is_empty()
                {
                    queue.write_buffer(&uv.uv_buffer, 0, bytemuck::cast_slice(&b.padded_uvs));
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
            obj.model.cpu_meshes = built.iter().map(BuiltMesh::cpu_mesh).collect();
            obj.model.mesh_bounds = built.iter().map(|b| b.bounds).collect();
            obj.model.bounds = cooked.bounds;
            obj.model.has_uvs = built.iter().any(|b| b.has_uvs);
            obj.geometry = Arc::clone(&cooked_arc);
            obj.raw_to_gpu = raw_to_gpu;
            // Geometry content changed: derived validation resources are
            // stale. The same-delta `SetValidation` (lowered right after
            // the upsert) rebuilds them against the new meshes.
            obj.validation = None;
            obj.validation_gpu = None;
            for mesh in &mut obj.model.meshes {
                mesh.degen_index_buffer = None;
                mesh.degen_num_elements = 0;
            }
            return Ok(());
        }

        // Full (re)build: new buffers with headroom; transform and
        // visibility survive a rebuild.
        let materials = resources::upload_cooked_materials(
            &cooked.materials,
            device,
            queue,
            &layouts.texture,
            &mut self.texture_cache,
        )?;
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
            let color_cap = with_headroom(b.color_bytes());

            let color_buffer = if b.wants_color_buffer() {
                b.colors.as_ref().map(|colors| {
                    create_with_capacity(
                        device,
                        queue,
                        "SceneObject Color Buffer",
                        color_cap,
                        bytemuck::cast_slice(colors),
                        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    )
                })
            } else {
                None
            };

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

            // UV-space wire resources (the UV pane), only for real UVs;
            // shares the edge index buffer, like the desktop loader.
            let uv_edge_data = if b.padded_uvs.is_empty() {
                None
            } else {
                let uv_buffer = create_with_capacity(
                    device,
                    queue,
                    "SceneObject UV Positions",
                    edge_pos_cap,
                    bytemuck::cast_slice(&b.padded_uvs),
                    wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                );
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("SceneObject UV Edge Bind Group"),
                    layout: &layouts.edge_geometry,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: uv_buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: edge_index.as_entire_binding(),
                        },
                    ],
                });
                Some(crate::model::UvEdgeData {
                    uv_buffer,
                    bind_group,
                })
            };

            meshes.push(Mesh {
                name: b.name.clone(),
                vertex_buffer,
                index_buffer,
                num_elements: b.indices.len() as u32,
                num_vertices: b.vertices.len() as u32,
                material: b.material_index,
                topology: b.topology,
                color_buffer,
                visible: true,
                edge_data: Some(EdgeData {
                    positions_buffer: edge_positions,
                    index_buffer: edge_index,
                    num_edges: (b.edge_indices.len() / 2) as u32,
                    bind_group: edge_bind_group,
                }),
                uv_edge_data,
                degen_index_buffer: None,
                degen_num_elements: 0,
            });
            caps.push(MeshCaps {
                vertex_bytes: vertex_cap,
                index_bytes: index_cap,
                edge_pos_bytes: edge_pos_cap,
                edge_idx_bytes: edge_idx_cap,
                color_bytes: color_cap,
            });
        }

        let model = Model {
            meshes,
            materials,
            bounds: cooked.bounds,
            mesh_bounds: built.iter().map(|b| b.bounds).collect(),
            cpu_meshes: built.iter().map(BuiltMesh::cpu_mesh).collect(),
            material_thumbnails,
            has_uvs: built.iter().any(|b| b.has_uvs),
        };

        if let Some(existing) = self.objects.get_mut(&id) {
            existing.model = model;
            existing.caps = caps;
            existing.geometry = Arc::clone(&cooked_arc);
            existing.raw_to_gpu = raw_to_gpu;
            // Fresh meshes carry no degen buffers; drop the stale overlay
            // resources (the same-delta `SetValidation` rebuilds them).
            existing.validation = None;
            existing.validation_gpu = None;
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
                    cast_shadow: true,
                    instance_buffer,
                    caps,
                    geometry: cooked_arc,
                    raw_to_gpu,
                    validation: None,
                    validation_gpu: None,
                },
            );
        }
        Ok(())
    }

    /// Applies a `SetValidation` op: dedupes by `Arc` identity, then
    /// (re)builds the overlay resources — the per-mesh category map, the
    /// issue edge index buffers, and the degenerate-triangle index buffers
    /// — exactly as the desktop load path does at model upload.
    fn set_validation(
        &mut self,
        device: &wgpu::Device,
        id: SceneObjectId,
        validation: Option<&Arc<ValidationResult>>,
    ) {
        let Some(obj) = self.objects.get_mut(&id) else {
            return;
        };
        match (validation, &obj.validation) {
            (None, None) => return,
            (Some(new), Some(current)) if Arc::ptr_eq(new, current) => return,
            _ => {}
        }
        // Clear the previous overlay resources in either direction.
        obj.validation_gpu = None;
        for mesh in &mut obj.model.meshes {
            mesh.degen_index_buffer = None;
            mesh.degen_num_elements = 0;
        }
        let Some(v) = validation else {
            obj.validation = None;
            return;
        };

        let gpu_mesh_count = obj.model.meshes.len();
        let mesh_cat = build_mesh_category_map(&v.report, gpu_mesh_count, &obj.raw_to_gpu);
        let edge_buffers: Vec<Option<(wgpu::Buffer, u32)>> =
            build_mesh_edge_indices(&v.report, gpu_mesh_count, &obj.raw_to_gpu)
                .into_iter()
                .enumerate()
                .map(|(mi, indices)| {
                    if indices.is_empty() {
                        None
                    } else {
                        let buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some(&format!("SceneObject Validation Edge Indices {mi}")),
                            contents: bytemuck::cast_slice(&indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });
                        Some((buf, indices.len() as u32))
                    }
                })
                .collect();

        // Degenerate-triangle index buffers, remapped raw-to-GPU and
        // resolved against each mesh's retained CPU index copy.
        for (raw_idx, faces) in v.degenerate_faces.iter().enumerate() {
            if faces.is_empty() {
                continue;
            }
            let Some(Some(gpu_idx)) = obj.raw_to_gpu.get(raw_idx) else {
                continue;
            };
            let Some(cpu) = obj.model.cpu_meshes.get(*gpu_idx) else {
                continue;
            };
            let degen_indices: Vec<u32> = faces
                .iter()
                .filter_map(|&fi| {
                    let base = fi as usize * 3;
                    let tri = cpu.indices.get(base..base + 3)?;
                    Some([tri[0], tri[1], tri[2]])
                })
                .flatten()
                .collect();
            if degen_indices.is_empty() {
                continue;
            }
            if let Some(mesh) = obj.model.meshes.get_mut(*gpu_idx) {
                mesh.degen_num_elements = degen_indices.len() as u32;
                mesh.degen_index_buffer = Some(device.create_buffer_init(
                    &wgpu::util::BufferInitDescriptor {
                        label: Some(&format!("SceneObject Degen Index Buffer {gpu_idx}")),
                        contents: bytemuck::cast_slice(&degen_indices),
                        usage: wgpu::BufferUsages::INDEX,
                    },
                ));
            }
        }

        obj.validation = Some(Arc::clone(v));
        obj.validation_gpu = Some(ObjectValidationGpu {
            mesh_cat,
            edge_buffers,
        });
    }
}

/// Whether two cooked geometries are identical by attribute-buffer and
/// material `Arc` identity (the engine's cook cache shares those `Arc`s
/// across frames, so pointer equality means content equality).
fn same_geometry(a: &CookedGeometry, b: &CookedGeometry) -> bool {
    fn same_opt<T>(x: Option<&Arc<T>>, y: Option<&Arc<T>>) -> bool {
        match (x, y) {
            (Some(x), Some(y)) => Arc::ptr_eq(x, y),
            (None, None) => true,
            _ => false,
        }
    }
    a.meshes.len() == b.meshes.len()
        && a.materials.len() == b.materials.len()
        && a.meshes.iter().zip(&b.meshes).all(|(x, y)| {
            Arc::ptr_eq(&x.positions, &y.positions)
                && Arc::ptr_eq(&x.indices, &y.indices)
                && same_opt(x.normals.as_ref(), y.normals.as_ref())
                && same_opt(x.tex_coords.as_ref(), y.tex_coords.as_ref())
                && same_opt(x.colors.as_ref(), y.colors.as_ref())
                && x.material_index == y.material_index
                && x.topology == y.topology
        })
        && a.materials
            .iter()
            .zip(&b.materials)
            .all(|(x, y)| Arc::ptr_eq(x, y))
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
    /// Padded `[u, 1-v, 0, 0]` per vertex for the UV-space wire pass
    /// (`uv_edge_data`); empty when the mesh has no real UVs. Same length
    /// as `padded_positions`, so the edge-positions capacity covers it.
    padded_uvs: Vec<[f32; 4]>,
    topology: MeshTopology,
    /// The cooked color lane, shared by refcount. Triangle and line meshes
    /// upload it as the location-12 vertex buffer; point meshes instead
    /// pack it sRGB8 into `padded_positions[i][3]` at build time.
    colors: Option<Arc<Vec<[f32; 4]>>>,
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
    /// Whether this mesh gets a location-12 color vertex buffer (points
    /// carry their color inside the padded positions instead).
    fn wants_color_buffer(&self) -> bool {
        self.colors.is_some() && self.topology != MeshTopology::Points
    }
    /// The CPU mirror for picking and review hashing. Both consume the
    /// index list as triangles, and points/lines are unpickable (M-4), so
    /// non-triangle meshes keep positions but expose no indices.
    fn cpu_mesh(&self) -> CpuMesh {
        CpuMesh {
            positions: self.cpu_positions.clone(),
            indices: if self.topology == MeshTopology::Triangles {
                self.indices.clone()
            } else {
                Vec::new()
            },
        }
    }
    fn color_bytes(&self) -> u64 {
        if self.wants_color_buffer() {
            (self.vertices.len() * 16) as u64
        } else {
            0
        }
    }
}

/// Encode one linear color channel to its sRGB byte (the GPU-standard
/// transfer curve; the point shader decodes it back to linear).
fn linear_to_srgb_u8(linear: f32) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let srgb = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0 + 0.5) as u8
}

/// Pack a linear RGBA color into the u32 the point shader unpacks
/// (`unpack4x8unorm` then sRGB decode); bit-preserved through the f32 slot.
pub(crate) fn pack_point_color(color: [f32; 4]) -> f32 {
    let bits = u32::from(linear_to_srgb_u8(color[0]))
        | u32::from(linear_to_srgb_u8(color[1])) << 8
        | u32::from(linear_to_srgb_u8(color[2])) << 16
        | 0xFF00_0000;
    f32::from_bits(bits)
}

/// The packed white default for uncolored point clouds.
const POINT_WHITE: u32 = 0xFFFF_FFFF;

/// Interleave cooked meshes into `ModelVertex` data — the same normals /
/// UV-default / tangent pipeline as `geometry::process_raw_model`, reading
/// the `Arc`-shared attribute buffers without cloning them. The second
/// return is the cooked-to-GPU index remap (empty meshes are skipped), the
/// same shape as the desktop loader's `raw_to_gpu`.
fn build_meshes(cooked: &CookedGeometry) -> (Vec<BuiltMesh>, Vec<Option<usize>>) {
    let mut out = Vec::with_capacity(cooked.meshes.len());
    let mut raw_to_gpu = Vec::with_capacity(cooked.meshes.len());
    for mesh in &cooked.meshes {
        // Per-topology drawability: a point cloud needs positions only;
        // indexed topologies also need indices.
        let drawable = !mesh.positions.is_empty()
            && (mesh.topology == MeshTopology::Points || !mesh.indices.is_empty());
        if !drawable {
            raw_to_gpu.push(None);
            continue;
        }
        raw_to_gpu.push(Some(out.len()));
        out.push(build_mesh(mesh));
    }
    (out, raw_to_gpu)
}

fn build_mesh(mesh: &CookedMesh) -> BuiltMesh {
    let positions: &[[f32; 3]] = &mesh.positions;
    let indices: &[u32] = &mesh.indices;
    let is_triangles = mesh.topology == MeshTopology::Triangles;

    let computed_normals;
    let normals: &[[f32; 3]] = if let Some(n) = &mesh.normals {
        n
    } else if is_triangles {
        computed_normals = compute_normals(positions, indices);
        &computed_normals
    } else {
        // Face-normal accumulation reads index triples; a pair or empty
        // list has no faces. Lines and points shade unlit anyway.
        computed_normals = vec![[0.0, 0.0, 1.0]; positions.len()];
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

    // The padded position's fourth slot is unused by the wire/validation
    // shaders (they read xyz); for a point cloud it carries the packed
    // sRGB8 color the point-quad shader unpacks.
    let padded_positions: Vec<[f32; 4]> = match (mesh.topology, &mesh.colors) {
        (MeshTopology::Points, Some(colors)) => positions
            .iter()
            .zip(colors.iter())
            .map(|(p, c)| [p[0], p[1], p[2], pack_point_color(*c)])
            .collect(),
        (MeshTopology::Points, None) => positions
            .iter()
            .map(|p| [p[0], p[1], p[2], f32::from_bits(POINT_WHITE)])
            .collect(),
        _ => positions.iter().map(|p| [p[0], p[1], p[2], 0.0]).collect(),
    };
    // The UV-space wire positions (V flipped, like the desktop loader).
    let padded_uvs: Vec<[f32; 4]> = if has_uvs {
        tex_coords
            .iter()
            .map(|uv| [uv[0], 1.0 - uv[1], 0.0, 0.0])
            .collect()
    } else {
        Vec::new()
    };
    // Wireframe edges: unique triangle edges for meshes, the segments
    // themselves for a polyline (its wireframe IS the polyline), nothing
    // for points. `extract_edges` walks index triples, so feeding it a
    // pair list would fabricate edges.
    let edge_indices = match mesh.topology {
        MeshTopology::Triangles => extract_edges(indices),
        MeshTopology::Lines => indices.to_vec(),
        MeshTopology::Points => Vec::new(),
    };
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
        padded_uvs,
        topology: mesh.topology,
        colors: mesh.colors.clone(),
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
            topology: MeshTopology::Triangles,
            colors: None,
        }],
        materials: Vec::new(),
        bounds,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cooked_mesh(name: &str, positions: Vec<[f32; 3]>, indices: Vec<u32>) -> CookedMesh {
        CookedMesh {
            name: name.to_string(),
            positions: Arc::new(positions),
            normals: None,
            tex_coords: None,
            indices: Arc::new(indices),
            material_index: None,
            topology: MeshTopology::Triangles,
            colors: None,
        }
    }

    fn tri() -> CookedMesh {
        cooked_mesh(
            "tri",
            vec![[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            vec![0, 1, 2],
        )
    }

    #[test]
    fn build_meshes_maps_raw_to_gpu_skipping_empties() {
        let cooked = CookedGeometry {
            meshes: vec![
                cooked_mesh("empty", vec![], vec![]),
                tri(),
                cooked_mesh("also-empty", vec![[0.0; 3]], vec![]),
            ],
            materials: Vec::new(),
            bounds: compute_bounds(&[[0.0; 3]]),
        };
        let (built, raw_to_gpu) = build_meshes(&cooked);
        assert_eq!(built.len(), 1);
        assert_eq!(raw_to_gpu, vec![None, Some(0), None]);
    }

    #[test]
    fn same_geometry_compares_by_arc_identity() {
        let a = CookedGeometry {
            meshes: vec![tri()],
            materials: Vec::new(),
            bounds: compute_bounds(&[[0.0; 3]]),
        };
        // Shared attribute Arcs: identical (the engine's per-frame
        // re-lowering case).
        let shared = CookedGeometry {
            meshes: vec![CookedMesh {
                name: "tri".to_string(),
                positions: Arc::clone(&a.meshes[0].positions),
                normals: None,
                tex_coords: None,
                indices: Arc::clone(&a.meshes[0].indices),
                material_index: None,
                topology: MeshTopology::Triangles,
                colors: None,
            }],
            materials: Vec::new(),
            bounds: a.bounds,
        };
        assert!(same_geometry(&a, &shared));

        // Equal content behind fresh Arcs: a recook; must re-upload.
        let recooked = CookedGeometry {
            meshes: vec![tri()],
            materials: Vec::new(),
            bounds: a.bounds,
        };
        assert!(!same_geometry(&a, &recooked));

        // Mesh-count change.
        let grown = CookedGeometry {
            meshes: vec![],
            materials: Vec::new(),
            bounds: a.bounds,
        };
        assert!(!same_geometry(&a, &grown));
    }

    /// The W2c dedupe channels: a changed colors Arc or a changed topology
    /// tag must never be swallowed as "same geometry" (the color-drag and
    /// topology-switch recook cases).
    #[test]
    fn same_geometry_sees_color_and_topology_changes() {
        let base = tri();
        let a = CookedGeometry {
            meshes: vec![CookedMesh {
                colors: Some(Arc::new(vec![[1.0, 0.0, 0.0, 1.0]; 3])),
                ..base.clone()
            }],
            materials: Vec::new(),
            bounds: compute_bounds(&[[0.0; 3]]),
        };
        // Same buffers, fresh colors Arc: a color recook, must re-upload.
        let recolored = CookedGeometry {
            meshes: vec![CookedMesh {
                colors: Some(Arc::new(vec![[0.0, 1.0, 0.0, 1.0]; 3])),
                ..a.meshes[0].clone()
            }],
            materials: Vec::new(),
            bounds: a.bounds,
        };
        assert!(!same_geometry(&a, &recolored));

        // Identical Arcs: dedupe holds.
        let same = CookedGeometry {
            meshes: vec![a.meshes[0].clone()],
            materials: Vec::new(),
            bounds: a.bounds,
        };
        assert!(same_geometry(&a, &same));

        // Same buffers, different topology tag.
        let retopo = CookedGeometry {
            meshes: vec![CookedMesh {
                topology: MeshTopology::Lines,
                ..a.meshes[0].clone()
            }],
            materials: Vec::new(),
            bounds: a.bounds,
        };
        assert!(!same_geometry(&a, &retopo));
    }

    /// W2b: point clouds and polylines build GPU meshes with the right
    /// counts, edge semantics, and packed point colors.
    #[test]
    fn build_meshes_admits_point_clouds_and_polylines() {
        let cloud = CookedMesh {
            name: "cloud".to_string(),
            positions: Arc::new(vec![[0.0; 3], [1.0; 3]]),
            normals: None,
            tex_coords: None,
            indices: Arc::new(vec![]),
            material_index: None,
            topology: MeshTopology::Points,
            colors: Some(Arc::new(vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 0.0, 1.0]])),
        };
        let wire = CookedMesh {
            name: "wire".to_string(),
            positions: Arc::new(vec![[0.0; 3], [1.0; 3], [2.0; 3]]),
            normals: None,
            tex_coords: None,
            indices: Arc::new(vec![0, 1, 1, 2]),
            material_index: None,
            topology: MeshTopology::Lines,
            colors: None,
        };
        let cooked = CookedGeometry {
            meshes: vec![cloud, wire],
            materials: Vec::new(),
            bounds: compute_bounds(&[[0.0; 3]]),
        };
        let (built, raw_to_gpu) = build_meshes(&cooked);
        assert_eq!(raw_to_gpu, vec![Some(0), Some(1)]);

        let cloud_b = &built[0];
        assert_eq!(cloud_b.topology, MeshTopology::Points);
        assert_eq!(cloud_b.vertices.len(), 2);
        assert!(cloud_b.edge_indices.is_empty(), "points have no wire form");
        assert!(!cloud_b.wants_color_buffer(), "point colors pack instead");
        // Packed colors: full-red sRGB stays 255; linear black keeps alpha.
        let red_bits = cloud_b.padded_positions[0][3].to_bits();
        assert_eq!(red_bits & 0xFF, 0xFF, "red channel");
        assert_eq!(red_bits >> 24, 0xFF, "alpha forced opaque");
        let black_bits = cloud_b.padded_positions[1][3].to_bits();
        assert_eq!(black_bits & 0x00FF_FFFF, 0, "black packs to zero rgb");
        // The CPU mirror exposes no indices (unpickable per M-4).
        assert!(cloud_b.cpu_mesh().indices.is_empty());

        let wire_b = &built[1];
        assert_eq!(wire_b.topology, MeshTopology::Lines);
        assert_eq!(
            wire_b.edge_indices,
            vec![0, 1, 1, 2],
            "a polyline's wireframe is itself"
        );
        assert!(wire_b.cpu_mesh().indices.is_empty());

        // An indexless point cloud is drawable; an indexless line is not.
        let cooked2 = CookedGeometry {
            meshes: vec![CookedMesh {
                name: "empty-wire".to_string(),
                positions: Arc::new(vec![[0.0; 3]]),
                normals: None,
                tex_coords: None,
                indices: Arc::new(vec![]),
                material_index: None,
                topology: MeshTopology::Lines,
                colors: None,
            }],
            materials: Vec::new(),
            bounds: compute_bounds(&[[0.0; 3]]),
        };
        let (built2, map2) = build_meshes(&cooked2);
        assert!(built2.is_empty());
        assert_eq!(map2, vec![None]);
    }

    #[test]
    fn point_color_packing_is_srgb8_with_white_default() {
        assert_eq!(
            pack_point_color([1.0, 1.0, 1.0, 1.0]).to_bits(),
            0xFFFF_FFFF
        );
        assert_eq!(
            pack_point_color([0.0, 0.0, 0.0, 0.0]).to_bits(),
            0xFF00_0000
        );
        // Mid-gray linear 0.5 encodes to sRGB ~188.
        let bits = pack_point_color([0.5, 0.5, 0.5, 1.0]).to_bits();
        let r = bits & 0xFF;
        assert!((186..=190).contains(&r), "got {r}");
    }
}
