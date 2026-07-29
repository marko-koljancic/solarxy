# Milestone spec template

The produced file is `Docs/SOLARXY-MILESTONE-<version>.md` at the workspace root, so paths
written inside it are workspace-relative (`Docs/...`, `Sources/solarxy/crates/...`), while the
paths in this checklist are relative to the Solarxy repo where the skill runs.

Fill every section. Keep the header block. Replace angle-bracket placeholders. Delete this
first paragraph in the produced file.

```
# Solarxy <version> milestone: <codename>

Status: source of truth for the Solarxy <version> milestone. Approved <date>. This doc sits
alongside the living roadmap `Docs/SOLARXY-ROADMAP.md` and the archived planning suite under
`Docs/Archive/`. Deviations are recorded as dated entries in the Amendments section (newest
first), per the workspace amendment discipline.
Scope: <one sentence on what this milestone is, drawn from the roadmap release program>.
Grounding: every feature claim is traceable to a path under `Sources/solarxy/crates/` or the
roadmap; current behavior is confirmed by direct code reading (file:line) on <date>.
```

## 1. Context and goals

- Why this milestone exists (from the roadmap release program), what it delivers, the
  intended outcome, and what it deliberately does not attempt.
- "Realities established by direct code reading (<date>)": the concrete code facts the scope
  rests on, each with `crates/.../file.rs:line`. Claims of absence belong here too, with the
  search that established them.

## 2. Scope

- In scope: the committed work items, grouped.
- Out of scope: what is deferred and to which later milestone, so it is not relitigated.

## 3. Architecture and affected systems

- Per work item, the crates and systems it touches and how the change fits the one-core
  architecture. Note the drift-test gates and working-agreement constraints that apply.

## 4. Feature specifications

Per feature: current behavior (cited), proposed behavior, files to change, param, port and
schema specifics, tests to add or update, effort grade, personas and journeys served.

## 5. Personas and user journeys

- The personas and journeys this milestone touches, and which work items serve each. Keep
  this Solarxy-native.

## 6. Task breakdown and sequencing

- A workstream table: id, task, crates, effort, acceptance, dependency and order. Sequence
  fixes first, then additive features, then polish.
- This table is the source for the GitHub board fan-out. Each row becomes a Task; rows that
  cross crates, span more than one work session, or carry their own acceptance criteria
  become Tasks with real sub-issues under the granularity rule in
  `.claude/skills/solarxy-tracker/SKILL.md`. Group rows into Epics before fanning out.

## 7. Testing and verification

- Unit tests, the registry drift tests (node count, doc-length and param and port docs,
  snapshot regeneration, README counts, wiki regeneration), renderer goldens where the
  renderer changes, `web/` typecheck, test and build, and a manual QA checklist per shell.

## 8. Risks, assumptions, dependencies

## 9. Milestone decision log

- One numbered ruling per behavior-changing or design choice, each with a recommendation for
  the maintainer to ratify. Mark which are ratified and which are still proposed.

## 10. Out of scope and backlog

- The larger items intentionally deferred, with the target milestone for each.

## 11. Definition of done and exit criteria

- The measurable bar for calling the milestone complete: tests green, drift clean, goldens
  updated, docs and version bumped, manual QA passed, and the cross-surface sweep in
  `.claude/skills/solarxy-sync/SKILL.md` complete.

## 12. Amendments

- Dated, newest first: `### <YYYY-MM-DD>: <short title>`. Start with the creation entry.
