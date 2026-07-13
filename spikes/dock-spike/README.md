# dock-spike (Phase 10 docking gate)

THROWAWAY harness for the web-expansion milestone's Phase 10 W1. It answers the
one question that decides the docking architecture:

> Does a WebGPU surface survive its canvas being detached from the DOM and
> re-attached somewhere else?

If yes, the canvas can live INSIDE a dockview panel as a module-level DOM node
that React never owns, and `fromJSON` (which rebuilds the panel tree, and is what
every desk apply runs) merely re-parents it. That is **option A**. If no, the
canvas must sit outside dockview and be absolutely positioned onto a rect the
panel publishes, which is **option B**, the architecture ratified in
`SOLARXY-WEB-PHASE8-EXPANSION.md` section 9.

This spike declares its own npm project, so `web/`'s build, typecheck, and test
jobs never see it.

## Run

```bash
npm install
npm run dev          # http://localhost:5177/         (option A)
                     # http://localhost:5177/?mode=b  (option B)
```

The harness exposes `window.__spike = { api, stats, render, canvas }`. Drive it
from the console rather than by clicking: Chrome freezes **both rAF and paint** in
a hidden/background tab (the recorded verification quirk), so a screenshot of a
background tab reports stale counters. `render()` forces one frame on demand,
which is the actual liveness assertion, and it works regardless of visibility.

## Verdict: option A. Adopt it.

Measured 2026-07-13 in Chrome (WebGPU, dpr 2). After every gesture, one frame was
forced and the surface was checked: `framesDelta: 1` means `getCurrentTexture()`
still worked, i.e. the surface is alive.

| Gesture | frames advanced | errors | canvas node identity | notes |
|---|---|---|---|---|
| baseline | yes | 0 | same | |
| maximize viewport group | yes | 0 | same | |
| exit maximize | yes | 0 | same | |
| maximize the *nodes* group (viewport hidden) | no (0) | 0 | same | correct: a hidden panel has zero size, so the frame is skipped, exactly as the app's `if (width == 0 \|\| height == 0) return` does. It renders again on restore. |
| float the nodes group | yes | 0 | same | |
| add / remove the review panel | yes | 0 | same | |
| **`fromJSON` (rebuilds the panel tree)** | **yes** | **0** | **same** | the killer case. The canvas is re-parented (`reparents` 1 -> 2 -> 3 -> 4 across repeats) and keeps rendering. |
| `clear()` + `fromJSON` | yes | 0 | same | |
| move the viewport panel into another group | yes | 0 | same | models a real tab drag; dockview moves the panel's content element wholesale, so the host div travels with it and the adoption effect is a no-op |
| move it back out | yes | 0 | same | |

`deviceLost` stayed `false` throughout. `configures` climbed only on real size
changes (8 over 13 frames), **not** per re-parent, so re-parenting is not even
forcing a surface reconfigure.

Run under `<StrictMode>` the whole time, so the double-invoked layout effect is
covered: `attachCanvas` early-returns when the parent is unchanged, which makes
adoption idempotent.

## The other findings, which the migration depends on

1. **Packaging (v7).** `dockview` no longer ships the React bindings: it just
   re-exports `dockview-core` (vanilla). `DockviewReact` lives in
   **`dockview-react`**, which re-exports all of `dockview`, so `dockview-react`
   is the single dependency and `dockview` need not be listed at all. CSS is at
   `dockview-react/dist/styles/dockview.css`. MIT. Pinned: `dockview-react@7.0.2`.

2. **Maximize is transient, for free.** `SerializedDockview` has no maximized
   field (confirmed in `dockview-core`'s type definitions and by asserting
   `JSON.stringify(toJSON())` never mentions it). So maximize can never leak into
   a Desk, and `captureDesk` does **not** need to exit maximize before `toJSON`.
   The plan's precaution there is unnecessary.

3. **`locked` does not pin a panel.** `DockviewGroupPanelLocked = boolean |
   'no-drop-target'` guards *drop* interactions only; it does not stop a tab being
   dragged out. The pin must cancel the drag instead:

   ```ts
   api.onWillDragPanel((e) => {
     if (e.panel.id === "viewport") e.nativeEvent.preventDefault();
   });
   ```

   Verified: dispatching a real `dragstart` on the Viewport tab comes back
   `defaultPrevented === true`, while the Nodes tab comes back `false` and stays
   draggable.

4. **A bad `fromJSON` really does wedge the instance** (dockview issue #341). A
   layout referencing an unknown component throws
   (`"Only React.memo(...), React.ForwardRef(...) and functional components are
   accepted as components"`) and leaves the dock with **zero panels**. `clear()`
   plus rebuilding the default layout recovers fully, and the WebGPU surface
   survives that too. So the try/catch + reset-to-Default + toast in the plan is
   both mandatory and sufficient.

## Consequences for Phase 10

- Adopt option A. The canvas keeps its hard-zero pane origin
  (`compute_panes` in `crates/solarxy-web/src/app.rs` passes `(0.0, 0.0)`), it
  fills its panel naturally, and there is no rect sync, no z-order layering, and
  no `pointer-events` juggling against dockview's drop targets.
- Record an amendment to `SOLARXY-WEB-PHASE8-EXPANSION.md` section 9: the overlay
  canvas is not required. Its premise ("`fromJSON` remounts panel content, so the
  canvas cannot live in a panel") is true of a *React-managed* canvas only.
- Option B remains implemented behind `?mode=b` in this spike, unused.
