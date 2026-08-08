// The furnace kernel: camera rays, the BSDF, and a constant environment.
//
// Composed over the traversal, the atlas, the material fragment, the sampler, the
// lobes and the camera.
//
// **This is not the path tracer either, and it is not a stand-in for one.** It has
// no light sampling, no next-event estimation, no multiple importance sampling and
// no accumulation buffer; every one of those arrives in its own stage. What it is
// is the smallest thing that can drive the BSDF end to end, which the material
// response needs for two reasons the probe cannot serve.
//
// The first is the white furnace test as a picture. A surface lit by a uniform
// environment should return exactly what reached it, so an energy-conserving
// material vanishes into its background; anything visible is a deficit or an excess,
// and where it sits in a roughness-by-metalness grid says which term is responsible.
// The probe measures that as a number, which is the assertion; this is what a person
// looks at.
//
// The second is that a lobe can be individually correct and collectively wrong. The
// probe drives one surface with one outgoing direction. Only a bounce loop exercises
// the frame construction on curved geometry, the ray offset, the transmissive
// budget and the volume attenuation, and those are where a plausible image comes
// apart.
//
// It is also deliberately the shape the real kernel grows into, so the accumulation
// stage replaces the sample loop rather than starting over.

@group(1) @binding(0) var furnace_out: texture_storage_2d<rgba32float, write>;

// The stand-in environment: two colours blended by the world up axis.
//
// Its own uniform rather than four more floats on `TraceParams`, because it is
// exactly the thing real environment sampling replaces, and a field documented in
// the shipped per-dispatch struct would have to be deleted rather than superseded.
//
// Setting both colours the same is the furnace configuration. Setting them
// differently makes a sphere legible, which is what the ignored render wants: a
// perfectly conserving material under a genuinely uniform environment is invisible,
// which is the correct answer and a poor photograph.
struct FurnaceParams {
    env_up: vec4f,
    env_down: vec4f,
}

@group(3) @binding(2) var<uniform> furnace: FurnaceParams;

fn furnace_environment(dir: vec3f) -> vec3f {
    let t = saturate(dir.y * 0.5 + 0.5);
    return mix(furnace.env_down.rgb, furnace.env_up.rgb, t);
}

// One camera ray carried until it leaves, is absorbed, or runs out of budget.
fn furnace_trace(pixel: vec2u, sample_index: u32) -> vec3f {
    var rng = rng_init(pixel, sample_index, params.samples, params.seed);

    let jitter = rand2_strat(&rng, RNG_DIM_PIXEL_JITTER);
    let primary = camera_ray(pixel, jitter);
    var origin = primary.origin;
    var direction = primary.direction;

    var radiance = vec3f(0.0);
    var throughput = vec3f(1.0);
    var transmissive_left = params.transmissive_bounces;

    // Beer-Lambert needs the distance travelled inside a medium, which is only
    // known at the *next* hit, so the medium the last transmissive scatter entered
    // is carried across one bounce.
    var in_medium = false;
    var medium_color = vec3f(1.0);
    var medium_distance = 0.0;

    for (var bounce = 0u; bounce < params.bounces; bounce += 1u) {
        let hit = trace_closest(origin, direction, 1e30);
        if !hit.hit {
            radiance += throughput * furnace_environment(direction);
            break;
        }

        // Attenuation is charged for the segment just crossed, before anything at
        // the far surface is evaluated.
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
        // masked-out point is simply not there, and the ray resumes past it without
        // scattering. It still spends a bounce, which a stochastic treatment would
        // not; that treatment belongs with the integrator and has a reserved
        // sampling dimension waiting for it.
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
        radiance += throughput * surf.emission;

        let rec = bsdf_sample(-direction, surf, &rng);
        if is_terminating_scatter(rec) {
            break;
        }

        if rec.transmissive {
            // The explicit budget, rather than handing a bounce back the way the
            // source does. Running out ends the path instead of turning glass
            // opaque, because an opaque answer would be a wrong colour where this
            // is an honestly incomplete one.
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

        // Step off the surface on the side the new direction leaves by, which is
        // the far side for a transmitted ray.
        let offset_dir = normal_ws * select(-1.0, 1.0, dot(rec.direction, normal_ws) > 0.0);
        origin = step_ray_origin(origin, direction, offset_dir, hit.t);
        direction = rec.direction;
        rng_next_bounce(&rng);
    }

    return radiance;
}

@compute @workgroup_size(8, 8, 1)
fn furnace_main(@builtin(global_invocation_id) gid: vec3u) {
    // Two bounds checks rather than one: the dispatch is rounded up to whole
    // workgroups, and the tile can be a partial one at the image edge.
    if gid.x >= params.tile_size.x || gid.y >= params.tile_size.y {
        return;
    }
    let pixel = params.tile_offset + gid.xy;
    if pixel.x >= params.resolution.x || pixel.y >= params.resolution.y {
        return;
    }

    // The sample loop is inside the kernel and there is no accumulation texture,
    // which is the one place this deliberately differs from what the real kernel
    // will do: ping-ponged accumulation is the next stage's, and a harness that
    // owned one would have to be unwound to get there. Long renders are paced by
    // tiling instead, which is what the tile uniforms already exist for.
    let samples = max(1u, params.samples);
    var accumulated = vec3f(0.0);
    for (var s = 0u; s < samples; s += 1u) {
        accumulated += furnace_trace(pixel, s);
    }

    textureStore(furnace_out, vec2i(pixel), vec4f(accumulated / f32(samples), 1.0));
}
