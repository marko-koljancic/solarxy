---
name: milestone-planner
description: "Plan, specify, or revise a Solarxy release milestone. Use when the user wants to spec a version (for example \"plan the 0.9.0 milestone\", \"spec the next release\", \"turn the release program into a build-ready plan\"), or to revise an existing milestone spec. Produces one code-grounded specification document under Docs/, in the established house style, and optionally the matching GitHub board fan-out."
---

# Solarxy milestone planner

Turn one entry of the roadmap release program into a build-ready milestone specification:
architecture, feature specs, task breakdown, personas and journeys served, acceptance
criteria, testing, decision log. The output is a planning document, not code.

## When to use

Use this when the user asks to plan or spec a Solarxy version, or to revise an existing
milestone spec. Each run produces or updates one file,
`../../Docs/SOLARXY-MILESTONE-<version>.md`, and optionally the board items that
implement it.

## Hard rules

1. Public-surface rules apply, per `.claude/skills/solarxy-domain/SKILL.md`. Never name the
   external node engine the roadmap benchmarks against, or any other checkout under
   `../../References/`, in a milestone spec or in code guidance. Describe every capability in
   Solarxy-native terms; cite Minimystix, Houdini, or Blender only when an outside reference
   genuinely clarifies intent.
2. Writing style, per the domain briefing: no emojis, no em or en dashes, no divider lines.
3. Ground every claim in code. Cite exact `crates/.../file.rs:line`, read the file to confirm,
   and state current behavior and proposed behavior distinctly. Claims of absence need a
   search behind them.
4. Honor the working agreement in all code guidance: no `unwrap` or `expect` outside tests;
   `thiserror` in library crates and `anyhow` in binaries; the zero-frontend-change contract.
5. Match the house style and the amendment discipline. See `references/`.

## Inputs to gather

- **Target scope.** `../../Docs/SOLARXY-ROADMAP.md` Part I section E, the release program, for
  the named items, plus the referenced Part II cards. Cite cards by id and describe the
  feature natively.
- **Code truth.** `CLAUDE.md` for crates, commands, the working agreement, the `web/` layout,
  and the drift tests. Then the exact files behind each item.
- **Personas and journeys.** `../../Docs/SOLARXY-ROADMAP.md` Part I section F. The fuller
  source is archived at `../../Docs/Archive/SOLARXY-UX-SPEC.md`.
- **Backlog.** `../../Docs/Archive/` for deferred items, tiered catalog entries, and
  implementation-log continuation notes, plus deferral markers in `crates/` and `web/`.
- **Prior art.** The most recent `../../Docs/SOLARXY-MILESTONE-*.md` for the current house
  style, which is more authoritative than the template when the two differ.

## Workflow

1. Resolve scope from the release program. Separate what the roadmap names from candidate
   backlog items you find, and keep the two labelled all the way through.
2. Delegate research and drafting to the `milestone-planner` subagent (Agent tool,
   `subagent_type: "milestone-planner"`). Give it the target version, the roadmap-named
   scope, and the pointers above. If that subagent type is unavailable, do the research
   directly under the same guardrails.
3. Assemble to `references/milestone-doc-template.md`, all sections, house-style header.
4. Grade every work item for effort, map each to the personas and journeys it serves, and
   write per-task acceptance criteria plus a milestone definition of done and a verification
   section (fmt, clippy, test, the registry drift tests, renderer goldens where the renderer
   changes, `web/` typecheck and test and build, manual QA per shell).
5. Present every behavior-changing item as a numbered milestone decision with a
   recommendation, so the maintainer ratifies on review.
6. Write the document. Add the cross-reference from `../../Docs/SOLARXY-ROADMAP.md` section E
   and a dated Amendments entry.
7. Run `references/house-style-checklist.md` before finishing.
8. **Offer the board fan-out.** A milestone spec and its GitHub representation should be
   authored once, not twice. When the spec is accepted, load
   `.claude/skills/solarxy-tracker/SKILL.md` and emit the milestone tracking issue, the
   epics, and their tasks in the golden shapes, applying the granularity rule to decide
   which tasks earn real sub-issues. Draft first, push only on explicit approval.

## References

- `references/milestone-doc-template.md`, the section-by-section template.
- `references/house-style-checklist.md`, the pre-finish verification checklist.
