// The edge-avoiding a-trous wavelet filter, after Dammertz, Sewtz, Hanika and
// Lensch, "Edge-Avoiding A-Trous Wavelet Transform for Fast Global
// Illumination Filtering" (High Performance Graphics 2010).
//
// Composed over `aov.wgsl`, whose octahedral unpacking turns the auxiliary
// lane back into the normal this steers by.
//
// # What it is for
//
// A path traced image at a handful of samples is unbiased and unusable: the
// mean is right and the variance is enormous, so the picture is the right one
// covered in grain. Averaging removes the grain and takes as long as it takes.
// This trades a little bias for a lot of variance, so a person can see the
// image while it converges.
//
// # Why a-trous rather than a bilateral blur
//
// One five-tap pass with a stride of 2^i covers the same support as a 2^i-wide
// blur at a fixed cost per level, so five levels reach a 33-pixel radius in
// five passes of 25 taps rather than one pass of 1089. That is the whole
// algorithm. The edge stopping is what keeps it from being a blur: each tap is
// weighted by how much the *guides* say it belongs to the same surface.
//
// # What steers it, and what does not
//
// Albedo, the world normal, and the colour's own distance. **Not depth**, and
// that is a platform limit rather than an omission: core WebGPU grants four
// storage textures per stage, the accumulator's two ping-ponged pairs spend all
// four, and the auxiliary texture's fourth lane is already carrying the normal
// at twelve bits per component, which is the whole exact-integer range an f32
// has. There is nowhere to put a depth channel. What that costs is the case two
// surfaces share an albedo and a normal across a depth discontinuity -- a floor
// seen past a table edge -- where this filter will reach across and a depth
// guide would not.

struct DenoiseParams {
    // The image, so a tap at the edge is rejected rather than clamped: clamping
    // would weight the border pixel several times and pull the edge toward it.
    resolution: vec2u,
    // Distance between taps, 1 at the first level and doubling. The hole in
    // "a-trous" -- French for "with holes" -- is this.
    stride: u32,
    // Which level this dispatch is, counted from zero. Tightens the colour
    // tolerance as the support widens, which is what stops the coarse levels
    // from smearing across everything the fine ones preserved.
    level: u32,
    // How many samples the mean being filtered averages. Noise falls as the
    // reciprocal square root of this, so the colour tolerance follows it down
    // and a converged image is left almost untouched.
    samples: u32,
    // How far apart two radiances may be before the tap is discounted, at one
    // sample and the finest level.
    sigma_color: f32,
    // The exponent on the cosine between two normals. Larger is a narrower
    // agreement: 128 rejects anything past about ten degrees.
    normal_power: f32,
    // How far apart two base colours may be before the tap is discounted. Small,
    // because an albedo edge is a material boundary and not something to blur
    // across.
    sigma_albedo: f32,
    // How fast the colour tolerance tightens as the support widens.
    level_falloff: f32,
    // Pads the struct to its own eight-byte alignment.
    reserved: u32,
}

// The colour being filtered, and where this level writes it. Separate textures
// rather than one read-write: WebGPU grants read-write storage access to
// `r32uint`, `r32sint` and `r32float` only.
@group(0) @binding(0) var dn_in: texture_storage_2d<rgba32float, read>;
@group(0) @binding(1) var dn_out: texture_storage_2d<rgba32float, write>;
// Albedo in `rgb`, the octahedral world normal in `a`. Read at every level and
// never written, because the guides describe the surface rather than the
// estimate and filtering them would defeat the point of having them.
@group(0) @binding(2) var dn_aux: texture_storage_2d<rgba32float, read>;

@group(1) @binding(0) var<uniform> dn: DenoiseParams;

// B3 spline, which is the kernel the paper uses and the reason the levels
// compose into something close to a Gaussian.
const ATROUS_KERNEL = array<f32, 5>(0.0625, 0.25, 0.375, 0.25, 0.0625);

@compute @workgroup_size(8, 8, 1)
fn denoise_main(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= dn.resolution.x || gid.y >= dn.resolution.y {
        return;
    }
    let p = vec2i(gid.xy);
    let center = textureLoad(dn_in, p);
    let center_aux = textureLoad(dn_aux, p);
    let center_normal = unpack_octahedral(center_aux.a);

    // The colour tolerance for this level and this sample count.
    //
    // Two divisions, and both matter. By the level, because the support doubles
    // each time and a tolerance that did not tighten would let the coarse
    // levels average across everything. By the square root of the sample count,
    // because that is how fast the noise this is hiding actually falls: at 256
    // samples the tolerance is a sixteenth of what it was at one, so a
    // converged image passes through nearly unchanged and the filter costs
    // detail only where there was nothing to lose.
    let level_scale = 1.0 / pow(max(dn.level_falloff, 1.0), f32(dn.level));
    let sample_scale = 1.0 / sqrt(f32(max(dn.samples, 1u)));
    let color_tolerance = max(dn.sigma_color * level_scale * sample_scale, 1e-6);
    let albedo_tolerance = max(dn.sigma_albedo, 1e-6);

    var sum = vec3f(0.0);
    var weight_sum = 0.0;
    for (var dy = -2; dy <= 2; dy += 1) {
        for (var dx = -2; dx <= 2; dx += 1) {
            let q = p + vec2i(dx, dy) * i32(dn.stride);
            // Rejected rather than clamped at the border, so the edge pixel
            // does not get counted five times and drag the result toward
            // itself.
            if q.x < 0 || q.y < 0 || q.x >= i32(dn.resolution.x) || q.y >= i32(dn.resolution.y) {
                continue;
            }
            let tap = textureLoad(dn_in, q);
            let tap_aux = textureLoad(dn_aux, q);

            let dc = center.rgb - tap.rgb;
            let w_color = exp(-dot(dc, dc) / (color_tolerance * color_tolerance));

            let tap_normal = unpack_octahedral(tap_aux.a);
            let w_normal = pow(max(dot(center_normal, tap_normal), 0.0), dn.normal_power);

            let da = center_aux.rgb - tap_aux.rgb;
            let w_albedo = exp(-dot(da, da) / (albedo_tolerance * albedo_tolerance));

            let k = ATROUS_KERNEL[dx + 2] * ATROUS_KERNEL[dy + 2];
            let w = k * w_color * w_normal * w_albedo;
            sum += tap.rgb * w;
            weight_sum += w;
        }
    }

    // The centre tap always carries weight, so this cannot divide by zero; the
    // guard is against a denormal weight sum on a pixel every guide disagreed
    // with, where returning the centre unchanged is the honest answer.
    var filtered = center.rgb;
    if weight_sum > 1e-8 {
        filtered = sum / weight_sum;
    }
    // The alpha lane rides through untouched. On the accumulator it counts the
    // samples that described a surface, and this filter is not in the business
    // of averaging counts; on a scratch texture it is whatever the level before
    // put there.
    textureStore(dn_out, p, vec4f(filtered, center.a));
}
