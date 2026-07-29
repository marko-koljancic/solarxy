---
name: solarxy-domain
description: "The shared Solarxy domain briefing: architecture invariants, the node-graph and computer-graphics vocabulary, the surfaces of record, the engagement protocol, the public-surface rules, and the canonical reading list. Load it to prime any session or agent working on Solarxy or Solarxy Web. Every role agent in .claude/agents/ reads this file before forming judgments."
---

# Solarxy Domain Briefing

This file orients; it does not duplicate. It carries only what every role needs and what no
single role owns: the invariants, the shared vocabulary, the surfaces, the public-surface
rules, and the reading list. Anything belonging to one role lives in that agent; anything
that is a current-state fact lives in `CLAUDE.md` (repo root) or the documents listed below.
When this briefing and those disagree, they win, and the disagreement is itself a finding
worth raising.

## What Solarxy is

Solarxy is a cross-platform 3D model viewer, validator, and reviewer in Rust on wgpu,
shipping a desktop GUI and a CLI today, plus a browser app that adds node-based parametric
modeling on the same core compiled to WebAssembly with a React frontend that mirrors engine
state. The product is mid-arc: it began as a viewer and debugger and is becoming an
authoring tool, with pipeline and delivery capability after that. Judgments about scope and
design should account for where on that arc a decision sits. The read-only Minimystix
checkout at `../../References/minimystx/` is an executable specification, never code to copy
verbatim.

## Architecture invariants

These hold everywhere; violating one is never a local decision.

1. **One Rust core, two shells.** The engine (`solarxy-graph`) and the renderer
   (`solarxy-renderer`) never depend on each other; they meet only at
   `solarxy_core::scene::SceneDelta`. On web both compile into one wasm instance, so cooked
   geometry never crosses into JavaScript.
2. **Mirror-and-command.** Rust owns all document state. The React frontend mirrors it from
   `EngineEvent` batches and mutates only by dispatching `Command`s. No document state is
   ever authored in JavaScript.
3. **Registry-driven UI (the zero-frontend-change contract).** The palette, typed handles,
   and parameter panel are pure interpreters of the registry snapshot. A node added in Rust
   needs zero frontend changes; a new `ParamType` or `DataType` variant is a deliberate,
   sanctioned frontend change.
4. **Boundary mirrors move in lockstep.** Any change to `Command`, `EngineEvent`, a snapshot
   shape, or a wasm method updates the hand-authored TS mirrors (`web/src/engine/types.ts`,
   `client.ts`, `session.ts`) and the engine snapshot tests in the same change.
5. **Desktop stays regression-free.** Native CI green at every phase boundary; a desktop
   smoke run whenever shared crates (core, renderer, kernel, formats) change.
6. **Chokepoint discipline.** Where the docs name a single function as the sole mutation path
   for a piece of state (IBL rebuilds, display-flag claims, the exclusive shadow caster),
   parallel paths are defects, not alternatives.

## Node-graph vocabulary

- **Cook**: evaluating a node's output from its inputs and params. Cooking is budgeted and
  resumable; the UI thread never blocks. Auto mode cooks on mutation; Manual mode marks stale
  and cooks on demand.
- **Display flag**: subflow-scoped radio (`active_output`) selecting which node defines a geo
  container's output. Root visibility is a different concept: an additive per-node `visible`
  param.
- **Bypass**: a node passes through, or contributes nothing, per its declared bypass
  behavior, without being removed.
- **Typed ports and coercion**: every port has a `DataType`; connections outside the
  exhaustive coercion matrix are rejected at drop time; lossy coercions are visually marked.
- **Variadic inputs**: ports that accept many connections with stable ordering and reorder
  commands.
- **Registry and versioning**: every node type has a `type_id`, a `type_version`, and an
  optional migration; params are declarative `ParamSpec`s; the registry snapshot is the
  frontend's only source of node knowledge.
- **Transactions**: `BeginTransaction` and `EndTransaction` group commands into one undo step;
  drags preview through a non-committing lane and commit once on release.

## Computer-graphics vocabulary

- The renderer is a multi-pass wgpu pipeline: shadow, gbuffer and SSAO, background or skybox,
  PBR main pass, overlays (grid, gizmo, validation), bloom, composite with tone mapping; per
  pane rendering with independent cameras inside one canvas.
- PBR materials carry factors plus five texture roles (base color, normal,
  metallic-roughness, occlusion, emissive); IBL provides ambient lighting; a single
  directional shadow map serves one exclusive caster light.
- Picking is CPU raycasting (Moller-Trumbore) over retained geometry, pane-aware; gizmo
  hit-testing is analytic ray-versus-handle math, not rendered-ID picking.
- Uniform buffers are hand-laid `#[repr(C)]` structs with explicit padding and size asserts;
  WGSL structs may declare a prefix of the CPU layout and omit trailing fields.
- The WebGPU canvas must never be React-remounted; it is moved or overlaid, never recreated.

## Surfaces of record

Solarxy is not one repository. Five surfaces carry state that can disagree, and keeping them
honest is part of every role's job, not an afterthought.

| Surface | Where | Primary owner |
|---|---|---|
| The code | this repo, `crates/` and `web/` | the three implementer roles |
| Planning docs and the roadmap artifact | `../../Docs/`, `../../Artifacts/solarxy-roadmap.html` | technical writer, with the product manager on intent |
| GitHub project, issues, milestones, labels | the public board and repo | product manager at milestone level, product owner below it |
| The public site | `web/` pages, edge and deploy in the separate mpw repo | product designer and frontend engineer, devops for edge and deploy |
| The wiki | `Sources/solarxy.wiki`, `develop` merged to `master` | technical writer |

They are separate repositories and cannot be updated atomically. A change that lands on one
surface opens a sweep against `.claude/skills/solarxy-sync/SKILL.md`, which is the only
guarantee they stay aligned. Silent drift between surfaces is the failure mode being designed
against, and it has happened before.

## Canonical reading list

Paths are relative to this repository's root. The workspace root is one level above
`Sources/`, so workspace content is reached with `../../`.

| Document | What it answers |
|---|---|
| `CLAUDE.md` (repo root) | Current code truth: crates, build and test commands, render pipeline, key patterns, working agreement. Read first, always. |
| `../../CLAUDE.md` | Workspace truth: repo layout, where a given change belongs, the release train, cross-repo etiquette. |
| `../../Docs/SOLARXY-ROADMAP.md` | The living plan. Part I: product snapshot (A), architecture (B), shipped history (C), active milestone (D), release program to 1.0 (E), personas and journeys (F). Part II: the long-range roadmap cards. |
| `../../Docs/SOLARXY-MILESTONE-PROGRAM.md` | The release ladder and how work is assigned to releases. |
| `../../Docs/SOLARXY-MILESTONE-<version>.md` | The build-ready spec for one release, with its decision log and amendments. |
| `../../Docs/Archive/SOLARXY-NODE-CATALOG.md` | The node-system contract: types, ports, coercion, params, versioning, registry. |
| `../../Docs/Archive/SOLARXY-UX-SPEC.md` | Personas, journeys, interaction model, keymap policy, the realtime UX contract. |
| `../../Docs/Archive/SOLARXY-WEB-INTEGRATION-PLAN.md` | Milestone architecture, the decision log, the wasm boundary, the scene format. |
| `../../Docs/Archive/SOLARXY-WEB-INTEGRATION-IMPLEMENTATION-LOG.md` | What actually happened per phase: deviations, measurements, continuation notes. |
| `../../Artifacts/solarxy-roadmap.html` | The hand-authored twin of the roadmap. Nothing generates or validates it, so it drifts unless updated deliberately. |

Read the section you need, not the whole document. `CLAUDE.md` is 270 lines and costs real
context; only the architect and the implementer roles routinely need it whole.

## How every role engages

**The engagement protocol lives in the Working Agreement section of `CLAUDE.md`.** Read that
section before proposing any plan. It states the full sequence: the ambiguity sweep, the
single batched question round, the 90 percent confidence bar, the plan format, and the rule
that execution waits for explicit confirmation.

What you need at decision time, without following the pointer, is when it applies. **It is
tiered by stakes, not universal.** Run it in full when the work changes scope, touches a
public surface, revisits a ratified decision, spans more than a couple of files, or is hard to
reverse. Skip it for a narrow unambiguous ask (a named bug, a single file, a direct question),
where the right move is to state assumptions inline and proceed. Asking three questions about
a one-line fix is its own failure.

Resolve ambiguity yourself first where the answer is knowable: read the code, check the
amendment history, run the search. Questions are for what genuinely cannot be determined.

## Public-surface rules

Everything under `.claude/`, the GitHub board and its issues, the public site, the wiki,
release notes, and commit messages are public artifacts. They are written to that standard.

- **Redaction.** Describe every capability in Solarxy-native terms. The reference checkouts
  under `../../References/` exist for inspiration and parity measurement only, and are never
  named in any public artifact, in code, or in a milestone spec. The sole exception is the
  Minimystix prototype, citable as an executable specification. Houdini and Blender are
  citable when an outside reference genuinely clarifies intent. Verify by deriving the
  wordlist rather than hard-coding it, which also keeps the redacted names out of tracked
  files:

  ```
  for n in $(ls ../../References/ | grep -v minimystx); do
    grep -ril "$n" <paths> && echo "REDACTION FAILURE: $n"
  done
  ```

  `../../References/` is gitignored, so the wordlist lives outside version control by
  construction. Only the living roadmap benchmarks against outside engines by name, and it is
  not currently a public document.
- **No secrets or operational detail.** No VPS paths, hostnames, tokens, or personal
  information in any file under `.claude/` or in any public item.
- **Writing style.** No emojis. No em dashes or en dashes; use a plain hyphen, a comma, or
  restructure the sentence. No decorative symbol glyphs such as arrows. No horizontal-rule
  dividers in body text; a YAML frontmatter fence is the only permitted use of three hyphens
  on their own line. This binds every role, so no agent restates it.
- **Planning codes never leave the planning docs.** Decision numbers, work-item codes, and
  stage and phase numbers are meaningless without the document open, and they collide across
  milestones. Comments, doc comments, test names, log lines, and public item text carry the
  substance instead. Version references are fine and often load-bearing. Enforced by
  `no_planning_codes_in_comments` in `crates/solarxy-core/tests/tokens_drift.rs`.

## Conventions that bind every role

- **Amendment discipline.** The planning docs are baselines. Scope changes ratify before
  execution; deviations record at code-completion, as dated level-3 headings with numbered,
  bold-led entries, newest first.
- **Exit criteria are the contract.** Scope pressure resolves by deferring to the documented
  backlog, never by quietly weakening a criterion.
- **The maintainer runs git.** Agents and sessions provide commit messages and steps; they do
  not commit or push. Messages, branch names, and pull request titles follow
  `.claude/skills/solarxy-git/SKILL.md`.
- **Findings cite `file:line`.** A claim about code not grounded in a read of that code is not
  a finding.

## Conflict rule

Defer current-state facts to `CLAUDE.md` and the code. If the code and `CLAUDE.md` disagree,
that is itself a finding. If a planning doc and the code disagree, check the amendment
history before assuming either is wrong; an unamended divergence is the finding.
