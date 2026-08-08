// The material probe: a material index and a uv set in, the record as the
// shader read it out, plus the surface it resolved to.
//
// Composed after the traversal, which declares the record and binds the pool,
// and after the atlas and the material fragment, which resolve the taps. It
// traverses nothing and reads no camera, so a wrong answer here is a layout or
// a sampling disagreement and cannot be anything the scene explains.
//
// The thing it exists to catch is invisible from Rust and invisible to the size
// guard: nine sixteen-byte blocks are nine chances to transpose two of them, and
// a transposition changes no size on either side. So the readout is not a
// summary. It echoes every scalar in declaration order, which makes the
// comparison against the Rust struct a byte-for-byte one.

// 32 bytes. The tail is three scalars rather than a `vec3u`, and that is not a
// style choice: WGSL aligns a three-component vector to 16, which would push it
// to offset 32 and make the struct 48 against the Rust twin's 32. The probe
// built to catch exactly that mistake in the material record made it here first,
// so `uniform_layout.rs` now carries a row for this struct too.
struct MaterialTap {
    // The vertex uv set: uv0 in `xy`, uv1 in `zw`.
    uv: vec4f,
    // Index into the material pool.
    material: u32,
    pad: array<u32, 3>,
}

// The probe's own group. Group 0 is the scene, which the traversal declares, and
// group 2 is the atlas, which the atlas fragment declares; this takes group 1,
// the accumulation group's number, which no probe binds.
@group(1) @binding(0) var<storage, read> material_taps: array<MaterialTap>;
@group(1) @binding(1) var<storage, read_write> material_results: array<vec4f>;

// Taps per row of the dispatch grid, and vec4s written per tap. The host owns
// both so the two sides cannot drift.
override MATERIAL_TAP_WIDTH: u32 = 64u;
override MATERIAL_RESULT_WIDTH: u32 = 12u;

@compute @workgroup_size(8, 8, 1)
fn material_probe(@builtin(global_invocation_id) gid: vec3<u32>) {
    let index = gid.y * MATERIAL_TAP_WIDTH + gid.x;
    if index >= arrayLength(&material_taps) {
        return;
    }
    let tap = material_taps[index];
    if tap.material >= arrayLength(&materials) {
        return;
    }
    let m = materials[tap.material];
    let out = index * MATERIAL_RESULT_WIDTH;

    // The record, verbatim and in declaration order. A block written here in
    // the wrong order is the same mistake as a block declared in the wrong
    // order, so the readout deliberately does no rearranging of its own.
    material_results[out + 0u] = m.base_color;
    material_results[out + 1u] = vec4f(m.emissive, m.emissive_strength);
    material_results[out + 2u] = vec4f(m.attenuation_color, m.attenuation_distance);
    material_results[out + 3u] = vec4f(m.sheen_color, m.sheen_roughness);
    material_results[out + 4u] = vec4f(m.specular_color, m.specular_intensity);
    material_results[out + 5u] = vec4f(m.metallic, m.roughness, m.ior, m.transmission);
    material_results[out + 6u] = vec4f(
        m.thickness,
        m.clearcoat,
        m.clearcoat_roughness,
        m.anisotropy,
    );
    material_results[out + 7u] = vec4f(
        m.anisotropy_rotation,
        m.iridescence,
        m.iridescence_ior,
        m.iridescence_thickness_min,
    );
    // The flags word crosses as its own bits rather than as a rounded float: a
    // shading model of 7 in bits 2 to 5 is 28, which a float carries exactly,
    // but the reserved bits above it would not survive a wider value.
    material_results[out + 8u] = vec4f(
        m.iridescence_thickness_max,
        m.occlusion_strength,
        m.alpha_cutoff,
        bitcast<f32>(m.flags),
    );

    // Then the surface the material fragment resolved, which is what a lobe
    // will actually read.
    let s = material_sample(m, tap.uv);
    material_results[out + 9u] = s.base_color;
    material_results[out + 10u] = vec4f(
        s.metallic,
        s.roughness,
        s.occlusion,
        select(0.0, 1.0, s.has_normal_map),
    );
    material_results[out + 11u] = vec4f(
        s.emissive,
        select(0.0, 1.0, material_alpha_passes(m, s)),
    );
}
