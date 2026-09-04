// The auxiliary channels' packing, shared by the kernel that writes them and
// the denoiser that steers by them.
//
// A fragment of its own rather than a copy in each, because these two functions
// are exact inverses and a drift between them is invisible: the normals would
// simply come back wrong, the edge-stopping weight would be plausible, and the
// filter would blur across a silhouette for a reason nothing points at.
//
// It declares no bindings and depends on nothing above it, so it composes first
// wherever it is needed.

// Quantization steps per octahedral component. Two of these multiply out to
// 2^24 - 1, which is the largest integer an f32 represents exactly, so the pair
// packs into one lane and comes back unchanged.
const OCT_STEPS: f32 = 4096.0;

// The world normal packed into one float, octahedrally.
//
// Two components rather than three, because the storage-texture budget is four
// and the accumulator needs all of them: the albedo takes three lanes and this
// takes the fourth. Octahedral mapping is the standard way to spend two numbers
// on a unit vector, and twelve bits each is about a twentieth of a degree, on a
// vector whose whole job is to steer an edge-stopping filter.
//
// Packed **arithmetically** rather than by reinterpreting the bits of a
// `pack2x16snorm`. The destination is a float texture, and an arbitrary bit
// pattern read as a float can be a denormal or a NaN, neither of which every
// platform is obliged to store and load unchanged. An integer below 2^24 is
// exact in an f32 everywhere, so this survives the round trip by construction
// rather than by luck. `solarxy_renderer::pathtrace::unpack_aov_normal` is the
// Rust half.
fn pack_octahedral(n: vec3f) -> f32 {
    let scaled = n / max(abs(n.x) + abs(n.y) + abs(n.z), 1e-8);
    var p = scaled.xy;
    if scaled.z < 0.0 {
        // Fold the lower hemisphere out across the octahedron's edges, which is
        // what makes the mapping continuous rather than seamed at the equator.
        p = (vec2f(1.0) - abs(scaled.yx)) * vec2f(
            select(-1.0, 1.0, scaled.x >= 0.0),
            select(-1.0, 1.0, scaled.y >= 0.0),
        );
    }
    let q = clamp((p * 0.5 + vec2f(0.5)) * (OCT_STEPS - 1.0), vec2f(0.0), vec2f(OCT_STEPS - 1.0));
    return floor(q.x + 0.5) * OCT_STEPS + floor(q.y + 0.5);
}

// The inverse of `pack_octahedral`.
//
// A lane written by a pixel that found no surface decodes to `+Z`, which is
// what the kernel writes there.
fn unpack_octahedral(packed: f32) -> vec3f {
    let combined = max(packed, 0.0);
    let qx = floor(combined / OCT_STEPS);
    let qy = combined - qx * OCT_STEPS;
    let e = (vec2f(qx, qy) / (OCT_STEPS - 1.0)) * 2.0 - vec2f(1.0);

    let z = 1.0 - abs(e.x) - abs(e.y);
    var v = vec3f(e, z);
    if z < 0.0 {
        // The inverse of the fold: the lower hemisphere was mapped out across
        // the octahedron's edges, so it comes back the same way.
        v = vec3f(
            (1.0 - abs(e.y)) * select(-1.0, 1.0, e.x >= 0.0),
            (1.0 - abs(e.x)) * select(-1.0, 1.0, e.y >= 0.0),
            z,
        );
    }
    if dot(v, v) <= 0.0 {
        return vec3f(0.0, 0.0, 1.0);
    }
    return normalize(v);
}
