//! Traced scene ingestion: the delta stream in, a packed arena out.
//!
//! This sits above [`super::arena`], which packs, and below [`super::TraceScene`],
//! which uploads. What is here is the middle: applying the same scene ops the
//! raster path applies, keeping one hierarchy per distinct mesh, and deciding
//! when anything has to be rebuilt.
//!
//! **There is no `wgpu` in this file, and that is a constraint rather than an
//! accident.** The expensive half of ingestion is the hierarchy build, and that
//! build has to move into the import worker, which hosts a headless wasm
//! instance with no GPU at all. A device handle reaching in here would make
//! that move a rewrite instead of a relocation.
//!
//! # Cost, and why nothing calls this from a frame loop yet
//!
//! [`Bvh::build_triangles`] over a million triangles is a few hundred
//! milliseconds, and it runs inside [`TraceSceneCache::apply`]. Until that
//! build is asynchronous, calling `apply` on a frame path stalls the frame for
//! as long as the model is large. That is the reason this type is driven by
//! tests and by the still-render job, and not by either shell's per-frame
//! drain of the delta stream.
//!
//! A repack is a full one: [`TraceArena::build`] concatenates everything, and
//! the top-level hierarchy sits first in the node and permutation buffers so
//! the kernel can start at node zero with no offset. Any change to the
//! top-level node count therefore shifts every mesh's bases, and there is no
//! prefix to reuse. The hierarchy cache is what keeps that affordable: a
//! transform change repacks (a memcpy proportional to the scene) but rebuilds
//! no hierarchy at all.
//!
//! The identified next increment, when a preview mode wants this per frame:
//! [`SceneOp::SetVisible`] and [`SceneOp::SetCastShadow`] provably cannot move
//! any world-space box, so they cannot change the top-level hierarchy, so they
//! need only rewrite [`super::arena::Instance::flags`] in place and re-upload
//! one buffer. Everything else genuinely repacks.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::Arc;

use cgmath::{Matrix4, SquareMatrix};
use solarxy_bvh::{Bvh, BvhStats};
use solarxy_core::aabb::AABB;
use solarxy_core::geometry::MeshTopology;
use solarxy_core::scene::{
    CameraDef, CookedGeometry, InstanceXform, LightDef, SceneDelta, SceneObjectId, SceneOp,
};

use super::arena::{ArenaMesh, ArenaPlacement, INSTANCE_CAST_SHADOW, INSTANCE_VISIBLE, TraceArena};

/// Identity of the geometry a hierarchy was built over.
///
/// A `HashMap` cannot key on `Arc::ptr_eq`, so the key is the pair of inner
/// addresses that comparison looks at. `Arc::as_ptr`, not `Vec::as_ptr`: an
/// empty `Vec`'s data pointer is a shared dangling sentinel, and two different
/// empty meshes would collide on it.
///
/// A triangle hierarchy is a function of exactly these two buffers, so nothing
/// else belongs in the key. Topology is absent because the cache is only ever
/// consulted for triangle meshes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MeshKey {
    positions: usize,
    indices: usize,
}

impl MeshKey {
    fn of(mesh: &solarxy_core::scene::CookedMesh) -> Self {
        Self {
            positions: Arc::as_ptr(&mesh.positions) as usize,
            indices: Arc::as_ptr(&mesh.indices) as usize,
        }
    }
}

/// A cached hierarchy, holding the buffers it was built over.
struct CachedHierarchy {
    bvh: Arc<Bvh>,
    /// Strong clones of the two allocations [`MeshKey`] names. They are the
    /// mitigation, not bookkeeping: an address only identifies an allocation
    /// while that allocation is alive, so an entry that did not hold its
    /// buffers open could be hit by a later, different mesh that the allocator
    /// happened to place at the same two addresses. Holding them recovers
    /// `Arc::ptr_eq` semantics inside a `HashMap`.
    _positions: Arc<Vec<[f32; 3]>>,
    _indices: Arc<Vec<u32>>,
    stats: BvhStats,
}

/// Identity of a packed vertex range, within one pack.
///
/// Wider than [`MeshKey`] on purpose. A hierarchy depends only on positions and
/// indices; the packed vertex data also depends on the normals and uvs written
/// beside them. Two meshes sharing positions and indices while carrying
/// different normals are a legal construction, and deduping them on the
/// hierarchy key would give one of them the other's normals.
///
/// This one needs no pinning: every `Arc` it names is held alive for the whole
/// call by the object that owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PackKey {
    positions: usize,
    indices: usize,
    normals: Option<usize>,
    uv0: Option<usize>,
}

/// One ingested mesh: a triangle mesh of some object, and where it sits.
struct TracedMesh {
    /// This mesh's index in the owning object's cooked mesh list, which is what
    /// relates it back to the raster path's view of the same object.
    cooked_index: u32,
    key: MeshKey,
    /// The placement list, or `None` for the implicit single identity, keeping
    /// the convention `CookedMesh::instances` sets.
    placements: Option<Arc<Vec<InstanceXform>>>,
    /// Object-space bounds, so a top-level box costs eight corner transforms
    /// per placement rather than one per vertex.
    bounds: AABB,
    material_base: u32,
}

/// One object's traced state.
pub struct TracedObject {
    pub transform: Matrix4<f32>,
    pub visible: bool,
    pub cast_shadow: bool,
    /// The triangle meshes this object contributes, in cooked order. Non-
    /// triangle and undrawable meshes are absent.
    meshes: Vec<TracedMesh>,
    /// The last-applied cooked geometry: the dedupe compares against it, and it
    /// owns every attribute buffer the pack reads.
    geometry: Arc<CookedGeometry>,
    /// First slot this object's materials occupy in the pool the material stage
    /// will build.
    material_base: u32,
}

impl TracedObject {
    /// `(cooked mesh index, placement count)` per ingested mesh, in cooked
    /// order. The fingerprint a comparison against the raster path uses.
    pub fn meshes(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        self.meshes.iter().map(|m| {
            let count = m
                .placements
                .as_ref()
                .map_or(1, |list| u32::try_from(list.len()).unwrap_or(u32::MAX));
            (m.cooked_index, count)
        })
    }
}

/// What the last pack took, and what it left behind.
///
/// Recomputed at each pack rather than accumulated, because this describes the
/// scene as it stands, which is what a panel shows, not a session total.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TraceSceneStats {
    pub objects: u32,
    /// Distinct packed vertex ranges, **after** dedupe, so this is smaller
    /// than the number of meshes the scene holds whenever two objects display
    /// the same prototype. It answers "how much vertex data is uploaded", not
    /// "how many meshes are traced"; the latter is the count of ingested
    /// meshes over [`TraceSceneCache::iter`].
    pub meshes: u32,
    /// Placements the kernel will walk.
    pub instances: u32,
    /// Drawable polyline meshes the tracer does not render.
    pub skipped_lines: u32,
    /// Drawable point clouds the tracer does not render.
    pub skipped_points: u32,
    /// Meshes with no positions, or no indices where the topology needs them.
    /// Nothing draws these either, so they are a diagnostic rather than a
    /// number to show anyone.
    pub skipped_empty: u32,
    /// Placements dropped because the composed transform had no usable inverse.
    pub singular_placements: u32,
    /// Triangles the hierarchy builder rejected, their indices pointing outside
    /// the position buffer. The count that explains a hole in a traced image.
    pub degenerate_triangles: u32,
    pub nodes: u32,
    pub triangles: u32,
    /// Leaves the depth cap forced. Non-zero means a hierarchy came out worse
    /// than the heuristic wanted.
    pub depth_capped_leaves: u32,
}

/// The traced scene: scene ops in, a packed [`TraceArena`] out.
#[derive(Default)]
pub struct TraceSceneCache {
    /// Deterministic id order, mirroring `SceneObjects` for the same reason:
    /// the pack order is the instance order, and the instance order is what the
    /// top-level hierarchy's leaves name.
    objects: BTreeMap<SceneObjectId, TracedObject>,
    /// One hierarchy per distinct positions-and-indices pair, shared by every
    /// object that names it.
    hierarchies: HashMap<MeshKey, CachedHierarchy>,
    arena: TraceArena,
    dirty: bool,
    /// Set by anything that can orphan a cached hierarchy.
    needs_sweep: bool,
    stats: TraceSceneStats,
    /// Kept for the light and camera stages, which have no consumer here yet.
    lights: Option<Vec<LightDef>>,
    cameras: Option<Vec<CameraDef>>,
}

impl TraceSceneCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Applies one delta batch, in order.
    ///
    /// Infallible, unlike the raster consumer's `apply`: nothing here touches a
    /// device, so there is no upload that can fail.
    pub fn apply(&mut self, delta: &SceneDelta) {
        for op in &delta.ops {
            // Every variant is matched, with no wildcard, so a scene op added
            // later cannot be silently dropped here.
            //
            // Two arms do nothing, and they are kept apart rather than merged
            // because they do nothing for two unrelated reasons. Collapsing
            // them into one pattern would delete one of the two explanations,
            // and the explanations are the entire content of those arms.
            #[allow(clippy::match_same_arms)]
            match op {
                SceneOp::UpsertGeometry { id, geometry } => self.upsert(*id, geometry),
                SceneOp::SetTransform { id, transform } => {
                    if let Some(obj) = self.objects.get_mut(id) {
                        obj.transform = Matrix4::from(*transform);
                        self.dirty = true;
                    }
                }
                SceneOp::SetVisible { id, visible } => {
                    if let Some(obj) = self.objects.get_mut(id) {
                        // The instance stays and its flag clears. The kernel
                        // already skips a cleared bit, and dropping it would
                        // renumber every later instance on a visibility toggle.
                        obj.visible = *visible;
                        self.dirty = true;
                    }
                }
                SceneOp::SetCastShadow { id, cast_shadow } => {
                    if let Some(obj) = self.objects.get_mut(id) {
                        obj.cast_shadow = *cast_shadow;
                        self.dirty = true;
                    }
                }
                SceneOp::Remove { id } => {
                    if self.objects.remove(id).is_some() {
                        self.dirty = true;
                        self.needs_sweep = true;
                    }
                }
                SceneOp::SetLights { lights } => self.lights = Some(lights.clone()),
                SceneOp::SetCameras { cameras } => self.cameras = Some(cameras.clone()),
                // Validation is an editor overlay: category tints and issue
                // edge lists drawn over the raster image. There is no traced
                // pass that could consume a result and no arena buffer it would
                // land in. The tracer renders the scene, not the editor's
                // annotations of it.
                SceneOp::SetValidation { .. } => {}
                // The environment is IBL and skybox state the shells' own
                // tracker owns, and for the tracer it becomes the equirect and
                // its sampling distribution in the sampled group, which a
                // wgpu-free type cannot build. Keeping the image here as well
                // would make a second authority for one HDRI.
                SceneOp::SetEnvironment { .. } => {}
                SceneOp::Clear => {
                    self.objects.clear();
                    self.hierarchies.clear();
                    self.lights = None;
                    self.cameras = None;
                    self.dirty = true;
                    self.needs_sweep = false;
                }
            }
        }

        // Swept once, here, after the whole batch. Not from inside the remove
        // arm: an entry is what holds its two buffers alive, so freeing one
        // mid-batch would let the allocator hand the same addresses to a mesh
        // arriving later in this same list, and the next lookup would hit a
        // hierarchy built over different geometry.
        if std::mem::take(&mut self.needs_sweep) {
            let live: HashSet<MeshKey> = self
                .objects
                .values()
                .flat_map(|o| o.meshes.iter().map(|m| m.key))
                .collect();
            self.hierarchies.retain(|key, _| live.contains(key));
        }
    }

    /// Repacks if anything changed since the last call, and hands back the
    /// arena when it did.
    ///
    /// `None` means the previous arena still stands and the GPU side has
    /// nothing to do. Stating that at the call site is why this returns an
    /// `Option` rather than being a getter that surprisingly takes `&mut self`.
    pub fn repack(&mut self) -> Option<&TraceArena> {
        if !std::mem::take(&mut self.dirty) {
            return None;
        }
        self.pack();
        Some(&self.arena)
    }

    /// The last packed arena, current or not.
    #[must_use]
    pub fn arena(&self) -> &TraceArena {
        &self.arena
    }

    #[must_use]
    pub fn stats(&self) -> TraceSceneStats {
        self.stats
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.objects.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&SceneObjectId, &TracedObject)> {
        self.objects.iter()
    }

    #[must_use]
    pub fn get(&self, id: SceneObjectId) -> Option<&TracedObject> {
        self.objects.get(&id)
    }

    /// The lights the engine published, stored but not yet packed.
    #[must_use]
    pub fn lights(&self) -> Option<&[LightDef]> {
        self.lights.as_deref()
    }

    /// The cameras the engine published, stored but not yet packed.
    #[must_use]
    pub fn cameras(&self) -> Option<&[CameraDef]> {
        self.cameras.as_deref()
    }

    /// How many slots the material pool must reserve, in this cache's object
    /// order.
    ///
    /// The bases this counts are correct **only** under a pool that gives each
    /// object one contiguous block and does not dedupe identical materials
    /// across objects. A pool that dedupes must recompute them rather than
    /// trust these, because a plausible image of the wrong materials is the
    /// failure mode, and it looks like a shading bug rather than a
    /// bookkeeping one.
    #[must_use]
    pub fn material_slots(&self) -> u32 {
        self.objects
            .values()
            .map(|o| {
                o.material_base + u32::try_from(o.geometry.materials.len().max(1)).unwrap_or(1)
            })
            .max()
            .unwrap_or(0)
    }

    /// The hierarchy built over one object's mesh, by its cooked index.
    #[must_use]
    pub fn hierarchy(&self, id: SceneObjectId, cooked_index: u32) -> Option<&Arc<Bvh>> {
        let obj = self.objects.get(&id)?;
        let mesh = obj.meshes.iter().find(|m| m.cooked_index == cooked_index)?;
        self.hierarchies.get(&mesh.key).map(|entry| &entry.bvh)
    }

    /// How many distinct hierarchies are cached.
    #[must_use]
    pub fn hierarchy_count(&self) -> usize {
        self.hierarchies.len()
    }

    fn upsert(&mut self, id: SceneObjectId, geometry: &Arc<CookedGeometry>) {
        // The same total-no-op check the raster path makes, against the same
        // identity: the engine re-lowers the whole delta every frame, so an
        // upsert whose buffers all match is nothing at all.
        if let Some(obj) = self.objects.get(&id)
            && crate::scene_objects::same_geometry(&obj.geometry, geometry)
        {
            return;
        }

        let mut meshes = Vec::new();
        for (index, mesh) in geometry.meshes.iter().enumerate() {
            // Matches the raster drawability predicate: a point cloud needs no
            // indices, everything else does. Meshes this rejects are counted
            // separately from the ones rejected for their topology, so the two
            // counters mean what their names say.
            let drawable = !mesh.positions.is_empty()
                && (mesh.topology == MeshTopology::Points || !mesh.indices.is_empty());
            if !drawable || mesh.topology != MeshTopology::Triangles {
                continue;
            }

            let key = MeshKey::of(mesh);
            self.hierarchies.entry(key).or_insert_with(|| {
                let bvh = Bvh::build_triangles(&mesh.positions, &mesh.indices);
                CachedHierarchy {
                    stats: bvh.stats(),
                    bvh: Arc::new(bvh),
                    _positions: Arc::clone(&mesh.positions),
                    _indices: Arc::clone(&mesh.indices),
                }
            });

            meshes.push(TracedMesh {
                cooked_index: u32::try_from(index).unwrap_or(u32::MAX),
                key,
                placements: mesh.instances.clone(),
                bounds: bounds_of(&mesh.positions),
                material_base: u32::try_from(mesh.material_index.unwrap_or(0)).unwrap_or(0),
            });
        }

        match self.objects.get_mut(&id) {
            Some(obj) => {
                // Replacing geometry keeps the object's own properties, which
                // is what the raster path does and what the engine assumes: it
                // sends a transform once and re-lowers geometry every frame.
                obj.meshes = meshes;
                obj.geometry = Arc::clone(geometry);
                self.needs_sweep = true;
            }
            None => {
                self.objects.insert(
                    id,
                    TracedObject {
                        transform: Matrix4::identity(),
                        visible: true,
                        cast_shadow: true,
                        meshes,
                        geometry: Arc::clone(geometry),
                        material_base: 0,
                    },
                );
            }
        }
        self.dirty = true;
    }

    fn pack(&mut self) {
        let mut stats = TraceSceneStats::default();

        // Material bases are assigned in object order, one block per object.
        // `max(1)` and the `unwrap_or(0)` below mirror the raster path exactly,
        // where an empty material list still occupies one slot holding a
        // synthesized default, so the two agree about which material a mesh is.
        let mut base = 0u32;
        for obj in self.objects.values_mut() {
            obj.material_base = base;
            base = base
                .saturating_add(u32::try_from(obj.geometry.materials.len().max(1)).unwrap_or(1));
        }

        // Re-count the skips from the live objects, since a mesh dropped at
        // ingest leaves no trace on the object it came from.
        for obj in self.objects.values() {
            for mesh in &obj.geometry.meshes {
                let drawable = !mesh.positions.is_empty()
                    && (mesh.topology == MeshTopology::Points || !mesh.indices.is_empty());
                if drawable {
                    match mesh.topology {
                        MeshTopology::Lines => stats.skipped_lines += 1,
                        MeshTopology::Points => stats.skipped_points += 1,
                        MeshTopology::Triangles => {}
                    }
                } else {
                    stats.skipped_empty += 1;
                }
            }
        }

        // The mesh table and the placement list are built in one pass, with the
        // top-level boxes beside the placements in a parallel vector: the
        // hierarchy's leaves index the placement slice, so the two orders are
        // the same order and must not be able to drift apart.
        let mut pack_index: HashMap<PackKey, u32> = HashMap::new();
        let mut mesh_positions: Vec<&Arc<Vec<[f32; 3]>>> = Vec::new();
        let mut mesh_indices: Vec<&Arc<Vec<u32>>> = Vec::new();
        let mut mesh_normals: Vec<Option<&Arc<Vec<[f32; 3]>>>> = Vec::new();
        let mut mesh_uv0: Vec<Option<&Arc<Vec<[f32; 2]>>>> = Vec::new();
        let mut mesh_bvh: Vec<&Arc<Bvh>> = Vec::new();
        let mut placements: Vec<ArenaPlacement> = Vec::new();
        let mut boxes: Vec<AABB> = Vec::new();

        for obj in self.objects.values() {
            let mut flags = 0u32;
            if obj.visible {
                flags |= INSTANCE_VISIBLE;
            }
            if obj.cast_shadow {
                flags |= INSTANCE_CAST_SHADOW;
            }

            for mesh in &obj.meshes {
                let Some(entry) = self.hierarchies.get(&mesh.key) else {
                    continue;
                };
                let Some(cooked) = obj.geometry.meshes.get(mesh.cooked_index as usize) else {
                    continue;
                };

                let pack_key = PackKey {
                    positions: mesh.key.positions,
                    indices: mesh.key.indices,
                    normals: cooked.normals.as_ref().map(|a| Arc::as_ptr(a) as usize),
                    uv0: cooked.tex_coords.as_ref().map(|a| Arc::as_ptr(a) as usize),
                };
                let slot = *pack_index.entry(pack_key).or_insert_with(|| {
                    mesh_positions.push(&cooked.positions);
                    mesh_indices.push(&cooked.indices);
                    mesh_normals.push(cooked.normals.as_ref());
                    mesh_uv0.push(cooked.tex_coords.as_ref());
                    mesh_bvh.push(&entry.bvh);
                    stats.nodes = stats.nodes.saturating_add(entry.stats.node_count);
                    stats.triangles = stats.triangles.saturating_add(entry.stats.prim_count);
                    stats.degenerate_triangles = stats
                        .degenerate_triangles
                        .saturating_add(entry.stats.skipped_prims);
                    stats.depth_capped_leaves = stats
                        .depth_capped_leaves
                        .saturating_add(entry.stats.depth_capped_leaves);
                    u32::try_from(mesh_positions.len() - 1).unwrap_or(u32::MAX)
                });

                let material_base = obj.material_base.saturating_add(mesh.material_base);
                let identity = [InstanceXform::IDENTITY];
                let list: &[InstanceXform] =
                    mesh.placements.as_deref().map_or(&identity, Vec::as_slice);
                for placement in list {
                    // The placement is object-local and the object transform is
                    // where the object sits in the world, so the object
                    // transform applies last. Same composition the raster
                    // instance rows use; a scatter's copies all move when the
                    // object does.
                    let world = obj.transform * Matrix4::from(placement.0);
                    let Some(inv_world) = affine_inverse(&world) else {
                        stats.singular_placements += 1;
                        continue;
                    };
                    boxes.push(mesh.bounds.transformed(&world));
                    placements.push(ArenaPlacement {
                        mesh: slot,
                        world: world.into(),
                        inv_world: inv_world.into(),
                        material_base,
                        flags,
                    });
                }
            }
        }

        let arena_meshes: Vec<ArenaMesh<'_>> = (0..mesh_positions.len())
            .map(|i| ArenaMesh {
                bvh: mesh_bvh[i],
                positions: mesh_positions[i],
                indices: mesh_indices[i],
                normals: mesh_normals[i].map(|a| a.as_slice()),
                uv0: mesh_uv0[i].map(|a| a.as_slice()),
            })
            .collect();

        let tlas = Bvh::build_tlas(&boxes);
        self.arena = TraceArena::build(&tlas, &arena_meshes, &placements);

        stats.objects = u32::try_from(self.objects.len()).unwrap_or(u32::MAX);
        stats.meshes = u32::try_from(arena_meshes.len()).unwrap_or(u32::MAX);
        stats.instances = u32::try_from(placements.len()).unwrap_or(u32::MAX);
        stats.nodes = stats
            .nodes
            .saturating_add(u32::try_from(tlas.nodes().len()).unwrap_or(u32::MAX));
        self.stats = stats;
    }
}

/// Object-space bounds over a position buffer.
fn bounds_of(positions: &[[f32; 3]]) -> AABB {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    for p in positions {
        for axis in 0..3 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    if positions.is_empty() {
        return AABB {
            min: cgmath::Point3::new(0.0, 0.0, 0.0),
            max: cgmath::Point3::new(0.0, 0.0, 0.0),
        };
    }
    AABB {
        min: cgmath::Point3::new(min[0], min[1], min[2]),
        max: cgmath::Point3::new(max[0], max[1], max[2]),
    }
}

/// The inverse of a placement, or `None` when there is not a usable one.
///
/// A garbage inverse is worse than a missing instance. The kernel transforms
/// every ray into object space with this matrix; a non-finite one gives the
/// ray NaN components, and a NaN in the slab test is neither a hit nor a miss
/// in any defined way. A zero-scale placement also encloses no volume, so
/// dropping it removes nothing a correct image would have shown.
///
/// Two guards rather than one. `invert` rejects only a determinant that
/// compares equal to zero, so a near-singular matrix passes it and returns
/// components in the 1e30 range; checking the result catches that overflow
/// without an absolute epsilon, which would wrongly reject a millimetre-scale
/// object in a metre-scale scene. Checking the input catches a non-finite
/// transform arriving from the graph before it can poison the inverse.
fn affine_inverse(world: &Matrix4<f32>) -> Option<Matrix4<f32>> {
    if !finite(world) {
        return None;
    }
    let inv = world.invert()?;
    finite(&inv).then_some(inv)
}

fn finite(m: &Matrix4<f32>) -> bool {
    let cols: &[[f32; 4]; 4] = m.as_ref();
    cols.iter().all(|c| c.iter().all(|v| v.is_finite()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use solarxy_core::scene::CookedMesh;

    fn tri_positions() -> Vec<[f32; 3]> {
        vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
    }

    fn mesh(topology: MeshTopology) -> CookedMesh {
        let indices = match topology {
            MeshTopology::Points => Vec::new(),
            _ => vec![0, 1, 2],
        };
        CookedMesh {
            name: "m".into(),
            positions: Arc::new(tri_positions()),
            normals: None,
            tex_coords: None,
            indices: Arc::new(indices),
            material_index: None,
            topology,
            colors: None,
            instances: None,
        }
    }

    fn geometry(meshes: Vec<CookedMesh>) -> Arc<CookedGeometry> {
        Arc::new(CookedGeometry {
            bounds: bounds_of(&tri_positions()),
            meshes,
            materials: Vec::new(),
        })
    }

    fn upsert(id: u64, geometry: &Arc<CookedGeometry>) -> SceneDelta {
        SceneDelta {
            ops: vec![SceneOp::UpsertGeometry {
                id: SceneObjectId(id),
                geometry: Arc::clone(geometry),
            }],
        }
    }

    fn translation(x: f32) -> InstanceXform {
        let mut m = InstanceXform::IDENTITY;
        m.0[3] = [x, 0.0, 0.0, 1.0];
        m
    }

    fn arena_bytes(cache: &TraceSceneCache) -> Vec<u8> {
        let a = cache.arena();
        let mut out = Vec::new();
        out.extend_from_slice(bytemuck::cast_slice(a.nodes()));
        out.extend_from_slice(bytemuck::cast_slice(a.prim_indices()));
        out.extend_from_slice(bytemuck::cast_slice(a.vertex_pos()));
        out.extend_from_slice(bytemuck::cast_slice(a.vertex_attr()));
        out.extend_from_slice(bytemuck::cast_slice(a.instances()));
        out
    }

    #[test]
    fn a_repeated_upsert_of_the_same_geometry_rebuilds_nothing() {
        let g = geometry(vec![mesh(MeshTopology::Triangles)]);
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &g));
        assert!(cache.repack().is_some(), "the first apply is a change");
        let first = Arc::clone(cache.hierarchy(SceneObjectId(1), 0).expect("a hierarchy"));

        cache.apply(&upsert(1, &g));
        assert!(
            cache.repack().is_none(),
            "an identical upsert should leave the GPU nothing to do"
        );
        assert!(Arc::ptr_eq(
            &first,
            cache.hierarchy(SceneObjectId(1), 0).expect("a hierarchy")
        ));
        assert_eq!(cache.hierarchy_count(), 1);
    }

    #[test]
    fn a_changed_material_table_reuses_every_hierarchy() {
        let g = geometry(vec![mesh(MeshTopology::Triangles)]);
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &g));
        cache.repack();
        let first = Arc::clone(cache.hierarchy(SceneObjectId(1), 0).expect("a hierarchy"));

        // Same mesh buffers, different material table: not the same geometry by
        // the shared identity rule, so the object rebuilds, but nothing below
        // the top level has changed.
        let recoloured = Arc::new(CookedGeometry {
            meshes: g.meshes.clone(),
            materials: vec![Arc::new(solarxy_core::RawMaterialData::default())],
            bounds: g.bounds,
        });
        cache.apply(&upsert(1, &recoloured));
        assert!(cache.repack().is_some());
        assert!(Arc::ptr_eq(
            &first,
            cache.hierarchy(SceneObjectId(1), 0).expect("a hierarchy")
        ));
        assert_eq!(cache.hierarchy_count(), 1);
    }

    #[test]
    fn a_shared_prototype_builds_one_hierarchy_and_packs_its_vertices_once() {
        let g = geometry(vec![mesh(MeshTopology::Triangles)]);
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &g));
        cache.apply(&upsert(2, &g));
        cache.repack();

        assert_eq!(cache.hierarchy_count(), 1);
        assert_eq!(cache.arena().vertex_pos().len(), 3, "one copy of the mesh");
        assert_eq!(cache.arena().instances().len(), 2);
        let bases: Vec<u32> = cache
            .arena()
            .instances()
            .iter()
            .map(|i| i.vertex_base)
            .collect();
        assert_eq!(bases[0], bases[1], "both instances share the packed range");
    }

    #[test]
    fn meshes_that_share_geometry_but_not_normals_pack_separately() {
        let base = mesh(MeshTopology::Triangles);
        let mut with_normals = base.clone();
        with_normals.normals = Some(Arc::new(vec![[0.0, 1.0, 0.0]; 3]));
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![base, with_normals])));
        cache.repack();

        // One hierarchy, because that depends on positions and indices alone;
        // two packed ranges, because the vertex data differs.
        assert_eq!(cache.hierarchy_count(), 1);
        assert_eq!(cache.arena().vertex_pos().len(), 6);
        assert_eq!(cache.stats().meshes, 2);
    }

    #[test]
    fn a_removed_object_frees_only_the_hierarchies_nothing_else_names() {
        let g = geometry(vec![mesh(MeshTopology::Triangles)]);
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &g));
        cache.apply(&upsert(2, &g));
        cache.repack();
        assert_eq!(cache.hierarchy_count(), 1);

        cache.apply(&SceneDelta {
            ops: vec![SceneOp::Remove {
                id: SceneObjectId(1),
            }],
        });
        assert_eq!(cache.hierarchy_count(), 1, "the sibling still names it");

        cache.apply(&SceneDelta {
            ops: vec![SceneOp::Remove {
                id: SceneObjectId(2),
            }],
        });
        assert_eq!(cache.hierarchy_count(), 0);
    }

    #[test]
    fn a_replaced_geometry_drops_the_hierarchy_it_no_longer_names() {
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.repack();
        assert_eq!(cache.hierarchy_count(), 1);

        // A different allocation, so a different key.
        cache.apply(&upsert(1, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.repack();
        assert_eq!(cache.hierarchy_count(), 1, "the old one is swept");
    }

    #[test]
    fn a_singular_placement_is_dropped_and_counted() {
        let mut m = mesh(MeshTopology::Triangles);
        let mut flattened = InstanceXform::IDENTITY;
        flattened.0[1] = [0.0; 4]; // zero scale on Y: no inverse exists
        m.instances = Some(Arc::new(vec![InstanceXform::IDENTITY, flattened]));
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![m])));
        cache.repack();

        assert_eq!(cache.stats().singular_placements, 1);
        assert_eq!(cache.arena().instances().len(), 1);
        for instance in cache.arena().instances() {
            for col in instance.inv_world {
                assert!(
                    col.iter().all(|v| v.is_finite()),
                    "a packed inverse must be finite: {instance:?}"
                );
            }
        }
    }

    #[test]
    fn non_triangle_meshes_are_skipped_with_a_count() {
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(
            1,
            &geometry(vec![
                mesh(MeshTopology::Triangles),
                mesh(MeshTopology::Lines),
                mesh(MeshTopology::Points),
            ]),
        ));
        cache.repack();

        let stats = cache.stats();
        assert_eq!(stats.meshes, 1);
        assert_eq!(stats.skipped_lines, 1);
        assert_eq!(stats.skipped_points, 1);
        assert_eq!(stats.skipped_empty, 0);
    }

    #[test]
    fn an_empty_point_cloud_is_not_reported_as_a_skipped_point_cloud() {
        // Order matters in the skip test: an empty mesh draws nowhere, so
        // counting it as a topology skip would tell the user about geometry
        // they could not see in the raster viewport either.
        let mut empty = mesh(MeshTopology::Points);
        empty.positions = Arc::new(Vec::new());
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![empty])));
        cache.repack();

        let stats = cache.stats();
        assert_eq!(stats.skipped_points, 0);
        assert_eq!(stats.skipped_empty, 1);
    }

    #[test]
    fn a_transform_change_moves_every_placement() {
        let mut m = mesh(MeshTopology::Triangles);
        m.instances = Some(Arc::new(vec![translation(0.0), translation(10.0)]));
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![m])));
        cache.repack();

        cache.apply(&SceneDelta {
            ops: vec![SceneOp::SetTransform {
                id: SceneObjectId(1),
                transform: Matrix4::from_translation(cgmath::Vector3::new(100.0, 0.0, 0.0)).into(),
            }],
        });
        assert!(cache.repack().is_some());

        let xs: Vec<f32> = cache
            .arena()
            .instances()
            .iter()
            .map(|i| i.world[3][0])
            .collect();
        assert_eq!(
            xs,
            vec![100.0, 110.0],
            "the object transform must move every copy, not only the first"
        );
    }

    #[test]
    fn an_invisible_object_keeps_its_instance_with_the_flag_clear() {
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.apply(&SceneDelta {
            ops: vec![
                SceneOp::SetVisible {
                    id: SceneObjectId(1),
                    visible: false,
                },
                SceneOp::SetCastShadow {
                    id: SceneObjectId(1),
                    cast_shadow: false,
                },
            ],
        });
        cache.repack();

        assert_eq!(cache.arena().instances().len(), 1, "kept, not dropped");
        assert_eq!(cache.arena().instances()[0].flags, 0);
    }

    #[test]
    fn material_bases_follow_object_order() {
        let with = |count: usize, material_index: Option<usize>| {
            let mut m = mesh(MeshTopology::Triangles);
            m.material_index = material_index;
            Arc::new(CookedGeometry {
                bounds: bounds_of(&tri_positions()),
                meshes: vec![m],
                materials: (0..count)
                    .map(|_| Arc::new(solarxy_core::RawMaterialData::default()))
                    .collect(),
            })
        };
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &with(2, Some(1))));
        cache.apply(&upsert(2, &with(0, None)));
        cache.apply(&upsert(3, &with(3, Some(2))));
        cache.repack();

        // Blocks of `max(1)` per object, in id order: 0, 2, 3. Then the mesh's
        // own index within its object's block.
        let bases: Vec<u32> = cache
            .arena()
            .instances()
            .iter()
            .map(|i| i.material_base)
            .collect();
        assert_eq!(bases, vec![1, 2, 5]);
        assert_eq!(cache.material_slots(), 6);
    }

    #[test]
    fn the_pack_is_the_same_whatever_order_the_objects_arrived_in() {
        let a = geometry(vec![mesh(MeshTopology::Triangles)]);
        let b = geometry(vec![mesh(MeshTopology::Triangles)]);

        let mut forward = TraceSceneCache::new();
        forward.apply(&upsert(1, &a));
        forward.apply(&upsert(2, &b));
        forward.repack();

        let mut backward = TraceSceneCache::new();
        backward.apply(&upsert(2, &b));
        backward.apply(&upsert(1, &a));
        backward.repack();

        assert_eq!(arena_bytes(&forward), arena_bytes(&backward));
    }

    #[test]
    fn every_instance_base_lands_inside_its_buffer() {
        // The kernel's `arrayLength` guard reports capacity once the buffers
        // carry headroom, so it no longer bounds the live data. This is the
        // explicit replacement for what used to be an accidental guarantee.
        let mut m = mesh(MeshTopology::Triangles);
        m.instances = Some(Arc::new(vec![translation(0.0), translation(4.0)]));
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![m])));
        cache.apply(&upsert(2, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.repack();

        let arena = cache.arena();
        for instance in arena.instances() {
            assert!((instance.bvh_root as usize) < arena.nodes().len());
            assert!((instance.prim_base as usize) < arena.prim_indices().len());
            assert!((instance.index_base as usize) < arena.prim_indices().len());
            assert!((instance.vertex_base as usize) < arena.vertex_pos().len());
        }
        assert_eq!(arena.vertex_pos().len(), arena.vertex_attr().len());
    }

    #[test]
    fn validation_and_environment_ops_change_nothing() {
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.repack();
        let before = arena_bytes(&cache);

        cache.apply(&SceneDelta {
            ops: vec![
                SceneOp::SetValidation {
                    id: SceneObjectId(1),
                    validation: None,
                },
                SceneOp::SetEnvironment {
                    hdri: None,
                    rotation: 0.0,
                    intensity: 1.0,
                    background: solarxy_core::scene::BackgroundKind::Keep,
                },
            ],
        });

        assert!(cache.repack().is_none(), "neither op dirties the arena");
        assert_eq!(before, arena_bytes(&cache));
    }

    #[test]
    fn clear_drops_every_hierarchy_and_the_stored_lists() {
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.apply(&SceneDelta {
            ops: vec![SceneOp::SetLights { lights: Vec::new() }],
        });
        cache.repack();
        assert_eq!(cache.hierarchy_count(), 1);
        assert!(cache.lights().is_some());

        cache.apply(&SceneDelta {
            ops: vec![SceneOp::Clear],
        });
        cache.repack();
        assert_eq!(cache.hierarchy_count(), 0);
        assert!(cache.is_empty());
        assert!(cache.lights().is_none());
        assert_eq!(cache.arena().instances().len(), 0);
    }

    #[test]
    fn lights_and_cameras_are_stored_without_dirtying_the_arena() {
        let mut cache = TraceSceneCache::new();
        cache.apply(&upsert(1, &geometry(vec![mesh(MeshTopology::Triangles)])));
        cache.repack();

        cache.apply(&SceneDelta {
            ops: vec![
                SceneOp::SetLights { lights: Vec::new() },
                SceneOp::SetCameras {
                    cameras: Vec::new(),
                },
            ],
        });
        assert!(
            cache.repack().is_none(),
            "neither has a buffer in this arena yet"
        );
        assert!(cache.lights().is_some());
        assert!(cache.cameras().is_some());
    }
}
