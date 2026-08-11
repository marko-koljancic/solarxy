// The path kernel: camera rays, the BSDF, next-event estimation with multiple
// importance sampling, and an environment.
//
// Composed over the traversal, the atlas, the material fragment, the sampler,
// the lobes, the environment, the lights, the camera and the auxiliary packing.
//
// This grew from the furnace kernel rather than replacing it, and the white
// furnace test still drives it: set both environment colours the same and every
// light out of the scene, and a surface lit by a uniform environment must
// return exactly what reached it. That test now measures the integrator rather
// than the material alone, which is the point of not having started over. The
// stage-four albedo table reproducing through this loop is the evidence that
// adding a second density did not disturb the first.
//
// Each dispatch draws a slice of one run's samples and folds them into a
// ping-ponged running mean, so a long render is paced by chunking rather than
// by asking for every sample at once. What one dispatch draws, where its slice
// starts, and how many the run converges to are three separate fields on the
// per-dispatch uniform for exactly that reason.

@group(1) @binding(0) var path_out: texture_storage_2d<rgba32float, write>;
// Albedo in `rgb` and the world normal folded into `a`. What the surface looked
// like, as opposed to what it returned.
@group(1) @binding(1) var path_aux: texture_storage_2d<rgba32float, write>;
// The other half of the ping-pong: what the run has averaged so far. Read only
// when `params.sample_base` says there is something in them, which is why
// nothing has to clear them before a run.
//
// A read side rather than reading `path_out` back: WebGPU grants read-write
// storage access to `r32uint`, `r32sint` and `r32float`, and this is none of
// those.
@group(1) @binding(2) var path_prev: texture_storage_2d<rgba32float, read>;
@group(1) @binding(3) var path_aux_prev: texture_storage_2d<rgba32float, read>;

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

// The octahedral packing the auxiliary lane uses lives in `aov.wgsl`, composed
// under this one, because the denoiser that reads the lane needs the same text.

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

    // Everything the path had gathered when it first scattered, which is what
    // the firefly clamp is forbidden to touch. See `clamp_firefly`.
    var direct = vec3f(0.0);
    var scattered = false;

    // The density of the scatter that produced the current ray, which is what
    // radiance found by chance is weighted against. Negative means the ray came
    // from the camera and nothing could have found it any other way, so it is
    // taken at full strength; see `scatter_mis_weight`.
    var scatter_pdf = -1.0;

    // How far this ray has flown since its direction was sampled. Zero except
    // past a masked or blended pass-through, which re-origins the ray
    // mid-flight; the light density the MIS weight divides by is measured from
    // the sample point, so the distance already crossed has to ride along.
    var flight = 0.0;

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
        let light_hit = intersect_lights(origin, direction, scene_t, params.light_count, flight);
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
            flight += hit.t;
            origin = step_ray_origin(origin, direction, -geo_ws * side, hit.t);
            continue;
        }

        // A blended surface resolves its coverage by probability rather than
        // by blending: with `1 - alpha` the ray is simply not stopped here.
        // The expectation matches the `1 - opacity` the shadow walk charges
        // for the same surface, which is what keeps the two techniques
        // telling one story about whether a pane blocks light; before this
        // arm existed they disagreed, and glass authored as blended alpha
        // traced opaque. The draw adds the primitive index, which is the open
        // range the alpha-test dimension owns: coverage tests do not advance
        // the bounce, so the index is what decorrelates successive panes
        // along one path. The crossing is charged to the transmissive budget
        // exactly as the shadow walk charges it; a ray out of budget shades
        // the pane opaque where a shadow ray out of budget reports it
        // blocked.
        if material_alpha_mode(m) == MAT_ALPHA_BLEND
            && transmissive_left > 0u
            && rand1_strat(&rng, RNG_DIM_ALPHA_TEST + hit.prim) >= sample.base_color.a {
            transmissive_left -= 1u;
            flight += hit.t;
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

        // The path has left its first surface, so everything gathered up to
        // here is direct: the emitter or environment a camera ray landed on,
        // this surface's own emission, and the connection this surface made to
        // a light. All three are found by aiming rather than by luck, and none
        // of them is what a clamp exists to bound.
        if !scattered {
            direct = radiance;
            scattered = true;
        }

        // Step off the surface on the side the new direction leaves by, which
        // is the far side for a transmitted ray.
        let offset_dir = normal_ws * select(-1.0, 1.0, dot(rec.direction, normal_ws) > 0.0);
        origin = step_ray_origin(origin, direction, offset_dir, hit.t);
        direction = rec.direction;
        flight = 0.0;
        rng_next_bounce(&rng);
    }

    out.radiance = direct + clamp_firefly(radiance - direct);
    return out;
}

// Scales one sample's indirect contribution back to `params.firefly_clamp` if
// it exceeds it, leaving its colour alone.
//
// A firefly is one sample in thousands that found a small bright source through
// a scatter whose density said it almost certainly would not, so the estimator
// charges it an enormous weight to stay unbiased. Averaging removes it
// eventually; at any sample count a person will wait for, it is a white speck
// that never fades. Clamping it is deliberately biased and the bias is bounded
// by what it removes, which is the trade the alternative -- a correct image
// nobody renders long enough to see -- does not offer.
//
// Only the indirect part is bounded, and that is what makes the default safe to
// leave on: a light panel viewed directly, or a sun in the environment, is
// radiance the camera ray aimed at rather than stumbled onto, and clamping it
// would dim the authored scene rather than remove noise from it.
fn clamp_firefly(v: vec3f) -> vec3f {
    if params.firefly_clamp <= 0.0 {
        return v;
    }
    let l = luminance(v);
    if l <= params.firefly_clamp {
        return v;
    }
    return v * (params.firefly_clamp / l);
}

@compute @workgroup_size(8, 8, 1)
fn path_main(@builtin(global_invocation_id) gid: vec3u) {
    // Two bounds checks rather than one: the dispatch is rounded up to whole
    // workgroups, and the tile can be a partial one at the image edge.
    if gid.x >= params.tile_size.x || gid.y >= params.tile_size.y {
        return;
    }
    // Which pixel of the **whole image** this invocation is. Everything that
    // depends on where the pixel sits in the picture reads this: the camera ray
    // through it, and the sampler's per-pixel decorrelation, so a tile draws the
    // same samples it would have drawn as part of an untiled render.
    let pixel = params.tile_offset + gid.xy;
    if pixel.x >= params.resolution.x || pixel.y >= params.resolution.y {
        return;
    }
    // Where it is **stored**, which is tile-local and not the same thing.
    //
    // This is what makes a tile cost a tile-sized target rather than an
    // image-sized one, and at eight thousand pixels square the difference is
    // two gigabytes of float storage against thirty-two megabytes. An untiled
    // render has a zero offset and the two coincide, which is why nothing
    // before this needed to tell them apart.
    let coord = vec2i(gid.xy);

    // This dispatch's slice of the run. `chunk` of zero means the whole thing,
    // which is what a one-shot dispatch wants and what every caller written
    // before there was an accumulator gets by default.
    let total = max(1u, params.samples);
    var draw = params.chunk;
    if draw == 0u {
        draw = total;
    }

    // The sample index is global, not local to the chunk. That is the whole
    // reason `sample_base` exists on the sampling side: the stratified sampler
    // divides one domain of `total` samples, so a chunk that re-drew indices
    // from zero would re-draw the same strata and the mean would stop moving.
    var sum = vec3f(0.0);
    var albedo = vec3f(0.0);
    var normal = vec3f(0.0);
    var described = 0u;
    for (var s = 0u; s < draw; s += 1u) {
        let result = path_trace(pixel, params.sample_base + s);
        sum += result.radiance;
        if result.hit {
            albedo += result.albedo;
            normal += result.normal;
            described += 1u;
        }
    }

    let base = f32(params.sample_base);
    let drawn = f32(draw);
    let fresh = params.sample_base == 0u;

    // What the run knew before this dispatch. Read once, because the read slot
    // is a storage texture and this is the only pixel of it anything here
    // touches.
    var prev_color = vec4f(0.0);
    var prev_aux = vec4f(0.0);
    if !fresh {
        prev_color = textureLoad(path_prev, coord);
        prev_aux = textureLoad(path_aux_prev, coord);
    }

    // An explicit running mean rather than a running sum, so the texture holds
    // a displayable image at every sample count and the resolve is a format
    // conversion rather than a division that has to be told the count.
    var color = sum / drawn;
    if !fresh {
        color = (prev_color.rgb * base + sum) / (base + drawn);
    }

    // How many samples across the whole run have found a surface worth
    // describing, carried in the colour target's alpha lane.
    //
    // That lane was a constant one and nothing read it: the resolve writes its
    // own alpha, because a partially transparent scene handed to the composite
    // would darken against whatever the target held. Spending it here is what
    // makes the auxiliary mean **exact** across chunks rather than an
    // approximation. The alternative was a fifth storage texture, and core
    // WebGPU grants four.
    //
    // It has to be a count of described samples and not of samples: a pixel on
    // a silhouette describes a surface in some samples and not others, and
    // weighting a chunk that described nothing as if it had described black is
    // exactly the drift this replaced.
    let described_before = select(0.0, prev_color.a, !fresh);
    let described_now = described_before + f32(described);
    textureStore(path_out, coord, vec4f(color, described_now));

    // The auxiliary channels: the albedo of the first surface rough enough to
    // look like itself, and its world normal.
    //
    // The normal is why this cannot be a plain running mean of the stored
    // value. A unit vector folded octahedrally into one lane comes back as a
    // direction and not as the magnitude a weighted mean needs, so the merge
    // reduces this chunk to its own mean first and then weights the two means
    // by how many samples each describes. With the count above, that is the
    // same number the one-shot dispatch computes.
    var chunk_albedo = vec3f(0.0);
    var chunk_normal = vec3f(0.0, 0.0, 1.0);
    if described > 0u {
        chunk_albedo = albedo / f32(described);
        // Renormalized after averaging, which is what makes several samples
        // across an edge point between the two surfaces instead of shrinking
        // toward zero.
        if dot(normal, normal) > 1e-12 {
            chunk_normal = normalize(normal);
        }
    }

    if described == 0u {
        // This chunk says nothing about a surface. Fresh, that is the sentinel
        // -- a lane written by a pixel that found nothing decodes to `+Z`, not
        // to a black wall facing nowhere. Otherwise it is what the run already
        // knows, unchanged.
        let keep = select(prev_aux, vec4f(chunk_albedo, pack_octahedral(chunk_normal)), fresh);
        textureStore(path_aux, coord, keep);
        return;
    }
    if described_before <= 0.0 {
        // The first surface this pixel has found, however many dispatches in.
        textureStore(path_aux, coord, vec4f(chunk_albedo, pack_octahedral(chunk_normal)));
        return;
    }

    let now = f32(described);
    let merged_albedo = (prev_aux.rgb * described_before + chunk_albedo * now) / described_now;
    var merged_normal = unpack_octahedral(prev_aux.a) * described_before + chunk_normal * now;
    if dot(merged_normal, merged_normal) > 1e-12 {
        merged_normal = normalize(merged_normal);
    } else {
        merged_normal = chunk_normal;
    }
    textureStore(path_aux, coord, vec4f(merged_albedo, pack_octahedral(merged_normal)));
}
