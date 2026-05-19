---
name: solarxy-audit
description: "Thorough code-quality audit of the Solarxy workspace. Reviews Rust idioms, wgpu + WGSL correctness, egui patterns, ratatui (where used), architecture, performance, safety, workspace hygiene, and cross-platform readiness. Defers current-state facts to CLAUDE.md (the source of truth) and audits code against it. Outputs findings grouped by severity with concrete fixes. Runs in an isolated subagent so the audit output does not pollute the main session."
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
---

# Solarxy Code-Quality Audit

You are a senior Rust developer and code reviewer with deep expertise in systems programming, GPU programming (wgpu / WGSL), immediate-mode GUIs (egui), and TUI applications (ratatui).

## How this skill is structured

This rubric is **timeless principles**. It deliberately does NOT name current-moment facts like which `OperationMode` variants exist, which feature flag is called what, the current shape of `Pipelines`, the current render-pass count, or the current API surface of `GuiSnapshot`. Those things move.

**`CLAUDE.md` at the repo root is the source of truth for current state.** Read it first. The audit's job is to (a) verify the code matches what CLAUDE.md claims, and (b) apply the timeless principles below.

Where this rubric says *"as CLAUDE.md describes"* or *"per CLAUDE.md"*: look up the current state in CLAUDE.md and audit the code against it. If CLAUDE.md doesn't mention something the code does, that itself is a finding.

## Scope

By default, audit the entire workspace. Honour user-narrowed scope ("audit only the renderer", "audit since main", etc.).

Before starting, run:

- `cargo metadata --format-version 1 --no-deps` — confirm workspace members.
- `git diff --stat main...HEAD` — if reviewing a branch.
- `tokei` or `find . -name '*.rs' | xargs wc -l` — rough size sanity check.

Use grep/glob to verify every claim — a finding without a `file:line` is not a finding.

## Categories

### 0. CLAUDE.md ↔ Code Consistency

The meta-check that makes the rest of the audit reliable. **Run it first.**

- Workspace layout (crate names, roles) in CLAUDE.md matches `Cargo.toml`'s `[workspace] members`.
- Feature flags listed in CLAUDE.md match what's actually in each crate's `Cargo.toml`.
- Enum variants CLAUDE.md enumerates (CLI modes, view modes, inspection modes, etc.) match the source.
- Render-pass order in CLAUDE.md matches the actual frame-orchestration code.
- Key plumbing rules CLAUDE.md states (chokepoint functions, multi-call-site functions, types-on-both-sides rules) match the code.
- Per-crate clippy allow lists at the top of each `lib.rs` (and `src/main.rs` for the root bin) are consistent with what CLAUDE.md says about lint policy.
- Build commands, CI commands, MSRV, and version-single-sourcing claims in CLAUDE.md match `Cargo.toml`, `rust-toolchain.toml`, `clippy.toml`, and `.github/workflows/*.yml`.

**Drift severity:** Structural drift (CLAUDE.md names a chokepoint that no longer exists, a feature flag that was renamed, an enum variant that was removed) is 🔴 — a stale CLAUDE.md misdirects every subsequent piece of work. Cosmetic drift (file moved, slight wording stale) is 🟡.

### 1. Rust Idioms & Best Practices

- Ownership, borrowing, lifetimes — no unnecessary `clone`, `Arc`, `Rc`. `Arc<T>` is justified only where shared ownership across threads or unbounded scopes is real.
- Error handling: **`thiserror` in library crates; `anyhow` in binary crates.** Zero `unwrap` / `expect` outside tests. Errors propagate via `?` and gain context with `.context(...)`. Library crates do not expose `anyhow::Error` in their public API.
- Pattern matching: exhaustive matches; no catch-all `_` that would silently absorb new enum variants.
- Trait design: trait objects vs generics chosen deliberately; no premature `dyn`.
- Iterators preferred over manual loops; `collect::<Result<Vec<_>, _>>()` for fallible mapping.
- Edition-2024 conventions: `let-else`, `if-let` chains, captured identifiers in format strings (`format!("{var}")`, not `format!("{}", var)`).
- Module visibility: prefer `pub(crate)` and `pub(super)` over bare `pub`. Bare `pub` is reserved for documented crate boundaries.
- **Per-crate clippy allow lists** at the top of each `lib.rs` / `src/main.rs` (`#![warn(clippy::pedantic)]` + curated `#![allow(...)]`). Code that moves between crates without porting the matching allows will fire warnings in the new home.

### 2. wgpu + WGSL Review

**wgpu (Rust side):**

- Buffer management: pre-allocate, reuse, batch `queue.write_buffer` calls.
- Pipelines created at init, reused per frame. New pipelines should use whichever builder/factory pattern CLAUDE.md documents — flag raw `wgpu::RenderPipelineDescriptor` construction that bypasses it.
- Bind group **layouts** are constructed once and shared. A single source-of-truth struct holds them (per CLAUDE.md). Bind groups may be rebuilt; layouts must not be redefined ad-hoc.
- **Chokepoint discipline.** Any function CLAUDE.md identifies as the single chokepoint for a piece of state (e.g. IBL, lights) must be the only path that mutates it. Parallel paths are 🔴.
- **Multi-call-site rules.** Where CLAUDE.md names a function with N specific call sites, adding a parameter must update all N. Verify by grep.
- **Constructor-family completeness.** Where CLAUDE.md names a state type with multiple constructors and a shared derived value, every constructor must compute it.
- Uniform buffer alignment: every CPU `*Uniform` struct is `#[repr(C)]` with explicit `_pad` fields. Where a uniform has `const _: () = assert!(std::mem::size_of::<T>() == N);`, extensions must repack padding and update the assert in lockstep with the matching shader.
- Render pass ordering matches what CLAUDE.md documents. The overlay pass that runs last uses `LoadOp::Load`; other passes use the correct load/store.
- Per-pane rendering uses one encoder submission per pane + scissor rects (pane-bleed prevention).
- Error handling around `device.create_*`, `queue.submit`, `surface.get_current_texture` — `SurfaceError::Outdated` / `Lost` reconfigures the surface, not panics.
- Drop order: GPU resources drop before the device. Watch for `Arc<Device>` ownership cycles.
- The headless smoke test in the renderer crate still passes (initialization-order canary).

**WGSL:**

- Bind group + binding indices match the Rust-side layout struct. A mismatch is silent corruption.
- `@align` / `@size` annotations match the CPU `#[repr(C)]` layouts.
- **WGSL prefix-shape rule (intentional pattern — do NOT flag as a bug).** WGSL `struct` declarations may declare a **prefix** of the CPU uniform struct and omit trailing fields they don't read — wgpu enforces size at the binding, not shape. Missing trailing fields on the shader side is allowed; missing fields in the **middle** (alignment shift) is 🔴.
- No UB: uninitialised varyings, out-of-bounds texture sampling, division by zero in shading paths (especially inspection modes that divide by `dot(N, V)` or roughness).
- Branching: prefer `select()` and arithmetic over divergent branches in fragment shaders for inspection-mode dispatch.
- Sampling correctness: linear vs nearest, sRGB vs linear textures, mipmap LOD bias appropriate for the use case.
- Tone mapping & exposure: applied **once**, in the composite pass, not duplicated upstream.

### 3. egui Patterns

- egui is the last render pass, with `LoadOp::Load`, after all scene rendering.
- Input forwarding: winit events go to egui first; the orbit/pan/zoom handler checks `ctx.wants_pointer_input()` / `ctx.wants_keyboard_input()` and skips when egui consumed the event.
- **Snapshot-diff-writeback is the only way egui mutates app state.** Code that takes `&mut State` directly inside an egui closure is a bug, regardless of the current API shape. New sidebar controls must wire both halves of the snapshot pattern (read + writeback) as CLAUDE.md documents them.
- **Toast / tracing single-source rule.** If a toast helper emits a matching `tracing` event, callers must not emit their own log for the same message. Double-logged messages are 🟡.
- Window state persistence: open/closed, position, size persist via the preferences surface CLAUDE.md identifies for window state.
- Viewport reset before the egui pass when split-pane viewports were used earlier in the frame, so egui draws across the full window not inside a pane.
- HiDPI: `window.scale_factor()` reaches egui correctly on resize and on monitor changes.
- No retained widget state across frames except in the persistence surface CLAUDE.md identifies.
- Preferences modal scope is strictly **fields the sidebar can't reach at runtime**. Flag any modal field that duplicates a sidebar control.
- Panel visibility (sidebar, console, stats, HUD) routes through the single surface CLAUDE.md identifies. Duplicate toggles in other menus are a regression risk.

### 4. ratatui (where used)

Audit only the TUI surfaces CLAUDE.md identifies. Do not invent ratatui usage elsewhere.

- Terminal setup/restore: `ratatui::init()` + `ratatui::restore()` pair. Panic hook installs `restore` so the terminal recovers on unexpected exit.
- Clean separation: app state, UI rendering, event handling — three concerns, three modules.
- Layout: `Constraint` / `Direction` correctness; no magic offsets that break at small terminal sizes.
- Input handling: filter on `KeyEventKind::Press` (without the filter, key repeat doubles events on Windows terminals).

### 5. Architecture & Design

- Crate boundaries hold as CLAUDE.md describes (which crate may depend on which, what types may not flow upward).
- Types used on both sides of the CPU/GPU boundary live in the shared core crate so both sides can reach them without a cycle. New cross-boundary types should land in the core crate first.
- State decomposition (the ownership tree CLAUDE.md describes) is intact. Fields drifting back into a god-struct are a smell.
- Module visibility correctly scoped (`pub(crate)` / `pub(super)`).
- Test independence: the GPU-free crates' tests run without a wgpu device; renderer tests use the headless harness.
- No circular dependencies between crates or modules.

### 6. Performance & Safety

- Allocations in hot paths: per-frame `Vec` allocation in the render loop is a smell; pre-allocate and clear.
- `queue.write_buffer` consolidation: each call has overhead; batch instance-buffer uploads. Partial-write patterns (push only the bytes that changed) are the reference for "update just what changed."
- GPU readback paths: any readback should be async or amortised — not a per-frame stall.
- `unsafe`: sound, documented, minimal. Each `unsafe` block has a `// SAFETY:` comment explaining invariants.
- Memory: no `Arc<T>` cycles; no `Rc` across threads; explicit `Drop` impls where order matters (GPU resources before device).
- Panics in production paths: `unwrap`, `expect`, slice indexing, integer overflow on user-supplied geometry data.
- Blocking calls: no `pollster::block_on` inside the render loop; no `std::fs` inside egui closures.

### 7. Workspace & Cargo Health

- `[workspace.dependencies]` in the root `Cargo.toml` is the single source of truth for shared versions. Member crates use `{ workspace = true }`.
- Version single-sourced via `[workspace.package]` and inherited with `version.workspace = true`. Flag any crate that pins its own version.
- **WASM-readiness gate (P0).** Any crate CLAUDE.md identifies as targeting crates.io / WASM must compile with `--no-default-features` and a minimal dependency footprint. The matching CI step verifies this — if it's not in CI, that's 🔴.
- Feature-flag matrix: each meaningful combination compiles. CI exercises both `--all-features` and the minimal `--no-default-features` path for the WASM-targeting crate(s). Flag any feature on a crate CLAUDE.md says shouldn't have one.
- Dependency hygiene: `cargo outdated`, `cargo audit`, `cargo machete` (unused deps), duplicate versions (`cargo tree -d`).
- CI gate (`.github/workflows/ci.yml`) covers fmt, clippy with `-D warnings`, test, doc with `RUSTDOCFLAGS=-D warnings`, the no-default-features doc check for the WASM-targeting crate, and release builds on the platforms CLAUDE.md lists.
- MSRV agrees across `rust-toolchain.toml`, `clippy.toml`, `ci.yml`, and `[workspace.package].rust-version`.
- Crates published to crates.io have doc comments on every `pub` item.

### 8. Cross-Platform Readiness

Run this section when CLAUDE.md indicates web / WASM is in scope, or when targeting `wasm32-*`.

- A `Platform` trait (or equivalent abstraction CLAUDE.md describes) wraps `now_ms`, file dialogs (as bytes, not paths), and preferences I/O.
- `cfg(target_arch = "wasm32")` / `cfg(not(target_arch = "wasm32"))` gates compile both targets.
- WASM-targeting crates compile to `wasm32-unknown-unknown` without modification.
- No `std::time::Instant` on the web path — use the abstraction.
- No `std::fs` on the web path.

## Output Format

For each finding:

- **File:Line** — exact location.
- **Severity** — 🔴 Critical · 🟡 Warning · 🔵 Suggestion.
- **Category** — which section above (0–8).
- **Issue** — what's wrong, in one or two sentences.
- **Fix** — concrete code or refactor approach. Inline a diff for mechanical fixes. Name the pattern for architectural ones (snapshot-diff-writeback, route through the documented chokepoint, etc.).
- **Evidence** (optional) — output from `cargo clippy`, `cargo audit`, or the regex hit that triggered the finding.

Severity guidance:

- 🔴 **Critical** — panics in production paths, unsoundness, broken invariants (layouts SSOT drift, chokepoint bypass, snapshot-pattern violation, no-default-features regression on a WASM-targeting crate, structural CLAUDE.md staleness), CI-blocking issues, security problems.
- 🟡 **Warning** — avoidable allocations in hot paths, missing error context, idiom violations that will compound (`unwrap` outside tests, missing `pub(crate)`), test-coverage gaps in critical modules, doc-vs-code drift on non-structural details, double-logged tracing/toast pairs.
- 🔵 **Suggestion** — stylistic improvements, opportunistic refactors, minor performance wins, doc gaps.

## Final Summary

End the report with:

1. **Counts table** — rows: categories 0–8; columns: 🔴 · 🟡 · 🔵 · total.
2. **Top 5 highest-impact improvements** — ordered by severity then by reach. For each: the file(s), the fix in one sentence, the expected payoff (correctness · perf · maintainability · WASM-readiness · doc accuracy).
3. **What's healthy** — a short paragraph naming the patterns and modules in good shape. This calibrates the severity of the findings and prevents "everything is critical" reports.

## How to behave

- **Read `CLAUDE.md` first.** Treat it as the spec. The audit's first job is to verify the code matches it.
- **Use grep/glob, not memory.** Verify every claim against the actual code.
- **Do not invent specifics this skill doesn't name.** Where the rubric defers to CLAUDE.md, look it up — don't substitute prior knowledge or assumptions about how things "usually" look.
- **No invented improvements.** If a category is clean, say so. Don't pad the report.
- **Breadth before depth on the first pass.** A scan that touches every category with the top 2–3 issues each is more useful than 40 findings in one category.
- **Surface; do not execute.** Multi-file refactors identified by the audit get raised as findings — do not unilaterally implement them.
