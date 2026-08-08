// Two-level ray traversal: the scene bindings, the intersection primitives,
// and the closest-hit and any-hit walks.
//
// This fragment declares no entry point. It is prepended to whichever kernel
// needs it, because WGSL has no include mechanism and the alternative is
// copying a traversal that has to stay identical to a CPU twin.
//
// # The twin
//
// Everything here mirrors `solarxy_bvh::traverse` term for term: the same node
// layout, the same epsilon, the same rejection order, the same child ordering,
// and the same fixed stack. That crate's parity corpus pins it to
// `solarxy_core::raycast`, and the renderer's corpus pins this to it, so a
// disagreement anywhere along the chain names which of the three is wrong.
// Changing one side without the other is how a path tracer renders a plausible
// image of the wrong geometry.
//
// # Uniformity discipline, an invariant rather than a style
//
// The browser's WGSL uniformity analysis rejects derivative-dependent work
// under non-uniform control flow at pipeline creation, with a message that is
// easy to misread, and a traversal is exactly the branchy shape that trips it.
// So: no `textureSample` and no other implicitly-derived sampling anywhere in
// this directory, level-sampled reads only, and no barrier inside a branch.
// `pathtrace_shader_source.rs` enforces it rather than trusting the comment.

// The node's fourth field is named `packed` here and `meta` in Rust. `meta` is
// a WGSL reserved keyword and the shader will not parse with it. The layout is
// by offset, so the two names describe the same four bytes; the uniform-layout
// test is what keeps them the same size.
//
// 32 bytes: `min` and `max` are vec3f, which WGSL aligns to 16 in the storage
// address space, and the u32 after each one fills the gap exactly. That is what
// makes the Rust `[f32; 3]` layout and this one the same bytes, and what makes
// `to_gpu_arrays` a cast rather than a transcode.
struct BvhNode {
    min: vec3f,
    // Leaf: first primitive in the permutation. Interior: right child index.
    // Both are relative to this hierarchy's own first node, never absolute.
    offset: u32,
    max: vec3f,
    // High bit marks a leaf; the low 31 are the primitive count on a leaf and
    // the split axis on an interior node.
    packed: u32,
}

// 160 bytes. The four bases are what let several instances share one
// hierarchy: they differ only in transform and material.
struct Instance {
    world: mat4x4f,
    inv_world: mat4x4f,
    bvh_root: u32,
    prim_base: u32,
    index_base: u32,
    vertex_base: u32,
    material_base: u32,
    flags: u32,
}

// 48 bytes. A zero normal means the source mesh carried none.
struct VertexAttr {
    normal: vec4f,
    tangent: vec4f,
    uv: vec4f,
}

// 256 bytes, and the offsets are not negotiable: every vector below sits on a
// 16-byte boundary because WGSL aligns vec3f and vec4f to 16 in the storage
// address space while Rust aligns [f32; 3] to 4. The Rust twin is
// `pathtrace::material::TracedMaterial`, which states each offset and is pinned
// to this by `tests/uniform_layout.rs` for size and by
// `tests/pathtrace_material.rs` for field order, since a transposition of two
// same-sized blocks changes no size.
//
// Five texture slots, not the seventeen the authoring model carries: the other
// twelve modulate scalars that are all present here, and the atlas packs five.
// See the Rust module documentation.
struct TracedMaterial {
    base_color: vec4f,

    emissive: vec3f,
    emissive_strength: f32,

    attenuation_color: vec3f,
    // Zero means no attenuation, standing in for an infinite default.
    attenuation_distance: f32,

    sheen_color: vec3f,
    sheen_roughness: f32,

    specular_color: vec3f,
    specular_intensity: f32,

    metallic: f32,
    roughness: f32,
    ior: f32,
    transmission: f32,

    // Zero thickness is thin-walled: no interior to attenuate through.
    thickness: f32,
    clearcoat: f32,
    clearcoat_roughness: f32,
    anisotropy: f32,

    anisotropy_rotation: f32,
    iridescence: f32,
    iridescence_ior: f32,
    iridescence_thickness_min: f32,

    iridescence_thickness_max: f32,
    occlusion_strength: f32,
    alpha_cutoff: f32,
    // Bits 0 to 1 alpha mode, bits 2 to 5 shading model, the rest reserved.
    flags: u32,

    // Per slot, in role order: base colour, normal, metallic-roughness,
    // occlusion, emissive.
    tex_rect: array<vec4f, 5>,
    // A descriptor of TEX_UNUSED_BIT means the slot carries no texture. Zero is
    // a legal descriptor naming layer zero, never an absent one.
    tex_desc: array<u32, 5>,
}

// The scene group. Binding numbers are fixed by the storage-buffer budget and
// are deliberately not contiguous: 5 belongs to the lights and 7 is the escape
// hatch a ninth logical array would otherwise force. Core WebGPU grants eight
// per stage and the design has to fit inside that permanently, so the numbering
// is not something a later stage renegotiates.
@group(0) @binding(0) var<storage, read> bvh_nodes: array<BvhNode>;
@group(0) @binding(1) var<storage, read> prim_indices: array<u32>;
@group(0) @binding(2) var<storage, read> vertex_pos: array<vec4f>;
@group(0) @binding(3) var<storage, read> vertex_attr: array<VertexAttr>;
@group(0) @binding(4) var<storage, read> materials: array<TracedMaterial>;
@group(0) @binding(6) var<storage, read> instances: array<Instance>;

// Alpha-mode and shading-model fields of `TracedMaterial.flags`.
const MAT_ALPHA_MODE_MASK: u32 = 0x3u;
const MAT_SHADING_MODEL_SHIFT: u32 = 2u;
const MAT_SHADING_MODEL_MASK: u32 = 0xFu;

const LEAF_FLAG: u32 = 0x80000000u;

// WGSL has no dynamic allocation, so the stack is fixed. Sixty-four against a
// builder that caps depth at 32, and a descent pushes at most one entry per
// level, so it runs at half capacity in the worst case the builder can produce.
// One stack per level rather than one shared: each walk then honours the bound
// on its own, and the twin stays readable.
const STACK_SIZE: u32 = 64u;

// Matches `solarxy_bvh::traverse`'s epsilon exactly. The two implementations
// must agree on which grazing hits count, not merely on the obvious ones.
const EPS: f32 = 1e-7;

const INSTANCE_VISIBLE: u32 = 1u;
const INSTANCE_CAST_SHADOW: u32 = 2u;

struct TriHit {
    hit: bool,
    t: f32,
    // Triangle within the instance's geometry, in the caller's numbering.
    prim: u32,
    // Barycentric `[w, u, v]`, so the point is `v0 * w + v1 * u + v2 * v`.
    // Full precision, deliberately: half-precision packing costs more than the
    // parity corpus's barycentric tolerance allows.
    bary: vec3f,
}

// A ray: an origin and a direction, world-space or object-space depending on
// who made it. The direction is not required to be unit length; see
// `to_object_space` for why that matters.
struct Ray {
    origin: vec3f,
    direction: vec3f,
}

struct Hit {
    hit: bool,
    // Distance along the WORLD-space ray, at both levels.
    t: f32,
    instance: u32,
    prim: u32,
    bary: vec3f,
    // Object-space geometric normal of the triangle, unnormalized.
    geo_normal: vec3f,
}

// Slab test against an axis-aligned box over `[0, t_max]`.
//
// The vectorized form of the CPU's per-axis loop, and equivalent to it
// including for NaN: WGSL's `min` and `max` return the non-NaN operand, which
// is exactly the "ignore this axis" answer the robust formulation wants when
// the origin lies on a slab plane of a zero-extent axis and
// `(bound - origin) * inf` is NaN. `f32::min` and `f32::max` are specified the
// same way, which is what lets the two be written differently and still agree.
fn slab_hit(origin: vec3f, inv_dir: vec3f, lo: vec3f, hi: vec3f, t_max: f32) -> bool {
    let t0 = (lo - origin) * inv_dir;
    let t1 = (hi - origin) * inv_dir;
    let near = min(t0, t1);
    let far = max(t0, t1);
    let t_near = max(max(near.x, near.y), max(near.z, 0.0));
    let t_far = min(min(far.x, far.y), min(far.z, t_max));
    return t_near <= t_far;
}

fn tri_vertex(inst: Instance, tri: u32, corner: u32) -> vec3f {
    let local = prim_indices[inst.index_base + tri * 3u + corner];
    return vertex_pos[inst.vertex_base + local].xyz;
}

// Moller-Trumbore, field for field the same arithmetic and the same rejection
// order as the CPU twin. `direction` is object-space and deliberately not
// renormalized, so `t` comes out in world units.
fn intersect_tri(inst: Instance, origin: vec3f, direction: vec3f, tri: u32) -> TriHit {
    var result: TriHit;
    result.hit = false;
    result.t = 0.0;
    result.prim = tri;
    result.bary = vec3f(0.0);

    let v0 = tri_vertex(inst, tri, 0u);
    let v1 = tri_vertex(inst, tri, 1u);
    let v2 = tri_vertex(inst, tri, 2u);

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = cross(direction, edge2);
    let a = dot(edge1, h);
    if abs(a) < EPS {
        return result;
    }
    let f = 1.0 / a;
    let s = origin - v0;
    let u = f * dot(s, h);
    if u < 0.0 || u > 1.0 {
        return result;
    }
    let q = cross(s, edge1);
    let v = f * dot(direction, q);
    if v < 0.0 || u + v > 1.0 {
        return result;
    }
    let t = f * dot(edge2, q);
    if t > EPS {
        result.hit = true;
        result.t = t;
        result.bary = vec3f(1.0 - u - v, u, v);
    }
    return result;
}

// The geometric normal of a triangle, object-space and unnormalized. Read
// separately from the intersection so the hot loop does not carry it.
fn tri_geo_normal(inst: Instance, tri: u32) -> vec3f {
    let v0 = tri_vertex(inst, tri, 0u);
    let v1 = tri_vertex(inst, tri, 1u);
    let v2 = tri_vertex(inst, tri, 2u);
    return cross(v1 - v0, v2 - v0);
}

// The world-space ray in an instance's object space.
//
// The direction is transformed but NOT renormalized. That is what keeps `t` in
// world units at both levels, so one `t_max` compares correctly across
// instances of different scale and the top-level box test keeps tightening
// against a hit found inside a child. Renormalizing here is the classic way to
// get a two-level traversal that looks right until two instances overlap.
fn to_object_space(inst: Instance, origin: vec3f, direction: vec3f) -> Ray {
    let o = (inst.inv_world * vec4f(origin, 1.0)).xyz;
    let d = (inst.inv_world * vec4f(direction, 0.0)).xyz;
    return Ray(o, d);
}

// Nearest triangle of one instance within `t_max`.
//
// Node indices are local to the instance's own hierarchy and `bvh_root` is
// added at the fetch, which is what lets a hierarchy be packed exactly as its
// builder emitted it and cached across repacks.
fn trace_blas_closest(inst: Instance, origin: vec3f, direction: vec3f, t_max: f32) -> TriHit {
    var result: TriHit;
    result.hit = false;
    result.t = t_max;
    result.prim = 0u;
    result.bary = vec3f(0.0);

    let inv_dir = vec3f(1.0) / direction;
    var stack: array<u32, STACK_SIZE>;
    var sp = 0u;
    var node_idx = 0u;

    loop {
        let node = bvh_nodes[inst.bvh_root + node_idx];
        if slab_hit(origin, inv_dir, node.min, node.max, result.t) {
            if (node.packed & LEAF_FLAG) != 0u {
                let first = inst.prim_base + node.offset;
                let count = node.packed & ~LEAF_FLAG;
                for (var k = 0u; k < count; k = k + 1u) {
                    let tri = prim_indices[first + k];
                    let candidate = intersect_tri(inst, origin, direction, tri);
                    if candidate.hit && candidate.t < result.t {
                        result.hit = true;
                        result.t = candidate.t;
                        result.prim = tri;
                        result.bary = candidate.bary;
                    }
                }
            } else {
                let left = node_idx + 1u;
                let right = node.offset;
                let axis = node.packed & ~LEAF_FLAG;
                // Descend the side the ray reaches first, so the running best
                // tightens before the far subtree is box-tested.
                var near = left;
                var far = right;
                if direction[axis] < 0.0 {
                    near = right;
                    far = left;
                }
                if sp < STACK_SIZE {
                    stack[sp] = far;
                    sp = sp + 1u;
                }
                node_idx = near;
                continue;
            }
        }
        if sp == 0u {
            break;
        }
        sp = sp - 1u;
        node_idx = stack[sp];
    }

    // A miss leaves `t` at the caller's budget, which would read as a hit at
    // that distance. Zero it, exactly as the top-level walk does.
    if !result.hit {
        result.t = 0.0;
    }
    return result;
}

// Whether anything in one instance blocks the ray within `t_max`.
//
// No child ordering and an early return, which is most of why an any-hit walk
// is worth having as its own function rather than a flag.
fn trace_blas_any(inst: Instance, origin: vec3f, direction: vec3f, t_max: f32) -> bool {
    let inv_dir = vec3f(1.0) / direction;
    var stack: array<u32, STACK_SIZE>;
    var sp = 0u;
    var node_idx = 0u;

    loop {
        let node = bvh_nodes[inst.bvh_root + node_idx];
        if slab_hit(origin, inv_dir, node.min, node.max, t_max) {
            if (node.packed & LEAF_FLAG) != 0u {
                let first = inst.prim_base + node.offset;
                let count = node.packed & ~LEAF_FLAG;
                for (var k = 0u; k < count; k = k + 1u) {
                    let candidate = intersect_tri(inst, origin, direction, prim_indices[first + k]);
                    if candidate.hit && candidate.t < t_max {
                        return true;
                    }
                }
            } else {
                if sp < STACK_SIZE {
                    stack[sp] = node.offset;
                    sp = sp + 1u;
                }
                node_idx = node_idx + 1u;
                continue;
            }
        }
        if sp == 0u {
            break;
        }
        sp = sp - 1u;
        node_idx = stack[sp];
    }

    return false;
}

// Nearest hit in the scene within `t_max`.
//
// The top-level hierarchy sits first in the node buffer, so this walk starts at
// node zero with no offset and its leaves name instances rather than triangles.
fn trace_closest(origin: vec3f, direction: vec3f, t_max: f32) -> Hit {
    var result: Hit;
    result.hit = false;
    result.t = t_max;
    result.instance = 0u;
    result.prim = 0u;
    result.bary = vec3f(0.0);
    result.geo_normal = vec3f(0.0);

    let inv_dir = vec3f(1.0) / direction;
    let instance_count = arrayLength(&instances);
    var stack: array<u32, STACK_SIZE>;
    var sp = 0u;
    var node_idx = 0u;

    loop {
        let node = bvh_nodes[node_idx];
        if slab_hit(origin, inv_dir, node.min, node.max, result.t) {
            if (node.packed & LEAF_FLAG) != 0u {
                let first = node.offset;
                let count = node.packed & ~LEAF_FLAG;
                for (var k = 0u; k < count; k = k + 1u) {
                    let index = prim_indices[first + k];
                    if index >= instance_count {
                        continue;
                    }
                    let inst = instances[index];
                    if (inst.flags & INSTANCE_VISIBLE) == 0u {
                        continue;
                    }
                    let ray = to_object_space(inst, origin, direction);
                    let sub = trace_blas_closest(inst, ray.origin, ray.direction, result.t);
                    if sub.hit {
                        result.hit = true;
                        result.t = sub.t;
                        result.instance = index;
                        result.prim = sub.prim;
                        result.bary = sub.bary;
                        result.geo_normal = tri_geo_normal(inst, sub.prim);
                    }
                }
            } else {
                let left = node_idx + 1u;
                let right = node.offset;
                let axis = node.packed & ~LEAF_FLAG;
                var near = left;
                var far = right;
                if direction[axis] < 0.0 {
                    near = right;
                    far = left;
                }
                if sp < STACK_SIZE {
                    stack[sp] = far;
                    sp = sp + 1u;
                }
                node_idx = near;
                continue;
            }
        }
        if sp == 0u {
            break;
        }
        sp = sp - 1u;
        node_idx = stack[sp];
    }

    if !result.hit {
        result.t = 0.0;
    }
    return result;
}

// Whether anything in the scene blocks the ray within `t_max`.
//
// `shadow_only` restricts the test to instances that cast, which is the whole
// reason the flag exists: a surface can be visible and not occlude.
fn trace_any(origin: vec3f, direction: vec3f, t_max: f32, shadow_only: bool) -> bool {
    let inv_dir = vec3f(1.0) / direction;
    let instance_count = arrayLength(&instances);
    var stack: array<u32, STACK_SIZE>;
    var sp = 0u;
    var node_idx = 0u;

    loop {
        let node = bvh_nodes[node_idx];
        if slab_hit(origin, inv_dir, node.min, node.max, t_max) {
            if (node.packed & LEAF_FLAG) != 0u {
                let first = node.offset;
                let count = node.packed & ~LEAF_FLAG;
                for (var k = 0u; k < count; k = k + 1u) {
                    let index = prim_indices[first + k];
                    if index >= instance_count {
                        continue;
                    }
                    let inst = instances[index];
                    if (inst.flags & INSTANCE_VISIBLE) == 0u {
                        continue;
                    }
                    if shadow_only && (inst.flags & INSTANCE_CAST_SHADOW) == 0u {
                        continue;
                    }
                    let ray = to_object_space(inst, origin, direction);
                    if trace_blas_any(inst, ray.origin, ray.direction, t_max) {
                        return true;
                    }
                }
            } else {
                if sp < STACK_SIZE {
                    stack[sp] = node.offset;
                    sp = sp + 1u;
                }
                node_idx = node_idx + 1u;
                continue;
            }
        }
        if sp == 0u {
            break;
        }
        sp = sp - 1u;
        node_idx = stack[sp];
    }

    return false;
}

// An object-space normal in world space.
//
// The inverse transpose, not the transform: a non-uniformly scaled instance
// shears its normals otherwise, and non-uniform scale is exactly what an
// instanced scene is full of. `inv_world` is already the inverse, so its
// transpose is one `transpose` away.
//
// Lives here rather than in a kernel because it belongs with the vertex-fetch
// helpers below it: every caller of `shading_normal` needs it, and the tangent
// beside it needs the other transform.
fn world_normal(inst: Instance, n: vec3f) -> vec3f {
    let m = transpose(mat3x3f(
        inst.inv_world[0].xyz,
        inst.inv_world[1].xyz,
        inst.inv_world[2].xyz,
    ));
    return normalize(m * n);
}

// An object-space tangent in world space.
//
// A tangent lies IN the surface, so it transforms by the world matrix, not by the
// inverse transpose a normal needs. Getting the two the same way round is the
// difference between a normal map that shades correctly under non-uniform scale
// and one that shears.
fn world_tangent(inst: Instance, t: vec3f) -> vec3f {
    let m = mat3x3f(
        inst.world[0].xyz,
        inst.world[1].xyz,
        inst.world[2].xyz,
    );
    return normalize(m * t);
}

// The shading normal at a hit, object space, falling back to the geometric
// normal when the source mesh carried none.
fn shading_normal(hit: Hit) -> vec3f {
    let inst = instances[hit.instance];
    let base = inst.index_base + hit.prim * 3u;
    let i0 = inst.vertex_base + prim_indices[base];
    let i1 = inst.vertex_base + prim_indices[base + 1u];
    let i2 = inst.vertex_base + prim_indices[base + 2u];
    let n = vertex_attr[i0].normal.xyz * hit.bary.x
        + vertex_attr[i1].normal.xyz * hit.bary.y
        + vertex_attr[i2].normal.xyz * hit.bary.z;
    if dot(n, n) < EPS {
        return hit.geo_normal;
    }
    return n;
}

// The interpolated texture coordinates at a hit: uv0 in `xy`, uv1 in `zw`, which
// is the shape `material_sample` reads. The arena writes only uv0 today, so the
// upper pair is zero.
fn shading_uv(hit: Hit) -> vec4f {
    let inst = instances[hit.instance];
    let base = inst.index_base + hit.prim * 3u;
    let i0 = inst.vertex_base + prim_indices[base];
    let i1 = inst.vertex_base + prim_indices[base + 1u];
    let i2 = inst.vertex_base + prim_indices[base + 2u];
    return vertex_attr[i0].uv * hit.bary.x
        + vertex_attr[i1].uv * hit.bary.y
        + vertex_attr[i2].uv * hit.bary.z;
}

// Where to restart a ray so it does not immediately hit the surface it left.
//
// The offset scales with the magnitude of the point rather than being a constant,
// because floating-point spacing does: a fixed epsilon that works at the origin is
// smaller than one representable step a thousand units out, and the ray then
// re-intersects the triangle it started on. `offset` is the direction to step in,
// normally the shading normal signed towards the side the ray is leaving on.
fn step_ray_origin(origin: vec3f, direction: vec3f, offset: vec3f, dist: f32) -> vec3f {
    let point = origin + direction * dist;
    let abs_point = abs(point);
    let scale = max(abs_point.x, max(abs_point.y, abs_point.z)) + 1.0;
    return point + offset * scale * 1e-4;
}

// A tangent at a hit, object space, derived from the triangle rather than
// interpolated from vertices. `w` carries handedness, and a `w` of zero means
// there is no usable tangent here.
//
// `VertexAttr.tangent` is written zero by the arena and this does not read it: a
// per-triangle basis needs no extra vertex attribute and no import-time pass, and
// it is available for every mesh that carries texture coordinates at all.
//
// The cost is that it is constant across a triangle and discontinuous at an edge
// wherever the texture parameterization changes direction. For a normal map that
// is the ordinary trade-off. For an anisotropic highlight it is visible as
// faceting on a surface whose normal is smooth, which is why the pack-time
// smoothed alternative is written down as the fix rather than forgotten: it wants
// a per-vertex accumulation the arena does not do yet.
//
// Two degeneracies return `w == 0` rather than a direction that happens to be
// finite: a mesh with no texture coordinates at all, where every uv is zero, and
// a triangle whose uv image has no area, which a seam or a collapsed island
// produces legitimately.
fn shading_tangent(hit: Hit) -> vec4f {
    let inst = instances[hit.instance];
    let base = inst.index_base + hit.prim * 3u;
    let i0 = inst.vertex_base + prim_indices[base];
    let i1 = inst.vertex_base + prim_indices[base + 1u];
    let i2 = inst.vertex_base + prim_indices[base + 2u];

    let p0 = vertex_pos[i0].xyz;
    let p1 = vertex_pos[i1].xyz;
    let p2 = vertex_pos[i2].xyz;

    let t0 = vertex_attr[i0].uv.xy;
    let t1 = vertex_attr[i1].uv.xy;
    let t2 = vertex_attr[i2].uv.xy;

    let e1 = p1 - p0;
    let e2 = p2 - p0;
    let d1 = t1 - t0;
    let d2 = t2 - t0;

    let det = d1.x * d2.y - d2.x * d1.y;
    if abs(det) < EPS {
        return vec4f(0.0);
    }
    let inv_det = 1.0 / det;

    let tangent = (e1 * d2.y - e2 * d1.y) * inv_det;
    if dot(tangent, tangent) < EPS {
        return vec4f(0.0);
    }
    let bitangent = (e2 * d1.x - e1 * d2.x) * inv_det;

    // Handedness from the sign of the determinant of the three, which is what a
    // mirrored uv island flips. The geometric normal is the reference rather than
    // the shading normal, because a shading normal can be bent far enough by
    // interpolation to invert the sign on a silhouette triangle.
    let handedness = select(-1.0, 1.0, dot(cross(hit.geo_normal, tangent), bitangent) >= 0.0);
    return vec4f(normalize(tangent), handedness);
}
