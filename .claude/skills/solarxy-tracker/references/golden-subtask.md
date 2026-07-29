# Golden: sub-task

The light form. A sub-task exists only when it earned the split: the work crosses crates,
spans more than one work session, or carries its own acceptance criteria. If none of those is
true, it is a checklist item inside its task and not an issue at all.

Because it earned the split for a stated reason, the body says which reason, so the
decomposition stays reviewable rather than becoming habit.

Title format: a plain imperative. For example `Differential fuzz corpus and CI gate`.

Labels: `level: subtask`, one `area:` label. Parent set to the task.

## Worked example

```markdown
## Summary

Build a fuzz corpus for the expression parser and wire it into CI as a gate, so malformed
input can never reach a panic path in a shipped build.

Parent task: #<n>
Split reason: crosses into CI configuration and carries its own acceptance criteria.

## Acceptance criteria

- [ ] A corpus of malformed and adversarial expression inputs lives in the repository and
      runs as part of the test suite.
- [ ] No input in the corpus produces a panic, an unbounded allocation, or a hang.
- [ ] The gate runs in CI on every pull request and fails the build on a new panic.
- [ ] A newly discovered failing input is added to the corpus as part of fixing it, so the
      corpus grows monotonically.

## Notes

The parser takes user input on every keystroke, so a panic here is not a crash in an edge
case, it is a crash during normal typing. That is why this is a gate rather than a
best-effort test.

## Verification

`cargo test -p <crate>` locally, and a green CI run on a branch containing a deliberately
reintroduced bad input to confirm the gate actually fails.
```

## Notes on the shape

- **Four sections, no more.** Summary, acceptance criteria, notes, verification. A sub-task
  that needs design considerations or a technical plan is a task that was misfiled.
- **The split reason is stated in the body.** It is one line, and it is what keeps the
  granularity rule honest over time. A sub-task that cannot name its reason should be a
  checklist item.
- **No context is repeated from the parent.** The parent link carries it.
- **Verification names the check that would actually catch a regression**, which here means
  deliberately breaking it once to confirm the gate fires. A gate nobody has seen fail is an
  assumption, not a gate.
