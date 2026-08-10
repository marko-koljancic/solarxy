// The still-render modal: shows a render arriving tile by tile, with a running
// count, a cancel, and a save.
//
// Unlike every other job dialog here, this one does not drive the work. The
// host owns the job and advances it one chunk per frame on its own; this reads
// progress off the `renderProgress` host event and drains finished tiles. That
// is what keeps a render that takes minutes from being a promise chain nobody
// can cancel cleanly, and it is why `run` returns as soon as the job starts.
//
// The canvas is the real output size, shown scaled by CSS. One buffer for both
// the preview and the save, which is what makes Save a `toBlob` on what you are
// already looking at rather than a second assembly pass. At the 8192 cap that
// is about 268 MB, so a warning appears above a threshold rather than a refusal:
// a machine that can render it can usually hold it, and a machine that cannot
// should be told why before it tries.

import { useEffect, useRef, useState } from "react";
import { getClient } from "../engine/session";
import { pushToast } from "../store/toasts";
import { useRenderJob } from "../store/renderJob";
import { Modal } from "./Modal";
import { Row, Section } from "./DialogRow";
import { Select } from "./Select";

/** What the render node asked for, handed over when the action fires. */
export interface StillRenderRequest {
  width: number;
  height: number;
  samples: number;
  engine: "raster" | "pathTraced";
  denoise: boolean;
  /** The `camera` node to shoot through, or null for the active pane's view. */
  camera: number | null;
}

/** Above this many megapixels the canvas is worth warning about. */
const CANVAS_WARN_MP = 24;

function download(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

export function StillRenderModal({
  request,
  onClose,
}: {
  request: StillRenderRequest;
  onClose: () => void;
}) {
  const job = useRenderJob();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const [started, setStarted] = useState(false);
  const [finished, setFinished] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busy = job.busy;

  const megapixels = (request.width * request.height) / 1_000_000;

  // Escape cancels the run rather than closing the dialog, which is the
  // established rule for every multi-frame job here: a render that vanished
  // when the dialog did would keep rendering with nothing showing it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      if (busy) {
        getClient().cancelStillRender();
        useRenderJob.getState().stop();
      } else {
        onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);

  // Drain finished tiles into the canvas. Polled on an animation frame rather
  // than pushed, because the tiles are bytes and the store is the wrong place
  // for 67 megapixels of them.
  useEffect(() => {
    if (!started) return undefined;
    let raf = 0;
    const pump = () => {
      const ctx = canvasRef.current?.getContext("2d");
      if (ctx) {
        for (;;) {
          const tile = getClient().takeStillTile();
          if (!tile) break;
          // Copied into a fresh buffer rather than viewed: the bytes come from
          // the wasm heap, and a view into it is invalidated by the next
          // allocation the module makes, which is every frame.
          const bytes = new Uint8ClampedArray(tile.width * tile.height * 4);
          bytes.set(tile.pixels);
          const data = new ImageData(bytes, tile.width, tile.height);
          ctx.putImageData(data, tile.x, tile.y);
        }
      }
      raf = requestAnimationFrame(pump);
    };
    raf = requestAnimationFrame(pump);
    return () => cancelAnimationFrame(raf);
  }, [started]);

  // The job reports itself done through the store; the dialog notices and stops
  // calling itself busy.
  useEffect(() => {
    if (started && !job.busy && job.tiles > 0) setFinished(true);
  }, [started, job.busy, job.tiles]);

  const start = () => {
    setError(null);
    setFinished(false);
    try {
      getClient().startStillRender(request);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      pushToast(message, "error");
      return;
    }
    useRenderJob.getState().start(request.width, request.height);
    setStarted(true);
  };

  const cancel = () => {
    getClient().cancelStillRender();
    useRenderJob.getState().stop();
  };

  const save = () => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.toBlob((blob) => {
      if (!blob) {
        pushToast("the image could not be encoded", "error");
        return;
      }
      const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      download(blob, `solarxy_still_${stamp}.png`);
    }, "image/png");
  };

  const pct = job.tiles > 0 ? (job.tile + job.sample / Math.max(1, job.samples)) / job.tiles : 0;

  return (
    <Modal
      id="still-render"
      title="Render Still"
      onClose={busy ? undefined : onClose}
      className="modal-wide"
      bodyLayout="column"
      closeOnEsc={false}
      closeOnBackdrop={!busy}
    >
      <Section title="Output">
        <Row label="Size" doc="Set on the render node. Large renders are drawn in tiles and assembled, so the size you ask for is the size you get.">
          <span className="prefs-unit">
            {request.width} x {request.height}
            {megapixels >= CANVAS_WARN_MP
              ? ` (${megapixels.toFixed(0)} megapixels; this needs about ${Math.round(
                  megapixels * 4,
                )} MB of browser memory to hold)`
              : ""}
          </span>
        </Row>
        <Row label="Engine" doc="Set on the render node. Path traced follows light through the scene; rasterized is the viewport's own renderer.">
          <Select
            ariaLabel="Engine"
            value={request.engine}
            options={[
              { value: "raster", label: "Rasterized" },
              { value: "pathTraced", label: "Path traced" },
            ]}
            onChange={() => {
              /* Read-only here: the render node owns it. */
            }}
            disabled
          />
        </Row>
        {request.engine === "pathTraced" && (
          <Row label="Samples" doc="Set on the render node's Quality. Four times the samples is half the noise.">
            <span className="prefs-unit">
              {request.samples} per pixel{request.denoise ? ", denoised" : ""}
            </span>
          </Row>
        )}
      </Section>

      <div className="still-preview">
        <canvas
          ref={canvasRef}
          width={request.width}
          height={request.height}
          className="still-canvas"
          aria-label="The still being rendered"
        />
      </div>

      {started && (
        <>
          {/* The bar animates; the text beside it does not, so a reader who
              has asked for reduced motion still has the count. */}
          <div className="turntable-progress" aria-hidden="true">
            <div
              className="turntable-progress-bar"
              style={{ width: `${Math.round(pct * 100)}%` }}
            />
          </div>
          <div className="prefs-unit" role="status" aria-live="polite">
            {busy
              ? `Tile ${Math.min(job.tile + 1, job.tiles)} of ${job.tiles}, sample ${job.sample} of ${job.samples}`
              : finished
                ? `Done: ${job.tiles} tiles`
                : "Cancelled"}
          </div>
        </>
      )}
      {error && <div className="prefs-unit">{error}</div>}

      <div className="modal-actions">
        <button className="btn" onClick={busy ? cancel : onClose}>
          {busy ? "Cancel" : "Close"}
        </button>
        <button className="btn" disabled={busy || !finished} onClick={save}>
          Save PNG
        </button>
        <button className="btn primary" disabled={busy} onClick={start}>
          {started ? "Render again" : "Render"}
        </button>
      </div>
    </Modal>
  );
}
