// The light probe: a shading point and a light in, what the sampler answered out.
//
// Composed after the traversal, the atlas, the material fragment, the sampler,
// the BSDF, the environment and the lights. It shades nothing and reads no
// camera, so a wrong answer here is the light sampling and cannot be anything
// the material explains.
//
// Two modes, selected by pipeline-overridable constant rather than by a uniform
// branch, so each specializes at pipeline creation and the unused half folds
// away.
//
//   sample     draw a connection to a light, and report its direction and density
//   intersect  take a direction, and report the density the light it hits carries
//
// Together they are what a histogram needs, and the reason the second one is not
// simply the first one's `pdf` field is the whole point of having two: the
// sampler derives its density from the point it chose on the rectangle, while
// the intersection derives it from where a ray *landed* on that rectangle.
// Those are two independent routes to one number, and a factor wrong in either
// shows up as a disagreement. Reporting the sampler's own density back would
// pass however wrong it was.
//
// The intersection mode also answers a question no histogram does. The mean of
// one over the density, over directions drawn from the sampler, is the light's
// solid angle, and a rectangle's solid angle has a closed form. That turns "the
// two halves agree" into "they agree with geometry", which is what catches both
// halves being wrong by the same factor.

// 48 bytes. Three sixteen-byte blocks with the scalar tail packed into the
// third, so nothing needs a pad; `uniform_layout.rs` carries a row for it.
struct LightTap {
    // The shading point a connection starts from. `w` unused.
    origin: vec4f,
    // A direction to test, in intersect mode. Unused in sample mode.
    direction: vec4f,
    // Index into the light pool.
    light: u32,
    // Which sample of `strata` this invocation draws, in sample mode.
    sample_index: u32,
    // How many samples the batch contains, which is the count the stratified
    // sampler divides its domain into. Zero or one asks for white noise.
    strata: u32,
    // Fixed across a batch, so every sample in it shares one stratified
    // sequence and only `sample_index` moves.
    seed: u32,
}

// The probe's own group, taking group 1 the way every other probe does: that is
// the accumulation group's number, which no probe binds.
@group(1) @binding(0) var<storage, read> light_taps: array<LightTap>;
@group(1) @binding(1) var<storage, read_write> light_results: array<vec4f>;

override LIGHT_TAP_WIDTH: u32 = 64u;
override LIGHT_RESULT_WIDTH: u32 = 2u;
override LIGHT_PROBE_MODE: u32 = 0u;

const LIGHT_PROBE_SAMPLE: u32 = 0u;
const LIGHT_PROBE_INTERSECT: u32 = 1u;

@compute @workgroup_size(8, 8, 1)
fn light_probe(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.y * LIGHT_TAP_WIDTH + gid.x;
    if index >= arrayLength(&light_taps) {
        return;
    }
    let tap = light_taps[index];
    let out = index * LIGHT_RESULT_WIDTH;

    if LIGHT_PROBE_MODE == LIGHT_PROBE_SAMPLE {
        // The pixel is fixed and only the sample index moves, which is the
        // stratified sampler's contract: one scramble per batch, one stratum
        // per sample.
        var rng = rng_init(vec2u(0u), tap.sample_index, tap.strata, tap.seed);
        let uv = rand2_strat(&rng, RNG_DIM_LIGHT);
        let s = sample_light(tap.light, tap.origin.xyz, uv);

        light_results[out + 0u] = vec4f(s.direction, s.pdf);
        light_results[out + 1u] = vec4f(s.radiance, s.distance);
    } else {
        let direction = normalize(tap.direction.xyz);
        let hit = intersect_lights(tap.origin.xyz, direction, 1e30, arrayLength(&lights));
        // A miss reports a zero density, which the host reads as "the
        // light-sampling technique could not have produced this direction"
        // rather than as an error.
        light_results[out + 0u] = vec4f(direction, hit.pdf);
        light_results[out + 1u] = vec4f(hit.radiance, hit.t);
    }
}
