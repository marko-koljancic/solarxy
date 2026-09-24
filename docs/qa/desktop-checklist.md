# Desktop QA checklist

The manual gate for the desktop viewer (`solarxy`). Run it before any release tag,
and whenever a shared crate (`solarxy-core`, `solarxy-renderer`, `solarxy-kernel`,
`solarxy-formats`) changes in a way the golden captures cannot see.

The automated half is `crates/solarxy-host/examples/golden.rs` (see
"Golden captures" below). This file covers what goldens cannot: interaction,
input, dialogs, and anything that needs a window.

Rendering has its own gate, `docs/qa/render-checklist.md`, because it spans all
three shells and half of what it checks is not on this one.

Record the run in the milestone spec's amendments: date, commit, platform, and
any box left unticked with the reason.

## Launch

```bash
cargo run --release -- --model res/models/xyzrgb_dragon.obj
```

## 1. Model loading

- [ ] `xyzrgb_dragon.obj` loads, auto-frames, and orbits smoothly.
- [ ] `res/models/knot/knot.obj` loads **with its texture** (OBJ + MTL + `map_Kd`). The knot is banded in colour with dark speckles, not white and not untextured.
- [ ] An STL loads (`crates/solarxy-formats/tests/fixtures/triangle.stl` or any STL).
- [ ] A PLY loads.
- [ ] A glTF/GLB loads (`crates/solarxy-formats/tests/fixtures/textured.glb`) **with its texture**.
- [ ] A colored-but-untextured material renders in its colour, **not white**. (This was a real bug fixed in Phase 14: `base_color_factor` was parsed but never reached the shader.)
- [ ] Drag-and-drop a model onto the window loads it.

## 2. Inspection modes (number keys, pointer over the viewport)

The keys are scoped: they act on the pane under the pointer and do nothing over the node canvas.
Each mode renders without artifacts and the pane's own Inspect label names it.

- [ ] 1 Shaded
- [ ] 2 Material ID
- [ ] 3 toggles the UV pane, and the pane's Display menu has `Exit UV Layout` as the way back.
- [ ] 4 Texel Density
- [ ] 5 Depth
- [ ] 6 Overdraw
- [ ] 7 AO Preview

## 3. Display and overlays (the per-pane menus)

None of these has a key since the two shells came to share one keymap. Each is reached from the
labels across the top of a pane.

- [ ] Wireframe and ghosted-wireframe, from the pane's view-mode menu.
- [ ] `Path Traced`, from the same menu, on a device that traces: the label reads it, the image
      converges, the readout beside the labels says `tracing...` and then climbs to `4096 spp`
      and parks, orbiting restarts it, and picking any view mode returns the pane to the
      rasterizer with its camera where it was.
- [ ] Normals, Bounds and Wireframe weight, from the Display menu's submenus.
- [ ] Grid and Axes toggle from the Display menu.
- [ ] Material overrides from the pane's material menu: Clay, Clay Dark, Chrome, Silhouette.
- [ ] Background lists six in this order: Gradient, White, Dark, Ayu, Black, HDRI Sky. With no HDRI
      loaded, HDRI Sky is greyed and says why on hover.
- [ ] `Look...` opens the pane's look editor, modeless and one per pane; `Light markers` toggles
      the pane's markers.
- [ ] **Validation overlay**: issues highlight, and the non-manifold **edge lines do not z-fight** with the surface. (WebGPU forbids `depthBias` on line topologies, so the depth bias was removed from that pipeline in Phase 0. If z-fighting is visible, the fix is a clip-space nudge in `vs_validation`.)

## 4. Layouts and cameras

- [ ] F1 single, F2 vertical split, F3 horizontal split, F4 quad, F5 three-left-big, from the keys
      and from the viewport's `View > Pane Layout`.
- [ ] Orbit, pan, zoom, and arrow-key nav work in every layout.
- [ ] The active pane follows the cursor; per-pane inspection modes are independent.
- [ ] Each pane's camera moves alone. Nothing links them, and none starts linked.
- [ ] `Z` fits the view, `T` `F` `L` `B` snap to a side, `P` and `O` switch projection.
- [ ] One pane of a split traced beside a raster pane: orbiting the raster pane leaves the traced
      pane's accumulation alone, and minimizing the window stops GPU work until it is restored.

## 5. Lighting

- [ ] Drag-drop an `.hdr` HDRI: sky renders, IBL lights the model.
- [ ] Drag-drop an `.exr` HDRI.
- [ ] The viewport's `View > Environment...` opens the dialog: load and clear the HDRI, the IBL
      mode, rotation and intensity. This is the only place the IBL mode is set.
- [ ] Shadows render; the shadow-catching floor works.

## 6. UV pane

- [ ] A UV pane opens and shows the layout.
- [ ] The overlap statistic computes (it is an async GPU readback).

## 7. Menus

- [ ] The global bar is five menus: File, Edit, Desks, Review, Help. It cannot be hidden.
- [ ] Every shortcut shown beside an entry does what it says when pressed.
- [ ] An entry that cannot act is greyed and says why on hover, and none disappears as the scene
      changes. `Help > Take a Tour` is one.
- [ ] The File menu has no `Close`. A document is left by opening another or by `New Scene`, and
      both ask first when there are unsaved changes.
- [ ] Each of the four panels with a bar of its own draws it across its top, docked or floated:
      the node panel (Add, View), the parameter panel (Node, Params, View), the viewport (View)
      and the text panel (File, View).
- [ ] The node panel's Add menu lists the node types of the network being looked at, by category,
      and a node added from it lands in view.
- [ ] The viewport's `Gizmo Orientation`, `Playbar` and `Export Turntable...` each act.
- [ ] The Review menu's `Import Review Notes...` is enabled with a document open and
      `Export Review Notes...` once a note exists; each opens a native dialog.

## 8. Panels and arrangements

- [ ] The Desks menu toggles seven panels (Nodes, Properties, Tree, Text, Assets, Texture Viewer,
      Attributes), and a tick always agrees with what is on screen. There is no Sidebar.
- [ ] The Viewport tab has no close button and cannot be dragged out into a window.
- [ ] A closed panel comes back beside a sensible neighbour rather than in a corner.
- [ ] Each of the six built-in arrangements applies: Default, Modeling, Review, Technical,
      LookDev, UV / Texturing. Applying one never touches the document or its unsaved state.
- [ ] `Save Current As...` saves under a name, the dialog says when a name would replace one, the
      saved arrangement applies, survives a restart, and `Delete Desk` removes it.
- [ ] `Maximize Panel` from each bar, and the backtick key over any panel, fills the window with
      that panel. The same entry, the same key and `Esc` restore it.
- [ ] Quit while a panel is maximized and relaunch: the whole arrangement comes back, not the one
      panel.
- [ ] `P` over the node canvas opens the floating parameter panel, with a pin of its own; `Esc`
      closes it.
- [ ] Preferences (`Ctrl/Cmd+,`) opens with four tabs: Startup, Appearance, View, Interface. The
      theme hot-swaps light and dark with no restart. The View tab has one row, the default
      background.
- [ ] The keyboard-shortcuts reference (`?`) lists what the keys actually do.

## 9. Review system

Review arms on any open document: the notes are the document's, the same ones the browser shows,
and every change is one undo step.

- [ ] `Shift+R` over the viewport enters review mode, opens the panel, and a click on geometry
      opens the note popup; Save adds the note and `Cmd/Ctrl+Z` removes it in one step.
- [ ] `N` toggles the review panel, from anywhere.
- [ ] Clicking a marker selects its note in the panel, in any mode. The selected note offers
      Complete, Reply, Edit, Re-place and Delete; Delete asks first, even with no replies.
- [ ] Translate the node under a note and the marker follows; change the geometry beneath it and
      the note lists under `Needs re-anchor` with the warning dot; Re-place clears both.
- [ ] Hide the node and the pin goes; show it and the pin returns.
- [ ] `Export Review Notes...` writes the sidecar beside the scene; `Import Review Notes...` of a
      file from an earlier release brings every note, reply, author and resolved state, and a
      second import of the same file adds none.

## 10. Screenshot

- [ ] `C` over the viewport opens the screenshot modal; Save As writes a PNG. The viewport's
      `View > Save Screenshot...` opens the same one.

## 11. Coming from an earlier version

Run these against a configuration file written by a previous release. Keep a copy, since the
desktop rewrites the file.

- [ ] The file loads, whatever it holds: an `[updater]` table, user backgrounds, a saved layout.
- [ ] A saved layout arrives in the Desks menu as `Saved Layout` on first launch, applies, and
      does not come back after being deleted and the application relaunched.
- [ ] User backgrounds are still in the file afterwards. If the stored default named one, the
      viewport starts on Gradient.
- [ ] A dock layout that names the Console, the Outliner or the Material Inspector restores
      without them and keeps everything else.
- [ ] No notice appears on launch, whatever the file holds; the keys are documented in the
      release notes and `?` opens the reference.
- [ ] `solarxy-cli --update` still runs. The desktop has no update check of its own.

## 12. Exit

- [ ] Quitting persists the dock layout; relaunching restores it.
- [ ] No panics in the terminal; `RUST_LOG=solarxy=debug` shows no errors. The desktop has no log
      panel, so the terminal is where this is read.

## Golden captures

The automated regression gate. The harness lives in `solarxy-host`, not in
`solarxy-renderer`: it drives the shared pane path, which is what puts the
extracted orchestration under the gate rather than beside it.

The script captures both models in one go, into `<out>/dragon` and `<out>/knot`:

```bash
bash scripts/capture_goldens.sh .goldens/<name>
```

Then compare each against a baseline captured the same way:

```bash
cargo run --release -p solarxy-host --example golden -- \
    compare .goldens/<baseline>/dragon .goldens/<name>/dragon --tolerance 0
cargo run --release -p solarxy-host --example golden -- \
    compare .goldens/<baseline>/knot .goldens/<name>/knot --tolerance 0
```

To capture one model on its own, which is what the script does twice:

```bash
# Untextured geometry/lighting/inspection coverage
cargo run --release -p solarxy-host --example golden -- \
    capture --model res/models/xyzrgb_dragon.obj --out .goldens/<name>/dragon

# TEXTURED coverage -- do not skip this one, see the note below
cargo run --release -p solarxy-host --example golden -- \
    capture --model res/models/knot/knot.obj \
    --out .goldens/<name>/knot
```

**Capture the baseline from a clean tree before you start**, not from an older
commit. CI compares against the pull request's base on one runner because
golden pixels are driver-dependent; locally, a before-and-after capture on the
same tree proves the same thing and needs no second checkout.

**Both models are required.** The dragon OBJ declares no `mtllib` and no `usemtl`,
so it exercises no material and no texture: it is structurally blind to the
albedo-texture path, the `base_color_factor` path, and texture filtering. Between
Phase 8 and Phase 15 the dragon captures were pixel-identical in all five modes
while the textured captures differed on 55k pixels -- the dragon simply could not see
the change. A textured baseline is what makes the gate meaningful.

**A clean diff is not automatically a pass.** Ask what the change *should* have
altered, and be suspicious if it altered nothing. When a diff appears, adjudicate
it: name the change that caused it, or treat it as a regression until proven
otherwise.
