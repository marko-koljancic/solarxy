# 0008. The renderer requires no optional GPU feature, and raises only the two size limits it needs

- **Status**: accepted
- **Date**: 2026-09-07

## Context

The same renderer runs on a native adapter through a desktop driver and in a browser through
WebGPU. Those two environments do not offer the same capabilities, and the browser's are the
smaller set by construction: core WebGPU is a guaranteed floor, and anything above it is a
per-browser, per-machine question.

A renderer that requires an optional capability has to answer what it does when the capability
is absent, and every honest answer is a second code path. A renderer that requires nothing
optional has no such question, and its capability surface reduces entirely to limits, which
are numbers rather than branches.

Limits still have to be dealt with, because the core floor is genuinely too small for real
work: buffer allocations are capped at 256 MiB by default and storage bindings at 128 MiB, and
a large model's vertex data clears both on hardware that offers gigabytes. The code records
that this was not hypothetical: both shells used to request the defaults and never read what
their adapter reported, so such a model failed to allocate on a machine with ample memory
(`crates/solarxy-renderer/src/limits.rs`).

## Options considered

### Option A: require no optional feature; raise only the size limits, field by field

Every device request passes an empty feature set. Limits start at the core defaults and two
named fields are raised to whatever the adapter reports, with no field ever lowered.

### Option B: request optional features where the adapter offers them

Ask for what is available and branch at runtime on what was granted.

Each optional feature is a configuration that only some machines execute, so the number of
shipped renderers multiplies and only one of them is the one being tested. In the browser most
of the interesting ones are unavailable anyway, so the branch would exist to serve the desktop
alone while every browser user runs the fallback, which is the path that would then be
under-exercised on the desktop.

### Option C: adopt a higher floor and refuse devices below it

State a minimum above core WebGPU and decline to run below it.

That excludes the browser, which is the primary surface. It also converts a graceful
limitation into a refusal at startup, which is the worst place for a capability problem to
appear.

## Decision

No optional GPU feature is required anywhere. Every device request in the workspace passes an
empty feature set and disables experimental features.

Limits come from one shared helper, `solarxy_renderer::limits::required_limits`, which takes
the core defaults as its base, raises exactly `max_buffer_size` and
`max_storage_buffer_binding_size` to the adapter's report, and never lowers any field.

## Consequences

The helper deliberately does not use the one-line merge that wgpu provides, and it says why
(`crates/solarxy-renderer/src/limits.rs`): that merge walks every field and takes the better
value, which raises the *count* limits along with the sizes. The path tracer's scene bind group
is designed against core WebGPU's budget of eight storage buffers per compute stage and spends
it deliberately. Raising a count limit would let a later change quietly exceed what the target
platform guarantees and fail only on the machines that guarantee least. So the merge names the
fields it touches, and adding a third is a decision rather than a default.

Two fields rather than one is also deliberate: the storage binding limit is the tighter of the
two on the raster path, because the edge position and edge index buffers are storage buffers,
and it is the binding limit for the traced arena as well, so raising only the buffer size would
move the failure rather than fix it. Everything else is left at the default on purpose, and the
two minimum-alignment fields are the reason the merge is written field by field rather than as
a loop.

The helper is not universal. Both graphical shells and the headless render command use it; the
command-line tool's optional live render window brings up its own device with the plain
defaults, so a preview window on a machine with a large adapter is limited differently from the
render it is previewing.

Requiring nothing optional does not by itself make the capability story complete, and three
gaps are real today.

The multisample count is a user preference offering 1, 2 or 4 and is never checked against the
adapter. Core WebGPU mandates 1 and 4 only, and nothing in the workspace queries texture format
features. A user selecting 2 on an adapter without that support fails validation at renderer
construction, from a setting the preferences dialog offered.

The tracer's capability predicate cannot fail. It asks for at least eight storage buffers and
four storage textures per stage, which are exactly the core defaults the helper floors every
request at, so any device the application successfully creates satisfies it. It has one caller,
which reports it to the browser for display; none of the sites that actually construct a tracer
consult it.

And the browser's diagnostic probe requests ten storage buffers per stage, above the core
guarantee and directly against the argument the limits helper is built on. The check that
matters is separate and correct: the probe also builds the shipped kernel on a device requested
at exactly the core defaults, so the strictest conformant target is genuinely exercised.

Enforcement: one helper with a test asserting no field is lowered. Nothing prevents a new
device request from bypassing it, and one already does.

## Notes

Two adjacent files disagree about the exact storage-buffer count the tracer's scene group
spends: the limits helper says six, the tracer says seven plus a coverage buffer, spending all
eight. The argument does not turn on which is right, but the number should be stated once. It
is recorded in [10-risks-and-open-questions.md](../10-risks-and-open-questions.md).
