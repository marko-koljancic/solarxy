# Parity checklist

The manual gate for the claim that the two shells present one product. Run it
before any release tag that changes a panel, a menu, a binding or the scene
file, and in full at the end of a release that touched the desktop shell.

The automated half is the family of tests that read the browser's own source
and compare it row by row with the desktop's tables: the global menus
(`crates/solarxy-app/src/gui/chrome/menu.rs`), the viewport bar, the pane
toolbar, the arrangements, the keymap (`state/input/keymap.rs`), the panel bars,
the samples menu and the rest. Those run on every `cargo test`. This file
covers what a test cannot: the same gesture on two screens, the same document
crossing between them, and the pixels the golden harness compares.

The desktop half of the interface has its own gate, `docs/qa/desktop-checklist.md`,
which this file does not repeat.

Record the run in the milestone spec's amendments: date, commit, both shells'
versions, and any box left unticked with the reason.

## Before you start

Both shells on the same commit. The desktop from `cargo run --release`; the
browser from `cd web && npm run dev`, opened in a WebGPU-capable browser on the
same machine. An empty scene on each.

The comparison tool the sections below use:

```bash
cargo run -p solarxy-graph --example scene_diff -- a.slxy b.slxy
```

It loads both files, cooks them to quiescence, and compares the documents
semantically: nodes are matched by name within their network, wires by the
names of the nodes they join, and the per-network canvas positions, the ids,
the selection and the timestamps are ignored, because two shells that agree on
the document are allowed to disagree on where the boxes sit. It prints one line
per difference and exits non-zero when there is any. Add `--view` to compare
the saved view as well (the pane cameras and display settings), which is off by
default because the pane count is host state.

## 1. The same input on both shells

One sequence, run once on each shell, using only bindings and menu entries both
shells have. The two socket drags in steps 7 and 8 are the one gesture with no
binding on either shell. Save at the end and compare the two files; the tool's
silence is the pass.

Where a step names a key, the pointer is over the surface the key belongs to:
the node canvas for the canvas keys, the viewport for the viewport keys.

1. `File > New Scene`.
2. Over the canvas, `Tab`; type `sopnet`; `Enter`.
3. Double-click the new container. The canvas shows its empty network and the
   breadcrumb names it.
4. `Tab`; type `box`; `Enter`.
5. `Tab`; type `sphere`; `Enter`.
6. `Tab`; type `merge`; `Enter`.
7. Drag the box's output socket onto the merge's input socket.
8. Drag the sphere's output socket onto the merge's input socket. The merge now
   has two wires in and a third, empty socket.
9. Click the merge; `E`. The merge carries the display flag and the viewport
   shows a box and a sphere.
10. Click the box. In the parameter panel, set size x to `2`; `Enter`.
11. Click the sphere. Set radius to `0.75`; `Enter`.
12. Click the box; `F2`; type `base`; `Enter`. The box is now named `base`.
13. Click the sphere; `B`. The sphere is bypassed and leaves the viewport.
14. In the node panel, `View > Fit Graph`. A view change only; it changes
    nothing in the document.
15. Click the breadcrumb's root entry. The canvas shows the root network.
16. Over the viewport, click the box. The container is selected, in the canvas
    and in the parameter panel.
17. `W`; drag the red handle a short way. The translate x field in the
    parameter panel moves on both shells.
18. In the parameter panel, set translate x to `1`; `Enter`. This normalises
    the hand drag to one number.
19. `Mod+D`. A second container appears, named by the engine.
20. `Mod+Z` until nothing changes, counting presses. Record the count for
    each shell; the desktop enables undo from the engine's depth and the
    browser always, so the count is an observable rather than a difference.
    Then `Mod+Shift+Z` the same number of times. Both containers are back.
21. Save: `Mod+S` on the desktop as `parity-desktop.slxy`; `File > Save
    Scene` in the browser as `parity-web.slxy`.
22. `cargo run -p solarxy-graph --example scene_diff -- parity-desktop.slxy
    parity-web.slxy`. Expect no lines.

- [ ] Every step produced the same visible result on both shells.
- [ ] The undo count is recorded for each shell.
- [ ] The tool printed nothing.

A line the tool prints is a finding: a fix in whichever shell diverged, or a
row in the divergence set with its reason.

## 2. The round trip

A document authored on one shell opens on the other with identical geometry,
materials, lighting and validation. The file from section 1 is the document.

- [ ] Open `parity-desktop.slxy` in the browser. The toast reports no warning.
      The viewport shows the same geometry; the parameter panel reads the same
      values on every node; the Validation tab, where present, reports the
      same counts. `File > Save Scene` as `crossed-web.slxy`, then
      `scene_diff parity-desktop.slxy crossed-web.slxy` prints nothing.
- [ ] Open `parity-web.slxy` on the desktop. No warning toast; the same checks;
      `Mod+S` as `crossed-desktop.slxy`, then
      `scene_diff parity-web.slxy crossed-desktop.slxy` prints nothing.
- [ ] Open each of the two scenes saved before the context vocabulary moved,
      `crates/solarxy-graph/tests/fixtures/scenes/v1-the-orrery.slxy` and
      `v1-texture-to-material.slxy`, on both shells. No warning toast on either,
      and every node keeps its parameters (the sample of the same name, opened
      beside it, reads identically).
- [ ] Embedded assets: drop `res/models/knot/knot.obj` on the desktop, save,
      open in the browser. The knot is textured. Import the same model in the
      browser, save, open on the desktop. Textured.
- [ ] Per-pane look and camera: on the desktop, `F2` for a vertical split, set
      pane 2 to Orthographic (`O`) and open its `Look...` and change the
      exposure; save; open in the browser. Pane 2 is orthographic and its
      `Look...` reads the same exposure. Reverse the direction.
- [ ] A scene carrying a description and canvas viewports (any scene the
      browser saved) opened on the desktop and saved again still carries them:
      `scene_diff --view` on the two files prints nothing, and the description
      is present in the desktop's file.
