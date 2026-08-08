// The sampler fragment: pseudo-random numbers, stratification, and the one
// shape sampler a lobe draws from. Declares no entry point and no bindings, so
// it parses on its own and is a base like the traversal and the atlas.
//
// Two generators, because they answer different questions. The lattice is plain
// white noise, which is what an image wants: cheap, decorrelated between
// neighbouring pixels, and unbiased at any sample count. The stratified draws
// are what a histogram wants: over a known number of samples they visit every
// stratum exactly once, so an empirical density converges as 1/N rather than as
// 1/sqrt(N) and a comparison against an analytic density can hold a tight
// tolerance without millions of samples.
//
// The reference's stratified sampler is NOT ported, and the reason is the
// binding budget rather than taste. It reads two textures generated on the CPU,
// a stratified table and a per-pixel blue-noise offset, and the sampled group
// reserves every free number it has for the environment. What is here instead is
// correlated multi-jittered sampling, which is self-contained arithmetic and is
// the textbook answer to the same problem: Andrew Kensler, "Correlated
// Multi-Jittered Sampling", Pixar technical memo 13-01.
//
// State is threaded as a pointer rather than held in a module-scope `var`, which
// is what the reference does. A pointer costs nothing here and buys two things:
// a probe can seed one deterministically and compare runs, and two generators
// can coexist in one invocation without one silently advancing the other.

const PI: f32 = 3.141592653589793;

// Which draw a call is making. The point is decorrelation: two dimensions drawn
// at the same bounce must not return the same numbers, and the same dimension
// drawn at successive bounces must not either. Values are labels rather than a
// running counter, so adding a dimension does not renumber the others and a
// stratified sequence stays reproducible across a change elsewhere.
//
// The alpha-test dimension is last and deliberately far from the others: a
// coverage test adds the primitive index to it, so it owns the open range above
// itself.
const RNG_DIM_PIXEL_JITTER: u32 = 0u;
const RNG_DIM_ENVIRONMENT: u32 = 1u;
const RNG_DIM_LOBE: u32 = 2u;
const RNG_DIM_DIRECTION: u32 = 3u;
const RNG_DIM_APERTURE: u32 = 4u;
const RNG_DIM_ROULETTE: u32 = 5u;
const RNG_DIM_ALPHA_TEST: u32 = 50u;

struct RngState {
    // The PCG lattice, advanced whole by every draw.
    lattice: vec4u,
    // Which sample this invocation is producing, and how many there are in
    // total. `strata` of zero or one turns stratification off and every draw
    // falls back to the lattice, which is what an accumulating render wants:
    // its sample count is not known in advance.
    sample_index: u32,
    strata: u32,
    // Bounce depth, so the same dimension drawn one bounce deeper is a
    // different sequence.
    bounce: u32,
    // Scrambles every hash in this invocation. Two invocations that agree on
    // everything else still decorrelate, which is what keeps a stratified image
    // from showing the structure of its own strata.
    scramble: u32,
}

// One step of the PCG lattice, by value.
//
// The reference writes this as a pointer function and then swizzles the pointer
// without dereferencing it, which is not valid WGSL and would not compile. By
// value is both correct and free: the whole state is one register-sized vector.
//
// https://www.pcg-random.org
fn pcg4d(v_in: vec4u) -> vec4u {
    var v = v_in * 1664525u + 1013904223u;
    v.x += v.y * v.w;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v.w += v.y * v.z;
    v = v ^ (v >> vec4u(16u));
    v.x += v.y * v.w;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v.w += v.y * v.z;
    return v;
}

// The murmurhash3 finalizer. Used for the per-dimension scrambles rather than
// for the sample stream, so what matters is avalanche rather than period.
fn rng_hash(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u;
    x *= 0x85ebca6bu;
    x ^= x >> 13u;
    x *= 0xc2b2ae35u;
    x ^= x >> 16u;
    return x;
}

fn rng_hash_combine(seed: u32, v: u32) -> u32 {
    // The golden-ratio constant and the two shifts are boost's `hash_combine`.
    // Any decent mixer would do; what must not happen is a plain xor, which
    // makes the pair (a, b) and the pair (b, a) collide.
    return seed ^ (rng_hash(v) + 0x9e3779b9u + (seed << 6u) + (seed >> 2u));
}

// A seed for one dimension of one bounce of one invocation.
fn rng_seed(rng: ptr<function, RngState>, dim: u32) -> u32 {
    let s = rng_hash_combine((*rng).scramble, (*rng).bounce);
    return rng_hash_combine(s, dim);
}

// Seeds a generator.
//
// `strata` is the total sample count when it is known and zero when it is not.
// A probe knows it, because it dispatches a fixed number of samples on purpose;
// an accumulating render does not, because the user stops it when the image
// looks finished.
fn rng_init(pixel: vec2u, sample_index: u32, strata: u32, seed: u32) -> RngState {
    var rng: RngState;
    // The lattice is seeded from all four of pixel, sample and seed, because two
    // pixels that differ only in y must not produce the same first draw. The
    // fourth lane takes a sum rather than a repeat so a diagonal is not a
    // fixed point.
    rng.lattice = vec4u(pixel.x, pixel.y, sample_index ^ seed, pixel.x + pixel.y + seed);
    rng.sample_index = sample_index;
    rng.strata = strata;
    rng.bounce = 0u;
    rng.scramble = rng_hash_combine(rng_hash_combine(seed, pixel.x), pixel.y);
    return rng;
}

// Moves to the next bounce, so every dimension re-seeds.
fn rng_next_bounce(rng: ptr<function, RngState>) {
    (*rng).bounce += 1u;
}

// Four white-noise words. Every draw advances the whole lattice and most callers
// use one or two of the four, which is what the reference does too: a partial
// advance would make the stream depend on how many components a caller happened
// to ask for.
fn rng_next4(rng: ptr<function, RngState>) -> vec4f {
    (*rng).lattice = pcg4d((*rng).lattice);
    return vec4f((*rng).lattice) / f32(0xffffffffu);
}

fn rand1(rng: ptr<function, RngState>) -> f32 {
    return rng_next4(rng).x;
}

fn rand2(rng: ptr<function, RngState>) -> vec2f {
    return rng_next4(rng).xy;
}

// Kensler's permutation of `0 .. l` under `p`.
//
// A hash reduced modulo `l` is not a permutation: it collides, which puts two
// samples in one stratum and leaves another empty, and the histogram it feeds
// then disagrees with the density for a reason that looks like a bug in the
// density. This is a real bijection for any `l`, by scrambling within the next
// power of two and rejecting what falls outside. Expected iterations are under
// two, and the loop is bounded in practice by that rejection rather than by a
// counter.
fn rng_permute(i_in: u32, l: u32, p: u32) -> u32 {
    if l <= 1u {
        return 0u;
    }
    // The next power of two above `l`, minus one, as a mask.
    var w = l - 1u;
    w |= w >> 1u;
    w |= w >> 2u;
    w |= w >> 4u;
    w |= w >> 8u;
    w |= w >> 16u;

    var i = i_in;
    loop {
        i ^= p;
        i *= 0xe170893du;
        i ^= p >> 16u;
        i ^= (i & w) >> 4u;
        i ^= p >> 8u;
        i *= 0x0929eb3fu;
        i ^= p >> 23u;
        i ^= (i & w) >> 1u;
        i *= 1u | (p >> 27u);
        i *= 0x6935fa69u;
        i ^= (i & w) >> 11u;
        i *= 0x74dcb303u;
        i ^= (i & w) >> 2u;
        i *= 0x9e501cc3u;
        i ^= (i & w) >> 2u;
        i *= 0xc860a3dfu;
        i &= w;
        i ^= i >> 5u;
        if i < l {
            break;
        }
    }
    return (i + p) % l;
}

// A jittered scalar in `[0, 1)`, one stratum per sample.
fn rand1_strat(rng: ptr<function, RngState>, dim: u32) -> f32 {
    let n = (*rng).strata;
    if n <= 1u {
        return rand1(rng);
    }
    let p = rng_seed(rng, dim);
    let s = rng_permute((*rng).sample_index % n, n, p);
    // The jitter comes from the lattice rather than from another hash, so the
    // sequence still advances and two consecutive draws of the same dimension
    // at the same bounce are not identical.
    return (f32(s) + rand1(rng)) / f32(n);
}

// A correlated multi-jittered pair in `[0, 1)^2`.
//
// The grid is as square as the sample count allows. A count that is not a
// perfect square gets a grid one column wider than tall, which stratifies both
// axes and simply gives the last row fewer samples than the others; that is a
// weaker guarantee than a square grid and a much stronger one than none.
fn rand2_strat(rng: ptr<function, RngState>, dim: u32) -> vec2f {
    let total = (*rng).strata;
    if total <= 1u {
        return rand2(rng);
    }
    let m = u32(ceil(sqrt(f32(total))));
    let n = (total + m - 1u) / m;

    let p = rng_seed(rng, dim);
    let sp = rng_permute((*rng).sample_index % total, total, p);
    let sx = rng_permute(sp % m, m, rng_hash(p ^ 0x68bc21ebu));
    let sy = rng_permute(sp / m, n, rng_hash(p ^ 0x02e5be93u));
    let j = rand2(rng);

    // Each axis is offset by the other's stratum, which is what makes this
    // correlated rather than two independent Latin hypercubes: the pair covers
    // the square, not just each margin.
    return vec2f(
        (f32(sx) + (f32(sy) + j.y) / f32(n)) / f32(m),
        (f32(sy) + (f32(sx) + j.x) / f32(m)) / f32(n),
    );
}

// A uniform direction on the sphere, from a pair in `[0, 1)^2`.
//
// The density is 1/(4*PI). `z` is the uniform variate directly, which is what
// makes this uniform in solid angle rather than in latitude.
fn sample_sphere(uv: vec2f) -> vec3f {
    let u = (uv.x - 0.5) * 2.0;
    let t = uv.y * PI * 2.0;
    let f = sqrt(max(0.0, 1.0 - u * u));
    return vec3f(f * cos(t), f * sin(t), u);
}
