# Desktop QA checklist

The manual gate for the desktop viewer (`solarxy`). Run it before any release tag,
and whenever a shared crate (`solarxy-core`, `solarxy-renderer`, `solarxy-kernel`,
`solarxy-formats`) changes in a way the golden captures cannot see.

The automated half is `crates/solarxy-renderer/examples/golden.rs` (see
"Golden captures" below). This file covers what goldens cannot: interaction,
input, dialogs, and anything that needs a window.

Record the run in `SOLARXY-WEB-INTEGRATION-IMPLEMENTATION-LOG.md`: date, commit,
platform, and any box left unticked with the reason.

## Launch

```bash
cargo run --release -- --model res/models/xyzrgb_dragon.obj
```

## 1. Model loading

- [ ] `xyzrgb_dragon.obj` loads, auto-frames, and orbits smoothly.
- [ ] `res/models/frog/ooz3d-export-model-*.obj` loads **with its texture** (OBJ + MTL + `map_Kd`). The frog is skin-textured, not white and not untextured.
- [ ] An STL loads (`crates/solarxy-formats/tests/fixtures/triangle.stl` or any STL).
- [ ] A PLY loads.
- [ ] A glTF/GLB loads (`crates/solarxy-formats/tests/fixtures/textured.glb`) **with its texture**.
- [ ] A colored-but-untextured material renders in its colour, **not white**. (This was a real bug fixed in Phase 14: `base_color_factor` was parsed but never reached the shader.)
- [ ] Drag-and-drop a model onto the window loads it.

## 2. Inspection modes (number keys 1-7)

Each renders without artifacts and the HUD/status bar names the active mode.

- [ ] 1 Shaded
- [ ] 2 Material ID
- [ ] 3 UV Map
- [ ] 4 Texel Density
- [ ] 5 Depth
- [ ] 6 Overdraw
- [ ] 7 AO Preview

## 3. Display and overlays

- [ ] Wireframe and ghosted-wireframe.
- [ ] Normals overlay.
- [ ] Grid and axis gizmo toggle.
- [ ] Bounds display.
- [ ] Material overrides (`Shift+M`): Clay, Clay Dark, Chrome, Silhouette.
- [ ] **Validation overlay**: issues highlight, and the non-manifold **edge lines do not z-fight** with the surface. (WebGPU forbids `depthBias` on line topologies, so the depth bias was removed from that pipeline in Phase 0. If z-fighting is visible, the fix is a clip-space nudge in `vs_validation`.)

## 4. Layouts and cameras

- [ ] F1 single, F2 vertical split, F3 horizontal split, F4 quad, F5 three-left-big.
- [ ] Orbit, pan, zoom, and arrow-key nav work in every layout.
- [ ] The active pane follows the cursor; per-pane inspection modes are independent.
- [ ] Linked and unlinked cameras both behave.

## 5. Lighting

- [ ] Drag-drop an `.hdr` HDRI: sky renders, IBL lights the model.
- [ ] Drag-drop an `.exr` HDRI.
- [ ] IBL toggle (`I` / `Shift+I`).
- [ ] Shadows render; the shadow-catching floor works.

## 6. UV pane

- [ ] A UV pane opens and shows the layout.
- [ ] The overlap statistic computes (it is an async GPU readback).

## 7. Panels and dialogs

- [ ] Sidebar, Outliner, Properties, Console, Material Inspector, Review Panel all open and dock.
- [ ] Material Inspector shows texture thumbnails for a textured model.
- [ ] Preferences (`Ctrl/Cmd+,`) opens; theme hot-swaps light/dark with no restart.
- [ ] Keyboard-shortcuts modal (`?`).
- [ ] Save Layout / Restore Saved Layout / Reset Layout.

## 8. Review system

- [ ] `Shift+R` enters review mode (amber indicator).
- [ ] Click geometry to place an annotation; it saves to the sidecar.
- [ ] Re-anchor and cascade-delete work.

## 9. Screenshot

- [ ] `C` opens the screenshot modal; Save As writes a PNG.

## 10. Exit

- [ ] Quitting persists the dock layout; relaunching restores it.
- [ ] No panics in the console; `RUST_LOG=solarxy=debug` shows no errors.

## Golden captures

The automated regression gate. Capture and compare:

```bash
# Untextured geometry/lighting/inspection coverage
cargo run --release -p solarxy-renderer --example golden -- \
    capture --model res/models/xyzrgb_dragon.obj --out .goldens/<name>-dragon

# TEXTURED coverage -- do not skip this one, see the note below
cargo run --release -p solarxy-renderer --example golden -- \
    capture --model "res/models/frog/ooz3d-export-model-20260329-181053.obj" \
    --out .goldens/<name>-frog

cargo run --release -p solarxy-renderer --example golden -- \
    compare .goldens/<baseline>-dragon .goldens/<name>-dragon --tolerance 0
cargo run --release -p solarxy-renderer --example golden -- \
    compare .goldens/<baseline>-frog .goldens/<name>-frog --tolerance 0
```

**Both models are required.** The dragon OBJ declares no `mtllib` and no `usemtl`,
so it exercises no material and no texture: it is structurally blind to the
albedo-texture path, the `base_color_factor` path, and texture filtering. Between
Phase 8 and Phase 15 the dragon captures were pixel-identical in all five modes
while the frog captures differed on 55k pixels -- the dragon simply could not see
the change. A textured baseline is what makes the gate meaningful.

**A clean diff is not automatically a pass.** Ask what the change *should* have
altered, and be suspicious if it altered nothing. When a diff appears, adjudicate
it: name the change that caused it, or treat it as a regression until proven
otherwise.
