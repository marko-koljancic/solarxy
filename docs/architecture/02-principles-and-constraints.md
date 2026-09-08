# Principles and constraints

A principle you cannot violate is not a principle, it is a description. Everything in the
first half of this document is written as a rule that a pull request could break, together
with what would actually stop it. Everything in the second half is a number the system cannot
argue with.

Each principle carries four things: the rule, why it exists, how it is enforced today, and one
real example of it holding or failing. Where the honest enforcement answer is "by convention
only", that is what it says. Six of the nine were already written as prose invariants in
`.claude/skills/solarxy-domain/SKILL.md:26-49`; they are restated here in enforceable form
rather than replaced.

## The nine rules

### P1. The engine and the renderer never depend on each other

**Rule.** `solarxy-graph` must not depend on `solarxy-renderer`, and `solarxy-renderer` must
not depend on `solarxy-graph`, in any dependency kind: normal, dev, build or target-conditional.
They communicate only through `solarxy_core::scene::SceneDelta`. `solarxy-host` must not
depend on `solarxy-graph` either, because it sits on the renderer's side of that line.

**Why.** This is what lets the engine compile without wgpu, lets the import worker run a
GPU-free WebAssembly instance, and lets the cook be tested without a device. It is also what
makes one WebAssembly instance hold both halves, so cooked geometry never crosses into
JavaScript.

**Enforcement today.** Nothing mechanical. Cargo refuses a true cycle, but neither forbidden
edge would be one, because `solarxy-graph` depends on neither of the other two. Adding either
edge compiles, passes `cargo clippy --workspace --all-features`, passes
`cargo test --workspace --all-features`, passes both `cargo doc` runs, and ships. The rule is
recorded in two comments: `crates/solarxy-host/Cargo.toml:53-58` and
`crates/solarxy-host/src/lib.rs:25-30`.

**Where it holds.** It holds everywhere today, and it is the only structural invariant in the
workspace that the build system currently backs at all, by refusing cycles. The three crates
that hold both sides are the shells: `solarxy-app`, `solarxy-web` and `solarxy-render`.

**Where it costs.** The rule forces genuine duplication rather than being free. The
two-valued render-engine concept is modelled three times, as `RenderEngine` at
`crates/solarxy-graph/src/nodes/export_nodes.rs:31-35`, as `StillEngine` at
`crates/solarxy-host/src/still.rs:198-206`, and as `RenderEngineArg` at
`crates/solarxy-cli/src/parser.rs:243-246`, with four hand-written map sites. See P9 for the
rule that governs that.

### P2. Rust owns document state, and the frontend mutates it only by command

**Rule.** No document state is authored in JavaScript. The React frontend is a display mirror:
gestures dispatch a `Command`, the returned `EventBatch` is applied to the mirror store, and
nothing else writes the document.

**Why.** One document model rather than two. A second model in TypeScript would be a second
thing to keep correct across a boundary that cannot be type-checked end to end.

**Enforcement today.** Partial, and structural rather than asserted. The mirror store has no
setters that write document fields directly; it applies batches. A monotonic `revision`
detects desync and recovers by re-snapshotting, at `web/src/store/mirror.ts:259-281`, which
turns a violation into a visible resync rather than silent divergence. The boundary's JSON
shape is pinned by `command_boundary_json_shape_is_camelcase` at
`crates/solarxy-graph/src/engine/tests.rs:383`. Nothing forbids a component from keeping
document-shaped state of its own.

**Example.** The note node is the single component keyed on a type id rather than driven by
the registry, `web/src/flow/NoteNode.tsx`, and it still writes through the engine's existing
parameters rather than holding its own text.

### P3. A node type added in Rust needs zero frontend changes

**Rule.** The palette, the typed handles, and the parameter panel are pure interpreters of the
registry snapshot. Adding a node type in `solarxy-graph` requires no edit under `web/`. Adding
a new `ParamType` or `DataType` variant is the deliberate exception and is a sanctioned
frontend change.

**Why.** The registry has 77 node types and the roadmap adds more. A contract that costs a
frontend edit per node type would make the frontend the bottleneck on the engine.

**Enforcement today.** A real test. `web/src/registry/extensibility.test.ts` constructs a node
type the frontend has never seen and asserts six properties of it: that it is discoverable and
context-filtered, that its canvas kind derives from its owner's descriptor rather than its
type id, that its handles are colourable and validatable, that it speaks the image vocabulary,
that every one of its parameters renders a widget with no unsupported type, and that drawable
node art always resolves from glyph and role hints.

**Example of the contract holding.** The registry count is asserted at
`crates/solarxy-graph/src/nodes/mod.rs:238` and mirrored into `schemas/registry.json` and
`schemas/node-reference.md`, both pinned by `crates/solarxy-graph/tests/registry_drift.rs`.

### P4. Boundary mirrors move in the same change as the boundary

**Rule.** Any change to `Command`, `EngineEvent`, a snapshot shape, or a wasm-exported method
updates the hand-authored TypeScript mirrors in `web/src/engine/types.ts`, `client.ts` and
`session.ts` in the same change. Any serde enum that crosses the boundary carries both
`rename_all` and `rename_all_fields`.

**Why.** The two sides are hand-authored and only the pair is correct. Neither half is wrong
on its own, which is exactly why nothing notices.

**Enforcement today.** One narrow test with a real history behind it.
`the_wasm_boundary_enums_rename_their_fields_too` at
`crates/solarxy-core/tests/tokens_drift.rs:1071` pins both boundary enums to carry both serde
attributes. It is a source-level text scan rather than a serialization test, because one of
the enums lives behind `cfg(target_arch = "wasm32")` and no native test can construct one. The
wider obligation, that the TypeScript mirrors move too, is convention.

**Example of the failure it exists for.** `rename_all = "camelCase"` on an enum renames the
variants and not the fields of a struct variant. Every field in the host event enum was a
single word until two multi-word fields were added, at which point they crossed the boundary
in snake case while the TypeScript declared camel, and the frontend read `undefined` for a
whole release.

### P5. The desktop shell stays regression-free

**Rule.** Native CI is green at every merge, and a change to a shared crate gets a desktop
smoke run before a release.

**Why.** The browser shell is the one getting new capability, and the desktop shell is the one
that already has users. Shared-crate work is where a browser feature silently breaks a viewer.

**Enforcement today.** Real, and partial. `.github/workflows/ci.yml` runs five jobs: a check
job with formatting, four clippy invocations, the whole workspace test suite and two rustdoc
runs under `-D warnings`; a three-operating-system release build matrix; a macOS GPU test job
that runs the renderer, host and render crates against a real adapter in both debug and
release; a golden-image job; and a web job. `docs/qa/desktop-checklist.md` carries the manual
half.

**Where it is thinner than it looks.** The golden job captures six pane modes over two models
at the pull request's base commit and again at the head, and pins `PaneEngine::Raster` at
`crates/solarxy-host/examples/golden.rs:70`, so the path tracer, the tiled still job and every
pixel the browser produces have no image regression gate. And `.github/workflows/ci.yml:22`
runs clippy without `--all-targets`, so tests and examples are never linted, including the
golden harness itself.

### P6. Where a single function is named the sole mutation path, parallel paths are defects

**Rule.** Some state has one legal writer. Image-based lighting rebuilds, display-flag claims
and the exclusive shadow caster are the named cases. A second path to the same state is a
defect, not an alternative.

**Why.** These are the pieces of state where a partial update produces a plausible wrong
picture rather than a crash, which is the hardest class of bug to notice.

**Enforcement today.** Convention only, and it is not currently holding across shells.

**Where it holds.** The exclusive shadow caster is genuinely single-pathed and engine-side:
granting the flag on one light clears it on the others in one undo step, at
`crates/solarxy-graph/src/engine/mod.rs:1851`. On the desktop, lighting rebuilds funnel through
one wrapper at `crates/solarxy-app/src/state/update.rs:24`, which calls the shared
`solarxy_host::rebuild_light_bind_group` at `crates/solarxy-host/src/lighting.rs:42`.

**Where it does not.** The browser shell never calls that function. It assembles its own
lights uniform, writes its own buffer and picks its own shadow caster at
`crates/solarxy-web/src/app/render.rs:969-997`, and installs its own bind group at
`app.rs:1053` and `app.rs:5075`. So the chokepoint is a chokepoint on one shell and a copy on
the other, which is the precise shape the rule was written to forbid.

### P7. Platform I/O enters only through a feature gate or an adapter crate

**Rule.** A crate in the WebAssembly dependency closure contains no ungated `std::fs`,
`std::net`, `std::process`, `std::env`, `std::thread` or `Instant` use. Filesystem behaviour
lives behind `fs`, `std-fs` or `serialization`, and the caller drops it with
`default-features = false`.

**Why.** Portability is bought here with Cargo feature negation rather than conditional
compilation, and it works: the whole workspace carries only 18 platform `cfg` attribute sites
in `src`, and the entire 6,489-line WebAssembly host sits behind one gate at
`crates/solarxy-web/src/lib.rs:30`.

**Enforcement today.** The compiler, once, in one job. `.github/workflows/ci.yml:200` runs
clippy for `wasm32-unknown-unknown` on `solarxy-web`, which is the only step that exercises
the reduced feature set. Every other build and test in CI is `--all-features`, and under
Cargo's feature unification a workspace-wide build has `serialization` and `std-fs` switched
on while compiling the engine. So the single wasm clippy step is the whole guard, and it works
only because every consumer remembered `default-features = false`.

**Where it holds.** `solarxy-kernel`, `solarxy-bvh` and `solarxy-imaging` have zero hits for
any of those paths. `solarxy-scenefile` uses only in-memory cursors. `solarxy-graph` uses only
an in-memory cursor for the export archive.

**Where it fails at the type level.** `crates/solarxy-core/src/geometry.rs:12` imports
`std::path::PathBuf` outside any gate, and `RawMaterialData` at `geometry.rs:423-623` carries
17 `Option<PathBuf>` texture-path fields. The type reaches the browser through
`GeometrySet::materials` at `crates/solarxy-kernel/src/set.rs:280`, where a host filesystem
path has no meaning. This is not hypothetical: `TextureRole::path_only` at
`crates/solarxy-renderer/src/pathtrace/scene.rs:274-285` branches on five of those fields, and
its own comment concedes that on the web there is no filesystem to hold and that the symptom
reads as a shading bug.

### P8. No optional GPU feature is ever required

**Rule.** Every device request passes `wgpu::Features::empty()`. Limits may be raised off the
adapter, never lowered from the core WebGPU defaults, and a raise names the field it touches.

**Why.** A code path gated on an optional feature is a code path some conformant adapter
cannot run, and the browser is the least forgiving consumer. Raising a count limit is worse
than raising a size limit, because it fails only on the hardware that guarantees least.

**Enforcement today.** By construction and by convention. Every device request in the
workspace passes `Features::empty()`, verifiable by search, including the test harnesses at
`crates/solarxy-renderer/tests/common/mod.rs:44`. Nothing prevents a new request from asking
for a feature.

**Example.** `crates/solarxy-renderer/src/limits.rs` raises exactly two fields,
`max_buffer_size` and `max_storage_buffer_binding_size`, each taking the larger of the core
default and the adapter's report so a weaker adapter still yields the baseline. Its own
documentation states why it does not use wgpu's one-line `or_better_values_from`: that helper
raises count limits too, and the path tracer's scene bind group already spends core WebGPU's
eight-storage-buffer budget deliberately.

**Where it is not shared.** The watch window requests `wgpu::Limits::default()` at
`crates/solarxy-cli/src/render_watch/mod.rs:212` and `:1094`, not `required_limits`. That is
safe for what it does, a textured quad, but it means the helper is used by three of the four
shells rather than all of them.

### P9. Duplication is allowed only where the boundary forbids a shared home, and every copy carries a compiler guard

**Rule.** P1 makes some duplication unavoidable. Where a concept must exist in more than one
place, each copy carries something that stops the compiler when the concept changes: an
exhaustive destructure with no rest pattern, or an exhaustive match with no wildcard arm.
Duplication without a guard is a defect.

**Why.** A duplicated concept costs edits. A duplicated concept with no guard costs a release.
The settings resolver exists in triplicate precisely because a camera aperture resolved
correctly out of a document, reached no renderer for a whole release, and every test passed
because they all used an aperture of zero.

**Enforcement today.** The guards are real but they are hand-written, and nothing asserts that
a new copy has one.

**Where it holds.** `trace_settings_for` exists once per shell, at
`crates/solarxy-app/src/state/still.rs:753`, `crates/solarxy-web/src/trace_settings.rs:28` and
`crates/solarxy-render/src/lib.rs:887`, and each opens by destructuring `RenderSettings`
exhaustively with no rest pattern, so a new settings field stops all three compiling. The
three render-engine enums are each matched exhaustively with no wildcard arm at all four map
sites, so a third engine variant halts compilation in every shell.

**Where it is weaker.** `denoise_settings_for` is character-identical in the same three files,
at `still.rs:811-818`, `trace_settings.rs:104-111` and `render/lib.rs:942-949`, with no
per-shell variation and no guard of its own. It is protected only indirectly, by the
exhaustive destructure one function above it in each file. That is a real guard, but it is
inherited rather than stated, and a fourth shell could copy the body without the caller.

## Hard constraints

These are not principles. They are numbers, and code either fits inside them or does not
ship.

### Payload budgets

| Budget | Value | Declared at |
|---|---|---|
| WebAssembly module, gzipped | 2,621,440 bytes, 2.5 MiB | `.github/workflows/web-release.yml:39` |
| Editor boot JavaScript, gzipped | 471,040 bytes, 460 KiB | `web-release.yml:57` |
| Player JavaScript, gzipped | 51,200 bytes, 50 KiB | `web-release.yml:46` |

These are real gates that fail a job, and they gate nothing on a pull request.
`web-release.yml` is invoked from `release.yml`'s post-announce jobs, which fire from a version
tag, so a change that doubles the boot payload passes every pull request check and is caught
only once the tag exists and the GitHub Release has been created. The bundle is already built
in `ci.yml`'s web job, which runs `npx vite build` at `ci.yml:220` and asserts nothing about
size.

One specific regression the budgets were written for is separately gated on every pull
request: `the_player_does_not_import_the_editors_ui_graph` at
`crates/solarxy-core/tests/tokens_drift.rs:635` bans seven value imports in the player
directory, after a release shipped a player carrying React and a docking library. That test
walks the directory without recursion, so a file in a subdirectory would escape it; the
directory holds one file today.

### Capture and still budgets

- **Screenshot and capture: 4.0 megapixels.** `MAX_CAPTURE_PIXELS = 4_000_000`, declared twice
  in `crates/solarxy-web/src/app/capture.rs`, at `:20` and `:54`, once for screenshots and once
  for turntable frames. Larger captures can lose the WebGPU device, and there is no device-loss
  recovery on the web.
- **Still render tile: 4,194,304 pixels**, `TILE_BUDGET_PIXELS` at
  `crates/solarxy-host/src/still.rs:57`, with a 128-pixel apron at `:80` for screen-space post
  passes and an 8,192-pixel maximum edge at `:83`. The preview tile budget is 256 by 256, at
  `:72`.
- **Floating-point still in the browser: 16,000,000 pixels**,
  `crates/solarxy-web/src/app/mod.rs:690`. This one is a WebAssembly constraint rather than a GPU
  one and the comment does the arithmetic: roughly forty bytes a pixel at peak, near 290
  megabytes, inside a 32-bit address space that also holds the document, the tracer's buffers
  and the page. It deliberately does not live in `solarxy-host` beside the other still
  constants, because the desktop renders that size comfortably.
- **Auxiliary pass planes: 192 MiB for one render**, `MAX_PASS_PLANE_BYTES` at `app.rs:692`.
  Stated in bytes rather than pixels because the planes are held whole, since a depth plane
  mapped tile by tile would band at every seam.

### Cook budgets

The resumable cook is given 6 milliseconds per frame in the browser, `COOK_BUDGET_MS` at
`crates/solarxy-web/src/app/mod.rs:73`, and 8 milliseconds on the desktop, `COOK_BUDGET` at
`crates/solarxy-app/src/state/update.rs:21`. Two numbers for one concept, in two shells, with
nothing reconciling them and no record of why they differ.

### The GPU floor

`wgpu::Features::empty()` at every device request, and `wgpu::Limits::default()` as the base
that no field is ever lowered from. `solarxy_renderer::limits::required_limits` raises
`max_buffer_size` and `max_storage_buffer_binding_size` off the adapter and nothing else. It
deliberately leaves `max_uniform_buffer_binding_size` alone, because no uniform here
approaches 64 KiB; the texture dimension limits alone, because the capture path clamps against
the device's own limits and its budget is a deliberate ceiling rather than a hardware one; and
the two minimum-offset-alignment fields alone, because a raise there means a worse value.
`buffer_ceiling` in the same file is the other half: a mesh that still will not fit is refused
with a message rather than a dead viewport.

### The WebAssembly address space

The browser build runs in a 32-bit address space with no threads and no filesystem. Three
things follow and are not negotiable.

- **No threading.** There is no `std::thread` in any crate in the wasm closure. Work that must
  leave the main thread leaves the WebAssembly instance entirely, into a second GPU-free
  instance running in a Web Worker, and comes back as bytes through hand-written transfer
  codecs in `crates/solarxy-kernel/src/transfer.rs` and `crates/solarxy-bvh/src/transfer.rs`.
- **No filesystem.** Persistence is the browser's origin-private file system and the File
  System Access API, and every loader in the wasm closure is byte-first, which is what
  `std-fs` being off actually means.
- **No unbounded memory.** An allocation failure takes the tab, not the operation, which is
  why the still and capture ceilings above are stated numbers rather than hopes.

### Toolchain and instruction set

Minimum supported Rust version 1.92, edition 2024, both at `Cargo.toml:23-24`, with
`clippy.toml` carrying the same MSRV as a second copy. There is no `rust-toolchain.toml`, so
the number lives in four unlinked places, and the workflow that builds the shipped desktop
bundles runs `rustup default stable` at `.github/workflows/native-bundle.yml:63`, unpinned.
Users therefore install binaries compiled by a toolchain no CI job exercised.

`.cargo/config.toml` forces `-C target-feature=+avx2,+fma` on all three x86_64 targets and
`-C target-cpu=apple-m1` on Apple silicon, unconditionally and for every profile. Three of the
five targets in `dist-workspace.toml:8-14` are x86_64, so every released x86_64 binary
requires AVX2 and FMA with no runtime dispatch. Below that hardware floor the failure is an
illegal instruction at startup. Nothing outside that config file states it.

### Other stated ceilings

`MAX_REF_DEPTH = 32` at `crates/solarxy-graph/src/refs.rs:31` bounds a cross-context reference
chain, as a backstop for a hand-edited document where the write-time cycle refusal was
bypassed. The bounding volume hierarchy builder targets a leaf of 5 and enforces a maximum
depth of 32 by emitting a leaf at the cap rather than panicking,
`crates/solarxy-bvh/src/build.rs:18,28`. Validation events are capped at 2,000 issues per
report, `crates/solarxy-graph/src/engine/mod.rs:420`.

## Non-functional targets that do not exist

Stated plainly, because a missing target reads as an unstated one and then gets invented per
change.

There is **no frame-time budget and no target frame rate**. There is **no cook latency
objective**: the 6 and 8 millisecond numbers are per-frame slices of a resumable cook, not a
deadline for a cook to finish. There is **no scene scale ceiling**: no maximum node, object or
triangle count is stated or checked. There is **no memory ceiling for the browser** beyond the
still and capture caps above, which bound individual operations rather than the session. There
is **no startup time budget**.

Nor is there anything that would defend such a number if one existed. There is no benchmark
framework of any kind, and the three files that read as benchmarks are marked ignored and
describe themselves as measurements rather than regression gates.

Those absences are carried as open items in
[10-risks-and-open-questions.md](10-risks-and-open-questions.md) rather than resolved here,
because inventing a number in an architecture document is worse than admitting there is none.

## Open questions

- Whether the two cook budgets differ for a measured reason or by drift.
- Whether the three payload budgets could move into the pull request web job, which already
  produces the bundle. Nothing in the workflows records a reason they could not.
- Whether the shipped x86_64 hardware floor of AVX2 and FMA is a decision with an accepted
  minimum, and if so where that minimum should be published.
- Whether `solarxy-renderer::limits::required_limits` should be the only device-request path,
  which would fold the watch window in, or whether that window's simpler needs justify its own.
