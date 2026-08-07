// The debug kernel: camera rays through the two-level traversal, one channel of
// the hit written to a storage texture.
//
// Composed after `traverse.wgsl`, which owns the scene bindings and both walks.
//
// This is not the path tracer. It exists because a traversal that is wrong in a
// way that still runs fast produces a visibly wrong picture long before it
// produces a suspicious timing, and because a browser can only tell us the
// kernel compiles if there is a kernel to compile. The shading it does is a
// readout of what the traversal returned, nothing more.

// The camera uniform, declared as a PREFIX of the shipped `CameraUniform`.
// wgpu enforces size at the binding rather than shape, so a shader may declare
// the leading fields it reads and omit the rest. Only a struct declared whole
// belongs in the uniform-layout table; this one is deliberately not.
struct Camera {
    view_position: vec4f,
    view_proj: mat4x4f,
    view: mat4x4f,
    proj: mat4x4f,
    inv_proj: mat4x4f,
}

// 24 bytes. Grows as later stages add the sample index, seed, bounce budget,
// aperture and the rest; growing a uniform is invisible to a layout that sets
// `min_binding_size: None`, so the field set stays honest about what has a
// consumer today.
struct TraceParams {
    // Where this dispatch's tile sits in the image, in pixels.
    tile_offset: vec2u,
    // The tile's size, which is what the dispatch is sized against.
    tile_size: vec2u,
    // The whole image, so a tile knows the aspect it is a part of.
    resolution: vec2u,
}

@group(1) @binding(0) var debug_out: texture_storage_2d<rgba32float, write>;

@group(3) @binding(0) var<uniform> camera: Camera;
@group(3) @binding(1) var<uniform> params: TraceParams;

// Which readout the kernel writes. A pipeline-overridable constant rather than
// a uniform branch or a second shader source: the three variants specialize at
// pipeline creation, there is exactly one kernel source in the binary, and the
// dead channels fold away instead of costing a branch per pixel.
override DEBUG_CHANNEL: u32 = 0u;

const DEBUG_NORMAL: u32 = 0u;
const DEBUG_DEPTH: u32 = 1u;
const DEBUG_INSTANCE: u32 = 2u;

// The world-space ray through the centre of a pixel.
//
// Perspective only. Orthographic cameras generate parallel rays with the origin
// varying per pixel, which arrives with the camera work; the picking path
// already learned that lesson and this kernel must not un-learn it.
fn camera_ray(pixel: vec2u) -> Ray {
    let uv = (vec2f(pixel) + vec2f(0.5)) / vec2f(params.resolution);
    // wgpu clip space: y points down in framebuffer space and up in NDC.
    let ndc = vec2f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    // The near plane sits at z = 0 in this convention, so unprojecting z = 0
    // gives a point on it and the direction to that point is the ray.
    let near_view = camera.inv_proj * vec4f(ndc, 0.0, 1.0);
    let dir_view = normalize(near_view.xyz / near_view.w);
    // A look-at view matrix is a rotation and a translation, so the inverse of
    // its rotation is its transpose and no inversion happens in the shader.
    let rot = transpose(mat3x3f(
        camera.view[0].xyz,
        camera.view[1].xyz,
        camera.view[2].xyz,
    ));
    return Ray(camera.view_position.xyz, normalize(rot * dir_view));
}

// An object-space normal in world space.
//
// The inverse transpose, not the transform: a non-uniformly scaled instance
// shears its normals otherwise, and non-uniform scale is exactly what an
// instanced scene is full of. `inv_world` is already the inverse, so its
// transpose is one `transpose` away.
fn world_normal(inst: Instance, n: vec3f) -> vec3f {
    let m = transpose(mat3x3f(
        inst.inv_world[0].xyz,
        inst.inv_world[1].xyz,
        inst.inv_world[2].xyz,
    ));
    return normalize(m * n);
}

// A stable colour per instance, so a two-instance scene reads as two colours
// rather than as one surface with a seam.
fn instance_colour(index: u32) -> vec3f {
    var h = index * 2654435761u;
    h ^= h >> 15u;
    h *= 2246822519u;
    h ^= h >> 13u;
    return vec3f(
        f32((h >> 0u) & 255u),
        f32((h >> 8u) & 255u),
        f32((h >> 16u) & 255u),
    ) / 255.0;
}

@compute @workgroup_size(8, 8, 1)
fn trace_debug(@builtin(global_invocation_id) gid: vec3u) {
    // Two bounds checks rather than one: the dispatch is rounded up to whole
    // workgroups, and the tile can be a partial one at the image edge.
    if gid.x >= params.tile_size.x || gid.y >= params.tile_size.y {
        return;
    }
    let pixel = params.tile_offset + gid.xy;
    if pixel.x >= params.resolution.x || pixel.y >= params.resolution.y {
        return;
    }

    let ray = camera_ray(pixel);
    let hit = trace_closest(ray.origin, ray.direction, 1e30);

    // Alpha carries whether anything was hit, so a reader can tell a miss from
    // a black surface without guessing.
    var out = vec4f(0.0, 0.0, 0.0, 0.0);
    if hit.hit {
        let inst = instances[hit.instance];
        var rgb = vec3f(0.0);
        if DEBUG_CHANNEL == DEBUG_NORMAL {
            let n = world_normal(inst, shading_normal(hit));
            // Face the viewer, so a closed mesh reads as a surface rather than
            // as a pair of unrelated hemispheres.
            let facing = select(-n, n, dot(n, ray.direction) < 0.0);
            rgb = facing * 0.5 + vec3f(0.5);
        } else if DEBUG_CHANNEL == DEBUG_DEPTH {
            // Raw world-space distance. The target is a full float, so there is
            // nothing to be gained by normalizing it into a range here and a
            // scale to get wrong.
            rgb = vec3f(hit.t, hit.t, hit.t);
        } else {
            rgb = instance_colour(hit.instance);
        }
        out = vec4f(rgb, 1.0);
    }

    textureStore(debug_out, vec2i(pixel), out);
}
