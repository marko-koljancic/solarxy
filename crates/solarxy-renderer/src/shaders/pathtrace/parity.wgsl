// The parity kernel: a corpus of rays in, one hit record each out.
//
// Composed after `traverse.wgsl`, and the only thing that can answer whether
// the WGSL traversal agrees with the CPU one. A shader cannot be unit tested,
// so the traversal is written twice and this is the joint: the same rays, from
// the same generator, through both, compared record by record.
//
// It deliberately shades nothing and reads no camera. What comes back is what
// the traversal returned, so a disagreement is a traversal disagreement rather
// than something a lighting term could explain away.

struct CorpusRay {
    // World-space origin in `xyz`; `w` unused.
    origin: vec4f,
    // World-space direction in `xyz`, unit length; `w` unused.
    direction: vec4f,
}

// 32 bytes.
struct CorpusHit {
    t: f32,
    instance: u32,
    prim: u32,
    // Bit 0: the closest-hit walk found something. Bit 1: the any-hit walk did.
    // Both are recorded because they are genuinely different traversals
    // answering the same question, and the corpus checks they agree.
    flags: u32,
    // Barycentric `[w, u, v]` in `xyz`; `w` unused.
    bary: vec4f,
}

// The probe's own group, and not the shipped kernel's group 1. This pipeline
// has its own layout, because a diagnostic that borrowed the accumulation
// group's shape would constrain the accumulator to suit the diagnostic.
@group(1) @binding(0) var<storage, read> corpus_rays: array<CorpusRay>;
@group(1) @binding(1) var<storage, read_write> corpus_hits: array<CorpusHit>;

const HIT_CLOSEST: u32 = 1u;
const HIT_ANY: u32 = 2u;

// The corpus is dispatched as a 2D grid because the workgroup shape is shared
// with the real kernel; this is the row length that turns a linear ray index
// into one. The host owns the value, so the two cannot drift.
override CORPUS_WIDTH: u32 = 64u;

// The ray budget, matching what the CPU side passes. Not infinity: WGSL has no
// infinity literal, and using a finite bound on one side and an infinite one on
// the other would be a difference between the implementations rather than
// between their answers.
const T_MAX: f32 = 1e30;

@compute @workgroup_size(8, 8, 1)
fn parity(@builtin(global_invocation_id) gid: vec3u) {
    let index = gid.y * CORPUS_WIDTH + gid.x;
    if index >= arrayLength(&corpus_rays) {
        return;
    }

    let ray = corpus_rays[index];
    let hit = trace_closest(ray.origin.xyz, ray.direction.xyz, T_MAX);

    var flags = 0u;
    if hit.hit {
        flags |= HIT_CLOSEST;
    }
    // `shadow_only` false: the CPU twin has no notion of a shadow flag, so the
    // comparison has to ask the question the twin can answer.
    if trace_any(ray.origin.xyz, ray.direction.xyz, T_MAX, false) {
        flags |= HIT_ANY;
    }

    corpus_hits[index] = CorpusHit(hit.t, hit.instance, hit.prim, flags, vec4f(hit.bary, 0.0));
}
