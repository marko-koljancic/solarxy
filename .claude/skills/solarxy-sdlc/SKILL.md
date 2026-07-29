---
name: solarxy-sdlc
description: "The Solarxy delivery pipeline end to end: the nine stages from an idea to a verified release, who owns each, and which file is authoritative for it. Use to answer where a piece of work sits in the process, what has to be true before it advances, and what comes next. Also carries the product lifecycle framing and the rule for what may enter the repository."
---

# The Solarxy delivery pipeline

This file is a map. **It carries no process of its own**: every stage names the file that owns
it, and that file wins on any detail. Its job is to answer three questions that nothing else
answers in one place: where is this work, what has to be true before it advances, and who owns
the next step.

## The nine stages

**1. Intent and clarification.** An idea becomes a request with known scope. Ambiguities are
surfaced and asked in one batch; work does not start on an unconfirmed plan.
Owner: the Working Agreement in `CLAUDE.md`, with the tiering rule in
`.claude/skills/solarxy-domain/SKILL.md`. Roles: whoever was asked.

**2. Strategy and release slotting.** The work gets a disposition and, if scheduled, a release.
Owner: `../../Docs/SOLARXY-MILESTONE-PROGRAM.md`, which is authoritative for *when* a card
lands, against `../../Docs/SOLARXY-ROADMAP.md`, which is authoritative for *what* it is.
Role: `product-manager`.

**3. Build-ready specification.** A release slot becomes a code-grounded spec with feature
specs, a task table, exit criteria, and a decision log.
Owner: the `milestone-planner` skill and its template and checklist. Role: `milestone-planner`.

**4. Decomposition to trackable work.** The spec's workstream table becomes a milestone, epics,
tasks, and only the sub-tasks that earn the split.
Owner: `.claude/skills/solarxy-tracker/SKILL.md`, which holds the four-level hierarchy, the
granularity rule, the golden item shapes, and the label taxonomy. Role: `product-owner`.

**5. Design.** Anything with a visual or interaction surface is designed before it is built.
Owner: `design/README.md` and the per-shell Pencil files; `.claude/skills/solarxy-brand/SKILL.md`
for the public pages. Role: `product-designer`.

**6. Implementation.** The plan becomes code, with the architect specifying anything that
crosses a boundary before either side is written.
Owner: `CLAUDE.md` for the crate map, commands, key patterns, and the working agreement. Work
happens on a short-lived `<type>/<slug>` branch off `main`, named and committed per
`.claude/skills/solarxy-git/SKILL.md`.
Roles: `architect`, then `rust-engineer`, `frontend-engineer`, `graphics-engineer`.

**7. Verification.** Exit criteria are walked one by one against evidence. A criterion without
evidence is not met.
Owner: the definition of done in the milestone spec. Role: `qa-engineer`, with `security-engineer`
where the change touches untrusted input, headers, the supply chain, or what a public surface
discloses.

**8. Release.** The version ships through the packaging and deploy pipeline.
Owner: the release train section of the **workspace** `../../CLAUDE.md`, which lives in a
separate private repository because it carries operational detail. Read it there; it is not
reproduced here. Milestone work rides a `release/<version>` branch and merges to `main` through
one pull request, per `.claude/skills/solarxy-git/SKILL.md`. Role: `devops-engineer`.

**9. Cross-surface sync.** The five surfaces are brought back into agreement.
Owner: `.claude/skills/solarxy-sync/SKILL.md`, which holds the change-type matrix and the
checklist that runs before anything is called done. Role: `technical-writer`, with the role that
made the change.

Stages are not strictly serial. Design runs alongside decomposition; verification starts as
soon as the first criterion is testable; sync runs continuously rather than only at release.
What is serial is the gating: **work does not advance past a stage whose owner has not signed
off on it**, and scope pressure resolves by deferring to the backlog rather than by weakening a
criterion.

## Where the product is going

Judging whether a stage is even the right one to be in needs the arc. Solarxy began as a viewer
and debugger, is becoming an authoring tool, and grows pipeline and delivery capability after
that. `../../Docs/SOLARXY-ROADMAP.md` section C tells the story so far; section E and the
program document tell the rest.

Every proposal sits somewhere on that arc: a step along it, a prerequisite for a later step, or
a detour that gets paid for twice. Naming which is `product-manager`'s call, and it belongs at
stage 2, before a release slot is spent. A feature that is excellent and off-arc is still a
detour.

## What may enter the repository

**Ask before adding.** The rule and its boundary are stated in the Working Agreement in
`CLAUDE.md`. It exists because this repository is public, because dependencies are a supply
chain rather than a convenience, and because a stray file at the repo root has already happened
once and had to be removed before merge.

## Reading this file

If you are asked to do something and are unsure it is time yet, find the stage, read the owning
file, and check the stage before yours actually closed. Most process failures in this project
have been a stage skipped rather than a stage done badly.
