# Golden: milestone

A milestone is two artifacts. The GitHub Milestone object holds a short description, because
that field is a single plain text box with no room for structure. The **milestone tracking
issue** holds the real write-up and the epic checklist.

Worked example below is Solarxy 0.8.1, which shipped on 2026-07-28. It is illustrative, not
the literal text that was used.

## The Milestone object description

Three sentences at most: what the release is, what it unlocks, and where to read more.

```
Expressions, runtime, and publishing. Parameters become computed rather than typed,
scenes gain a play model, and an authored scene can be exported as a standalone page an
end user can open. Full scope, progress, and release notes: #<tracking-issue>
```

Set the due date. Leave it open until every epic under it is closed.

## The tracking issue

Title format: `[<version>] <Theme>`, for example `[0.8.1] Expressions, runtime, and publishing`.

Labels: `level: milestone`, plus the `area:` labels the release actually touches.

```markdown
## Summary

Until now every parameter in a Solarxy scene held a literal value. If you wanted twenty
copies of an object spaced along a curve, you typed twenty numbers, and changing the
spacing meant typing twenty more. This release makes parameters computable: a parameter
can hold an expression that reads other parameters, the current copy index, or scene
time, and recomputes whenever its inputs change.

Three capabilities follow from that. Scenes gain a runtime, so a graph can be played
rather than only inspected. Attribute wrangling lets per-point logic be written directly
rather than assembled from nodes. And a finished scene can be exported as a standalone
page, so the person who receives your work does not need Solarxy to open it.

## Problem

Solarxy could describe a shape but not a relationship. Every value was independent, so
intent lived in the author's head instead of in the document, and any change that should
have been one edit was N edits. Scenes were also inert: there was no notion of time, so
nothing could move. And there was no way to hand a finished scene to someone who does not
run the tool, which meant the work could be made but not delivered.

## Goal

A parameter can be computed from other values in the scene. A scene can play. A finished
scene can be given to someone else as a link.

## Value

- **Authoring becomes parametric rather than manual.** Change one input, and everything
  derived from it follows. This is the difference between a modeling tool and a drawing
  tool.
- **Scenes can express time.** Animation, simulation, and anything else that evolves now
  have a foundation to sit on.
- **Work becomes deliverable.** Export produces something a client or colleague can open
  in a browser with no install and no account.

## Highlights

- Expression engine, with expressions valid in any numeric or vector parameter.
- Attribute wrangle, for per-point logic written as code rather than as a node chain.
- Runtime and play model, giving scenes a time axis.
- Standalone web export.
- Physically based area lights.

## Scope

**In:** the five capabilities above, plus the documentation and site work that makes them
findable.

**Out:** a full animation system with keyframes and curves; simulation solvers; and
scripting beyond per-parameter and per-point expressions. Each is tracked separately and
none is blocked by this release.

## Release notes

Registry grows to 76 node types. Scene schema stays at version 1, so scenes authored
before this release open unchanged. Expressions are opt-in per parameter: an untouched
parameter behaves exactly as it did.

## Epics

- [ ] #<n> Expression engine
- [ ] #<n> Attribute wrangle
- [ ] #<n> Runtime and play model
- [ ] #<n> Standalone web export
- [ ] #<n> Physically based area lights
```

## Notes on the shape

- **Summary and Problem are written for someone who has never used the product.** They lead
  with the user's situation, not the implementation. Notice that neither names a crate.
- **Problem is concrete about the cost of the status quo.** "Every value was independent, so
  one change was N edits" is checkable. "The system was inflexible" would not be.
- **Value bullets say what becomes possible**, not what was built. The distinction is the
  whole reason a public board reads as a product rather than a work log.
- **Scope names what is out and where it went.** An empty or missing Out section is the most
  common way a public tracker loses credibility, because it reads as though nothing was
  traded away, which is never true.
- **Release notes carry compatibility facts** a user needs before upgrading: what grew, what
  stayed stable, and what is opt-in.
- The epic checklist uses issue references, so GitHub renders live status and the milestone
  shows its own progress without anyone maintaining a percentage by hand.
- **There is no Links section, and that is deliberate.** A tracking issue used to end with one
  naming the milestone specification, and the specifications live in the private
  workspace-root repository rather than in this one, so nineteen public items spent months
  telling readers to open a file they cannot reach. The board is the public record; the
  specification is internal. Do not reintroduce the section, and do not name a path under
  `Docs/` anywhere in an item's body, including in prose. If a reader needs more than the item
  carries, the item is too thin, which is a reason to write more Summary rather than to link
  away.
