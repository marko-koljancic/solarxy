# Releasing 0.8.1

The release runbook for this version. Everything in-repo is prepared; what
remains needs your git and artifacts that do not exist until CI has run.

**This file was rewritten on 2026-07-28.** The previous version described the
scope re-split that was reversed on that date, and it told you no
`[golden-accept]` was needed on the merge. Both were wrong by the time you
would have read them. If anything below disagrees with your memory of the
earlier version, this one is right.

## What 0.8.1 actually ships

The **full original scope**, restored on 2026-07-28. There is no v0.8.3.

- Expressions on any numeric parameter, with `ch()` cross-node references,
  geometry queries, a rename that rewrites referring expressions, and
  auto-numbered graph-unique node names.
- The `attribute_wrangle` node: the same language, run per point or primitive.
- A scene clock with a Playbar, and a tick that dirties only what reads time.
- Standalone web export: **File > Export web bundle** writes a zip that runs
  on any static host.
- Physically based rect-area lights through linearly transformed cosines.
- The backlog tier: `pscale` in `copy_to_points`, scatter density-by-attribute,
  a global point-size preference. Plus OBJ vertex colours and the Max-style
  gizmo fade.
- Two rounds of testing feedback (22 items) and a pre-tag quality pass.

All of it is **Solarxy Web only**. `solarxy-app` is still unwired from
`solarxy-graph`, so desktop has no parameters to drive and no light nodes to
author; it receives the world-space shading hoist underneath, with output
unchanged. The release notes must say so rather than imply a desktop feature.

## What is already done

| Prepared | Where |
|---|---|
| Version 0.8.0 to 0.8.1 | `Cargo.toml` `[workspace.package]`, `web/package.json`, both lockfiles |
| winget manifests for 0.8.1 | `packaging/winget/manifests/k/Koljam/Solarxy/0.8.1/` (3 YAML, `{{INSTALLER_SHA256}}` placeholder intact) |
| Registry snapshot + node reference | `schemas/registry.json`, `../solarxy.wiki/Node-Reference.md`. Registry at **76** |
| Release notes | `../solarxy.wiki/Release-Notes.md`, v0.8.1 section at the top |
| Four new wiki pages | `Expressions.md`, `Attribute-Wrangle.md`, `Runtime-And-Playback.md`, `Publishing-A-Scene.md`, all linked from `_Sidebar.md` |
| Eight updated wiki pages | `Node-Reference`, `Solarxy-Web`, `User-Guide`, `Keyboard-Shortcuts`, `Architecture`, `FAQ`, `Release-Notes`, `_Sidebar` |
| Landing copy and counts | `web/index.html` (76 node types, an "Animate and publish" card) |
| README counts | 76 node types, gated by `registry_drift.rs` |
| About dialog | Refreshed for the 0.8.1 product, with the four new wiki pages linked |
| Roadmap and milestone docs | workspace root `Docs/` |

## Gate status, measured 2026-07-28 after the quality pass

- `cargo fmt --all --check` clean
- `cargo clippy --workspace --all-features -- -D warnings` clean
- `cargo clippy --workspace --all-targets` clean
- `cargo clippy -p solarxy-web --target wasm32-unknown-unknown -- -D warnings` clean
- `cargo test -p solarxy-scenefile --features schemars-gen` clean
- `cargo test --workspace`: **1,259 passed, 0 failed** across 37 binaries
- Renderer suite green under `SOLARXY_REQUIRE_GPU=1`
- `web/`: typecheck clean, **301 vitest passed**, production build green
- Budgets, all three green: wasm 1,716,883 / 2,621,440; editor boot JS
  389,430 / 471,040; player JS 17,629 / 51,200 (gzip, freshly built wasm)
- Goldens: **13 of 14 captures bit-identical at tolerance 0.** The one diff is
  `frog/validation`, the known nondeterminism, proven by capturing identical
  code twice and reproducing it at the same magnitude.

## The merge PR needs `[golden-accept]`

**This is not optional and it is the easiest thing to get wrong.** The goldens
job takes `pull_request.base.sha`, so merging `0.8.1/expression-engine` into
`main` compares against **0.8.0** and surfaces the world-space shading hoist
diff, which is large and expected.

That diff was measured against `main` on 2026-07-28: five of six differing
captures are bit-identical to the hoist-alone numbers recorded in the W0a
amendment, and the sixth is the `frog/validation` nondeterminism. Nothing
after the hoist moved a pixel.

Put `[golden-accept]` in the merge commit message with that justification.

## Order of operations

The winget directory **must be committed before the tag is pushed**, or
`.github/workflows/winget-release.yml` hard-throws `"No winget manifest at
..."`. It already exists; just make sure it is in the commit.

1. **Commit the main repo** on `0.8.1/expression-engine`. Message below.
2. **Desktop smoke test.** Yours: the renderer's lighting loop moved to world
   space this release, and two shared-code fixes (the label uniform layout and
   the playback pacing gate) need a human eye on a window.
   ```
   cargo r --release -- --model res/models/xyzrgb_dragon.obj
   ```
   Confirm: the model is lit and shaded correctly with no black viewport; the
   grid does not shimmer; toggling shading modes and the axis views behaves.
3. **Screenshots.** Yours, deliberately deferred to now so shapes are captured
   once. The four new wiki pages carry no imagery, and `Solarxy-Web.md` has
   one existing app screenshot that predates the Playbar.
4. **Open the PR to `main` and merge it with `[golden-accept]`.**
5. **Tag and push `v0.8.1`.** CI/CD produces the installers, native bundles
   and the MSI; the winget workflow fills the SHA256 from the real MSI and
   submits.
6. **Homebrew** (`Sources/homebrew-solarxy/`). Only possible after step 5,
   because the SHA256s come from the published assets. Bump `version` and
   `sha256` in `Casks/solarxy.rb` (DMG) and `Formula/solarxy-cli.rb`
   (portable zip).
7. **Wiki.** Commit on `master` (branches are in sync, so the `develop` hop is
   being skipped this release). GitHub renders `master` only.
8. **Deploy the web app** to the Hetzner VPS, and **add the `/player` nginx
   rule** below before or with that deploy.

## The nginx `/player` rule (new this release)

`player.html` is a **new third Vite entry** alongside the landing page and the
app. The build already produces it and the exported bundle already works
without any server change, but the hosted `/player` route needs a rule beside
the existing `/app` one. Without it, `/player` 404s and only the hosted dev
route is lost; exported bundles are unaffected either way.

The edge configuration lives in a separately-owned repository outside this
workspace, so this is a **draft to reconcile against the real file**, not a
patch. Mirror whatever shape the existing `/app` rule uses:

```nginx
# Beside the existing `/app` location.
location = /player {
    try_files /player.html =404;
}
```

If `/app` is written with a trailing-slash variant or a `rewrite`, match that
form instead. `gzip_static on` already covers the new page: the release
workflow pre-compresses every `.html`, `.js`, `.css`, `.wasm` and `.json` in
the bundle, `player.html.gz` included.

Verify after deploying:

```bash
curl -sI https://solarxy.koljam.com/player | head -1          # expect 200
curl -s  https://solarxy.koljam.com/player | grep -c player-  # expect >= 1
```

## Suggested commit message

```
Release 0.8.1: expressions, wrangles, playback and publishing

The release where a Solarxy scene stops being a static arrangement of
settings and starts being something that computes, moves, and can leave
the editor.

Parameters compute. Any numeric param can hold a formula that does
arithmetic, calls ~30 builtins, reads another node with ch("box1/width"),
and measures its own gathered geometry with npoints() and bbox(). A new
expr/ module drops into the one seam resolve_params always reserved, so an
expression result rejoins the literal path before conform, clamp and unit
conversion and cannot smuggle an out-of-range value past the resolver.
Cross-node references are a separate DAG over (NodeId, key) pairs: cycles
are refused at set time, and a rename rewrites every expression pointing at
the renamed node inside the rename's own undo step. That required a naming
model the codebase never had, so nodes now mint graph-unique auto-numbered
names.

The attribute wrangle takes the same language down to a single point. It is
not a second grammar: a Scope trait switches the ONE parser between refusing
element scope and resolving it to slot indices, so operators, precedence,
the builtins, ch() and $T are the same code in both. The kernel owns the
geometry mechanics behind an ElementFn; the graph owns the language.

A scene clock gives both something that moves. One tick is one frame, so $T
is exactly $F / $FPS and frame 90 is reproducible on any machine. The tick
refuses to advance while the previous frame is still draining, so a heavy
scene plays slowly rather than skipping and the frame counter never runs
ahead of the picture.

And File > Export web bundle puts the result on a URL. The archive carries
the engine rather than a recording, so an expression- or wrangle-driven
scene keeps computing for whoever opens the link.

rect_area_light stops approximating. It shades through linearly transformed
cosines against the rectangle's four corners, so Width, Height and a
restored Rotate reach the image, and a new Two Sided toggle emits from the
back face. The tables are fitted by examples/gen_ltc_lut rather than
copied, and tests/ltc_fit.rs checks that fit by QUALITY rather than
parameter equality, because two correct non-convex fits land on different
optima.

Two rounds of testing feedback (22 items) and a pre-tag quality pass are
folded in. The pass removed 155 milestone planning codes from comments and
test titles, replacing each with what the code stood for, and added a gate
so they cannot come back: those codes mean nothing without a planning
document open, and the decision codes collide across milestones. It also
extracted the draft-commit contract that three text fields had each
copy-pasted, which is what made a fourth field's per-keystroke commit bug
visible, and gated naming::node_name against its TypeScript mirror because
expressions address nodes by name.

Goldens are 13 of 14 captures bit-identical; the one diff is the known
frog/validation nondeterminism, proven by capturing identical code twice.

Both features are Solarxy Web only: solarxy-app is still unwired from
solarxy-graph, so desktop has no params to drive and no light nodes to
author. Desktop gets the world-space shading hoist underneath, with output
unchanged.
```

## Suggested wiki commit message

```
Document 0.8.1: expressions, wrangles, playback and publishing

Four new pages: Expressions (the grammar, ch() paths, geometry queries, the
naming model, the sandbox limits), Attribute-Wrangle, Runtime-And-Playback
and Publishing-A-Scene, all linked from the sidebar.

Release notes for v0.8.1. Node-Reference regenerated at 76 node types.
Solarxy-Web covers the new capabilities, the Playbar, the Text panel and
the seven sample scenes. Keyboard-Shortcuts gains the playback bindings.
Architecture's node count corrected. Two FAQ entries for the questions this
release will actually generate, including why area-light intensity does not
behave like a point light's and why a heavy scene plays at a lower frame
rate rather than skipping frames.
```

## Delete this file once 0.8.1 is out

It is version-specific. The durable record is the Amendments section of
`Docs/SOLARXY-MILESTONE-0.8.1.md`.
