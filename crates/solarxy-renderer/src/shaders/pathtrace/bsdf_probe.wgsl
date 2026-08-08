// The BSDF probe: a material and a direction in, what the lobes answered out.
//
// Composed after the traversal, the atlas, the material fragment, the sampler and
// the BSDF. It traverses nothing and reads no camera, so a wrong answer here is
// the material response and cannot be anything the scene explains.
//
// Two modes, selected by pipeline-overridable constant rather than by a uniform
// branch, so each specializes at pipeline creation and the unused half folds away.
//
//   sample    draw a direction, and report it with the density and the throughput
//   evaluate  take a direction, and report the density and the throughput for it
//
// Together they are what the histogram comparison needs. Binning directions from
// the first and asking the second for the density at those bins compares a sampler
// against an independently written density; comparing a sampler to itself would
// pass no matter how wrong both were.
//
// Both modes write the same three blocks, so the host reads one shape and the two
// are directly comparable.

// 48 bytes. Three sixteen-byte blocks with the scalar tail packed into the third,
// so nothing needs a pad; `uniform_layout.rs` carries a row for it because a
// probe's request struct crosses the boundary like any other record, and the
// material probe's tap taught that lesson the loud way.
struct BsdfTap {
    // Outgoing direction, TANGENT space, z up. `w` unused.
    wo: vec4f,
    // Incident direction, tangent space. Read in evaluate mode only.
    wi: vec4f,
    // Index into the material pool.
    material: u32,
    // Which sample of `strata` this invocation draws, in sample mode.
    sample_index: u32,
    // How many samples the batch contains, which is the count the stratified
    // sampler divides its domain into. Zero or one asks for white noise.
    strata: u32,
    // Fixed across a batch, so every sample in it shares one stratified
    // sequence and only `sample_index` moves.
    seed: u32,
}

// The probe's own group. Group 0 is the scene and group 2 is the atlas, both bound
// through the layouts the kernel uses; this takes group 1, the accumulation
// group's number, which no probe binds.
@group(1) @binding(0) var<storage, read> bsdf_taps: array<BsdfTap>;
@group(1) @binding(1) var<storage, read_write> bsdf_results: array<vec4f>;

override BSDF_TAP_WIDTH: u32 = 64u;
override BSDF_RESULT_WIDTH: u32 = 3u;
override BSDF_PROBE_MODE: u32 = 0u;

const BSDF_PROBE_SAMPLE: u32 = 0u;
const BSDF_PROBE_EVAL: u32 = 1u;

@compute @workgroup_size(8, 8, 1)
fn bsdf_probe(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.y * BSDF_TAP_WIDTH + gid.x;
    if index >= arrayLength(&bsdf_taps) {
        return;
    }
    let tap = bsdf_taps[index];
    if tap.material >= arrayLength(&materials) {
        return;
    }
    let m = materials[tap.material];
    let out = index * BSDF_RESULT_WIDTH;

    // The probe supplies the geometry itself: a flat surface whose normal is +z
    // and whose tangent is +x. That fixes the frame, so a direction handed in as
    // tangent space and a direction handed back the same way round-trip exactly
    // through an orthonormal rotation, and an anisotropic lobe has a real tangent
    // to orient against.
    //
    // The uv set is zero. A probe scene carries no textures, so every descriptor
    // is unused and every tap falls back to its factor, which is what makes the
    // answer a property of the lobes rather than of the atlas.
    let s = material_sample(m, vec4f(0.0));
    let surf = surface_from(m, s, vec3f(0.0, 0.0, 1.0), vec4f(1.0, 0.0, 0.0, 1.0), 1.0);

    let wo_ts = normalize(tap.wo.xyz);
    let world_wo = surf.frame * wo_ts;

    // Recomputed rather than returned by the sampler: the same surface and the
    // same outgoing direction give the same weights, and threading them out of
    // `bsdf_sample` would widen its record for a diagnostic.
    let wo_c = normalize(surf.inv_clearcoat_frame * world_wo);
    let w = get_lobe_weights(wo_ts, wo_c, surf);

    if BSDF_PROBE_MODE == BSDF_PROBE_SAMPLE {
        // The pixel is fixed and only the sample index moves, which is the
        // stratified sampler's contract: one scramble per batch, one stratum per
        // sample. Seeding from the invocation index instead would give every
        // sample its own scramble and stratify nothing.
        var rng = rng_init(vec2u(0u), tap.sample_index, tap.strata, tap.seed);
        let rec = bsdf_sample(world_wo, surf, &rng);
        let wi_ts = surf.inv_frame * rec.direction;

        bsdf_results[out + 0u] = vec4f(wi_ts, rec.pdf);
        bsdf_results[out + 1u] = vec4f(rec.color, f32(rec.lobe));
    } else {
        let wi_ts = normalize(tap.wi.xyz);
        let e = bsdf_result(world_wo, surf.frame * wi_ts, surf);

        bsdf_results[out + 0u] = vec4f(wi_ts, e.pdf);
        // No lobe: an evaluation is not attributable to one, which is the whole
        // point of a mixture density.
        bsdf_results[out + 1u] = vec4f(e.color, f32(LOBE_NONE));
    }

    // The selection distribution, both modes. The histogram needs it to know how
    // much of the density each lobe was responsible for, and a zero here explains
    // an empty bin without anyone having to guess.
    bsdf_results[out + 2u] = vec4f(w.diffuse, w.specular, w.transmission, w.clearcoat);
}
