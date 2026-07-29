# Golden: task

A task is one unit of work with its own acceptance criteria, parented to an epic. It is the
level most issues live at, and the level a contributor would pick up.

Title format: a plain imperative, no version prefix; the milestone field carries the release.
For example `Expression language and evaluator`.

Labels: `level: task`, the `area:` labels it touches, plus `bug`, `blocked`, or `security`
when true. Parent set to the epic.

## Worked example

```markdown
## Summary

Implement the expression language and its evaluator: parse an expression string into a
form that can be evaluated, and evaluate it against a context supplying parameter values,
the current copy index, and scene time.

Parent epic: #<n>

## Acceptance criteria

- [ ] Arithmetic, comparison, and the built-in function set parse and evaluate correctly,
      covered by unit tests including precedence and associativity cases.
- [ ] Evaluation is deterministic across platforms: the same expression and context
      produce bit-identical results on native and on the wasm target.
- [ ] A malformed expression returns a structured error naming the position, never panics,
      and never leaves the evaluator in a partially applied state.
- [ ] Numeric edge cases are defined and tested: division by zero, non-finite literals,
      and overflow each have a specified behavior rather than an accidental one.
- [ ] The crate compiles to `wasm32-unknown-unknown` with no platform-specific dependency
      added.
- [ ] `cargo clippy --all-targets` is clean at the crate's pedantic level.

## Design considerations

- The evaluator is pure and has no access to the filesystem, the clock, or any global
  state. Everything it needs arrives in the context it is handed. This is what makes
  determinism achievable rather than aspirational.
- Errors are values, not panics. The engine must survive a bad expression, because the
  expression comes from user input and arrives on every keystroke while being typed.
- Parsing and evaluation are separate steps, so a parsed expression can be cached and
  re-evaluated cheaply when only its inputs changed. Re-parsing per cook would put string
  work on the hot path.

## Technical plan

Add the module to the engine crate. Errors use the crate's existing error type, per the
working agreement that library crates use `thiserror`. No `unwrap` or `expect` outside
tests. The parse step produces an owned tree; the evaluate step borrows it and the
context. Keep the built-in function set small and documented; every function added is a
compatibility commitment, because scenes will be saved using it.

## Notes

Expression text is saved in the scene file, so the language is a compatibility surface
from the first release that ships it. Anything ambiguous now becomes a migration later.
Worth an extra pass on the grammar before the first save format lands.

## Future considerations and out of scope

User-defined functions, string expressions, and any editor affordance beyond a plain text
field. Parameter integration and dependency tracking are a sibling task, not this one:
this task delivers the language, not its wiring into parameters.

## Verification

`cargo test -p <crate>`, plus the wasm-target build. Determinism is checked by evaluating
a fixed corpus on both targets and comparing results.

## Sub-tasks

- [ ] #<n> Differential fuzz corpus and CI gate
```

## Notes on the shape

- **Summary is two or three sentences and names the parent.** A task does not restate the
  epic's problem statement; a reader who wants the why follows the link. Restating context at
  every level is how trackers become unreadable.
- **Acceptance criteria are specific enough to disagree with.** "Numeric edge cases are
  defined and tested" names three cases. A criterion nobody could fail is not a criterion.
- **Design considerations here are local**, about this unit of work. The epic holds the ones
  that span tasks.
- **Technical plan names the working-agreement constraints that apply.** This is what lets
  someone who has not read `CLAUDE.md` still produce conforming code.
- **Notes is for the thing a reader should know that fits nowhere else.** Here it is that the
  grammar becomes a compatibility surface immediately, which changes how carefully the first
  version should be reviewed. If there is nothing like that, omit the section rather than
  padding it.
- **Sub-tasks appear only when they earned it.** The fuzz corpus is a sub-issue because it
  crosses into CI, carries its own acceptance criteria, and is a separate session's work.
  Writing the parser and writing the evaluator would not be: those are one session, one
  criterion set, one crate, and belong as checklist items if they need listing at all.
