# Golden: epic

An epic is a capability a user could name. It groups tasks and is closed only when all of
them are. It carries the design reasoning that would otherwise be repeated in every task
beneath it.

Title format: `[<version>] <Capability>`, for example `[0.8.1] Expression engine`.

Labels: `level: epic`, the `area:` labels it touches. Milestone set. Effort and Priority set
on the board.

## Worked example

```markdown
## Summary

Make parameters computable. A numeric or vector parameter can hold an expression instead
of a literal, referencing other parameters, the current copy index, or scene time. When
an input changes, everything derived from it recomputes automatically.

## Problem and value

Every parameter today holds a literal. Intent that spans parameters ("these twenty copies
are evenly spaced", "this radius is always half that one") cannot be expressed, so it
lives in the author's memory and is re-entered by hand on every change. This is the
single largest gap between Solarxy and a parametric modeler.

With expressions, a scene records relationships rather than a snapshot of values. The
practical effect is that scenes become editable after the fact by someone other than
their author.

## Scope

**In:** the expression language and its evaluator; expression support in numeric and
vector parameters; dependency tracking so an expression re-evaluates when its inputs
change; cycle detection; error surfacing in the parameter panel.

**Out:** expressions in string parameters, deferred until there is a use case; a visual
expression editor; and user-defined functions. None is blocked by this work.

## Acceptance criteria

- [ ] Any numeric or vector parameter accepts an expression, and the parameter panel shows
      the computed value alongside the expression text.
- [ ] An expression referencing another parameter re-evaluates when that parameter
      changes, without a manual recook.
- [ ] A reference cycle is refused at the point the expression is set, with a message
      naming the cycle, and leaves the document unchanged.
- [ ] An invalid expression leaves the last valid value in place and marks the parameter
      as errored rather than cooking with a wrong number.
- [ ] Expressions round-trip through save and load with identical results.
- [ ] Evaluation is deterministic: the same document produces the same values on any
      machine and any platform.

## Design considerations

- **Determinism is a hard requirement, not a preference.** Scene files are shared, and a
  scene that evaluates differently on another machine is worse than one that fails. This
  rules out anything touching wall-clock time, locale, or iteration order that is not
  itself deterministic.
- **Errors degrade rather than propagate.** A single bad expression must not prevent the
  rest of the document from cooking. The chosen behavior is to hold the last valid value
  and mark the parameter.
- **The dependency graph is separate from the node graph.** A parameter can depend on a
  parameter in another node without an edge existing between those nodes, so cycle
  detection has to run over its own graph.

## Technical plan

The evaluator lives in the engine crate and is pure: no filesystem, no clock, no
allocation surprises. Parameters gain an optional expression alongside their value; the
cook path resolves expressions before evaluating a node. Dependency edges are recorded at
set time, which is also when cycles are refused, so the cook path never has to detect
one. The whole crate must compile to the wasm target, so the language runtime cannot pull
in anything platform-specific.

## Dependencies

None inbound. Attribute wrangle and the runtime both build on this, so it sequences
first within the release.

## Future considerations

String expressions, a visual editor, and user-defined functions are the natural
extensions. The language design should not foreclose them, but none is in scope here.

## Tasks

- [ ] #<n> Expression language and evaluator
- [ ] #<n> Parameter integration and dependency tracking
- [ ] #<n> Error surfacing in the parameter panel
- [ ] #<n> Documentation and node reference
```

## Notes on the shape

- **Summary and Problem stay in the evaluator's register**, exactly as in the milestone.
  The epic is where a reader decides whether to care.
- **Acceptance criteria are the contract** and belong to the epic, not scattered across
  tasks. Each is observable: a person could check it without reading the diff. "Evaluation
  is deterministic" is testable; "the evaluator is robust" would not be.
- **Design considerations carry the why**, so the tasks beneath do not each re-argue it.
  Every consideration here names a constraint and the decision it forced.
- **Technical plan is prose, not a file list.** It says where things live and what shape
  they take. Naming exact files is the task's job, because files move and an epic outlives
  that churn.
- **Out of scope entries say where the work went** or that it is unblocked. "Not doing it"
  without disposition invites the same question every time the epic is read.
- Effort and Priority live on the board as fields, not in the body, so they can be changed
  without editing prose.
