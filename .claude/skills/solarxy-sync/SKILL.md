---
name: solarxy-sync
description: "Keep the five Solarxy surfaces in step: the code, the planning docs and the public roadmap page's data module, the public GitHub board, the public site, and the wiki. Use after any change that could be visible on more than one of them, and always before declaring a task, epic, milestone, or release done. Answers the question 'what else does this change imply' with the exact files and commands."
---

# Solarxy cross-surface sync

Solarxy spans five surfaces that live in four separate repositories. **They cannot be updated
atomically.** No commit, no pull request, and no release can move them together, so the only
thing keeping them honest is running this deliberately. Silent drift is the failure mode: the
roadmap's interactive twin has drifted badly more than once, and a shipped release once had
no representation on the board at all.

The five surfaces, and where they live:

| Surface | Location |
|---|---|
| Code | this repo, `crates/` and `web/` |
| Planning docs and the roadmap data module | `../../Docs/`, `web/src/roadmap/data.ts` |
| GitHub board, issues, milestones | the public project and repo |
| Public site | `web/` pages here; edge and deploy in the separate site repo |
| Wiki | `Sources/solarxy.wiki`, published from `master` only |

The roadmap data module is the interactive twin of `../../Docs/SOLARXY-ROADMAP.md` and
renders publicly at solarxy.koljam.com/roadmap. It replaced the retired internal artifact
(now archived at `../../Docs/Archive/solarxy-roadmap.html`). Because it ships on a public
page, every edit to it is a public statement and the redaction rule applies in full: the
module and the page it feeds never name a reference checkout, an internal doc path, or a
planning code.

## By change type

Find the row that describes what happened. Do everything under it, or say explicitly which
part you are deferring and why.

### A feature lands in the code

- Code: tests green, the drift tests clean, goldens updated if the renderer moved.
- Docs: if behavior differs from what the milestone spec predicted, that is a dated
  amendment in the spec, not a silent divergence. Update `CLAUDE.md` if a crate, pattern,
  chokepoint, or command changed.
- GitHub: move the task to In Testing, then Done. Tick the checklist item in its parent.
- Wiki and site: usually nothing yet. Defer to the release row.

### A task or sub-task closes

- GitHub: close with reason completed. Confirm the parent's sub-issue progress advanced. If
  the task was the last one under its epic, the epic is now closable.
- Docs: nothing, unless the task changed a documented decision.

### An epic closes

- GitHub: close the epic, tick it in the milestone tracking issue.
- Docs: if the epic's scope shifted while it was open, the milestone spec gets a dated
  amendment recording the deviation and its reason.

### A node type is added or its descriptor changes

- Code: registry count assert, registry snapshot regenerated with the diff reviewed rather
  than blind, `type_version` bumped with a migration decision when the descriptor changed.
- Docs: amend the living node catalog, `../../Docs/SOLARXY-NODE-CATALOG.md`, moving the type
  from a planned row to the shipped roster. Update every place a node-type count appears.
- Roadmap page: the node-type count appears in more than one place in `data.ts` and those
  places have disagreed with each other before. Update all of them, and the landing page's
  stats band, which carries the same count.
- Wiki: regenerate the node reference.
- GitHub: nothing beyond the task itself.

### Scope changes, or a ratified decision is amended

- Docs: a dated amendment in the milestone spec, newest first, plus the roadmap if the
  release program moved. Node-spec changes also amend the living node catalog; interaction
  changes also amend the UX spec.
- Roadmap page: if a card was regraded, rescoped, deferred, or dropped, the corresponding
  data array in `web/src/roadmap/data.ts` changes. See the data-module table below.
- GitHub: reflect the new scope in the affected items. An item deleted from scope is closed
  as not planned with a comment saying where the work went, never silently deleted.

### A release ships

Run this only after the release train has completed. The order in the workspace `CLAUDE.md`
is authoritative for the train itself; this row covers what the train does not do.

- GitHub: close the milestone. Confirm every epic under it is closed. Archive the board items
  for the release.
- Docs: update the roadmap's shipped history and release program, add the dated amendment,
  and record the release in the milestone spec's amendments.
- Roadmap page: update every count, the release entries, and the changelog array in
  `data.ts`, plus the landing stats band if a count it shows moved.
- Wiki: release notes on `develop`, then merged to `master`. A page that exists only on
  `develop` is not published.
- Site: if the release changed or added a route, the edge config in the separate repo changes
  first and is validated before deploy, because that file is shared with another site.
  Verify the live page by its body or content length, never by status code: the vhost ends in
  a fallback that answers 200 with the landing page for every unknown path, which once hid a
  missing route for an entire release.
- Verify the packaging fan-out by artifact, not by job colour. Jobs that need a credential
  fail soft: a warning, a skip, and a green run that published nothing. Confirm the
  downstream commit or release asset actually exists.

### A public page changes

- Code: the page is a build entry here.
- Site: the route is an exact-match location in the edge config in the separate repo, and
  header directives do not inherit, so every location that sets any header restates the full
  set. Without the location, the URL silently serves the landing page with a 200.
- Docs and GitHub: a new public surface is worth a roadmap mention and a board item.

## The data-module table

`web/src/roadmap/data.ts` is hand-authored. Nothing generates it and nothing validates its
content against the docs, so a doc change desynchronizes it silently, and since it renders
on the public /roadmap page the drift is now visible to everyone. When you change the left
column, change the right one in the same pass.

| Doc change | `data.ts` array |
|---|---|
| The release ladder or program | `PROGRAM`, and `RELEASE_PLAN` for the road to 1.0 subset |
| A card regraded or rescoped | `CARDS` |
| A card's release or disposition | `PROGRAM.cards`, `BACKLOG_WAVES`, or the deferred and will-not lists. Never store a release on a card; the artifact derives it |
| A persona added or changed | `PERSONAS` |
| A journey added or changed | `JOURNEYS` and `COVERAGE` |
| A load-bearing commitment | `COMMITMENTS` |
| Node-type, crate, or release counts | `STATS`, the hero chips, and the footer, which have disagreed with each other before; also the landing stats band in `web/index.html` |

## Before declaring anything done

- [ ] Code: tests green, drift tests clean, goldens current, desktop unaffected or verified.
- [ ] Docs: every claim that changed is amended, dated, newest first. No unamended
      divergence between a spec and the code.
- [ ] Roadmap page: every count and array touched by the change is updated in `data.ts`,
      and the counts agree with each other and with the landing stats band.
- [ ] GitHub: items reflect reality. Nothing shipped is still open; nothing open is already
      shipped. Parents and checklists are consistent.
- [ ] Wiki: published from `master`, not sitting on `develop`.
- [ ] Site: routes verified by body, deploys verified by artifact.
- [ ] Redaction: no public artifact names a reference checkout other than the Minimystix
      prototype. Run the check from the public-surface rules in
      `.claude/skills/solarxy-domain/SKILL.md`.

A surface you deliberately skipped is named in the handover, with the reason. A surface you
forgot is the thing this file exists to prevent.
