# 0009. The render backend contract is declared in the renderer and implemented in the host

- **Status**: accepted
- **Date**: 2026-09-07
- **Embodied in**: 0.9.0

## Context

Until a second renderer existed there was nothing to abstract. The shared host crate
deliberately had no renderer trait, and its own documentation states the reasoning: one
implementation with two callers is deduplication, and a trait designed against a backend that
does not exist yet gets redesigned when the real one arrives, refactoring every host twice
(`crates/solarxy-host/src/lib.rs:12`).

The path tracer is that second implementation, and it forced the question. A host now has to
drive one of two renderers per pane, state what the one it is holding can do, and composite
the result identically either way. Three call sites need it: the browser's per-pane render
loop, the tiled still-render job, and the headless render command.

Where the contract could be declared was then constrained rather than open. The tracer lives
in `solarxy-renderer`, beside the pipelines and the shader composition it needs.
`solarxy-renderer` cannot depend on `solarxy-host`, which sits above it. So a trait declared in
the host could not be implemented by the tracer.

## Options considered

### Option A: declare the contract in the renderer, implement the raster half in the host

The trait lives in `crates/solarxy-renderer/src/backend.rs` so that a backend living in the
renderer can implement it. `RasterBackend` lives in `solarxy-host`, beside the pass chain it
wraps.

### Option B: declare the contract in the host, beside the raster implementation

The tidier reading, since the host is where shared host behaviour goes and where both
implementations would then sit together.

It requires moving the path tracer out of the renderer and into the host, which means moving
the workspace's only compute path away from the crate that owns every pipeline, every bind
group layout and the shader composition it depends on, purely to satisfy the placement of a
trait. Alternatively the tracer stays and cannot implement the trait, which is not an option at
all.

### Option C: no trait; each host branches on an engine enumeration

Two concrete types and a match at every call site.

Three hosts would each carry the branch, and each would have to be edited when a third backend
arrives. Worse, an enumeration is identity, and branching on identity is precisely what the
capability design exists to prevent.

## Decision

`RenderBackend` is declared in `crates/solarxy-renderer/src/backend.rs:57`, together with the
per-frame bundle a pane's encode takes and the capability record a host reads. The raster
implementation lives in `solarxy-host`, beside `encode_pane_passes`, because that is where the
pass chain is. The path-traced implementation lives in the renderer, inside the compute path.

`BackendCaps` states capability and never identity.

## Consequences

The capability rule is written into the contract and it is a rule rather than a preference
(`backend.rs`, module documentation): the capability record "must never grow a field or method
that says *which* backend it is. A `fn is_path_tracer()` would let hosts branch on identity,
and the first time one did, adding a third backend would mean editing every host again. The
test of the design is that a screen-space GI backend, or a hardware-ray-tracing one, slots in
without a host change."

Per-pane state lives inside the backend, keyed by the pane index the frame context carries. The
alternative, handing each pane a backend-specific view type, needs an associated type, and an
associated type makes a boxed trait object impossible, which is exactly how hosts hold these.
A shell therefore holds one backend of each kind, not one per pane, because a scene is per
session and four panes showing it must not mean four copies of the geometry on the GPU.

What the contract deliberately excludes is as load-bearing as what it includes. Device
creation, surface configuration, the post chain and capture stay concrete and shared. A backend
produces a linear high-dynamic-range view and everything downstream of that view is one code
path, which is why a traced image inherits exposure, the grading slots, tone mapping, bloom and
the selection rim by construction rather than by discipline.

Owning the scene is what makes the raster side a real implementation rather than a wrapper.
`RasterBackend` holds `SceneObjects`, both shells read the document through it, and it
assembles its own draw list, because a list built by a shell would borrow the scene inside the
backend while encoding needs it mutably.

Three honest gaps in the contract as it stands. Its own documentation says "Four operations and
nothing else" while the trait declares eight methods, four of them defaulted. Two of those,
setting a lens and encoding a depth pass, are implemented only by the tracer and have no
capability field, so a host cannot ask whether the call it just made will do anything, which is
the exact pattern the capability record exists to provide. And the raster implementation accepts
the trait's one output parameter and ignores it, writing the renderer's own target instead; the
reason is recorded honestly at the implementation, but the shape of the trait promises a
retargetable renderer that half of it is not.

The trait is also exercised polymorphically by fewer callers than it looks. The browser
dispatches per pane and carries the drawing backend's occlusion capability into the composite;
the still job dispatches; the desktop viewport calls the raster backend unconditionally and
hardcodes its capability at the composite. So the desktop cannot show a traced pane, and
nothing in the code states that as a decision.

Enforcement: Cargo, for the placement. The dependency edge from `solarxy-host` to
`solarxy-renderer` and the absence of the reverse edge are what make this arrangement the only
one that compiles.

## Notes

A fourth consumer exists and is easy to miss: the headless render command drives a boxed
backend through the same trait. It cannot reach the raster backend's upload-error drain, which
is an inherent method rather than a trait method, so a headless render whose geometry failed to
upload writes a silently incomplete image.
