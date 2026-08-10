// The path kernel: camera rays, the BSDF, next-event estimation with multiple
// importance sampling, and an environment.
//
// Composed over the traversal, the atlas, the material fragment, the sampler,
// the lobes, the environment, the lights and the camera.
//
// This grew from the furnace kernel rather than replacing it, and the white
// furnace test still drives it: set both environment colours the same and every
// light out of the scene, and a surface lit by a uniform environment must
// return exactly what reached it. That test now measures the integrator rather
// than the material alone, which is the point of not having started over. The
// stage-four albedo table reproducing through this loop is the evidence that
// adding a second density did not disturb the first.
//
// What is still missing is accumulation: the sample loop lives inside the
// kernel and there is no history texture, so a long render is paced by tiling.
// Replacing that loop with a ping-ponged accumulator is the next stage, and the
// shape here is what it replaces rather than what it starts over from.

@group(1) @binding(0) var path_out: texture_storage_2d<rgba32float, write>;
// Albedo in `rgb` and the world normal folded into `a`. What the surface looked
// like, as opposed to what it returned.
@group(1) @binding(1) var path_aux: texture_storage_2d<rgba32float, write>;

// What one path saw, beyond the light it carried back.
struct PathResult {
    radiance: vec3f,
    // The base colour at the first hit that was not a mirror, and its world
    // normal. A denoiser steers by these: they are the same surface the noise
    // sits on and they arrive free of it, so an edge in them is an edge worth
    // preserving and an edge in the colour alone is probably noise.
    albedo: vec3f,
    normal: vec3f,
    // Whether anything was found at all. A ray that left into the sky has no
    // surface to describe, and writing zeroes for it would tell a denoiser the
    // sky is a black wall facing nowhere.
    hit: bool,
}

// Which surfaces are worth describing.
//
// A mirror shows what is behind the camera rather than what it is made of, so
// its own albedo says nothing about the pixel and its normal steers a filter
// toward an edge that is not there. The auxiliary channels are recorded at the
// first hit rough enough to look like itself.
const AOV_MIN_ROUGHNESS: f32 = 0.05;

// Quantization steps per octahedral component. Two of these multiply out to
// 2^24 - 1, which is the largest integer an f32 represents exactly, so the pair
// packs into one lane and comes back unchanged.
const OCT_STEPS: f32 = 4096.0;

// The world normal packed into one float, octahedrally.
//
// Two components rather than three, because the storage-texture budget is four
// and the accumulator needs two of them: the albedo takes three lanes and this
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
// other half.
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

// One camera ray carried until it leaves, is absorbed, or runs out of budget.
fn path_trace(pixel: vec2u, sample_index: u32) -> PathResult {
    var out: PathResult;
    out.radiance = vec3f(0.0);
    out.albedo = vec3f(0.0);
    out.normal = vec3f(0.0);
    out.hit = false;

    var rng = rng_init(pixel, sample_index, params.samples, params.seed);

    // A tent rather than a box, and it costs the same. A box weights the corner
    // of a pixel as heavily as its middle; a tent falls off toward the edge and
    // overlaps its neighbours, which is what reconstructs an edge instead of
    // stair-stepping it.
    let jitter = tent_jitter(rand2_strat(&rng, RNG_DIM_PIXEL_JITTER));
    let primary = camera_ray(pixel, jitter, rand2_strat(&rng, RNG_DIM_APERTURE));
    var origin = primary.origin;
    var direction = primary.direction;

    var radiance = vec3f(0.0);
    var throughput = vec3f(1.0);
    var transmissive_left = params.transmissive_bounces;

    // The density of the scatter that produced the current ray, which is what
    // radiance found by chance is weighted against. Negative means the ray came
    // from the camera and nothing could have found it any other way, so it is
    // taken at full strength; see `scatter_mis_weight`.
    var scatter_pdf = -1.0;

    // Beer-Lambert needs the distance travelled inside a medium, which is only
    // known at the *next* hit, so the medium the last transmissive scatter
    // entered is carried across one bounce.
    var in_medium = false;
    var medium_color = vec3f(1.0);
    var medium_distance = 0.0;

    for (var bounce = 0u; bounce < params.bounces; bounce += 1u) {
        let hit = trace_closest(origin, direction, 1e30);
        let scene_t = select(1e30, hit.t, hit.hit);

        // Lights are not geometry and are not in the hierarchy, so a ray finds
        // one only because this asks. Tested against the surface distance so a
        // panel behind a wall stays behind it.
        let light_hit = intersect_lights(origin, direction, scene_t, params.light_count);
        if light_hit.hit {
            var carried = throughput;
            if in_medium {
                carried *= transmission_attenuation(light_hit.t, medium_color, medium_distance);
            }
            radiance += carried
                * light_hit.radiance
                * scatter_mis_weight(scatter_pdf, light_hit.pdf, params.light_count);
            // An area light is an opaque emitter: the path ends on it.
            break;
        }

        if !hit.hit {
            // Weighted, because next-event estimation samples the environment
            // too and this is the same radiance arrived at the other way.
            radiance += throughput
                * env_radiance(direction)
                * scatter_mis_weight(scatter_pdf, env_pdf(direction), params.light_count);
            break;
        }

        // Attenuation is charged for the segment just crossed, before anything
        // at the far surface is evaluated.
        if in_medium {
            throughput *= transmission_attenuation(hit.t, medium_color, medium_distance);
        }

        let inst = instances[hit.instance];
        let m = materials[inst.material_base];
        let uv = shading_uv(hit);
        let sample = material_sample(m, uv);

        let geo_ws = world_normal(inst, hit.geo_normal);
        let side = select(-1.0, 1.0, dot(geo_ws, -direction) > 0.0);

        // Coverage is treated as a pass-through rather than as a probability: a
        // masked-out point is simply not there, and the ray resumes past it
        // without scattering. It still spends a bounce, which a stochastic
        // treatment would not; that treatment belongs with the integrator and
        // has a reserved sampling dimension waiting for it.
        if !material_alpha_passes(m, sample) {
            origin = step_ray_origin(origin, direction, -geo_ws * side, hit.t);
            continue;
        }

        let normal_ws = world_normal(inst, shading_normal(hit));
        let tangent_obj = shading_tangent(hit);
        var tangent_ws = vec4f(0.0);
        if tangent_obj.w != 0.0 {
            tangent_ws = vec4f(world_tangent(inst, tangent_obj.xyz), tangent_obj.w);
        }

        let surf = surface_from(m, sample, normal_ws, tangent_ws, side);

        // The auxiliary channels, at the first surface rough enough to look
        // like itself. Recorded here rather than at the first hit outright
        // because a mirror shows what is behind the camera, so its albedo says
        // nothing about the pixel and its normal would steer a denoiser toward
        // an edge that is not there.
        if !out.hit && max(surf.alpha.x, surf.alpha.y) >= AOV_MIN_ROUGHNESS {
            out.hit = true;
            out.albedo = surf.color;
            out.normal = normal_ws;
        }

        // Emission is added unweighted, and that is the recorded limitation:
        // emissive geometry is not in the importance-sampling scheme, so a path
        // finds a glowing surface only by scattering onto it. Making it a light
        // means sampling triangles by area and power, which is its own piece of
        // work.
        radiance += throughput * surf.emission;

        // The connection to a light, from a point already stepped off the
        // surface so the shadow ray does not start inside it.
        let shading_origin = step_ray_origin(origin, direction, geo_ws * side, hit.t);
        radiance += throughput * direct_light(
            -direction,
            surf,
            geo_ws * side,
            shading_origin,
            params.light_count,
            params.transmissive_bounces,
            &rng,
        );

        let rec = bsdf_sample(-direction, surf, &rng);
        if is_terminating_scatter(rec) {
            break;
        }

        if rec.transmissive {
            // The explicit budget, rather than handing a bounce back the way
            // the source does. Running out ends the path instead of turning
            // glass opaque, because an opaque answer would be a wrong colour
            // where this is an honestly incomplete one.
            if transmissive_left == 0u {
                break;
            }
            transmissive_left -= 1u;
            // Entering a solid volume starts attenuating; a thin wall has no
            // interior, and leaving one stops.
            in_medium = !surf.thin_film && side > 0.0;
            medium_color = surf.attenuation_color;
            medium_distance = surf.attenuation_distance;
        }

        let survival = russian_roulette(throughput, rec.color, rec.pdf, bounce, &rng);
        if survival <= 0.0 {
            break;
        }
        throughput *= survival * rec.color / rec.pdf;
        if !is_finite_v3(throughput) {
            break;
        }
        scatter_pdf = rec.pdf;

        // Step off the surface on the side the new direction leaves by, which
        // is the far side for a transmitted ray.
        let offset_dir = normal_ws * select(-1.0, 1.0, dot(rec.direction, normal_ws) > 0.0);
        origin = step_ray_origin(origin, direction, offset_dir, hit.t);
        direction = rec.direction;
        rng_next_bounce(&rng);
    }

    out.radiance = radiance;
    return out;
}

@compute @workgroup_size(8, 8, 1)
fn path_main(@builtin(global_invocation_id) gid: vec3u) {
    // Two bounds checks rather than one: the dispatch is rounded up to whole
    // workgroups, and the tile can be a partial one at the image edge.
    if gid.x >= params.tile_size.x || gid.y >= params.tile_size.y {
        return;
    }
    let pixel = params.tile_offset + gid.xy;
    if pixel.x >= params.resolution.x || pixel.y >= params.resolution.y {
        return;
    }

    // The sample loop is inside the kernel and there is no accumulation
    // texture, which is the one place this deliberately differs from what the
    // finished kernel will do: ping-ponged accumulation is the next stage's,
    // and a harness that owned one would have to be unwound to get there. Long
    // renders are paced by tiling instead, which is what the tile uniforms
    // already exist for.
    let samples = max(1u, params.samples);
    var accumulated = vec3f(0.0);
    var albedo = vec3f(0.0);
    var normal = vec3f(0.0);
    var described = 0u;
    for (var s = 0u; s < samples; s += 1u) {
        let result = path_trace(pixel, s);
        accumulated += result.radiance;
        if result.hit {
            albedo += result.albedo;
            normal += result.normal;
            described += 1u;
        }
    }

    textureStore(path_out, vec2i(pixel), vec4f(accumulated / f32(samples), 1.0));

    // Averaged over the samples that found a surface rather than over all of
    // them, so a pixel on the silhouette of an object reports that object's
    // colour at the strength it covers the pixel, not diluted by the sky behind
    // it. The normal is renormalized after averaging, which is what makes the
    // average of several samples across an edge point between them instead of
    // shrinking toward zero.
    var aux = vec4f(0.0, 0.0, 0.0, pack_octahedral(vec3f(0.0, 0.0, 1.0)));
    if described > 0u {
        let averaged = normal / f32(described);
        var n = vec3f(0.0, 0.0, 1.0);
        if dot(averaged, averaged) > 1e-12 {
            n = normalize(averaged);
        }
        aux = vec4f(albedo / f32(described), pack_octahedral(n));
    }
    textureStore(path_aux, vec2i(pixel), aux);
}
