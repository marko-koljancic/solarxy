---
name: solarxy-tracker
description: "Operate the public Solarxy GitHub project board: create and write milestones, epics, tasks and sub-tasks in the house shapes, apply the label taxonomy and pastel palette, link the hierarchy, move items through status, and clean up obsolete content. Use whenever work needs to be represented on GitHub, when a milestone is fanned out into issues, or when the board needs to be brought back in step with what actually shipped."
---

# Solarxy tracker

The board is public. Everything written here is read by people evaluating whether Solarxy is
worth their time, by anyone considering contributing, and by whoever is judging the project
on its delivery record. Write accordingly, and apply the public-surface rules from
`.claude/skills/solarxy-domain/SKILL.md` without exception.

## The board

The project is user-owned, public, and already exists; do not create a second one. Resolve it
by name rather than hard-coding a number that could change:

```
gh project list --owner <owner>
gh project field-list <number> --owner <owner> --format json
gh project item-list <number> --owner <owner> --format json --limit 200
```

Built-in fields already present and sufficient: Title, Assignees, Status, Labels, Milestone,
Repository, Parent issue, Sub-issues progress, plus the timestamps. Status carries Backlog,
To Do, In Progress, In Testing, Done.

**Two custom fields are worth adding, and only two.** Effort (S, M, L) and Priority (P0, P1,
P2). Deliberately not added: an Area field, because area is a label and two sources of truth
for the same fact will disagree; and a Release field, because the native Milestone field
already carries it.

## Hierarchy

Four levels. GitHub native Issue Types are unavailable on this account, so the level is
carried by a label plus the native parent link, not by an issue type.

- **Milestone.** A GitHub Milestone object, whose description is short by necessity, plus a
  **milestone tracking issue** carrying the full write-up and the epic checklist. The
  Milestone object links to the tracking issue.
- **Epic.** An issue. A coherent capability that a user could name. Groups tasks.
- **Task.** An issue, parented to its epic. One unit of work with its own acceptance
  criteria.
- **Sub-task.** An issue, parented to its task, created only when it earns the split.

**The granularity rule.** A task splits into real sub-issues only when the work crosses
crates, spans more than one work session, or carries its own acceptance criteria. Otherwise
it stays a checklist item inside the task. A decomposition that produces a hundred items
nobody maintains is a failure; so is one epic hiding six weeks of unexamined work.

Linking uses native flags, no GraphQL needed:

```
gh issue edit <child> --parent <parent>
gh issue edit <parent> --add-sub-issue <child>
gh issue edit <child> --remove-parent
```

## Writing the items

Section order and depth come from `references/golden-milestone.md`, `golden-epic.md`,
`golden-task.md`, and `golden-subtask.md`. Read the golden for the level you are writing
before writing it.

The shapes are layered on purpose, because the board serves three audiences at once and
blending their registers produces prose that serves none of them:

- **Summary and Value** are for someone evaluating the product. Outcome first: what becomes
  possible, what it replaces, what it costs. Crate names only where the crate is the point.
- **Acceptance Criteria, Technical Plan, Dependencies** are for someone who might build it.
  A contract precise enough that a stranger could satisfy it, naming files, invariants, and
  tests.
- **Credibility** is carried by structure rather than by adjectives: the same section order
  every time, an Out of Scope section that is actually populated, and criteria written before
  the work rather than reconstructed after it.

**Weight peaks at the epic, then decreases.** The milestone is a summary for someone deciding
whether to care; the epic is where the design reasoning and the acceptance contract live,
because every task beneath it inherits them; tasks and sub-tasks get progressively lighter as
that inherited context stops needing restatement. Nothing is restated at two levels: a
sub-task does not repeat its task's context, it points at it.

## Labels

The full table, with colors and the token each comes from, is in
`references/label-palette.md`. The rules that govern it:

- Colors are the product's own pastels, taken from `web/src/landing/landing.css` and the
  upstream editorial system, never GitHub's defaults. The board should look like the same
  product as the site.
- **Color encodes family, not identity.** Within a family the colors are distinct; across
  families reuse is fine, because the prefix disambiguates.
- Every issue carries exactly one `level:` label and at least one `area:` label.
- One saturated color, the brand clay, is reserved for `blocked` and `security`, so an alert
  never competes with ordinary categorization.
- Every color is a light pastel, so GitHub renders label text dark, matching the product's
  on-pastel ink.

## Cleanup policy

- **Archive board items; never delete issues or milestones.** Closed issues are the public
  delivery record and inbound links to them must keep working. Clearing the board means
  archiving items, which is reversible and leaves the issues untouched.
- **Retire duplicate labels rather than leaving three names for one idea.** Where a legacy
  label and a taxonomy label overlap, relabel the issues, then delete the legacy label.
- Labels that shadow a native field (a `milestone: x` label next to a real Milestone object)
  are removed; the native field wins.
- Never delete the project itself. The URL is public and stable.

## Redaction

A leak on this surface is public, permanent, and indexed, which is why it gets called out
again here rather than left to the general rule. No item title, body, comment, or label names
any reference checkout other than the Minimystix prototype. Describe every capability in
Solarxy-native terms.

Run the redaction check from the public-surface rules in
`.claude/skills/solarxy-domain/SKILL.md` against every drafted item **before** pushing, not
after. On this surface there is no quiet fix: an edit leaves history.

## Workflow

1. **Draft first, push second.** Write every item to files and have them reviewed before
   anything is created. Board writes are public the instant they land.
2. Create or confirm the Milestone object and its tracking issue.
3. Create epics, then tasks parented to epics, then only the sub-tasks that earn the split.
4. Apply labels, milestone, status, and the Effort and Priority fields.
5. Add every issue to the project board.
6. Run the redaction check and re-read each body once as an outsider would.
7. Sweep the other surfaces with `.claude/skills/solarxy-sync/SKILL.md`; a board change
   almost always implies a doc or artifact change.

Useful write commands:

```
gh issue create --title "<title>" --body-file <path> --label "<a>,<b>" --milestone "<name>"
gh issue edit <n> --add-label "<label>" --milestone "<name>"
gh project item-add <number> --owner <owner> --url <issue-url>
gh issue close <n> --reason completed
gh api repos/<owner>/<repo>/milestones -f title="..." -f description="..." -f due_on="..."
```

Status and custom fields are set with `gh project item-edit`, which needs the item id and
field id from `item-list` and `field-list`.
