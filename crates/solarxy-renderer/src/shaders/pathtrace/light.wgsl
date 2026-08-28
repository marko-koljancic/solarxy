// The light fragment: sampling a light, finding one a scattered ray hit,
// walking a shadow ray through what is transmissive, and the direct-lighting
// estimator that puts the four together. Declares no entry point.
//
// Composed over the traversal (which declares the light record and the scene
// group), the material, the sampler, the BSDF (whose `bsdf_result` this calls)
// and the environment (whose arm shares the estimator's choice).
//
// # Why next-event estimation exists at all
//
// A path that only ever scatters finds a light by accident. For a small or
// distant one that almost never happens, so the image converges as the square
// root of how often it does, which is to say not at all: a one-metre lamp in a
// room is found by maybe one path in ten thousand, and the ones that find it
// arrive carrying ten thousand times the average, which is a white speck. Next
// event estimation asks at every scatter "what if I went straight to a light
// from here", which finds it every time, and the variance collapses.
//
// # Why that is not the whole answer, and what MIS does about it
//
// Sampling the light is exactly wrong for the case the scattering was right
// for. A near-mirror looking at a large area light has a response concentrated
// in a few directions; drawing a point uniformly on the light lands almost
// everywhere the mirror does not reflect, and the one draw that lands in the
// highlight is charged a tiny density and comes back enormous. So each
// technique is good precisely where the other is bad. Multiple importance
// sampling runs both and weights each by how likely it was to have produced the
// direction it produced, so the estimator is as good as whichever technique was
// suited, without knowing in advance which that was.
//
// The weight is the power heuristic with an exponent of two, from Veach and
// Guibas, "Optimally Combining Sampling Techniques for Monte Carlo Rendering",
// SIGGRAPH 1995. It is the same choice the source material makes.
//
// # Delta lights, and the inconsistency that is on purpose
//
// A point, spot or directional light is a mathematical idealization: it has no
// area, so no scattered ray can ever hit it, so there is no second technique to
// combine and its weight is one. Giving it a `radius` makes it sample an extent
// and cast a penumbra, and it is *still* treated as a delta light. That is
// inconsistent and it is the source's treatment, kept: the extent buys the soft
// shadow, while making the light genuinely intersectable would need it to be a
// surface a scattered ray could find and to carry its own solid-angle density.
// Rectangles are that surface, do carry one, and are the only kind here with
// two estimators.

// Which estimator the kernel runs. A pipeline-overridable constant rather than
// a uniform branch, the same way the debug kernel selects its channel: the
// three specialize at pipeline creation and the unused arms fold away.
//
// This exists to be *tested*, not to be configured. Multiple importance
// sampling is a partition of unity across the two techniques, so all three
// modes must converge to the same image, and only the variance may differ. That
// equality is the sharpest available check that the material's one-sample
// mixture density is the right thing to weight a light's density against, which
// is the question this whole stage turns on. Nothing ships a way to choose.
override PATH_ESTIMATOR: u32 = 0u;

// Both techniques, weighted. The only mode a render should ever use.
const ESTIMATOR_MIS: u32 = 0u;
// Scattering alone: no connection to a light, and radiance found by a scattered
// ray is taken whole. Correct, and hopeless at finding a small light.
const ESTIMATOR_SCATTER: u32 = 1u;
// Connections alone: radiance a scattered ray stumbles into is discarded,
// because the connection at the previous bounce already accounted for it.
// Correct, and blind to anything a connection cannot reach.
const ESTIMATOR_NEE: u32 = 2u;

const LIGHT_POINT: u32 = 0u;
const LIGHT_DIRECTIONAL: u32 = 1u;
const LIGHT_SPOT: u32 = 2u;
const LIGHT_RECT: u32 = 3u;

const LIGHT_TWO_SIDED: u32 = 1u;

// How far along a shadow ray counts as "reached the light" rather than "hit
// the light's own surface". Relative, because a shadow ray to a distant light
// is long and an absolute margin would be lost in it.
const SHADOW_REACH: f32 = 0.9999;

// Iterations a shadow ray may spend walking through surfaces before it gives
// up and reports itself blocked.
//
// Separate from the transmissive budget and larger, because a pass-through
// costs a step without being a transmissive event: an alpha-masked leaf is not
// there at all, and a chain of them must not exhaust glass's allowance. Running
// out reports **blocked**, which is the conservative answer: an over-dark
// contact reads as a shadow, and the other way round reads as light leaking
// through a wall, which is the one people file bugs about.
const SHADOW_MAX_STEPS: u32 = 16u;

// The power heuristic with an exponent of two.
//
// `a` is the density of the technique that produced the sample and `b` the
// density the other technique would have given it. Squaring is what makes this
// suppress the low-density technique harder than the balance heuristic does,
// which is the whole point: the samples that blow up are the ones a technique
// was unlikely to have taken.
fn mis_power(a: f32, b: f32) -> f32 {
    let aa = a * a;
    let bb = b * b;
    let total = aa + bb;
    if total <= 0.0 {
        return 0.0;
    }
    return aa / total;
}

// One drawn connection to a light.
struct LightSample {
    // Unit, from the shading point toward the light.
    direction: vec3f,
    // How far a shadow ray must reach. A directional light is effectively
    // unbounded.
    distance: f32,
    // Radiance arriving along `direction`, with distance and cone falloff
    // already applied.
    radiance: vec3f,
    // Solid-angle density, or one for a delta light.
    pdf: f32,
    // Whether multiple importance sampling has a second technique for this
    // light. False for every punctual kind, radius or not.
    delta: bool,
}

// What a ray found when it was tested against the lights themselves.
struct LightHit {
    hit: bool,
    t: f32,
    radiance: vec3f,
    // The solid-angle density the light-sampling technique would have given
    // this direction, which is the other half of the weight.
    pdf: f32,
}

// The windowed inverse-power falloff, from Lagarde and de Rousiers, "Moving
// Frostbite to Physically Based Rendering", equation 26.
//
// The window is what makes a range mean something: a plain inverse square never
// reaches zero, so a light with a stated cutoff would still contribute past it
// and the number would be a lie. The fourth-power window brings it to exactly
// zero at the range with a smooth approach rather than a step.
fn light_distance_falloff(dist: f32, range: f32, decay: f32) -> f32 {
    var falloff = 1.0 / max(pow(dist, decay), 1e-6);
    if range > 0.0 {
        let ratio = dist / range;
        let window = saturate(1.0 - ratio * ratio * ratio * ratio);
        falloff *= window * window;
    }
    return falloff;
}

// The spot cone's falloff between its inner and outer half-angles.
fn light_cone_falloff(cone_cos: f32, penumbra_cos: f32, angle_cos: f32) -> f32 {
    return smoothstep(cone_cos, penumbra_cos, angle_cos);
}

// A point on a disc of `radius` square to `axis`, offset from the origin.
//
// `uv.x` goes under a square root because area grows as the square of the
// radius: taking it linearly would crowd samples at the centre and make the
// penumbra wrong in a way that looks like the light is smaller than it is.
fn light_disc_offset(u: vec3f, v: vec3f, radius: f32, uv: vec2f) -> vec3f {
    let r = radius * sqrt(uv.x);
    let theta = uv.y * 2.0 * PI;
    return u * (r * cos(theta)) + v * (r * sin(theta));
}

// Draws a connection to light `index` from `origin`.
fn sample_light(index: u32, origin: vec3f, uv: vec2f) -> LightSample {
    var out: LightSample;
    out.direction = vec3f(0.0, 1.0, 0.0);
    out.distance = 0.0;
    out.radiance = vec3f(0.0);
    out.pdf = 0.0;
    out.delta = true;

    let light = lights[index];
    let emission = light.color * light.intensity;

    if light.kind == LIGHT_DIRECTIONAL {
        // No position and no falloff: the rays are parallel and the source is
        // infinitely far, so there is nothing for distance to attenuate.
        out.direction = light.axis;
        out.distance = 1e30;
        out.radiance = emission;
        out.pdf = 1.0;
        return out;
    }

    if light.kind == LIGHT_RECT {
        // Uniform over the rectangle. `u` and `v` are the full edges, so the
        // offsets run from minus to plus a half edge.
        let point = light.position + light.u * (uv.x - 0.5) + light.v * (uv.y - 0.5);
        let to_light = point - origin;
        let dist_sq = dot(to_light, to_light);
        if dist_sq <= 0.0 || light.area <= 0.0 {
            return out;
        }
        let dist = sqrt(dist_sq);
        let direction = to_light / dist;
        // The cosine at the light, between the emitting face and the direction
        // the light travels to reach us. `axis` points away from the emitting
        // side, so a receiver in front of the panel sees a positive dot.
        var cos_light = dot(direction, light.axis);
        if (light.flags & LIGHT_TWO_SIDED) != 0u {
            cos_light = abs(cos_light);
        }
        if cos_light <= 1e-6 {
            // Behind the panel, or edge on. Not lit, rather than lit by a
            // density that is about to divide by nearly zero.
            return out;
        }
        out.direction = direction;
        out.distance = dist;
        out.radiance = emission;
        // Area density converted to solid angle: the rectangle subtends less as
        // it recedes and as it turns away, which is exactly these two factors.
        out.pdf = dist_sq / (light.area * cos_light);
        out.delta = false;
        return out;
    }

    if light.kind == LIGHT_SPOT {
        // The extent is a disc inscribed in the cone rather than one at the
        // apex, which is the source's construction: placed a distance
        // `radius / tan(half-angle)` along the beam, a disc of `radius`
        // subtends exactly the cone, so widening the cone widens the penumbra
        // the way a real reflector does.
        var point = light.position;
        if light.radius > 0.0 {
            let cos_outer = clamp(light.cone_cos, -1.0, 1.0);
            let sin_outer = sqrt(max(0.0, 1.0 - cos_outer * cos_outer));
            let tan_outer = sin_outer / max(cos_outer, 1e-4);
            let start = light.radius / max(tan_outer, 1e-4);
            point = light.position - light.axis * start
                + light_disc_offset(light.u, light.v, light.radius, uv);
        }
        let to_light = point - origin;
        let dist = length(to_light);
        if dist <= 0.0 {
            return out;
        }
        let direction = to_light / dist;
        let angle_cos = dot(direction, light.axis);
        let cone = light_cone_falloff(light.cone_cos, light.penumbra_cos, angle_cos);
        out.direction = direction;
        out.distance = dist;
        out.radiance = emission
            * cone
            * light_distance_falloff(dist, light.range, light.decay);
        out.pdf = 1.0;
        return out;
    }

    // Point. A sphere looks like a disc from every direction, so the extent is
    // a disc square to whoever is asking rather than a point on a sphere: the
    // silhouette is what casts the penumbra, and this samples it directly
    // instead of sampling a hemisphere and throwing half of it away.
    var to_light = light.position - origin;
    var dist = length(to_light);
    if dist <= 0.0 {
        return out;
    }
    var direction = to_light / dist;
    if light.radius > 0.0 {
        let basis_u = normalize(cross(select(
            vec3f(1.0, 0.0, 0.0),
            vec3f(0.0, 1.0, 0.0),
            abs(direction.x) > 0.9,
        ), direction));
        let basis_v = cross(direction, basis_u);
        let point = light.position
            + light_disc_offset(basis_u, basis_v, light.radius, uv);
        to_light = point - origin;
        dist = length(to_light);
        if dist <= 0.0 {
            return out;
        }
        direction = to_light / dist;
    }
    out.direction = direction;
    out.distance = dist;
    out.radiance = emission * light_distance_falloff(dist, light.range, light.decay);
    out.pdf = 1.0;
    return out;
}

// Tests a ray against the rectangles, which are the only lights it can hit.
//
// Lights are not geometry and are not in the hierarchy: they are a short array
// the estimator picks from, and a scattered ray finds one only because this
// walks it. That is why the loop is linear and why nothing here is worth
// accelerating until a scene has hundreds of area lights, which the raster path
// could not display at all.
// `count` is passed rather than read from the per-dispatch uniform, so this is
// pure geometry over the light array and the probe that tests it needs no
// camera and no environment bound to ask a question about a rectangle.
// `travelled` is how far the ray had already flown since the direction was
// sampled, for a ray that re-origined past a masked or blended surface.
// Occlusion and radiance are properties of the remaining segment; the density
// is not, and it is the one output measured from the sample point.
fn intersect_lights(
    origin: vec3f,
    direction: vec3f,
    t_max: f32,
    count: u32,
    travelled: f32,
) -> LightHit {
    var out: LightHit;
    out.hit = false;
    out.t = t_max;
    out.radiance = vec3f(0.0);
    out.pdf = 0.0;

    for (var i = 0u; i < count; i += 1u) {
        let light = lights[i];
        if light.kind != LIGHT_RECT || light.area <= 0.0 {
            continue;
        }
        let denom = dot(direction, light.axis);
        var facing = denom;
        if (light.flags & LIGHT_TWO_SIDED) != 0u {
            facing = abs(denom);
        }
        // A ray travelling *toward* the emitting face has a positive component
        // along `axis`, since `axis` points out of the back.
        if facing <= 1e-6 {
            continue;
        }
        let t = dot(light.position - origin, light.axis) / denom;
        if t <= 1e-4 || t >= out.t {
            continue;
        }
        // Inside the rectangle: project the offset onto each edge, scaled by
        // that edge's own squared length so the result is a fraction of it.
        let offset = origin + direction * t - light.position;
        let su = dot(offset, light.u) / dot(light.u, light.u);
        let sv = dot(offset, light.v) / dot(light.v, light.v);
        if abs(su) > 0.5 || abs(sv) > 0.5 {
            continue;
        }
        out.hit = true;
        out.t = t;
        out.radiance = light.color * light.intensity;
        // The density next-event estimation would have used for this same
        // transport, which is what the MIS weight divides by. Measured from
        // where the direction was sampled rather than from where the ray
        // currently is: charging only the remaining segment understates the
        // distance-squared conversion and over-weights the scatter side of
        // the partition, which the blended-pane estimator comparison caught.
        let r = travelled + t;
        out.pdf = (r * r) / (light.area * facing);
    }
    return out;
}

// How much of a shadow ray survives the journey, as a colour.
//
// Not a boolean, which is the whole point: tinted glass has to tint its shadow,
// and an alpha-masked leaf has to not be there at all. Ported from the source's
// `attenuateHit` with one deliberate departure. Where it decides stochastically
// whether a partly transmissive surface blocks the ray, this multiplies by the
// fraction that survives. Both are unbiased; multiplying has lower variance,
// and it means a sheet of half-transparent glass produces a half shadow rather
// than a shadow that is present in half the samples.
//
// Returns black when the ray is blocked, which the caller checks rather than
// dividing by.
fn shadow_attenuation(
    origin_in: vec3f,
    direction: vec3f,
    distance: f32,
    transmissive_budget: u32,
    light_count: u32,
) -> vec3f {
    // An area light is an opaque emitter, and it is **not geometry**: it lives
    // in a short array the estimator picks from rather than in the hierarchy, so
    // a walk that only descends the hierarchy passes straight through a panel
    // and lights the surface behind it with whatever was beyond.
    //
    // Only the estimator comparison could find this, and it did. A scattered ray
    // that reaches a panel stops there, so the scattering technique already
    // treated the panel as opaque; the connection technique did not, and lit
    // every point that could see the environment *through* the panel. The two
    // estimators then disagreed by two percent on a scene with a single sphere,
    // which is a difference no image would have shown and no other test looks
    // for.
    //
    // A connection aimed at a light is unaffected: it stops short of its target
    // by `SHADOW_REACH`, so the light it is aimed at sits past the end of the
    // ray and is not found here. Another panel in the way is.
    if intersect_lights(origin_in, direction, distance, light_count, 0.0).hit {
        return vec3f(0.0);
    }

    var color = vec3f(1.0);
    var origin = origin_in;
    var travelled = 0.0;
    var transmissive_left = transmissive_budget;

    for (var step = 0u; step < SHADOW_MAX_STEPS; step += 1u) {
        let remaining = distance - travelled;
        if remaining <= 0.0 {
            return color;
        }
        let hit = trace_closest(origin, direction, remaining);
        if !hit.hit {
            // Nothing else between here and the light.
            return color;
        }

        let inst = instances[hit.instance];
        let geo = world_normal(inst, hit.geo_normal);
        let entering = dot(geo, direction) < 0.0;
        // Step past the surface on the far side, before any early exit below,
        // so every `continue` advances and none of them can loop in place.
        let next_origin = step_ray_origin(origin, direction, select(geo, -geo, entering), hit.t);
        travelled += hit.t;

        // An instance excluded from shadows is not there for this ray. It does
        // not spend a transmissive event either: it is a visibility rule, not
        // a material.
        if (inst.flags & INSTANCE_CAST_SHADOW) == 0u {
            origin = next_origin;
            continue;
        }

        let m = materials[inst.material_base];
        let sample = material_sample(m, shading_uv(hit));
        if !material_alpha_passes(m, sample) {
            // Masked out: the surface is not there at this point.
            origin = next_origin;
            continue;
        }

        // What fraction of the ray gets through. Transmission is the physical
        // route; a blend-mode surface's opacity is the authored one, and a
        // surface may have either.
        //
        // A refractive solid takes neither. This walk is a straight segment
        // from a shading point to a light, and a solid does not admit one:
        // light reaching the far side arrives along a bent path, and there is
        // generally no straight path at all. Refusing costs nothing and
        // removes a false answer, leaving that transport to the scattering
        // technique, which can find it. A thin surface is untouched, because
        // its parallel faces preserve direction and the straight segment is
        // the real path there.
        //
        // Decided on `thickness`, the same field `surf.thin_film` reads, so
        // this walk and the scatter walk cannot disagree about what a surface
        // is. The kernel has no notion of slab against ball, so a thick flat
        // pane loses its connection too, though physically it admits one with
        // a lateral offset. That is the price of one field deciding both, and
        // it is the price the material response already pays.
        //
        // Zeroing the transmission rather than the survival below is
        // deliberate: coverage is a separate route. A blend-mode surface is
        // partly not there at all, and where it is not there the segment is
        // straight and the connection is real, which is exactly how the
        // scatter walk reads it. It also gates the entering tint for free,
        // and correctly: what passes a solid passes through a hole in its
        // coverage rather than through glass, so it is not stained.
        let solid = m.thickness != 0.0;
        let transmission = select((1.0 - sample.metallic) * m.transmission, 0.0, solid);
        var opacity = 1.0;
        if material_alpha_mode(m) == MAT_ALPHA_BLEND {
            opacity = sample.base_color.a;
        }
        let survives = max(transmission, 1.0 - opacity);
        if survives <= 1e-4 {
            return vec3f(0.0);
        }
        if transmissive_left == 0u {
            // Out of budget. Blocked rather than transparent, for the same
            // reason as running out of steps.
            return vec3f(0.0);
        }
        transmissive_left -= 1u;

        color *= survives;
        if entering {
            // Tint on the way in, in proportion to how transmissive the
            // surface is: an opaque-but-blended sheet tints nothing.
            color *= mix(vec3f(1.0), sample.base_color.rgb, transmission);
        } else {
            // And attenuate by the medium on the way out, which is the only
            // point at which the distance through it is known.
            //
            // Not gated on the rule above, which is deliberate rather than an
            // oversight. That rule withdraws a claim about direction; this
            // describes a segment the ray genuinely crossed. It is reachable
            // for a solid only through that solid's coverage, and absorption
            // applies to whatever crosses the interior however it got in.
            color *= transmission_attenuation(hit.t, m.attenuation_color, m.attenuation_distance);
        }
        if dot(color, color) <= 0.0 {
            return vec3f(0.0);
        }
        origin = next_origin;
    }
    return vec3f(0.0);
}

// How many things the estimator picks between: every light, plus the
// environment when there is one.
fn light_choice_count(light_count: u32) -> f32 {
    return f32(light_count) + select(0.0, 1.0, env_present());
}

// The direct-lighting estimate at one shading point: one light or the
// environment, connected to, weighted, and returned per unit throughput.
//
// One draw rather than one per light, which is what makes the cost independent
// of how many lights a scene has. A forty-light scene spends the same here as a
// one-light scene and converges more slowly instead, which is the trade a
// progressive renderer is built to make.
//
// The caller multiplies by its own throughput. `origin` is the already-offset
// ray origin, and `geo_normal` is the geometric normal, which is what decides
// whether a sample is below the surface: the shading normal can point a
// direction the geometry does not admit, and trusting it there lets light
// through the back of a face.
fn direct_light(
    world_wo: vec3f,
    surf: Surface,
    geo_normal: vec3f,
    origin: vec3f,
    light_count: u32,
    transmissive_budget: u32,
    rng: ptr<function, RngState>,
) -> vec3f {
    if PATH_ESTIMATOR == ESTIMATOR_SCATTER {
        return vec3f(0.0);
    }
    let denom = light_choice_count(light_count);
    if denom <= 0.0 {
        return vec3f(0.0);
    }

    var direction: vec3f;
    var distance: f32;
    var radiance: vec3f;
    var pdf: f32;
    var delta: bool;

    // One draw does both jobs: scaled by the count it names which of them was
    // picked, and the leftover fraction is not reused for anything, so the
    // choice stays uniform. The environment is the entry past the last light.
    let pick = rand1_strat(rng, RNG_DIM_LIGHT_PICK) * denom;
    let index = u32(pick);
    if index < light_count {
        let s = sample_light(index, origin, rand2_strat(rng, RNG_DIM_LIGHT));
        direction = s.direction;
        distance = s.distance;
        radiance = s.radiance;
        pdf = s.pdf;
        delta = s.delta;
    } else {
        let e = env_sample(rng);
        direction = e.direction;
        distance = 1e30;
        radiance = e.radiance;
        pdf = e.pdf;
        delta = false;
    }

    if pdf <= 0.0 || dot(radiance, radiance) <= 0.0 {
        return vec3f(0.0);
    }
    // Below the geometry: no amount of shading normal makes this reachable.
    if dot(geo_normal, direction) <= 0.0 {
        return vec3f(0.0);
    }

    // Stop just short of the light, so a rectangle's own surface, or a wall the
    // light is mounted on, does not shadow it.
    let attenuation = shadow_attenuation(
        origin,
        direction,
        distance * SHADOW_REACH,
        transmissive_budget,
        light_count,
    );
    if dot(attenuation, attenuation) <= 0.0 {
        return vec3f(0.0);
    }

    // The material's own density for this direction, from the entry point that
    // exists for exactly this: a density that came from the sampler that drew
    // the direction would be comparing the sampler to itself.
    let eval = bsdf_result(world_wo, direction, surf);
    if eval.pdf <= 0.0 || any(eval.color < vec3f(0.0)) {
        return vec3f(0.0);
    }

    // The density of having picked this light *and* this direction on it.
    let pick_pdf = pdf / denom;
    // A delta light has no second technique to share with, so it takes the
    // whole contribution; so does the connections-only mode, where there is no
    // second technique at all.
    let shares = !delta && PATH_ESTIMATOR == ESTIMATOR_MIS;
    let weight = select(1.0, mis_power(pick_pdf, eval.pdf), shares);
    return attenuation * radiance * eval.color * weight / pick_pdf;
}

// The weight for radiance a scattered ray found on its own, which is the other
// half of every multiple-importance pair.
//
// `scatter_pdf` below zero means the ray came from the camera rather than from
// a scatter, and then the weight is one: next-event estimation never had a
// chance at this direction, so there is no second technique to share with. That
// is what keeps an area light and the sky at full brightness when looked at
// directly, and it is the difference between a correct image and one that is
// half as bright everywhere the camera sees a light.
fn scatter_mis_weight(scatter_pdf: f32, light_pdf: f32, light_count: u32) -> f32 {
    if scatter_pdf < 0.0 {
        return 1.0;
    }
    if PATH_ESTIMATOR == ESTIMATOR_SCATTER {
        return 1.0;
    }
    let denom = light_choice_count(light_count);
    if denom <= 0.0 || light_pdf <= 0.0 {
        return 1.0;
    }
    if PATH_ESTIMATOR == ESTIMATOR_NEE {
        // The connection at the previous bounce already counted this radiance.
        // Counting it again here is the classic double-count that makes a
        // next-event renderer twice as bright as it should be.
        return 0.0;
    }
    return mis_power(scatter_pdf, light_pdf / denom);
}
