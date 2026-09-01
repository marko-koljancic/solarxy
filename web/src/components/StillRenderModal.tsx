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
import type {
  GraphContext,
  RenderSettings,
  StillFormat,
  StillSpace,
  StillTileDto,
} from "../engine/types";
import { pushToast } from "../store/toasts";
import { useRenderJob } from "../store/renderJob";
import { Modal } from "./Modal";
import { Row, Section } from "./DialogRow";
import { Select } from "./Select";

/** What the render node asked for, handed over when the action fires.
 *
 * The settings are the engine's answer rather than this side's, so what the
 * dialog shows is what the job will use. The node's address rides along because
 * pressing Render re-resolves it: what renders is what the node says at that
 * moment, not what it said when the dialog opened. */
export interface StillRenderRequest {
  ctx: GraphContext;
  node: number;
  settings: RenderSettings;
}

/** Above this many megapixels the canvas is worth warning about. */
const CANVAS_WARN_MP = 24;

/** The largest float still the host will assemble, in megapixels.
 *
 * Mirrors `MAX_FLOAT_STILL_PIXELS` in the wasm host, which is what actually
 * refuses. Stated here too so the dialog can say why the button is off before
 * somebody spends minutes finding out. */
const FLOAT_MAX_MP = 16;

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
  // The format is a property of the render rather than of the save, so it is
  // chosen here and locked once tiles start arriving.
  const [format, setFormat] = useState<StillFormat>("png");
  const [space, setSpace] = useState<StillSpace>("sceneLinear");
  // What the finished render actually is, read back from the host rather than
  // from what was asked for, so a label cannot drift from the file.
  const [renderedSpace, setRenderedSpace] = useState<StillSpace | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const busy = job.busy;

  const megapixels = (request.settings.width * request.settings.height) / 1_000_000;

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

  // Drain finished tiles and the picture so far into the canvas. Polled on an
  // animation frame rather than pushed, because the tiles are bytes and the
  // store is the wrong place for 67 megapixels of them.
  useEffect(() => {
    if (!started) return undefined;
    let raf = 0;
    const paint = (ctx: CanvasRenderingContext2D, t: StillTileDto) => {
      // Copied into a fresh buffer rather than viewed: the bytes come from
      // the wasm heap, and a view into it is invalidated by the next
      // allocation the module makes, which is every frame.
      const bytes = new Uint8ClampedArray(t.width * t.height * 4);
      bytes.set(t.pixels);
      ctx.putImageData(new ImageData(bytes, t.width, t.height), t.x, t.y);
    };
    const pump = () => {
      const ctx = canvasRef.current?.getContext("2d");
      if (ctx) {
        // Previews first, tiles second, so that within one frame a finished
        // tile always lands on top of the unfinished look at the same
        // rectangle rather than under it.
        for (;;) {
          const preview = getClient().takeStillPreview();
          if (!preview) break;
          paint(ctx, preview);
        }
        for (;;) {
          const tile = getClient().takeStillTile();
          if (!tile) break;
          paint(ctx, tile);
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
      getClient().startStillRender(request.ctx, request.node, format, space);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      pushToast(message, "error");
      return;
    }
    useRenderJob.getState().start(request.settings.width, request.settings.height);
    setRenderedSpace(getClient().stillFloatSpace());
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

  const saveExr = () => {
    try {
      const bytes = getClient().saveStillExr();
      // Copied out of the wasm heap before it becomes a Blob, for the reason
      // the tile drain gives: a view into that heap dies at the next
      // allocation the module makes.
      const stamp = new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19);
      download(new Blob([new Uint8Array(bytes)], { type: "image/x-exr" }), `solarxy_still_${stamp}.exr`);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      pushToast(message, "error");
    }
  };

  // A scene-referred render is shown through a clamp and an sRGB encode,
  // because a canvas is eight bits and scene-referred light is not. Said out
  // loud, because the difference between the preview and the file is the whole
  // point of saving the file.
  const previewNote =
    renderedSpace === "sceneLinear"
      ? "Preview is clamped and display-encoded; the file carries the scene-referred floats."
      : renderedSpace === "display"
        ? "Preview matches the file, which carries the same look without the quantization."
        : null;

  const oversizeForFloat = format === "exr" && megapixels > FLOAT_MAX_MP;

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
            {request.settings.width} x {request.settings.height}
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
            value={request.settings.engine}
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
        {request.settings.engine === "pathTraced" && (
          <Row label="Samples" doc="Set on the render node's Quality. Four times the samples is half the noise.">
            <span className="prefs-unit">
              {request.settings.samples} per pixel{request.settings.denoise ? ", denoised" : ""}
            </span>
          </Row>
        )}
        <Row
          label="Format"
          doc="Chosen before rendering, because it decides what the renderer reads back and cannot change once tiles are arriving. PNG is eight bits with the look already applied. EXR is 32-bit float, which is what a compositing package wants."
        >
          <Select
            ariaLabel="Format"
            value={format}
            options={[
              { value: "png", label: "PNG (8-bit)" },
              { value: "exr", label: "EXR (32-bit float)" },
            ]}
            onChange={(v) => setFormat(v as StillFormat)}
            disabled={busy}
          />
        </Row>
        {format === "exr" && (
          <Row
            label="Space"
            doc="Which floats the file carries. Scene-linear is light with no exposure, tone map or grade applied, which is what a compositing package expects to be handed and what lets it apply a look of its own. Display is the finished look without the quantization. The same choice, and the same default, as the command line."
          >
            <Select
              ariaLabel="Space"
              value={space}
              options={[
                { value: "sceneLinear", label: "Scene-linear" },
                { value: "display", label: "Display-referred" },
              ]}
              onChange={(v) => setSpace(v as StillSpace)}
              disabled={busy}
            />
          </Row>
        )}
        {format === "exr" && megapixels > FLOAT_MAX_MP && (
          <Row label="" doc="">
            <span className="prefs-unit">
              A float still is limited to {FLOAT_MAX_MP} megapixels and this one is{" "}
              {megapixels.toFixed(1)}. Render it smaller, or take it from the command line, which
              writes the same image with no such limit.
            </span>
          </Row>
        )}
      </Section>

      <div className="still-preview">
        <canvas
          ref={canvasRef}
          width={request.settings.width}
          height={request.settings.height}
          className="still-canvas"
          aria-label="The still being rendered"
        />
      </div>
      {started && previewNote && (
        <div className="prefs-unit">{previewNote}</div>
      )}

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
        {renderedSpace && (
          <button className="btn" disabled={busy || !finished} onClick={saveExr}>
            Save EXR
          </button>
        )}
        <button className="btn primary" disabled={busy || oversizeForFloat} onClick={start}>
          {started ? "Render again" : "Render"}
        </button>
      </div>
    </Modal>
  );
}
