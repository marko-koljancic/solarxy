# Render QA checklist

The manual gate for rendering, across all three shells. Run it before any
release tag, and whenever the still job, the tracer, or a shell's render
surface changes.

The automated half is `both_native_shells_resolve_one_scene_the_same_way` in
`crates/solarxy-app/src/state/still.rs`, which runs on every `cargo test` and
proves the desktop and the headless command read one document identically. This
file covers what that cannot: pixels, on three surfaces, one of which is a
browser.

Record the run in the milestone spec's amendments: date, commit, platform, and
any box left unticked with the reason.

## The scenes

Two shipped samples, which between them carry the four features a render can
diverge on. Neither is authored for this checklist, so neither can drift away
from what people actually open.

| Scene | Carries |
|---|---|
| `web/public/samples/cornell-box.slxy` | Transmission at ior 1.5, a rectangular area light |
| `web/public/samples/procedural-lookdev.slxy` | A voronoi and noise texture network through a principled surface, an area light, a hemisphere light, and a camera carrying a gamma of 1.35 |

## 1. One scene, three shells

For each scene, render at the size and sample count the scene's own render node
asks for, changing nothing.

```bash
solarxy-cli render web/public/samples/cornell-box.slxy -o /tmp/cli-cornell.png
```

- [ ] Command line renders and writes the file.
- [ ] Desktop: `Render → Render Still…`, press Render, then `Save As…` as PNG.
- [ ] Browser: the render node's Render Still action, press Render, then Save PNG.
- [ ] The three images agree. Compare them by eye first, then numerically with
      the golden harness's comparator:
      `cargo run -p solarxy-host --example golden -- compare <dir_a> <dir_b> --tolerance N`.
- [ ] **The tolerance used, and the measured difference, are written into the
      release's amendment.** A tolerance chosen to make a run pass is not a
      tolerance; it is a way of not looking.

**What the difference is allowed to be, and why it is not zero.** This release
ruled on 2026-08-31 that a seed makes a render reproducible *on the surface that
rendered it*, and made no cross-surface promise. The headless path accumulates
in chunks of eight and the browser in chunks of one, each for a reason sound on
its own surface, and floating-point summation is not invariant to the grouping.
The measurement that forced the ruling: on the bundled Cornell box at matched
size, samples and seed, background pixels were byte-identical and the lit subject
differed by a mean of 1.37 of 255.

So the check is that the images agree *within that bound*, not that they match.
A difference an order of magnitude larger than 1.37 is not reassociation and
should be treated as a defect.

## 2. The picture arrives while it renders

- [ ] Browser, 1920 by 1080, path traced: an image appears within the first
      few chunks and visibly improves, rather than appearing at the end.
- [ ] The page stays responsive throughout: the canvas orbits, menus open.
- [ ] The same render takes the same wall time it did before previews existed,
      within noise. **Record the before and after.**
- [ ] Desktop: the modal's preview fills in the same way.
- [ ] A rasterized still is unchanged and shows no mid-tile refinement, because
      it has nothing to refine.
- [ ] A render larger than four megapixels is genuinely tiled and still
      assembles without seams.

## 3. Elapsed and remaining

- [ ] All three shells show an elapsed that matches a clock on the wall.
- [ ] None shows a remaining before it has a rate to say one from.
- [ ] **A tiled render**, past four megapixels, where the right-hand column and
      bottom row are narrower: the estimate does not run long at the end. This
      is the case the area weighting fixed, and a single-tile render will not
      show it.
- [ ] The same render reads the same way in each shell: `4m 12s`, not `252.0s`
      in one of them.

## 4. Auxiliary passes, browser

- [ ] A traced render with albedo, normal and depth all requested produces all
      three.
- [ ] The pass selector defaults to the beauty and switches immediately,
      including while the render is still running.
- [ ] Switching to a pass and back shows the picture again rather than losing it.
- [ ] A pass that was not requested is offered as unavailable and says what
      would produce it.
- [ ] **A rasterized render offers the beauty alone**, not three disabled rows.
      This exercises a capability flag that had no consumer before 0.9.0.
- [ ] Save all delivers one archive; its passes open in a compositor.
- [ ] The displayed passes look the same as the terminal's:
      `solarxy-cli render <scene> --watch --aov albedo,normal,depth`.
- [ ] A render with no passes requested shows no selector and is otherwise
      unchanged.

## 5. The render window

- [ ] Pan by dragging and zoom at the cursor: a detail stays under the pointer.
- [ ] Both work while the render is running, and the tiles keep landing in the
      right places underneath.
- [ ] Zoom stops at both ends; the picture cannot be dragged off the glass.
- [ ] Fit and 100% both land where they say, at a non-square size.
- [ ] Narrowing the window sheds the readings first, then the field labels,
      while the action, the pass selector and the view controls stay.
- [ ] Escape cancels a running render rather than closing the window.

## 6. Floating point, desktop

- [ ] A PNG still is unchanged in every respect.
- [ ] EXR in both spaces writes, and both open in a compositor.
- [ ] **A desktop EXR and a browser EXR of one scene carry the same values.**
      Both assemble through one accumulator and encode through one encoder since
      0.9.0, so a difference here is a real defect rather than a tolerance
      question.
- [ ] The suggested filename carries the extension the chosen format decides.
- [ ] Cancelling the save picker leaves the dialog able to save again.
- [ ] Render again after changing the format renders in the new one.

## 7. The transparent background

Author `transparent_background` on a scene's render node. The Cornell box is the
scene to prefer: its walls fill the frame, so add a camera pulled back far
enough that sky shows around the box, or use any scene with visible sky.

The convention to hold every file against: **the floating-point files carry
premultiplied alpha and the eight-bit files carry straight alpha.** The two
therefore differ numerically at partially covered pixels, on purpose; a
compositor that honours each convention composites both identically over the
same plate.

- [ ] Both engines honour the option: a rasterized transparent still and a
      traced one are transparent in the same places.
- [ ] The environment still lights the scene: the subject in a transparent
      render matches the same render with a background, and only the background
      differs. A mirrored or glossy surface still carries the sky in its
      reflections.
- [ ] A mirrored surface against the sky is opaque in the matte.
- [ ] The saved PNG opens with its transparency in an image editor and in a
      browser; a silhouette is antialiased rather than staircased.
- [ ] The saved EXR opens in a compositor and composites cleanly over a plate:
      no dark fringe, no halo at silhouettes.
- [ ] **A browser PNG and a command-line PNG of one transparent scene carry the
      same values.** The browser encodes through the engine's own encoder
      precisely so a canvas round trip cannot corrupt the straight alpha; a
      difference here is that defect resurfacing.
- [ ] The render window shows a checker through the matte, in both themes, and
      the checker holds its scale under zoom. The desktop modal's preview shows
      its checker the same way.
- [ ] An opaque render of the same scene shows no checker anywhere and is
      byte-for-byte what it was.
- [ ] A graded transparent render is transparent in exactly the places the
      ungraded one is.

## 8. Light markers stay out of the picture

Markers are viewport furniture: an aiming aid, not scene content. They belong
on screen while you work and nowhere in a delivered image. Two of the three
channels this covers leaked before 0.9.0, so check all of them rather than
assuming the marker is the only one.

Use any scene with lights; the Cornell box has an area light and the look-dev
sample has an area light plus a hemisphere, which between them cover the
distinguishable shapes.

- [ ] Every light in the scene carries a marker, and the six kinds are
      distinguishable from each other at a glance.
- [ ] A marker holds its size as the camera dollies in and out, and is no
      harder to click far away than near.
- [ ] Clicking a marker selects that light: the node canvas highlights the same
      node and the parameter panel shows its params.
- [ ] A marker drawn over geometry takes the click; a click just off it selects
      the geometry behind.
- [ ] Turning **Display > Light markers** off in a pane removes them from that
      pane only, and clicks there fall through to geometry again.
- [ ] **A rendered still carries no markers, no transform gizmo and no light or
      camera helpers**, with a light selected and the move tool armed while the
      render runs. Check a rasterized still; a traced one draws no overlays at
      all by construction.
- [ ] **A saved screenshot carries no markers and no gizmo**, taken with a
      light selected and the move tool armed. Camera and light helpers do stay,
      because those are switched on per node like the grid.
- [ ] A light's own Show Helper still draws its world-scaled wireframe, and
      still grows and shrinks with the camera, which is what tells it apart
      from the marker.

## 9. Transforming a light in the viewport

Browser only: the desktop shell draws no light marker channel and has no gizmo,
so none of this applies there until that shell gets its canvas.

Section 8 covers the markers themselves. This covers manipulating what they
point at. Use a scene with several light types; the look-dev sample carries an
area light and a hemisphere, and adding a point and a spot to it exercises the
rest in one place.

- [ ] Selecting a light and arming Move draws a manipulator on it, and dragging
      moves the light **with the scene relighting continuously**, shadows
      included, rather than on release.
- [ ] A drag is one undo step: one Ctrl+Z puts the light back where it was.
- [ ] Escape mid-drag restores the light exactly, and leaves nothing stranded
      (the parameter panel and the viewport agree afterwards).
- [ ] Each light type offers exactly what it can use, checked by selecting each
      in turn and reading the tool column: a point light offers Move; a
      directional and a spot offer Move and Aim; a rect area offers Move,
      Rotate and Scale; an ambient and a hemisphere offer none of them.
- [ ] **The Aim tool moves the point a light points at**, not the light: arm it
      on a spot, and the handles sit out at its target while the cone swings to
      follow.
- [ ] A rect area's Scale tool shows two edge cubes and the centre cube and
      **no third axis cube**, and dragging each changes the panel's Width and
      Height in metres.
- [ ] Select a mesh, arm Scale, then select a point light: the manipulator
      disappears and the Scale button greys out. Select the mesh again and
      Scale is live and still armed.
- [ ] Geometry is unchanged throughout: a mesh still moves, rotates and scales
      exactly as it did, including dragging inside a container's subflow where
      the drag mints a transform node.
- [ ] Right-clicking each light type offers actions that apply to it rather
      than a menu of disabled entries, and **Frame selection** finds a light
      dragged far out of frame.
- [ ] Reset transform on a light resets what that light has. On a rect area it
      also resets Width and Height, which is deliberate: those are what its
      size handles write.
