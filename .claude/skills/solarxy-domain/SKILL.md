---
name: solarxy-domain
description: "The shared Solarxy domain briefing: architecture invariants, the node-graph and computer-graphics vocabulary, the canonical reading list, and the conflict rule. Load it to prime any session or agent working on Solarxy or Solarxy Web. Every role agent in .claude/agents/ reads this file before forming judgments."
---

# Solarxy Domain Briefing

This file is deliberately thin. It orients; it does not duplicate. Current-state facts live in `CLAUDE.md` (repo root) and the planning documents listed below; when this briefing and those documents disagree, they win, and the disagreement is itself a finding worth raising.

## What Solarxy is

Solarxy is a cross-platform 3D model viewer, validator, and reviewer in Rust on wgpu, shipping a desktop GUI and CLI today. The active milestone (Solarxy Web) merges it with the Minimystix prototype into a browser app: node-based parametric modeling plus the full inspection/validation/review toolset, on one shared Rust core compiled to WebAssembly, with a React frontend that mirrors engine state. The read-only Minimystix checkout at `../_reference/minimystx/` is an executable specification, never code to copy verbatim.

## Architecture invariants

These hold everywhere; violating one is never a local decision.

1. **One Rust core, two shells.** The engine (`solarxy-graph`) and the renderer (`solarxy-renderer`) never depend on each other; they meet only at `solarxy_core::scene::SceneDelta`. On web both compile into one wasm instance, so cooked geometry never crosses into JavaScript.
2. **Mirror-and-command.** Rust owns all document state. The React frontend mirrors it from `EngineEvent` batches and mutates only by dispatching `Command`s. No document state is ever authored in JavaScript.
3. **Registry-driven UI (the zero-frontend-change contract).** The palette, typed handles, and parameter panel are pure interpreters of the registry snapshot. A node added in Rust needs zero frontend changes; a new `ParamType` or `DataType` variant is a deliberate, sanctioned frontend change.
4. **Boundary mirrors move in lockstep.** Any change to `Command`, `EngineEvent`, a snapshot shape, or a wasm method updates the hand-authored TS mirrors (`web/src/engine/types.ts`, `client.ts`, `session.ts`) and the engine snapshot tests in the same change.
5. **Desktop stays regression-free.** Native CI green at every phase boundary; a desktop smoke run whenever shared crates (core, renderer, kernel, formats) change.
6. **Chokepoint discipline.** Where the docs name a single function as the sole mutation path for a piece of state (IBL rebuilds, display-flag claims, the exclusive shadow caster), parallel paths are defects, not alternatives.

## Node-graph vocabulary

- **Cook**: evaluating a node's output from its inputs and params. Cooking is budgeted and resumable; the UI thread never blocks. Auto mode cooks on mutation; Manual mode marks stale and cooks on demand.
- **Display flag**: subflow-scoped radio (`active_output`) selecting which node defines a geo container's output. Root visibility is a different concept: an additive per-node `visible` param.
- **Bypass**: a node passes through (or contributes nothing) per its declared bypass behavior, without being removed.
- **Typed ports and coercion**: every port has a `DataType`; connections outside the exhaustive NxN coercion matrix are rejected at drop time; lossy coercions are visually marked.
- **Variadic inputs**: ports that accept N connections with stable ordering and reorder commands.
- **Registry and versioning**: every node type has a `type_id`, a `type_version`, and an optional migration; params are declarative `ParamSpec`s; the registry snapshot is the frontend's only source of node knowledge.
- **Transactions**: `BeginTransaction`/`EndTransaction` group commands into one undo step; drags preview through a non-committing lane and commit once on release.

## Computer-graphics vocabulary

- The renderer is a multi-pass wgpu pipeline: shadow, gbuffer/SSAO, background or skybox, PBR main pass, overlays (grid, gizmo, validation), bloom, composite with tone mapping; per-pane rendering with independent cameras (F1 to F5 layouts) inside one canvas.
- PBR materials carry factors plus five texture roles (base color, normal, metallic-roughness, occlusion, emissive); IBL provides ambient lighting; a single directional shadow map serves one exclusive caster light.
- Picking is CPU raycasting (Moller-Trumbore) over retained geometry, pane-aware; gizmo hit-testing is analytic ray-versus-handle math, not rendered-ID picking.
- Uniform buffers are hand-laid `#[repr(C)]` structs with explicit padding and size asserts; WGSL structs may declare a prefix of the CPU layout.
- The WebGPU canvas must never be React-remounted; it is moved or overlaid, never recreated.

## Canonical reading list

Paths are relative to this repository's root; the planning docs live one level up at the workspace root.

| Document | What it answers |
|----------|-----------------|
| `CLAUDE.md` (repo root) | Current code truth: crates, build/test commands, render pipeline, key patterns, working agreement. Read first, always. |
| `../SOLARXY-WEB-INTEGRATION-PLAN.md` | The milestone architecture, the 29-decision log, phases 0 to 7, risks, and the amendment history. |
| `../SOLARXY-WEB-PHASE8-EXPANSION.md` | The road to public beta: phases 8 to 16, ratified decisions, technical designs (gizmos, Image pipeline, deploy, render flags, docking), per-phase exit criteria. |
| `../SOLARXY-NODE-CATALOG.md` | The node-system contract (types, ports, coercion, params, versioning, registry) and the full node catalog with per-node specs. |
| `../SOLARXY-UX-SPEC.md` | Personas, user journeys, the interaction model, keymap policy, the realtime UX contract, display-flag semantics. |
| `../SOLARXY-WEB-INTEGRATION-IMPLEMENTATION-LOG.md` | What actually happened per phase: deviations, measurements, continuation notes. |

## Conventions that bind every role

- **Amendment discipline.** The planning docs are baselines. Scope changes ratify before execution; deviations record at code-completion, as dated level-3 headings with numbered, bold-led entries, newest first. Silent drift is the failure mode being designed against.
- **Exit criteria are the contract.** Scope pressure resolves by deferring to the documented backlog, never by quietly weakening a criterion.
- **Verification is Chrome-only** until the Phase 16 QA matrix, by standing decision; code-verified branches are called out as such in the log.
- **The maintainer runs git.** Agents and sessions provide commit messages and steps; they do not commit or push.
- **Findings cite `file:line`.** A claim about code that is not grounded in a read of that code is not a finding.

## Conflict rule

Defer current-state facts to `CLAUDE.md` and the code. If the code and `CLAUDE.md` disagree, that is itself a finding. If a planning doc and the code disagree, check the amendment history before assuming either is wrong; an unamended divergence is the finding.
