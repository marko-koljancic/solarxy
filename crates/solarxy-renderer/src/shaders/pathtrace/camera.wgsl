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

// 56 bytes, and declared whole, so it is in the uniform-layout table.
//
// No padding word: every member aligns to eight or four, so the struct aligns to
// eight and fifty-six is already a multiple of it. Adding a `vec3f` or a `vec4f`
// here would raise the alignment to sixteen and need one, which is why the
// environment is a separate uniform rather than four more floats on the end.
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
    // How many entries of `lights` next-event estimation may pick from.
    //
    // Here rather than `arrayLength(&lights)`, which WGSL does offer, because
    // the buffer never shrinks below one whole record: an empty scene's
    // padding record would otherwise be a black light at the origin taking a
    // share of the estimator's probability.
    light_count: u32,
    // The aperture's radius in world units, resolved out of the camera's
    // f-number. Zero is a pinhole.
    aperture_radius: f32,
    // How far in front of the camera is sharp, in world units.
    focus_distance: f32,
    // Aperture blades. Zero, one and two are circular; three or more is a
    // polygon.
    aperture_blades: u32,
}

@group(3) @binding(0) var<uniform> camera: Camera;
@group(3) @binding(1) var<uniform> params: TraceParams;

// A tent-filtered offset within a pixel, from a uniform pair.
//
// Maps `[0, 1)` onto `[-1, 1)` weighted toward the centre, so a sample near the
// pixel's own centre counts for more than one at its edge. Strictly better than
// a box for the same cost, which is why the source material uses one: a box
// filter weights the corner of a pixel as heavily as its middle and leaves
// edges looking slightly ragged at any sample count.
//
// The returned offset spans a whole pixel either side of the centre, so
// neighbouring pixels overlap, which is what reconstructs an edge rather than
// stair-stepping it.
fn tent_jitter(uv: vec2f) -> vec2f {
    let t = uv * 2.0;
    let x = select(1.0 - sqrt(2.0 - t.x), sqrt(t.x) - 1.0, t.x < 1.0);
    let y = select(1.0 - sqrt(2.0 - t.y), sqrt(t.y) - 1.0, t.y < 1.0);
    return vec2f(x, y) + 0.5;
}

// A point on the aperture, in the lens plane's own coordinates.
//
// A circular opening for `blades` under three, and a regular polygon otherwise,
// which is the shape a real iris leaves on an out-of-focus highlight. The
// polygon is sampled by picking a wedge and then a point in the triangle that
// wedge spans, which is uniform over the whole polygon rather than uniform in
// angle: sampling the angle uniformly would crowd the samples toward the
// vertices and give a bokeh with bright corners.
fn aperture_offset(uv: vec2f, blades: u32) -> vec2f {
    if blades < 3u {
        let r = sqrt(uv.x);
        let theta = uv.y * 2.0 * PI;
        return vec2f(r * cos(theta), r * sin(theta));
    }
    let count = f32(blades);
    let wedge = floor(uv.x * count);
    // The leftover fraction of the same variate, rescaled across the wedge it
    // selected, which is exactly uniform there and costs no second draw.
    let a = fract(uv.x * count);
    let b = uv.y;
    // Two barycentric coordinates over the wedge's triangle, folded so the
    // pair covers it once rather than covering half of it twice.
    var u = a;
    var v = b;
    if u + v > 1.0 {
        u = 1.0 - u;
        v = 1.0 - v;
    }
    let step = 2.0 * PI / count;
    let first = vec2f(cos(wedge * step), sin(wedge * step));
    let second = vec2f(cos((wedge + 1.0) * step), sin((wedge + 1.0) * step));
    return first * u + second * v;
}

// The world-space ray through a point in a pixel.
//
// `jitter` is the position within the pixel, in `[0, 1)^2`; `lens` is a pair
// for the aperture, ignored when the aperture is a pinhole. Pass `vec2f(0.5)`
// for the pixel centre, which is what a debug readout wants.
//
// # Two points, not an eye and a direction
//
// Both the near and the far plane are unprojected and the ray runs between
// them. Anchoring the origin at the eye instead is correct for a perspective
// camera and **wrong for an orthographic one**, whose rays are parallel with
// the origin varying per pixel: every pixel would collapse onto the view axis.
// That is not hypothetical, it is the bug that froze gizmo drags in axis views
// before 0.8.0, and `solarxy_core::raycast::screen_to_world_ray` reconstructs
// the pick ray this way for exactly that reason. This is the same construction
// against the same matrices, so the two cannot disagree about where a pixel
// points, and it needs no flag saying which projection is in use.
fn camera_ray(pixel: vec2u, jitter: vec2f, lens: vec2f) -> Ray {
    let uv = (vec2f(pixel) + jitter) / vec2f(params.resolution);
    // wgpu clip space: y points down in framebuffer space and up in NDC.
    let ndc = vec2f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    let near_h = camera.inv_proj * vec4f(ndc, 0.0, 1.0);
    let far_h = camera.inv_proj * vec4f(ndc, 1.0, 1.0);
    let near_view = near_h.xyz / near_h.w;
    let far_view = far_h.xyz / far_h.w;

    // A look-at view matrix is a rotation and a translation, so the inverse of
    // its rotation is its transpose and no inversion happens in the shader. The
    // translation is the eye, which the uniform already carries.
    let rot = transpose(mat3x3f(
        camera.view[0].xyz,
        camera.view[1].xyz,
        camera.view[2].xyz,
    ));
    var origin = rot * near_view + camera.view_position.xyz;
    var direction = normalize(rot * (far_view - near_view));

    // A projection matrix's third column carries -1 in its `w` for a
    // perspective frustum and 0 for an orthographic box. That is what says
    // whether there is a lens at all: parallel rays come from no aperture, so
    // an orthographic camera has nothing to open and nothing to focus.
    let perspective = abs(camera.proj[2].w) > 0.5;
    if params.aperture_radius <= 0.0 || !perspective {
        return Ray(origin, direction);
    }

    // The focus point: where this pinhole ray crosses the focus plane. The
    // plane is square to the view axis rather than a sphere around the eye, so
    // the distance is measured along the axis and the ray is extended by more
    // than that toward the edges of frame, which is what a real lens does.
    let forward = -normalize(rot * vec3f(0.0, 0.0, 1.0));
    let axis_cos = dot(direction, forward);
    if axis_cos <= 1e-4 {
        return Ray(origin, direction);
    }
    let focus = origin + direction * (params.focus_distance / axis_cos);

    // And the aperture: move the eye onto the lens and re-aim at the same
    // focus point. Everything at the focus distance is unmoved, which is what
    // makes it sharp, and everything else is displaced in proportion to how far
    // from it it sits.
    let offset = aperture_offset(lens, params.aperture_blades) * params.aperture_radius;
    let right = rot * vec3f(1.0, 0.0, 0.0);
    let up = rot * vec3f(0.0, 1.0, 0.0);
    origin += right * offset.x + up * offset.y;
    direction = normalize(focus - origin);
    return Ray(origin, direction);
}
