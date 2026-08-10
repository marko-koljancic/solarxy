// The debug kernel: camera rays through the two-level traversal, one channel of
// the hit written to a storage texture.
//
// Composed after `traverse.wgsl`, which owns the scene bindings and both walks,
// and after `camera.wgsl`, which owns the per-dispatch uniforms and the ray.
//
// This is not the path tracer. It exists because a traversal that is wrong in a
// way that still runs fast produces a visibly wrong picture long before it
// produces a suspicious timing, and because a browser can only tell us the
// kernel compiles if there is a kernel to compile. The shading it does is a
// readout of what the traversal returned, nothing more.

@group(1) @binding(0) var debug_out: texture_storage_2d<rgba32float, write>;

// Which readout the kernel writes. A pipeline-overridable constant rather than
// a uniform branch or a second shader source: the three variants specialize at
// pipeline creation, there is exactly one kernel source in the binary, and the
// dead channels fold away instead of costing a branch per pixel.
override DEBUG_CHANNEL: u32 = 0u;

const DEBUG_NORMAL: u32 = 0u;
const DEBUG_DEPTH: u32 = 1u;
const DEBUG_INSTANCE: u32 = 2u;

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

    // The pixel centre: a readout of what the traversal returned wants no
    // filtering, and a jittered debug channel would shimmer between frames for
    // no diagnostic gain.
    let ray = camera_ray(pixel, vec2f(0.5), vec2f(0.5));
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
