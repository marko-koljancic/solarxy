# Pre-finish checklist for a milestone spec

Run every check before declaring the milestone doc done. Paths are relative to the Solarxy
repo root, where the skill runs.

## Content

- [ ] Every work item is grounded in a `crates/.../file.rs:line` citation that was read and
      confirmed, with current behavior stated distinctly from proposed behavior.
- [ ] Every claim of absence names the search that established it, not an assumption.
- [ ] Every behavior-changing item appears as a numbered milestone decision with a
      recommendation, and is marked ratified or proposed.
- [ ] Every work item maps to at least one persona and one journey.
- [ ] Effort is assigned per task; sequencing puts fixes before additive features before
      polish.
- [ ] A verification section names the exact checks: `cargo fmt`, `cargo clippy`,
      `cargo test`, the registry drift tests, renderer goldens if the renderer changes,
      `web/` typecheck and test and build, and manual QA per shell.
- [ ] A definition of done gives a measurable completion bar and references the cross-surface
      sweep in `.claude/skills/solarxy-sync/SKILL.md`.
- [ ] The registry-count impact is stated when node types are added: the count assert, the
      snapshot regeneration, the README counts, and the wiki node reference regeneration.

## Guardrails

- [ ] **Redaction.** The spec names no reference checkout other than the Minimystix
      prototype. Run the check from the public-surface rules in
      `.claude/skills/solarxy-domain/SKILL.md` against the finished file; zero matches
      required. Describe every capability in Solarxy-native terms; cite Minimystix, Houdini,
      or Blender only when an outside reference genuinely clarifies intent.
- [ ] No emojis, no em or en dashes, no divider lines in body text. A YAML frontmatter fence
      is the only permitted use of three hyphens on their own line.
- [ ] Code guidance honors the working agreement: no `unwrap` or `expect` outside tests;
      `thiserror` in library crates and `anyhow` in binaries; the zero-frontend-change
      contract.
- [ ] **Planning codes stay in the document.** Decision numbers, work-item codes, and stage
      and phase numbers mean nothing to a reader without this document open, and they collide
      across specs: the same decision number names a different decision in every milestone.
      Where the spec dictates a comment, a doc comment, a test name, or a log line, it must be
      written in durable prose carrying the substance, not the code. Version references are
      fine and often load-bearing. Enforced by `no_planning_codes_in_comments` in
      `crates/solarxy-core/tests/tokens_drift.rs`.

## Cross-references

- [ ] `../../Docs/SOLARXY-ROADMAP.md` links to the new milestone doc, in section E and in a
      dated Amendments entry.
- [ ] Every count the artifact mirrors is updated, per the artifact-sync table in
      `.claude/skills/solarxy-sync/SKILL.md`.
