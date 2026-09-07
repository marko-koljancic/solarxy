# Risks and open questions

Two lists. The first is ranked by expected cost, meaning impact weighted by how likely the
thing is to actually happen. The second is questions the code cannot answer, grouped by theme,
each phrased so the maintainer can rule on it.

An open question stated clearly is worth more than a confident guess. Several entries below
were guessed wrong in an earlier reading of this codebase and corrected by re-reading it, which
is the argument for listing them rather than resolving them by inference.

## Risks

Impact and likelihood are stated plainly rather than scored, because a number would imply a
precision this assessment does not have.

### R1. Parity is blocked on a crate that does not exist

**Impact.** High. The desktop shell dispatches two of the engine's 35 commands. Everything
above the engine is written once per shell, so every feature added to one is absent from the
other until someone writes it twice.

**Likelihood.** Certain. This is a present state, not a hazard.

**Mitigation.** [ADR 0012](adr/0012-shared-application-layer-is-a-new-crate.md) and Group E of
[09-evolution-and-roadmap.md](09-evolution-and-roadmap.md). The mitigation is large and is
broken into individually shippable pieces there.

**What makes it worse if untreated.** Every feature added to the browser shell in the meantime
widens the gap, and the gap is the work.

### R2. No architectural invariant is mechanically enforced

**Impact.** High, and compounding. The three non-edges that make the layering real live in
comments and in this document set. Nothing fails when one is violated.

**Likelihood.** Moderate per change, high over time. The workspace already contains one
dependency-kind inversion, benign in effect, which demonstrates that "it compiles" is not
evidence a layer holds.

**Mitigation.** Group A of [09](09-evolution-and-roadmap.md). The dependency assertion is
small, and it catches the whole class.

### R3. The boundary mirror is 80 types deep and six variants pinned

**Impact.** High when it fires, and it fires silently. A variant added in Rust and forgotten in
TypeScript compiles on both sides. The field-renaming trap has already cost one release, with
two dialog readouts blank the whole time.

**Likelihood.** High. The boundary changes often, and Group E will change it more.

**Mitigation.** A4 in [09](09-evolution-and-roadmap.md), an exhaustiveness test. Small.

### R4. Shipped binaries are built by a toolchain that never verified them

**Impact.** Moderate to high, and hard to diagnose. Continuous integration pins the toolchain;
the workflow that builds the bundles does not, and the release tool installs its own.

**Likelihood.** Low per release, but the failure mode is a code-generation difference that
appears only in the artefact a user installs, which is the worst place to find one.

**Mitigation.** D1. Small, and it may turn out the unpinned channel was deliberate, in which
case the mitigation is a verification job on the same channel rather than a pin.

### R5. The payload budgets gate nothing before a release

**Impact.** Moderate. The three budgets are real and numbered, and they run after the release is
announced. A regression is found at the point it is most expensive to correct.

**Likelihood.** Moderate. The bundle grows with every dependency, and the frontend has several
large ones.

**Mitigation.** D2. The pull request job already builds the bundle.

### R6. The largest file in the workspace has no tests

**Impact.** Moderate to high. The wasm host is 6,489 lines with 169 methods on one type and no
test module; the whole crate has 11 tests, none touching the host. It contains the still-render
pump, the encoding paths, the gizmo drag handling and the frame loop.

**Likelihood.** Certain that changes there are unverified; moderate that a given change breaks
something silently.

**Mitigation.** F2, and note that Group E moves behaviour out of this exact file, so the two
reinforce each other.

### R7. Nothing bounds cook-cache residency on a 32-bit address space

**Impact.** High when it happens, because the failure is the browser tab, not an error message.
A deep chain over a heavy import retains every intermediate, and the browser has roughly four
gigabytes of address space rather than the machine's memory.

**Likelihood.** Unknown, which is itself the problem. No ceiling is stated and no measurement
exists.

**Mitigation.** None today. It needs a stated ceiling first, which is Q9 below.

### R8. The material model disagrees with itself across two renderers

**Impact.** Moderate to high for anyone who renders. One parameter has two incompatible
physical meanings, another is honoured by one renderer and ignored by the other, and texture
filtering differs in both space and mip policy.

**Likelihood.** Certain, and currently visible to anyone comparing viewport to render.

**Mitigation.** Group C, made necessary by
[ADR 0013](adr/0013-path-tracer-is-the-shading-ground-truth.md).

### R9. Migration compatibility is tested against itself

**Impact.** Moderate. Every migration test constructs its input with the current writer, so the
tests prove self-consistency rather than that the migration reads a file an earlier release
wrote. Separately, the schema migration steps once where its documentation describes a loop,
which is correct with one step defined and wrong with two.

**Likelihood.** Low today, certain on the day a second schema version ships.

**Mitigation.** B7 and B9. Both small, and B9 is work only a person with an old build can do.

### R10. Two review models, both maintained, no adapter

**Impact.** Moderate. A review authored on one shell is not readable by the other, and both
models are actively maintained, so the divergence grows.

**Likelihood.** Certain, present.

**Mitigation.** F4, which is blocked on a ruling rather than on effort.

### R11. Silent cache and dependency leaks in the cook

**Impact.** Moderate individually, and each is silent. A grading table surviving a document
load, a bypassed environment still contributing, a primitive attribute lane misaligned after a
topology change, a wrangle that never re-cooks, a parameter reset that does not re-cook its
referrers, and time-driven values frozen at frame zero.

**Likelihood.** Varies per item; several are certain when the triggering action is taken.

**Mitigation.** Group B. All small to medium, and three of them need a ruling before a patch.

### R12. A capability guard that can never fire

**Impact.** Low today, moderate later. The function that asks whether a device can path-trace
cannot return false for any device the application creates, and no shell consults it before
building a tracer. It reads as a safety net and is not one.

**Likelihood.** Low now. It becomes a real risk the moment the limits posture changes, because
the guard will still not fire.

**Mitigation.** Either make it meaningful or remove it. Recorded in
[06b-rendering-and-shading.md](06b-rendering-and-shading.md).

### R13. Multisampling is a user preference never validated against the device

**Impact.** Low to moderate. The preference accepts a sample count that flows unchecked into
texture creation and every multisample-aware pipeline.

**Likelihood.** Unknown, because whether the offered intermediate value is supported on the
adapters actually shipped to has not been established.

**Mitigation.** Validate against device capability, or remove the option. Small either way.

### R14. An undocumented hardware floor on x86_64

**Impact.** Moderate for the affected user, who gets an illegal instruction rather than a
message. The build configuration forces two instruction-set extensions on all x86_64 targets
with no runtime dispatch, and three of the five shipped targets are x86_64.

**Likelihood.** Low and shrinking with hardware age, but the failure is total and unexplained.

**Mitigation.** Decide whether it is a deliberate floor. If it is, publish it in the system
requirements. Recorded as Q13 below.

## Open questions

Grouped by theme. Each is a question the code genuinely cannot answer, phrased so a ruling
closes it. Where two documents asked the same question, it appears once.

### The shared layer and parity

**Q1. Is the desktop node editor a fresh implementation, or does it wait on the shared layer?**
Nothing in the code indicates which. The answer decides whether Group E is a prerequisite for
the desktop editor or runs beside it, which is the difference between one large sequence and
two smaller ones.

**Q2. Which review model wins, the sidecar or the engine model?** Both are actively maintained,
neither has an adapter to the other, and each is used by exactly one shell. Only a decision
closes this; the code argues both ways because both work.

**Q3. Are the two shells' preference stores meant to converge?** There is no migration path and
no shared schema either way, so the current state does not indicate an intent. A browser session
being deliberately independent of a desktop installation is a defensible answer.

**Q4. Should the terminal's live-preview window keep its own GPU device, or route through the
shared render host?** Its own header argues only why it owns its device, which is a narrower
question than whether it should remain a separate GPU host.

**Q5. Is the desktop's file-loaded scene representation intended to survive?** The still-render
path already routes a file load through the document path, which suggests not, but nothing
states it.

### The engine and the cook

**Q6. Is a cross-node parameter read inside a wrangle program a supported feature?** The node's
shipped help advertises it and the cook wires the capability through, but the dependency index
does not see it. If supported, B4 is a defect; if not, the help is wrong. Either way something
changes.

**Q7. Was frame-zero scene lowering chosen deliberately?** Making the lowering see the live
clock would animate time-driven lights, cameras and transforms, and would change what any
time-dependent golden capture lowers. Reproducibility is a legitimate reason to freeze it; the
call sites do not say.

**Q8. Was the grading-table cache deliberately excluded from the engine reset, and should
bypass clear the environment and grading side channels?** The per-node forget path removes the
grading entry, which argues oversight. Bypass clears validation but not the other two, which
argues either way. A test asserting the intended behaviour would settle both.

**Q9. What is the intended residency ceiling for the cook cache on the browser?** Nothing bounds
it today, and the browser's address space is 32-bit. This is the precondition for treating R7 as
anything other than an unknown.

**Q10. Is finer-than-node dirty granularity intended?** Whole-node invalidation is the current
model. Per-output-port or per-parameter granularity is the conventional refinement, and whether
it is wanted decides how much a slider drag is allowed to cost.

**Q11. What is the intended aggregate cook budget for a document with many child networks?**
The per-network forward-progress rule means the effective overrun scales with the number of
networks, and no aggregate cap is stated.

### Rendering and shading

**Q12. Which of the two meanings of `thickness` is the parameter's?** The tracer is
authoritative on shading under ADR 0013, but the rasterizer's optical-path-length reading is the
more conventional one for the name. This blocks C1.

**Q13. Should the inspection modes and the UV layout pane bypass the finishing chain?** They are
display-intent data currently passing through tone mapping, exposure and the grading slots, in
company with two modes that already bypass it. Consistency argues one way; nobody has said which.

**Q14. Are the built-in background colours linear scene values or display values?** The answer
changes what the background looks like and whether the current appearance is the intended one.

**Q15. Is the linear tone mode supposed to differ from no tone mapping?** They are currently
byte-identical, so one of them is redundant or one is unimplemented.

**Q16. What is the plan for the unused principled texture slots?** The traced material record
carries five slots where the imported material carries seventeen. The remaining twelve modulate
scalars the record already holds and arrive only from one import path. Widening the record is
two array lengths and twelve enum arms; nothing persists it, so the choice stays open.

**Q17. Was the absence of GPU timestamp queries a decision?** The workspace requires no optional
GPU features anywhere, and timestamp queries are optional, so measuring per-pass cost means
changing that posture to request-if-available. That is a change to
[ADR 0008](adr/0008-no-optional-gpu-features.md), not just a new measurement.

**Q18. Was the desktop viewport's inability to show a traced pane a scope decision?** The
backend contract, the capability struct and the per-pane keying were all built to make it
possible, and only one shell takes the branch.

### Build, release and platform

**Q19. Was the unpinned toolchain for shipped binaries deliberate?** Picking up newer code
generation for artefacts is a legitimate reason. If it was deliberate, the fix is a verification
job on the same channel rather than a pin.

**Q20. Is the x86_64 instruction-set floor a deliberate, accepted hardware minimum?** If yes, it
belongs in the published system requirements, where it currently does not appear. If no, it
needs runtime dispatch or a lower baseline.

**Q21. Do the two cook budgets differ for a measured reason?** Six milliseconds in the browser
and eight on the desktop, with nothing in either file recording why.

**Q22. Is any configuration other than all-features intended to be supported?** One continuous
integration step exercises a reduced feature set; everything else builds and tests with all
features under Cargo's feature unification, so a user's reduced-feature build is largely
unverified.

**Q23. Are the eleven publishable crates meant to be published?** Only three are marked
otherwise, and one crate's stability claim names a semantic-versioning checker as its guard while
no such job exists in the workflows.

**Q24. Was the lint step's omission of non-library targets deliberate?** The locally documented
command includes them and continuous integration does not, so the two disagree and a body of
code is never linted.

### Product surface

**Q25. Is an external extension surface intended?** The node type identifier's own documentation
reserves a namespace for a future third-party naming scheme, and rejects the separator that
scheme would use. That is the only evidence in the codebase that anyone outside the two
first-party shells is expected to add a node type.

This question is deliberately not designed for anywhere in this set. Public interface stability
is currently scoped to the two shells and the scene file format, and widening that scope is a
product decision with a large architectural consequence: it would make the node type descriptor,
the registry snapshot and the migration mechanism into public interfaces with compatibility
obligations they do not have today.

**Q26. What response headers, including any content security policy, does the deployed site
set?** The edge configuration lives outside this repository and could not be verified from it,
so [06-cross-cutting-concerns.md](06-cross-cutting-concerns.md) states the client-side posture
without being able to state the served one.

**Q27. Where is it recorded that the manual quality checklists were run for a given release?**
The checklists say the record goes into a milestone document that lives outside this repository,
so whether a release was manually verified cannot be answered from here.

## What would close the most

Three rulings unblock a disproportionate amount of the work above: Q1, because it determines
whether Group E is a prerequisite or a parallel track; Q2, because F4 cannot start without it;
and Q12, because C1 and C5 both wait on it.

Three small pieces of work close the most risk: A1, A4 and D2. None is large, and between them
they cover R2, R3 and R5.
