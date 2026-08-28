// The rand probe: a list of taps in, the stratified pair each one drew out.
//
// Composed after `rand.wgsl`, and the only instrument that can vary the pixel.
// Every other probe fixes the pixel at the origin and varies the sample index,
// which is the right shape for asking whether a density matches its sampler
// and is structurally blind to whether two pixels share a point set. The
// defect class this exists for is exactly that: a sampler whose permutations
// reorder each pixel's cell set without ever changing it, so the residual at
// the target count is a stationary pattern rather than noise.
//
// It traverses nothing and shades nothing, so a wrong answer here is the
// sampler's own arithmetic rather than something a scene could explain.

struct RandTap {
    // The pixel the generator is seeded for.
    pixel: vec2u,
    // Which sample of the sequence to draw.
    sample_index: u32,
    // The total sample count, which is what turns stratification on.
    strata: u32,
    // The per-render seed.
    seed: u32,
    // The dimension label, as the kernel's `RNG_DIM_*` values.
    dim: u32,
    // Nonzero draws the scalar path, zero the pair path; both rotate per
    // pixel and both need the instrument.
    scalar: u32,
    pad0: u32,
}

@group(0) @binding(0) var<storage, read> rand_taps: array<RandTap>;
@group(0) @binding(1) var<storage, read_write> rand_results: array<vec4f>;

// Taps per row of the dispatch grid; the host owns the value so the two cannot
// drift.
override TAP_WIDTH: u32 = 64u;

@compute @workgroup_size(8, 8, 1)
fn rand_probe(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.y * TAP_WIDTH + gid.x;
    if index >= arrayLength(&rand_taps) {
        return;
    }
    let tap = rand_taps[index];
    var rng = rng_init(tap.pixel, tap.sample_index, tap.strata, tap.seed);
    if tap.scalar != 0u {
        let value = rand1_strat(&rng, tap.dim);
        rand_results[index] = vec4f(value, 0.0, 0.0, 0.0);
    } else {
        let pair = rand2_strat(&rng, tap.dim);
        rand_results[index] = vec4f(pair, 0.0, 0.0);
    }
}
