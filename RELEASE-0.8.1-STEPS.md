# Releasing 0.8.1

Scratch handover, not a tracked document. Delete it once the release is out.

Everything in-repo is prepared: the version is bumped, both lockfiles are
synced, the winget manifest directory exists, and the wiki is written. What
remains needs your git and artifacts that do not exist until CI has run.

## What is already done

| Prepared | Where |
|---|---|
| Version 0.8.0 to 0.8.1 | `Cargo.toml` `[workspace.package]`, `web/package.json`, both lockfiles |
| winget manifests for 0.8.1 | `packaging/winget/manifests/k/Koljam/Solarxy/0.8.1/` (3 YAML, `{{INSTALLER_SHA256}}` placeholder intact) |
| Registry snapshot + node reference | `schemas/registry.json`, `../solarxy.wiki/Node-Reference.md` |
| Release notes | `../solarxy.wiki/Release-Notes.md`, new v0.8.1 section at the top |
| New wiki page | `../solarxy.wiki/Expressions.md`, linked from `_Sidebar.md` |
| Landing copy | `web/index.html` (node count was already correct at 74) |
| Roadmap and milestone docs | workspace root `Docs/` |

## Gate status at the time of writing

- `cargo fmt --all --check` clean
- `cargo clippy --workspace --all-features -- -D warnings` clean
- `cargo clippy -p solarxy-web --target wasm32-unknown-unknown -- -D warnings` clean
- `cargo test --workspace`: **1124 passed, 0 failed**
- `web/`: typecheck clean, **219 vitest passed**, production build green
- Goldens: **delta 0** on every deterministic mode. **No `[golden-accept]`
  is needed for this release.** The one `frog/validation` diff is the
  pre-existing nondeterminism; it reproduces between two captures of
  identical code.

## Order of operations

The winget directory **must be committed before the tag is pushed**, or
`.github/workflows/winget-release.yml` hard-throws `"No winget manifest at
..."`. It is already created, so just make sure it is in the commit.

1. **Commit the main repo.** Suggested message below.
2. **Desktop smoke test.** The renderer's lighting loop moved to world
   space this release, so open the GUI once and confirm a model looks
   right: `cargo r --release -- --model res/models/xyzrgb_dragon.obj`
3. **Tag and push.** `v0.8.1`. CI/CD produces the installers, native
   bundles and the MSI, and the winget workflow fills the SHA256 from the
   real MSI and submits.
4. **Homebrew** (`Sources/homebrew-solarxy/`). Only possible after step 3,
   because the SHA256s come from the published assets. Bump `version` and
   `sha256` in `Casks/solarxy.rb` (DMG) and `Formula/solarxy-cli.rb`
   (portable zip).
5. **Wiki.** Commit on `develop`, then merge to `master`. GitHub renders
   `master` only, so the pages are invisible until that merge.
6. **Deploy the web app** to the Hetzner VPS (solarxy.koljam.com).

## Suggested commit message

```
Release 0.8.1: expressions and physically based area lights

Parameters compute. Any numeric param can hold a formula that does
arithmetic, calls ~30 builtins, reads another node with ch("box1/width"),
and measures its own gathered geometry with npoints() and bbox(). A new
expr/ module (lexer, parser, evaluator, sandbox limits) drops into the one
seam resolve_params always reserved, so an expression result rejoins the
literal path before conform, clamp and unit conversion and cannot smuggle
an out-of-range value past the resolver. Cross-node references are a
separate DAG over (NodeId, key) pairs: cycles are refused at set time, and
a rename rewrites every expression pointing at the renamed node inside the
rename's own undo step. That required a naming model the codebase never
had, so nodes now mint graph-unique auto-numbered names.

rect_area_light stops approximating. It shades through linearly
transformed cosines against the rectangle's four corners, so Width, Height
and a restored Rotate reach the image; a new Two Sided toggle emits from
the back face. The tables are fitted by examples/gen_ltc_lut rather than
copied, and tests/ltc_fit.rs checks that fit against the published
reference by fit QUALITY rather than parameter equality, because two
correct non-convex fits land on different optima: the median cell scores
1.001 and 56 of 240 beat the reference. LightEntry grows 64 to 96 bytes
(the milestone doc's 80 was arithmetic done in prose), the WGSL struct
moves in lockstep, and the light bind group gains the two tables.

Goldens are delta 0 on every deterministic mode: the captured models carry
no light nodes, so the rect-area branch is unreachable by the gate and no
re-baseline is needed.

Three F1 defects that only a browser could find, all fixed: geometry
queries silently answered 0 on an unconnected input instead of naming the
problem, so width = npoints() clamped to the floor and cooked an invisible
box; the parameter panel's readout had no geometry capability at all and
reported valid expressions as unavailable while the node cooked green; and
the Text widget committed per keystroke while Enter double-dispatched
through its own blur, making one 7-character rename nine undo steps.

0.8.1's scope was re-split during implementation: the attribute wrangle,
the runtime and web export move to 0.8.3, because area lights are a hard
dependency of 0.8.2's path tracer and the original cut order would have
cut them. Recorded in the milestone and roadmap amendments.

Both features are Solarxy Web only: solarxy-app is still unwired from
solarxy-graph, so desktop has no params to drive and no light nodes to
author. Desktop gets the world-space shading hoist underneath, with
output unchanged.
```

## Suggested wiki commit message

```
Document 0.8.1: expressions and area lights

New Expressions page covering the grammar, ch() paths, geometry queries,
the naming model and the sandbox limits. Release notes for v0.8.1,
including why area-light intensity does not behave like a point light's.
Node-Reference regenerated for rect_area_light v3. Two FAQ entries for the
questions this release will actually generate. Architecture's node count
corrected from 33 to 74.
```
