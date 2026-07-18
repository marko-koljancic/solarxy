// The one WebGPU canvas, owned by the module and NOT by React.
//
// (docking) made this necessary and the dock-spike proved it safe.
// dockview's `fromJSON` rebuilds the panel tree, which is exactly what every
// desk apply runs, and that unmounts panel content. A canvas rendered as JSX
// would be recreated there and the WebGPU surface lost (the Rust host holds a
// `wgpu::Surface` built from this element and refuses to re-boot).
//
// So the element is created once, here, and the viewport panel merely ADOPTS it:
// `appendChild` on an already-parented node MOVES it, preserving the
// `GPUCanvasContext`. The spike measured this across drag, float, tab-move,
// maximize, `fromJSON`, `clear()` + `fromJSON`, and StrictMode's double-mount:
// the surface kept rendering every time and never even reconfigured.
//
// The trade: React event props do not work on a node React does not render, so
// the viewport's pointer routing attaches listeners imperatively (Viewport.tsx).

let canvas: HTMLCanvasElement | null = null;

/** The one canvas, created on first use. Lazily, not at module scope: the vitest
 * environment has no DOM, and a module-scope `document.createElement` would make
 * every test that transitively imports the session fail to even collect. */
export function viewportCanvas(): HTMLCanvasElement {
  if (!canvas) {
    canvas = document.createElement("canvas");
    canvas.className = "viewport-canvas";
  }
  return canvas;
}

/** Moves the canvas into `host`. Idempotent: re-adopting the same host is a
 * no-op, which is what makes it safe under StrictMode's double-invoked layout
 * effect and under dockview moves that carry the host along with the panel. */
export function adoptViewportCanvas(host: HTMLElement): void {
  const el = viewportCanvas();
  if (el.parentElement === host) return;
  host.appendChild(el);
}

/**
 * Reconciles the backing store and the host's surface with the canvas's current
 * CSS size. Called once per frame, deliberately.
 *
 * A ResizeObserver is not enough now that dockview can re-parent, hide and
 * re-show the canvas: an observer that misses one resize desynchronises the Rust
 * pane rects from the DOM, and then the ghost toolbars drift and picking aims at
 * the wrong pixels. (Measured: after a panel move the observer went silent and
 * the backing store stayed two sizes stale.) Two integer compares per frame buy
 * immunity to every observer-lifetime question, and the desktop's
 * `sync_render_target_dims` does the same thing for the same reason.
 *
 * A hidden panel reports zero size: skip, rather than thrash the surface down to
 * 1x1 and back.
 */
export function syncCanvasSize(resize: (w: number, h: number, dpr: number) => void): void {
  const el = viewportCanvas();
  const cssW = el.clientWidth;
  const cssH = el.clientHeight;
  if (cssW === 0 || cssH === 0) return;
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.floor(cssW * dpr));
  const h = Math.max(1, Math.floor(cssH * dpr));
  if (el.width === w && el.height === h) return;
  el.width = w;
  el.height = h;
  // The dpr rides along: it is not constant for the session (browser zoom, a
  // move to another monitor), and the host scales every pointer coordinate and
  // pane rect by it.
  resize(w, h, dpr);
}
