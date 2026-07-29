# The `.claude` directory

This directory configures [Claude Code](https://claude.com/claude-code), the AI coding agent
used to build Solarxy. It is committed deliberately, not by accident.

If you are a human reading the repository, this file is your entry point. **Nothing here is
loaded automatically**, so it costs you nothing to skip. The file that *is* loaded into every
session is `CLAUDE.md` at the repository root.

## Why a 3D viewer ships role-persona files

Solarxy is built by one maintainer working with AI agents. The work spans a Rust workspace of
twelve crates, a WebGPU renderer, a React frontend, three shipping surfaces, a packaging
pipeline across five channels, and a public site. No single generalist prompt holds that well.

So the work is split by domain, and each domain gets an agent with its own scope, its own tools,
and its own rules. An agent is a markdown file: frontmatter declaring what it is and what it may
touch, then instructions. A skill is the same idea for a procedure rather than a role. Together
they make the project's conventions explicit and reviewable instead of living in one person's
head, which is also why they are public: they document how this software is actually made.

## What is in here

- `agents/` one file per role. Each declares what it owns, what it must not do, and where it
  hands off.
- `skills/` one directory per procedure, each with a `SKILL.md`. Some carry reference files.
- `commands/` reserved; currently empty, because skills already provide slash invocation.

The current roster, with one line on when to reach for each, is in the **Agents and skills**
section of `CLAUDE.md`. It is not duplicated here, deliberately: this file would go stale
against it, and that has already happened once.

## The domains

Every agent reads `.claude/skills/solarxy-domain/SKILL.md` before forming a judgment. That briefing
carries what no single role owns: the architecture invariants, the shared vocabulary, the five
surfaces of record, the public-surface rules, and the reading list.

| Domain | Owned by | Artifacts it governs | Process lives in |
|---|---|---|---|
| Product strategy | `product-manager` | The roadmap, the release program, positioning and release themes | `../../Docs/SOLARXY-ROADMAP.md`, `../../Docs/SOLARXY-MILESTONE-PROGRAM.md` |
| Delivery management | `product-owner` | Epics, tasks, acceptance criteria, exit gates | `.claude/skills/solarxy-tracker/SKILL.md` |
| Design | `product-designer` | The per-shell Pencil files, the UX specification, the keymap, the public page design | `design/README.md`, `.claude/skills/solarxy-brand/SKILL.md` |
| Engineering | `architect`, then `rust-engineer`, `frontend-engineer`, `graphics-engineer` | The crates, the renderer, the frontend, the wasm boundary | `CLAUDE.md` |
| Quality | `qa-engineer` | Tests, drift and golden gates, exit-criteria evidence | The definition of done in each milestone specification |
| Documentation | `technical-writer` | Planning docs and amendments, the wiki, release notes, public item text | `.claude/skills/solarxy-sync/SKILL.md` |
| Operations | `devops-engineer` | CI, packaging, size budgets, the edge and deploy path | The release train, in the workspace-level `CLAUDE.md` |
| Security | `security-engineer` | Headers and policy, supply chain, untrusted input, public disclosure | Reviewed per change, no standing document |

The four project-specific roles that are advisory (`product-manager`, `product-owner`,
`product-designer`, `technical-writer`) produce findings and drafts and never edit code. The
implementers write code. `architect` designs and does not implement. None of them run git; the
maintainer does.

## How work flows

`.claude/skills/solarxy-sdlc/SKILL.md` maps the nine stages from an idea to a verified release, names
the owner of each, and points at the authoritative file. Start there if you want to know how a
change actually gets from proposed to shipped.

## Conventions worth knowing

- **This directory is public, and treated as such.** No secrets, no host or deployment detail,
  and a redaction rule about which third-party projects may be named. Operational specifics live
  in a separate private repository.
- **Every shared rule has exactly one home.** If two files would state the same rule, one states
  it and the other points. This is enforced by review rather than by tooling, and it has already
  had to be repaired once.
- **The house writing style is deliberate**: no emoji, no em or en dashes, no arrow glyphs, no
  horizontal-rule dividers. It applies to everything authored here.
- **Additions are gated.** New dependencies, top-level directories, crates, and binary assets
  need the maintainer's approval first. The rule and its boundary are in the Working Agreement
  in `CLAUDE.md`.
