// The depth pass: one primary ray per pixel, and how far away what it found is.
//
// Composed over the traversal, the sampler and the camera. Declares an entry
// point and one binding.
//
// # Why this is a pass of its own rather than a fourth accumulator channel
//
// Because depth must not be averaged. A pixel on a silhouette sees one surface
// in some samples and another in the rest, and the mean of two distances
// describes neither: it places a surface in the gap between them, where there
// is nothing. Everything else the accumulator holds is a quantity whose mean is
// the answer; this is not one. So it draws a single ray through the middle of
// the pixel, with no jitter and no aperture, which is what a compositing
// package expects a depth pass to be.
//
// It also sidesteps a ceiling, and that is a consequence rather than the
// reason: core WebGPU grants four storage textures per compute stage and the
// accumulator's two ping-ponged pairs spend all four, so there was no room for
// a depth lane even if averaging one had been sound.
//
// # The group it takes
//
// Its own, group 1, which is the accumulator's number in every other kernel and
// is free here because this kernel binds no accumulator. The traversal keeps
// group 0 and the camera keeps group 3, both already declared by the fragments
// beneath this one, so nothing reserved moves. The precedent is the traversal
// parity kernel, and the reason is the one its comment gives: a pass that is
// not the shipped path kernel should not constrain the shipped path kernel's
// shape.
@group(1) @binding(0) var depth_out: texture_storage_2d<r32float, write>;

// What a ray that found nothing writes.
//
// Large and finite rather than infinite. A compositor divides by depth, tests
// against it and interpolates it, and an infinity turns every one of those into
// a value that is not a number, silently, several steps later. This is the same
// magnitude the traversal uses as its ray budget, so a miss reads as "further
// than anything could have been" rather than as a special case.
const DEPTH_MISS: f32 = 1e30;

@compute @workgroup_size(8, 8, 1)
fn depth_main(@builtin(global_invocation_id) gid: vec3u) {
    // Two bounds checks, for the two reasons the path kernel states: the
    // dispatch is rounded up to whole workgroups, and a tile at the image edge
    // is a partial one.
    if gid.x >= params.tile_size.x || gid.y >= params.tile_size.y {
        return;
    }
    // The pixel of the **whole image**, which is what the camera ray is built
    // through, against the tile-local coordinate it is **stored** at. An
    // untiled render has a zero offset and the two coincide.
    let pixel = params.tile_offset + gid.xy;
    if pixel.x >= params.resolution.x || pixel.y >= params.resolution.y {
        return;
    }

    // The middle of the pixel, and a lens sample that is ignored: the host
    // writes an aperture radius of zero into this dispatch's parameters, so
    // `camera_ray` returns before it reads the pair.
    let ray = camera_ray(pixel, vec2f(0.5), vec2f(0.0));
    let hit = trace_closest(ray.origin, ray.direction, DEPTH_MISS);

    // How far along the camera's axis the surface is **from the camera**, which
    // is what a compositing package means by depth.
    //
    // Not the ray's own length. The two differ by the cosine between the ray
    // and the axis, which is one at the middle of frame and falls away towards
    // the corners, so reporting the length would give a downstream defocus a
    // focal surface curved like a sphere about the eye rather than a plane
    // square to the lens.
    //
    // Written as the axial component of the vector from the eye to the surface,
    // rather than as the hit distance times that cosine, because the ray does
    // not start at the eye: `camera_ray` unprojects the near plane, which is
    // what makes it correct for an orthographic camera. Scaling the hit
    // distance alone would report every surface a near plane closer than it is.
    // This one expression is both corrections and needs no uniform either of
    // them would have needed.
    var distance = DEPTH_MISS;
    if hit.hit {
        let surface = ray.origin + ray.direction * hit.t;
        distance = min(
            dot(surface - camera.view_position.xyz, camera_forward()),
            DEPTH_MISS,
        );
    }
    textureStore(depth_out, vec2i(gid.xy), vec4f(distance, 0.0, 0.0, 0.0));
}
