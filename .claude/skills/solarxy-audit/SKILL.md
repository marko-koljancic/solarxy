---
name: solarxy-audit
description: "Thorough code-quality audit of the Solarxy workspace. Reviews Rust idioms, wgpu and WGSL correctness, egui patterns, ratatui where used, the TypeScript and React frontend, the public pages, architecture, performance, safety, comment quality and maintainability, workspace hygiene, and cross-platform readiness. Defers current-state facts to CLAUDE.md and audits code against it. Outputs findings grouped by severity with concrete fixes. Runs in an isolated subagent so the audit output does not pollute the main session."
disable-model-invocation: true
context: fork
agent: Explore
allowed-tools:
  - Read
  - Grep
  - Glob
  - Bash(cargo *)
  - Bash(rg *)
  - Bash(fd *)
  - Bash(git diff*)
  - Bash(git log*)
  - Bash(wc *)
  - Bash(tokei*)
  - Bash(npm run typecheck*)
  - Bash(npm test*)
---

# Solarxy code-quality audit

You are a senior reviewer with deep expertise in Rust systems programming, GPU programming
(wgpu and WGSL), immediate-mode GUIs (egui), TUI applications (ratatui), and the React and
TypeScript frontend.

## How this skill is structured

This rubric is **timeless principles**. It deliberately does not name current-moment facts
like which enum variants exist, which feature flag is called what, or the current render-pass
count. Those move.

**`CLAUDE.md` at the repo root is the source of truth for current state.** Read it first. The
audit's job is to verify the code matches what `CLAUDE.md` claims, and to apply the principles
below. Where this rubric says "per CLAUDE.md", look up the current state and audit against it.
If `CLAUDE.md` does not mention something the code does, that itself is a finding.

## Scope

By default, audit the entire workspace. Honour a user-narrowed scope. Before starting, run
`cargo metadata --format-version 1 --no-deps` to confirm members, `git diff --stat main...HEAD`
if reviewing a branch, and a rough size check.

Verify every claim by grep or glob. A finding without a `file:line` is not a finding.

## Categories

### 0. CLAUDE.md and code consistency

The meta-check that makes the rest reliable. Run it first.

- Workspace layout, crate names and roles match `[workspace] members`.
- Feature flags listed match each crate's `Cargo.toml`.
- Enumerated variants match the source.
- Render-pass order matches the frame-orchestration code.
- Named plumbing rules (chokepoint functions, multi-call-site functions, types-on-both-sides)
  match the code.
- Per-crate clippy allow lists are consistent with the stated lint policy.
- Build commands, CI commands, MSRV, and version single-sourcing match the real files.

Structural drift (a named chokepoint that no longer exists, a renamed feature flag, a removed
variant) is Critical, because a stale `CLAUDE.md` misdirects every subsequent piece of work.
Cosmetic drift is a Warning.

### 1. Rust idioms

- Ownership and borrowing: no unnecessary `clone`, `Arc`, or `Rc`. Shared ownership is
  justified only where it is real.
- Error handling: `thiserror` in library crates, `anyhow` in binaries. Zero `unwrap` or
  `expect` outside tests. Errors propagate with `?` and gain `.context(...)`. Library crates
  do not expose `anyhow::Error` publicly.
- Exhaustive matches; no catch-all that would silently absorb a new variant.
- Trait objects versus generics chosen deliberately; no premature dynamic dispatch.
- Iterators over manual loops; fallible mapping collects into a `Result`.
- Edition 2024 conventions, including captured identifiers in format strings.
- `pub(crate)` and `pub(super)` over bare `pub`, which is reserved for documented boundaries.
- Per-crate clippy allow lists travel with code that moves between crates.

### 2. wgpu and WGSL

On the Rust side: buffers pre-allocated, reused, and written in batched calls; pipelines built
at init and reused, through the documented builder rather than raw descriptors; bind group
layouts constructed once from the single source of truth, with bind groups rebuilt but layouts
never redefined ad hoc; chokepoint discipline, where any function named as the sole mutation
path for a piece of state is verified by grep to still be the only one, and a parallel path is
Critical; multi-call-site rules, where adding a parameter updates every named call site;
constructor-family completeness, where every constructor of a state type computes its shared
derived value; uniform alignment, where each CPU uniform is `#[repr(C)]` with explicit padding
and any size assert is updated in lockstep with the matching shader; pass ordering as
documented, with the final overlay pass loading rather than clearing; per-pane rendering with
one encoder submission per pane and scissor rects; surface errors reconfiguring rather than
panicking; GPU resources dropping before the device.

On the WGSL side: bind group and binding indices match the Rust layout, since a mismatch is
silent corruption; alignment annotations match the CPU layout; sampling correctness for linear
versus sRGB and appropriate LOD; no division by zero in shading paths, especially in
inspection modes; tone mapping applied once, in the composite pass, never duplicated upstream.

**The prefix-shape rule is an intentional pattern and must not be flagged as a bug.** A WGSL
struct may declare a prefix of the CPU uniform and omit trailing fields it does not read,
because size is enforced at the binding rather than the shape. A field missing from the
*middle*, which shifts alignment, is Critical.

### 3. egui patterns

- egui is the last pass, loading rather than clearing, after all scene rendering.
- Input forwarding reaches egui first, and the camera handler skips events egui consumed.
- The snapshot read and writeback pair is the only way egui mutates app state. Taking a
  mutable reference to state inside an egui closure is a bug regardless of API shape, and a
  new control must wire both halves.
- If a toast helper emits a tracing event, callers must not also log the same message.
- Viewport is reset before the egui pass when split panes were used earlier in the frame.
- Preferences-modal scope is strictly fields the live surfaces cannot reach at runtime; a
  modal field duplicating a live control is a finding.
- Panel visibility routes through the single documented surface.

### 4. ratatui, where used

Audit only the TUI surfaces `CLAUDE.md` identifies; do not invent usage elsewhere. Check the
terminal setup and restore pair with a panic hook that restores; separation of state,
rendering, and event handling; layout constraints that survive small terminals; and key events
filtered to presses, without which repeat doubles events on some terminals.

### 5. Architecture and design

- Crate boundaries hold as documented, and the dependency direction never inverts.
- Types used on both sides of a boundary live in the shared core crate.
- The documented ownership tree is intact; fields drifting back into a god-struct are a smell.
- GPU-free crates' tests run without a device.
- No circular dependencies between crates or modules.

### 6. Performance and safety

- No per-frame allocation in the render loop; pre-allocate and clear.
- Buffer writes consolidated; partial writes push only what changed.
- Readbacks are async or amortized, never a per-frame stall.
- `unsafe` is sound, minimal, and each block carries a safety comment naming its invariants.
- No reference cycles; explicit drop order where it matters.
- No panicking paths on user-supplied geometry: indexing, overflow, and unchecked counts.
- No blocking calls in the render loop, and no filesystem access inside UI closures.

### 7. Workspace and cargo health

- Shared versions single-sourced in workspace dependencies; members inherit.
- Version single-sourced in workspace package metadata; no crate pins its own.
- Any crate targeting wasm compiles with no default features and a minimal footprint, and CI
  verifies it. Missing that CI step is Critical.
- Feature combinations compile; CI exercises both all-features and the minimal path.
- Dependency hygiene: audit, unused dependencies, duplicate versions.
- The CI gate covers formatting, clippy with warnings denied, tests, docs with warnings
  denied, the no-default-features doc check, and release builds on the documented platforms.
- MSRV agrees across every file that states it.

### 8. Cross-platform readiness

Run when wasm is in scope. Platform-specific behavior sits behind the documented abstraction;
both targets compile under their cfg gates; wasm-targeting crates build for
`wasm32-unknown-unknown` unmodified; no wall-clock instant type and no filesystem access on
the web path.

### 9. TypeScript and React frontend

- **The frontend owns no document state.** Rust owns it; the frontend mirrors it and mutates
  only by dispatching commands. A component holding document state is Critical.
- **Boundary mirrors move in lockstep.** A change to a command, event, snapshot, or wasm
  method must update the hand-authored TypeScript mirrors and their shape tests in the same
  change. A mirror that drifted from the Rust serde shape is Critical, because it fails at
  runtime rather than at compile time.
- **The registry-driven contract holds.** The palette, typed handles, and parameter panel
  interpret the registry snapshot rather than hard-coding node knowledge. A node-type name
  appearing in the frontend outside the one sanctioned exception is a finding.
- **The WebGPU canvas is never React-remounted.** Layout changes move or overlay it. Missing
  keys on split children, or a conditional that unmounts the canvas subtree, is Critical.
- Typing: no `any` at the boundary; discriminated unions for event and command variants so a
  new variant fails the build rather than falling through.
- Effects and subscriptions clean up; no listener or worker leaks across panel remounts.
- Per-frame work stays out of the React render path.

### 10. Public pages

- A page has both a build entry and a matching exact-match route at the edge. A page with no
  route silently serves the landing page with a 200, so the audit checks for the pair.
- No third-party font, style, or script reference, which the content security policy blocks in
  production while working locally.
- Every heading that is a link target has a stable id and scroll margin that clears the fixed
  nav.
- Both themes are styled; contrast is checked in both; pastels are used as fills rather than
  as text color.
- Wide content scrolls inside its own container; the page body never scrolls horizontally.
- Motion is gated behind a reduced-motion preference.

### 11. Comment quality and maintainability

The standard the implementer agents are held to, made auditable.

- **No planning codes.** Phase numbers, stage numbers, work-item codes, and decision
  identifiers are meaningless without the planning document open, and they collide across
  milestones. Enforced by `no_planning_codes_in_comments` in
  `crates/solarxy-core/tests/tokens_drift.rs`, which also scans test titles because those
  appear in CI output. A comment carrying one is a finding even if the test does not yet catch
  its form. Version references are fine and often load-bearing.
- **No comment that restates the code.** A comment directly above a line that says what the
  line says is noise that rots. Flag them.
- **No changelog in comments.** History belongs in git.
- A comment earns its place by explaining why, naming a constraint the code cannot show, or
  recording an invariant. Missing comments on genuinely non-obvious invariants are a finding
  in the other direction.
- **Architectural fit.** Flag changes that bolt onto a seam rather than extending it,
  abstractions introduced speculatively with one caller, and logic placed where a reader would
  not look for it.
- **Scaling.** Flag anything whose cost grows with node count, object count, or document size
  in a way the design does not account for.

## Output format

For each finding: the exact `file:line`; a severity of Critical, Warning, or Suggestion; the
category number; the issue in one or two sentences; and a concrete fix, inlined as a diff for
mechanical ones or named as a pattern for architectural ones. Add evidence, meaning the
command output or the regex hit, where it helps.

Severity guidance. **Critical** covers panics in production paths, unsoundness, broken
invariants (layout drift, chokepoint bypass, snapshot-pattern violation, a boundary mirror out
of step, a remounted canvas, a no-default-features regression), CI-blocking issues, and
security problems. **Warning** covers avoidable allocations in hot paths, missing error
context, idiom violations that compound, coverage gaps in critical modules, non-structural
doc drift, and comment-quality violations. **Suggestion** covers stylistic improvements,
opportunistic refactors, and minor wins.

## Final summary

End with a counts table by category and severity; the five highest-impact improvements ordered
by severity then reach, each naming the files, the one-sentence fix, and the expected payoff;
and a short paragraph naming what is healthy, which calibrates the rest and prevents a report
where everything reads as critical.

## How to behave

- Read `CLAUDE.md` first and treat it as the spec.
- Verify with grep and glob, never from memory.
- Do not invent specifics this skill does not name; where the rubric defers, look it up.
- If a category is clean, say so. Do not pad the report.
- Breadth before depth on the first pass: every category with its top few issues beats forty
  findings in one category.
- Surface, do not execute. Multi-file refactors are raised as findings, never implemented.
