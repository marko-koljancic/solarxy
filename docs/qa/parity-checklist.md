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

## 3. The walk

Every panel, menu, menu entry and binding, compared entry by entry on both
shells. Most of it is held by a test that reads the browser's own source, so
what is left here is what a test cannot see: what a panel lists once there is
a scene in it, labels the browser computes at runtime, and how a key behaves
on a real window.

### Held by a test

These run on every `cargo test` and need no one at a keyboard. Each reads the
browser component named beside it and compares row by row, and each named
difference is checked in reverse, so a difference that stops applying fails
the build rather than lingering.

| Surface | Held by | Against |
|---|---|---|
| The five global menus and their order | `chrome/menu.rs` | `menu/MenuBar.tsx` |
| The arrangement entries and the seven panel toggles | `chrome/menu.rs` | `menu/MenuBar.tsx` |
| The viewport bar, its pane layouts and handle frames, with their keys | `chrome/viewport_bar.rs` | `ViewportMenuBar.tsx` |
| The node pane View menu, its order and every key | `panels/nodes/menus.rs` | `menu/NodePaneViewMenu.tsx` |
| The connection styles | `panels/nodes/menus.rs` | `store/ui.ts` |
| The Add menu lead entry and its key | `panels/nodes/menus.rs` | `menu/NodesMenu.tsx` |
| The three Properties menus, their order and every key | `panels/params/menus.rs` | `menu/PropertiesMenus.tsx` |
| View modes, inspections, overrides, normals, bounds, wireframe weights, backgrounds | `chrome/pane_toolbar.rs` | `PaneToolbar.tsx` |
| The six standard views and their keys | `chrome/pane_toolbar.rs` | `PaneToolbar.tsx` |
| Fit view, UV Layout and the two projections, with their keys | `chrome/pane_toolbar.rs` | `PaneToolbar.tsx` |
| The four Display submenus that say what they are set to | `chrome/pane_toolbar.rs` | `PaneToolbar.tsx` |
| The viewport context menu: heading, five tools with keys, six entries in order | `chrome/viewport_context_menu.rs` | `ViewportContextMenu.tsx` |
| The ten panel titles, and that the panel sets are the same set | `gui/dock.rs` | `dock/layouts.ts`, `dock/api.ts` |
| The review sections, the filter and its hint, the empty state | `panels/review/panel.rs` | `review/ReviewPanel.tsx` |
| Every binding, in every scope | `state/input/keymap.rs` | `input/keymap.ts` |
| The shortcuts reference lists exactly the bindings | `gui/modals/shortcuts.rs` | its own `state/input/keymap.rs`, which the row above holds to the browser |
| The six arrangements | `gui/arrangement.rs` | `store/desks.ts` |

### By hand

About an hour per shell, on a scene with geometry, a camera, a light, a
material network and at least one review note.

- [ ] **Each panel, with something in it.** Open all ten on both shells and
      compare what they list, not what their bar offers: the tree rows and
      their state marks, the parameter panel tabs for a node of each family,
      the assets grid and its kinds, the attributes table headers and paging,
      the texture panel with an image network open, the text panel with a
      snippet in each context, the node info card. Empty states too, since a
      panel says something different with nothing to show.
- [ ] **The Add submenus, per category.** The category names and the node
      types under each are computed from the registry on both shells and no
      reader can compare them, so they are compared here: every category the
      current context offers, in the same order, holding the same types under
      the same display names, in the object, geometry, material and image
      contexts.
- [ ] **Every binding, in every scope.** Press each listed key three times,
      with the pointer over the viewport, over the node canvas, and over
      neither, and confirm the same thing happens on both shells, including
      the ones that mean two things in two scopes (`B`, `C`, `E`, `F`, `L`,
      `P`). This is the half the tables cannot prove: they agree about what
      is bound, not about what the window does with it.
- [ ] **The six modals.** Preferences, Keyboard Shortcuts, About, Environment,
      Unsaved changes and Recovery: open each on both shells and compare the
      fields, their order, the wording of the buttons, and what Escape does.

Findings that are not fixed go to the milestone document's divergence set
with a reason, never into this file.
