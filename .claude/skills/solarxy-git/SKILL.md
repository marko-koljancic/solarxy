---
name: solarxy-git
description: "The commit message and branch naming conventions: Conventional Commits with a closed type and scope set, the branch model and its lifecycle, pull request titles, and the golden-accept footer. Use whenever handing the maintainer a commit message, naming a branch, or writing a pull request title."
---

# Commit and branch conventions

The maintainer runs git; agents hand over messages and steps. **This file defines the format
of what is handed over.** Commit messages are public artifacts, so the writing-style rules in
`.claude/skills/solarxy-domain/SKILL.md` apply: no emoji, no em or en dashes, no arrow glyphs.

Nothing validates these today. There is no hook, no commitlint, no CI check. The format is
adopted for readability and to keep automation possible later, so it holds only if it is
actually followed.

## Commit format

```
type(scope): imperative subject under 72 characters

Body, separated from the subject by a blank line. Prose is welcome and
usually warranted: explain why, not what. Wrap around 72 columns.

Footer(s)
```

### Types

A closed set. Use nothing outside it.

| Type | For |
|---|---|
| `feat` | A new capability a user could notice. |
| `fix` | A defect in shipped behavior. |
| `docs` | Documentation only, in any surface. |
| `style` | Formatting with no behavior change. Rare, since `cargo fmt` is automatic. |
| `refactor` | Restructuring with no behavior change and no new capability. |
| `perf` | A change whose point is speed or memory. State the measurement in the body. |
| `test` | Tests only. |
| `build` | Packaging and distribution: cargo-dist, the bundles, the installers, the manifests. |
| `ci` | Workflow files and CI configuration. |
| `chore` | Maintenance that fits nothing above. |
| `revert` | Reverting an earlier commit. Name it in the body. |

`build` versus `ci` versus `chore` is the only ambiguous boundary here: `ci` is a file under
`.github/workflows/`, `build` is how the product is packaged and shipped, `chore` is the rest.

### Scopes

Also a closed set, and deliberately the **same seven words as the `area:` labels** on the
GitHub board (`.claude/skills/solarxy-tracker/references/label-palette.md`), so a commit's
scope and its issue's label are the same word.

| Scope | Covers |
|---|---|
| `engine` | Document, topology, cook, registry, undo, scene file. |
| `renderer` | Render passes, shaders, pipelines, lighting, capture. |
| `web` | The browser frontend, the wasm host, the public pages. |
| `cli` | The command-line binary and its analysis output. |
| `desktop` | The native GUI shell and its panels. |
| `docs` | Wiki, reference documentation, planning docs, release notes. |
| `infra` | CI, packaging, release train, deploy, edge. |

Two utility scopes exist for changes that have no area:

| Scope | Covers |
|---|---|
| `release` | Version bumps and release preparation. |
| `deps` | Dependency additions, removals, and upgrades. |

**Omit the scope in two cases.** When it would repeat the type, write `docs: ...` rather than
`docs(docs): ...`. When a change genuinely spans several areas, omit it rather than inventing a
compound like `engine-renderer`.

### Subject

Imperative mood, as if completing "this commit will ...". Lowercase after the colon. **No
trailing period.** Seventy-two characters maximum including the type and scope.

The long narrative subjects in this repository's recent history read well but belong in the
body. Three existing commits have subjects over 800 characters, one at 936, because a body was
written without a blank line separating it. **A body is always preceded by a blank line.**

### Body

Optional but usually worth writing, and already present on most commits here. Explain why the
change was made, what it does not do, and anything a reader would otherwise have to reconstruct
from the diff. Wrap around 72 columns. Do not restate the subject.

### Footers

- `[golden-accept]` when the change intentionally re-baselines renderer goldens. See below.
- `BREAKING CHANGE: <what breaks and what to do>` for anything that breaks a consumer.
- Issue references (`Refs #123`, `Closes #123`) when an item on the board is involved.

### Breaking changes

Mark with `!` after the scope, and a `BREAKING CHANGE:` footer explaining the migration:

```
feat(engine)!: require an explicit mode on copy and array nodes

BREAKING CHANGE: documents authored before this release migrate on load,
which writes the previous behavior explicitly rather than relying on a
default.
```

In this project the usual triggers are a scene-schema version change, a node `type_version`
bump, a `Command` or `EngineEvent` shape change, or a CLI flag removal. A change that is
handled by a migration is still breaking; the migration is the mitigation, not the exemption.

## The golden-accept footer

The renderer's golden-capture job fails a pull request whose render output moved. When the move
is intentional and adjudicated, the token records that.

The check in `.github/workflows/ci.yml` reads the tip commit's **entire message** with
`git log -1 --pretty=%B`, concatenates the pull request title, and does a **fixed-string**
match for the literal `[golden-accept]`. So the token works in the subject, the body, the
footer, or the pull request title, and it is case-sensitive. Put it in the footer, and justify
the diff in the pull request body.

Because the match is a fixed string, a message that merely writes about the token carries it:
"this commit needs no [golden-accept]" arms the bypass the moment that commit sits at a pull
request tip. When referring to the token without meaning it, write golden-accept with no
brackets.

## Branches

`main` is always releasable. Everything else is short-lived.

| Kind | Pattern | For |
|---|---|---|
| Work | `<type>/<slug>` | One focused change. The type matches the commit type, so `feat/`, `fix/`, `docs/`, `chore/`, and so on. |
| Milestone | `release/<version>` | A whole release, for example `release/0.8.2`. |

Slugs are lowercase, hyphenated, and describe the change rather than the ticket:
`feat/instance-transforms`, not `feat/issue-42`.

**Lifecycle.** Branch off current `main`. Commit as many times as the work needs. Open one pull
request back to `main`. Merge, then **delete the branch**. Merged branches accumulating on the
remote is the current state and is what this rule exists to stop.

Whether a pull request is squashed or merged is the maintainer's call per request; both are
enabled, and the release merges have deliberately preserved history.

**The wiki is different and stays different.** `Sources/solarxy.wiki` publishes from `master`
and is edited on `develop`; that flow is unaffected by anything here.

## Pull request titles

Same convention as commits, because the golden-capture job greps the title and because the
pull request list is public.

```
feat(engine): carry instance transforms in the scene contract
chore(release): 0.8.2, rendering foundations
```

Append `[golden-accept]` to the title or put it in the commit footer, either satisfies the
check.

## Worked examples

A focused change with the reasoning in the body:

```
feat(engine): carry instance transforms in the scene contract

The cooked mesh gains an optional list of per-instance matrices. Absence
keeps today's meaning exactly, one implicit identity instance, so every
existing consumer is unaffected and this is not a breaking change to the
type.

Refs #142
```

A renderer change that moves pixels on purpose:

```
fix(renderer): repack the light uniform padding for the fourth caster

The struct grew past its size assert when the caster array was extended.
Padding is repacked rather than the assert relaxed, so the WGSL side
still matches byte for byte.

[golden-accept]
```

Documentation, with the scope omitted because it would repeat the type:

```
docs: document the design source of truth and the Pencil constraint

The per-shell design files were invisible to every agent, and nothing
warned that opening one with a text tool corrupts it.
```

Maintenance with no area:

```
chore(deps): upgrade wasm-bindgen to match the pinned CLI

The CLI version and the crate dependency must agree exactly or the
generated bindings fail at load.
```
