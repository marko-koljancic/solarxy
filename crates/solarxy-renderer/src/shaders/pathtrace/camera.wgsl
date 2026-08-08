// The camera fragment: the per-dispatch uniforms and the ray through a pixel.
// Declares no entry point.
//
// This lived inside the debug kernel until a second kernel needed it. Composing
// the two together would have worked and would have dragged the debug channel
// constants and a second entry point into every kernel that wanted a camera
// ray, so the shared half moved out instead. Nothing about it changed in the
// move except `TraceParams` growing the fields the bounce loop reads.
//
// Composed over the traversal, whose `Ray` this returns.

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

// 40 bytes, and declared whole, so it is in the uniform-layout table.
//
// No padding field: every member aligns to eight or four, so the struct aligns
// to eight and forty is already a multiple of it. Adding a `vec3f` or a `vec4f`
// here would raise the alignment to sixteen and need a pad, which is why the
// harness environment is a separate uniform rather than four more floats on the
// end: it is a stand-in for the environment sampling a later stage brings, and a
// field documented here would have to be deleted rather than replaced.
struct TraceParams {
    // Where this dispatch's tile sits in the image, in pixels.
    tile_offset: vec2u,
    // The tile's size, which is what the dispatch is sized against.
    tile_size: vec2u,
    // The whole image, so a tile knows the aspect it is a part of.
    resolution: vec2u,
    // How many scattering events a path may have. Counted for every scatter,
    // transmissive or not.
    bounces: u32,
    // How many of those may additionally be transmissive.
    //
    // A separate budget rather than the reference's trick of decrementing the
    // loop counter on a transmissive hit, which spends a bounce and then hands
    // it back so that glass does not eat the whole path. That works and it makes
    // the bounce count mean two different things depending on what was hit.
    // Here the accounting is explicit: `bounces` is the ceiling on scattering
    // events and this is the ceiling on the transmissive subset.
    transmissive_bounces: u32,
    // How many samples this dispatch integrates per pixel, and the count the
    // stratified sampler divides its domain into. Zero or one turns
    // stratification off.
    samples: u32,
    // Decorrelates one dispatch from the next. A fixed value is what makes a
    // render reproducible.
    seed: u32,
}

@group(3) @binding(0) var<uniform> camera: Camera;
@group(3) @binding(1) var<uniform> params: TraceParams;

// The world-space ray through a point in a pixel.
//
// `jitter` is the position within the pixel, in `[0, 1)^2`. Pass `vec2f(0.5)`
// for the centre, which is what a debug readout wants; a converging render
// passes a sampled offset, and the filter that implies is the caller's choice
// rather than this function's.
//
// Perspective only. Orthographic cameras generate parallel rays with the origin
// varying per pixel, which arrives with the camera work; the picking path
// already learned that lesson and this kernel must not un-learn it.
fn camera_ray(pixel: vec2u, jitter: vec2f) -> Ray {
    let uv = (vec2f(pixel) + jitter) / vec2f(params.resolution);
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
