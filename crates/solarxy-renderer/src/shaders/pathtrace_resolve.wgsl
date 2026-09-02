// The traced image's handoff into the shared post chain.
//
// One fullscreen triangle copying the accumulator's running mean into the
// linear high-dynamic-range target the composite reads. That is the whole pass,
// and its smallness is the point: everything a look is made of -- exposure, both
// grading tables, the tone map, the grade, bloom, the selection rim -- already
// runs downstream of that target for the rasterizer, so a traced image inherits
// all of it by landing in the same place rather than by growing its own copy.
//
// It lives beside the other fullscreen passes rather than under `pathtrace/`
// because it is an ordinary render pass: it declares a vertex and a fragment
// stage, it is never composed into a compute kernel, and the rules that
// directory enforces about derivatives and barriers have nothing to say about
// it.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) id: u32) -> VertexOutput {
    let uv = vec2<f32>(f32((id << 1u) & 2u), f32(id & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

// The accumulation target, read rather than sampled.
//
// `rgba32float` is unfilterable without an optional feature, so the read is a
// `textureLoad` with no sampler. The accumulator is usually the target's size
// and the load is then an exact copy; the interactive preview's resolution
// scale makes it smaller, and addressing by the fragment's normalized
// coordinate turns the same load into a nearest upscale. Nearest is the honest
// choice for an unfilterable mean: what the preview trades away is resolution,
// and interpolation would dress that up as blur.
@group(0) @binding(0) var accumulated: texture_2d<f32>;

// The matte's inputs, bound only by the matte pipeline: the kernel's coverage
// counts, row-major at the accumulator's size, and how many samples the run
// has drawn so far. A count over a count rather than a stored fraction,
// because integer sums are what keep a chunked run and a one-shot run on the
// identical matte; the division happens here, once, per resolve, which is
// also what makes a mid-render preview's partial fraction correct for free.
@group(0) @binding(1) var<storage, read> resolve_coverage: array<u32>;
@group(0) @binding(2) var<uniform> resolve_drawn: u32;

@fragment
fn fs_resolve(in: VertexOutput) -> @location(0) vec4<f32> {
    // Clip-space y points up while texel rows grow down, so the quad's uv is
    // flipped before it addresses the accumulator. At equal sizes this indexes
    // exactly the fragment's own texel: uv interpolates to pixel centers, so
    // the product truncates to the fragment's integer position.
    let dims = textureDimensions(accumulated);
    let screen = vec2<f32>(in.uv.x, 1.0 - in.uv.y);
    let texel = clamp(
        vec2<i32>(screen * vec2<f32>(dims)),
        vec2<i32>(0, 0),
        vec2<i32>(dims) - vec2<i32>(1, 1),
    );
    // Alpha is one rather than the accumulator's: that lane is the described
    // count steering the denoiser, not a matte, and handing the composite a
    // partially transparent scene would darken it against whatever the target
    // held. A transparent render resolves through `fs_resolve_matte` below,
    // which has a real matte to write; this entry point is every opaque
    // render, unchanged.
    return vec4<f32>(textureLoad(accumulated, texel, 0).rgb, 1.0);
}

@fragment
fn fs_resolve_matte(in: VertexOutput) -> @location(0) vec4<f32> {
    // The same addressing as `fs_resolve`, for the same reasons.
    let dims = textureDimensions(accumulated);
    let screen = vec2<f32>(in.uv.x, 1.0 - in.uv.y);
    let texel = clamp(
        vec2<i32>(screen * vec2<f32>(dims)),
        vec2<i32>(0, 0),
        vec2<i32>(dims) - vec2<i32>(1, 1),
    );
    // The colour stays the accumulator's mean, which under the transparent
    // flag is already weighted by coverage: a camera miss contributed
    // nothing. The matte beside it is the plain fraction of camera rays that
    // found a surface, and the composite is what divides the weighting out
    // before the display chain.
    let cell = u32(texel.y) * dims.x + u32(texel.x);
    let coverage = f32(resolve_coverage[cell]) / f32(max(resolve_drawn, 1u));
    return vec4<f32>(textureLoad(accumulated, texel, 0).rgb, coverage);
}
