// The environment fragment: what a ray finds when it leaves the scene, and how
// the estimator draws a direction toward it. Declares no entry point.
//
// Composed over the sampler, whose stratified draws it uses.
//
// # Why an environment needs a distribution of its own
//
// An outdoor sky is mostly dim and a little blinding, and the blinding part is a
// ten-thousandth of the image. Drawing directions uniformly finds the sun about
// once in ten thousand tries and charges each find a tiny density, so it comes
// back carrying ten thousand times the average. That is not slow convergence, it
// is an image made of white specks. Drawing in proportion to brightness finds it
// on nearly every sample and charges a correspondingly high density, and the two
// cancel.
//
// # The search, and why it is a search
//
// The distribution is two cumulative tables, built on the CPU, and this walks
// them by binary search. The alternative is to store the *inverse* and read it in
// one interpolated tap, which is what the source material does and what an
// earlier draft of this did: it is one texture read instead of twenty, and it is
// an approximation, because linearly interpolating an inverse gives a sampler
// whose real density is not the piecewise-constant one the pdf then reports.
// This stage exists to establish that densities describe their samplers. Twenty
// texture reads against a hierarchy traversal that costs far more is a good
// price for not opening a second front.
//
// # Falling back to a constant
//
// A scene with no HDRI has `size` zero and gets two colours blended by the world
// up axis, which is also what makes the white furnace test expressible: set both
// the same, take every light out, and an energy-conserving surface must vanish
// into its background.

@group(2) @binding(3) var env_map: texture_2d<f32>;
@group(2) @binding(4) var env_sampler: sampler;
// The cumulative row weights, one row of `size.y` texels.
@group(2) @binding(5) var env_marginal: texture_2d<f32>;
// Per row, the cumulative pixel weights within it: `size.x` by `size.y`.
@group(2) @binding(6) var env_conditional: texture_2d<f32>;

// 64 bytes, declared whole, so it is in the uniform-layout table.
struct EnvParams {
    // The constant fallback when there is no image: radiance looking up, `w`
    // unused.
    env_up: vec4f,
    // And looking down.
    env_down: vec4f,
    // The distribution's dimensions. **Zero means there is no image**, which is
    // the one flag everything here branches on.
    size: vec2u,
    // The sum of every weight, which is the density's denominator.
    total_weight: f32,
    // A plain multiplier on the environment's contribution, as authored.
    intensity: f32,
    // Yaw about the world up axis, as its cosine and sine. Precomputed on the
    // CPU rather than taken as an angle, because the kernel needs both at every
    // lookup and neither changes within a dispatch.
    rotation: vec2f,
    // Which strategy `env_sample` uses. Exists to be measured, not configured:
    // both are unbiased and converge to the same image, so keeping the uniform
    // baseline in the shipped kernel is what makes "importance sampling
    // converges faster" a repeatable measurement rather than a number recorded
    // once.
    sampling: u32,
    _pad: u32,
}

const ENV_SAMPLING_IMPORTANCE: u32 = 0u;
const ENV_SAMPLING_UNIFORM: u32 = 1u;

@group(3) @binding(2) var<uniform> env: EnvParams;

// One draw from the environment: where, how bright, and how likely.
struct EnvSample {
    direction: vec3f,
    radiance: vec3f,
    // Solid-angle density. Zero means the draw is unusable and the caller must
    // discard it rather than divide by it.
    pdf: f32,
}

// Yaw a direction about +Y, by the stored cosine and sine.
//
// Matches `rotate_yaw` in the skybox and the main shader exactly, which is what
// the parity between the traced environment and the viewport's rests on: a
// rotation applied one way round here and the other way round there is a scene
// lit from the opposite side of itself.
fn env_rotate(d: vec3f, c: f32, s: f32) -> vec3f {
    return vec3f(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

// A world direction to its place in the image.
//
// The same mapping `sample_equirect` uses on the Rust side and `fs_skybox` uses
// on the raster side: longitude from `atan2(z, x)` and latitude from `acos(y)`,
// with `v` measured down from the pole.
fn env_direction_to_uv(dir: vec3f) -> vec2f {
    let u = atan2(dir.z, dir.x) * (0.5 / PI) + 0.5;
    let v = acos(clamp(dir.y, -1.0, 1.0)) / PI;
    return vec2f(u, v);
}

// And back again.
fn env_uv_to_direction(uv: vec2f) -> vec3f {
    let phi = (uv.x - 0.5) * 2.0 * PI;
    let theta = uv.y * PI;
    let sin_theta = sin(theta);
    return vec3f(sin_theta * cos(phi), cos(theta), sin_theta * sin(phi));
}

// Whether there is an image to sample, as opposed to the constant fallback.
fn env_has_image() -> bool {
    return env.size.x > 0u && env.size.y > 0u && env.total_weight > 0.0;
}

// Whether there is anything worth sampling at all.
//
// A black environment is not merely dark: it would take a share of the
// estimator's probability and return nothing every time, which reads as a scene
// uniformly too dim by the ratio of lights to lights-plus-one. So it is excluded
// from the choice entirely rather than sampled and discarded.
fn env_present() -> bool {
    if env.intensity <= 0.0 {
        return false;
    }
    if env_has_image() {
        return true;
    }
    let total = env.env_up.rgb + env.env_down.rgb;
    return total.r + total.g + total.b > 0.0;
}

// The radiance arriving from `dir`, which is what a ray that left the scene
// finds and what the estimator's environment arm evaluates.
fn env_radiance(dir: vec3f) -> vec3f {
    if !env_has_image() {
        let t = saturate(dir.y * 0.5 + 0.5);
        return mix(env.env_down.rgb, env.env_up.rgb, t) * env.intensity;
    }
    let rotated = env_rotate(dir, env.rotation.x, env.rotation.y);
    let uv = env_direction_to_uv(rotated);
    // Level zero explicitly. There is no mip chain, and an implicit level would
    // need a derivative the uniformity analysis has no way to prove uniform in a
    // branchy kernel.
    return textureSampleLevel(env_map, env_sampler, uv, 0.0).rgb * env.intensity;
}

// The luminance of one texel of the image, read at its own centre.
//
// Nearest, deliberately: the density is piecewise constant per texel, so it has
// to be evaluated from the texel the sample fell in rather than from a blend of
// it with its neighbours. Filtering here would make the reported density a
// smoothed version of the one the sampler actually used, which is exactly the
// mismatch this design is avoiding.
fn env_texel_luminance(x: u32, y: u32) -> f32 {
    let texel = textureLoad(env_map, vec2u(x, y), 0).rgb;
    return luminance(texel);
}

// The density this distribution assigns to a direction, per unit solid angle.
//
// The `sin(theta)` appears twice and does not cancel: once as the weight the
// cell carries, taken at the cell's own centre, and once in the Jacobian that
// turns a density over the unit square into one over the sphere, taken at the
// direction itself. Writing both rather than cancelling them is what makes this
// exactly the density of the sampler below, whose draws are uniform within a
// cell while solid angle is not.
fn env_pdf(dir: vec3f) -> f32 {
    if !env_has_image() || env.sampling == ENV_SAMPLING_UNIFORM {
        // Uniform over the sphere, which is what the constant fallback's
        // sampler draws.
        return 1.0 / (4.0 * PI);
    }
    let rotated = env_rotate(dir, env.rotation.x, env.rotation.y);
    let uv = env_direction_to_uv(rotated);
    let size = vec2f(f32(env.size.x), f32(env.size.y));
    let x = min(u32(saturate(uv.x) * size.x), env.size.x - 1u);
    let y = min(u32(saturate(uv.y) * size.y), env.size.y - 1u);

    let theta = uv.y * PI;
    let sin_theta = sin(theta);
    if sin_theta <= 0.0 {
        return 0.0;
    }
    let theta_cell = (f32(y) + 0.5) / size.y * PI;
    let weight = env_texel_luminance(x, y) * sin(theta_cell);
    return weight * size.x * size.y / (env.total_weight * 2.0 * PI * PI * sin_theta);
}

// The first index in `[lo, hi)` of `row` whose cumulative value is at least `r`.
//
// A plain binary search, written out because WGSL has no library one, and
// bounded by a counted loop rather than by the invariant: a table that arrived
// malformed would otherwise spin forever inside a shader, which on some
// platforms is a device reset rather than a wrong pixel. Thirty-two iterations
// covers any table a four-billion-texel image could carry.
fn env_search(row: u32, count: u32, r: f32) -> u32 {
    var lo = 0u;
    var hi = count;
    for (var i = 0u; i < 32u; i += 1u) {
        if lo >= hi {
            break;
        }
        let mid = lo + (hi - lo) / 2u;
        if textureLoad(env_conditional, vec2u(mid, row), 0).r < r {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    return min(lo, count - 1u);
}

// The same search over the marginal table, which is one row.
fn env_search_marginal(count: u32, r: f32) -> u32 {
    var lo = 0u;
    var hi = count;
    for (var i = 0u; i < 32u; i += 1u) {
        if lo >= hi {
            break;
        }
        let mid = lo + (hi - lo) / 2u;
        if textureLoad(env_marginal, vec2u(mid, 0u), 0).r < r {
            lo = mid + 1u;
        } else {
            hi = mid;
        }
    }
    return min(lo, count - 1u);
}

// The variate's position within the cumulative step the search stopped on.
//
// Half an open interval away from a division by zero: a step of zero width is a
// texel the search cannot have selected on merit, so the midpoint is as good an
// answer as any and is the one that cannot produce an infinity.
fn env_rescale(lo: f32, hi: f32, r: f32) -> f32 {
    let span = hi - lo;
    if span <= 0.0 {
        return 0.5;
    }
    return saturate((r - lo) / span);
}

fn env_rescale_conditional(row: u32, col: u32, r: f32) -> f32 {
    var lo = 0.0;
    if col > 0u {
        lo = textureLoad(env_conditional, vec2u(col - 1u, row), 0).r;
    }
    let hi = textureLoad(env_conditional, vec2u(col, row), 0).r;
    return env_rescale(lo, hi, r);
}

fn env_rescale_marginal(row: u32, r: f32) -> f32 {
    var lo = 0.0;
    if row > 0u {
        lo = textureLoad(env_marginal, vec2u(row - 1u, 0u), 0).r;
    }
    let hi = textureLoad(env_marginal, vec2u(row, 0u), 0).r;
    return env_rescale(lo, hi, r);
}

// Draws a direction toward the environment.
fn env_sample(rng: ptr<function, RngState>) -> EnvSample {
    var out: EnvSample;
    let uv = rand2_strat(rng, RNG_DIM_ENVIRONMENT);

    if !env_has_image() || env.sampling == ENV_SAMPLING_UNIFORM {
        out.direction = sample_sphere(uv);
        out.radiance = env_radiance(out.direction);
        out.pdf = 1.0 / (4.0 * PI);
        return out;
    }

    // Row first, then column within it, which is what "two-dimensional
    // piecewise constant" means: the marginal picks a latitude in proportion to
    // how much light that band of sky carries, and the conditional picks a
    // longitude within it.
    let row = env_search_marginal(env.size.y, uv.y);
    let col = env_search(row, env.size.x, uv.x);

    // Where inside the chosen cell, and this is **not** the leftover fraction of
    // the same variate. The search consumed the variate's ordering, so the
    // fractional part of it is correlated with the cell it selected: a table
    // whose entries are unevenly spaced would then put every sample toward the
    // same edge of its cell, which is a bias no histogram over cells could see.
    // Rescaling the variate across the interval it landed in is the standard fix
    // and is exactly uniform: `(r - lo) / (hi - lo)` is the position within the
    // step the search stopped on.
    let size = vec2f(f32(env.size.x), f32(env.size.y));
    let jitter = vec2f(
        env_rescale_conditional(row, col, uv.x),
        env_rescale_marginal(row, uv.y),
    );
    let cell_uv = vec2f((f32(col) + jitter.x) / size.x, (f32(row) + jitter.y) / size.y);

    let local = env_uv_to_direction(cell_uv);
    // Back out of the image's frame into the world, which is the inverse yaw.
    out.direction = env_rotate(local, env.rotation.x, -env.rotation.y);
    out.radiance = textureSampleLevel(env_map, env_sampler, cell_uv, 0.0).rgb * env.intensity;

    let theta = cell_uv.y * PI;
    let sin_theta = sin(theta);
    if sin_theta <= 0.0 {
        out.pdf = 0.0;
        return out;
    }
    let theta_cell = (f32(row) + 0.5) / size.y * PI;
    let weight = env_texel_luminance(col, row) * sin(theta_cell);
    out.pdf = weight * size.x * size.y / (env.total_weight * 2.0 * PI * PI * sin_theta);
    return out;
}
