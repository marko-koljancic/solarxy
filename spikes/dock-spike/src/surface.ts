// The spike's whole point: ONE WebGPU surface that must survive every
// dockview gesture. This models what `solarxy-web` does in Rust (wgpu holds a
// Surface created from the canvas element, configures it once, and acquires a
// texture per frame) closely enough to answer the only question that matters:
//
//   does a GPUCanvasContext survive the canvas element being detached from the
//   DOM and re-attached somewhere else?
//
// If it does, the canvas can live INSIDE a dockview panel as a module-level DOM
// node that React never owns, and `fromJSON` (which rebuilds panel content, and
// is what every desk apply runs) merely re-parents it.
//
// The canvas is created once, at module scope, and is never destroyed.

export const canvasEl: HTMLCanvasElement = document.createElement("canvas");
canvasEl.id = "spike-canvas";
canvasEl.style.width = "100%";
canvasEl.style.height = "100%";
canvasEl.style.display = "block";

export interface SurfaceStats {
  /** Frames whose `getCurrentTexture()` + submit succeeded. */
  frames: number;
  /** Frames that threw. A re-parent that killed the surface shows up here. */
  errors: number;
  /** The last error message, if any. */
  lastError: string | null;
  /** Times the canvas was (re-)attached to a host element. */
  reparents: number;
  /** Times `configure()` ran. Should be 1 + one per real size change, and must
   * NOT climb with re-parents. */
  configures: number;
  /** `true` once the GPUDevice reports itself lost. Fatal. */
  deviceLost: boolean;
  /** The id of the element currently parenting the canvas. */
  parent: string;
}

const stats: SurfaceStats = {
  frames: 0,
  errors: 0,
  lastError: null,
  reparents: 0,
  configures: 0,
  deviceLost: false,
  parent: "(none)",
};

export function surfaceStats(): SurfaceStats {
  return { ...stats, parent: canvasEl.parentElement?.id ?? "(detached)" };
}

let device: GPUDevice | null = null;
let context: GPUCanvasContext | null = null;
let format: GPUTextureFormat = "bgra8unorm";
let booted = false;
let lastW = 0;
let lastH = 0;

/** Boots WebGPU over the module canvas. Idempotent, like the real
 * `bootSession` (session.ts refuses to re-boot). */
export async function bootSurface(): Promise<void> {
  if (booted) return;
  booted = true;

  const gpu = navigator.gpu;
  if (!gpu) throw new Error("WebGPU unavailable");
  const adapter = await gpu.requestAdapter({ powerPreference: "high-performance" });
  if (!adapter) throw new Error("no adapter");
  device = await adapter.requestDevice();
  void device.lost.then((info) => {
    stats.deviceLost = true;
    stats.lastError = `device lost: ${info.reason} ${info.message}`;
  });

  const ctx = canvasEl.getContext("webgpu");
  if (!ctx) throw new Error("no webgpu context");
  context = ctx;
  format = gpu.getPreferredCanvasFormat();

  requestAnimationFrame(frame);
}

/** Sizes the backing store and reconfigures, exactly like the app's
 * ResizeObserver path (physical px = CSS px * dpr). */
function syncSize(): void {
  if (!device || !context) return;
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.floor(canvasEl.clientWidth * dpr));
  const h = Math.max(1, Math.floor(canvasEl.clientHeight * dpr));
  if (w === lastW && h === lastH) return;
  lastW = w;
  lastH = h;
  canvasEl.width = w;
  canvasEl.height = h;
  context.configure({ device, format, alphaMode: "opaque" });
  stats.configures += 1;
}

/** A clear-colour loop. The colour is driven by the frame counter so a frozen
 * surface is visible at a glance, not just in the stats. */
function frame(): void {
  requestAnimationFrame(frame);
  renderOnce();
}

/** One frame, on demand. Separated from the rAF loop because Chrome freezes
 * rAF in a hidden tab (the recorded verification quirk), and the spike's whole
 * assertion is "does `getCurrentTexture()` still work after a re-parent" --
 * which is answered by forcing a frame, not by waiting for one. */
export function renderOnce(): void {
  if (!device || !context) return;
  // A detached canvas has zero size; skip rather than configure to 1x1 and
  // churn. This mirrors the app's `if (width == 0 || height == 0) return`.
  if (canvasEl.clientWidth === 0 || canvasEl.clientHeight === 0) return;

  try {
    syncSize();
    const t = stats.frames * 0.01;
    const view = context.getCurrentTexture().createView();
    const encoder = device.createCommandEncoder();
    const pass = encoder.beginRenderPass({
      colorAttachments: [
        {
          view,
          clearValue: {
            r: 0.15 + 0.15 * Math.sin(t),
            g: 0.18,
            b: 0.30 + 0.15 * Math.cos(t),
            a: 1,
          },
          loadOp: "clear",
          storeOp: "store",
        },
      ],
    });
    pass.end();
    device.queue.submit([encoder.finish()]);
    stats.frames += 1;
  } catch (err) {
    stats.errors += 1;
    stats.lastError = err instanceof Error ? err.message : String(err);
  }
}

/** Moves the one canvas into `host`. `appendChild` on an already-parented node
 * MOVES it (no clone, no recreate), which is the whole hypothesis. */
export function attachCanvas(host: HTMLElement): void {
  if (canvasEl.parentElement === host) return;
  host.appendChild(canvasEl);
  stats.reparents += 1;
  // A fresh host has a different size; force a reconfigure check next frame.
  lastW = 0;
  lastH = 0;
}
