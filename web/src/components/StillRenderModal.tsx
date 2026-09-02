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

import type { PointerEvent as ReactPointerEvent, WheelEvent as ReactWheelEvent } from "react";
import { useEffect, useRef, useState } from "react";
import { getClient } from "../engine/session";
import type {
  GraphContext,
  RenderSettings,
  StillFormat,
  StillPass,
  StillPasses,
  StillSpace,
  StillTileDto,
} from "../engine/types";
import { pushToast } from "../store/toasts";
import { zipSync } from "fflate";
import { useRenderJob } from "../store/renderJob";
import { useViewState } from "../store/viewState";
import { formatDurationMs } from "../render/duration";
import { passAvailable, passOptions } from "../render/passes";
import {
  actualSize,
  pan,
  viewRect,
  zoomAbout,
  zoomOf,
  type Size,
  type ViewMode,
} from "../render/view";
import { Modal } from "./Modal";
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
  // The picture itself, off screen. The visible canvas shows whichever pass is
  // selected, so the beauty needs somewhere of its own to live: switching to
  // the albedo and back has to replay what already arrived rather than lose it,
  // and the passes are held on the Rust side while the beauty is only ever
  // here. It is also what Save writes, so what is saved does not depend on what
  // is being looked at.
  const beautyRef = useRef<HTMLCanvasElement | null>(null);
  // The finished tiles, byte for byte, kept only for a transparent render. A
  // canvas stores its backing premultiplied, so reading a straight-alpha
  // image back out of one round-trips every partially covered pixel through
  // a multiply and a divide; the PNG a transparent render saves is encoded
  // from this copy instead, through the engine's own encoder, so the
  // browser's file and the command line's carry the same values.
  const pristineRef = useRef<Uint8ClampedArray | null>(null);
  const [started, setStarted] = useState(false);
  const [finished, setFinished] = useState(false);
  const [pass, setPass] = useState<StillPass>("beauty");
  const [passes, setPasses] = useState<StillPasses | undefined>(undefined);
  // The view over the picture. `null` is the letterbox fit, which is also what
  // a resize falls back to on its own, because the fit is recomputed from the
  // sizes at hand rather than stored.
  const [view, setView] = useState<ViewMode>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const [stage, setStage] = useState<Size>({ w: 0, h: 0 });
  const dragRef = useRef<{ x: number; y: number } | null>(null);
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
    // Finished tiles only, and copied in the same turn the tile crossed, for
    // the wasm-heap reason paint gives. Previews never land here: they are a
    // look at a tile that has not finished, and the pristine copy is the
    // file.
    const keep = (t: StillTileDto) => {
      const dst = pristineRef.current;
      if (!dst) return;
      const { width } = request.settings;
      for (let row = 0; row < t.height; row += 1) {
        const src = row * t.width * 4;
        dst.set(t.pixels.subarray(src, src + t.width * 4), ((t.y + row) * width + t.x) * 4);
      }
    };
    const pump = () => {
      const beauty = beautyRef.current?.getContext("2d");
      const visible = canvasRef.current?.getContext("2d");
      // Painted into the visible canvas as well as the offscreen one, rather
      // than blitting the whole picture every frame: a tile is a small
      // rectangle and the whole picture is not.
      const showing = pass === "beauty" ? visible : null;
      if (beauty) {
        // Previews first, tiles second, so that within one frame a finished
        // tile always lands on top of the unfinished look at the same
        // rectangle rather than under it.
        for (;;) {
          const preview = getClient().takeStillPreview();
          if (!preview) break;
          paint(beauty, preview);
          if (showing) paint(showing, preview);
        }
        let landed = false;
        for (;;) {
          const tile = getClient().takeStillTile();
          if (!tile) break;
          paint(beauty, tile);
          if (showing) paint(showing, tile);
          keep(tile);
          landed = true;
        }
        // A pass only exists for tiles that finished, so it is worth remapping
        // exactly when one has. A preview carries no passes and changes nothing
        // here.
        if (landed && pass !== "beauty" && visible) showPass(visible, pass);
      }
      raf = requestAnimationFrame(pump);
    };
    raf = requestAnimationFrame(pump);
    return () => cancelAnimationFrame(raf);
  }, [started, pass]);

  // Draws one pass into the visible canvas, whole. Mapped on the Rust side by
  // the same functions the terminal's watch window uses, so the two windows
  // cannot come to show the same plane differently.
  const showPass = (ctx: CanvasRenderingContext2D, which: StillPass) => {
    const pixels = getClient().stillPassDisplay(which);
    if (!pixels) return;
    const { width, height } = request.settings;
    const bytes = new Uint8ClampedArray(width * height * 4);
    bytes.set(pixels);
    ctx.putImageData(new ImageData(bytes, width, height), 0, 0);
  };

  // Switching is a replay of what already arrived, never a re-render.
  useEffect(() => {
    const view = canvasRef.current?.getContext("2d");
    const beauty = beautyRef.current;
    if (!view || !beauty) return;
    if (pass === "beauty") {
      view.clearRect(0, 0, beauty.width, beauty.height);
      view.drawImage(beauty, 0, 0);
    } else {
      showPass(view, pass);
    }
    // `showPass` closes over the request's size, which does not change while a
    // dialog is open.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pass, started]);

  // The job reports itself done through the store; the dialog notices and stops
  // calling itself busy.
  useEffect(() => {
    if (started && !job.busy && job.tiles > 0) setFinished(true);
  }, [started, job.busy, job.tiles]);

  const start = () => {
    setError(null);
    setFinished(false);
    // Back to the beauty: the passes a previous run produced are gone, and a
    // selector left pointing at one would be pointing at nothing.
    setPass("beauty");
    const { width, height } = request.settings;
    // The offscreen picture, sized to this render. Recreated per run rather
    // than resized, because a resize would leave the previous render's pixels
    // in whatever part of it the new one does not cover.
    const beauty = document.createElement("canvas");
    beauty.width = width;
    beauty.height = height;
    beautyRef.current = beauty;
    // Fresh per run like the canvas, and only when the render carries a
    // matte: an opaque render saves through the canvas as it always has.
    pristineRef.current = request.settings.transparentBackground
      ? new Uint8ClampedArray(width * height * 4)
      : null;
    canvasRef.current?.getContext("2d")?.clearRect(0, 0, width, height);
    try {
      getClient().startStillRender(request.ctx, request.node, format, space);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      setError(message);
      pushToast(message, "error");
      return;
    }
    useRenderJob.getState().start(width, height);
    setRenderedSpace(getClient().stillFloatSpace());
    setPasses(getClient().stillPasses());
    setStarted(true);
  };

  const cancel = () => {
    getClient().cancelStillRender();
    useRenderJob.getState().stop();
  };

  // The transparent render's PNG, encoded from the pristine tile copy through
  // the engine's own encoder, or undefined when this render is opaque and the
  // canvas path applies. Copied out of the wasm heap before it becomes a
  // Blob, for the reason every other boundary read gives.
  const pngBytes = (): Uint8Array<ArrayBuffer> | undefined => {
    const pristine = pristineRef.current;
    if (!pristine) return undefined;
    const { width, height } = request.settings;
    const raw = new Uint8Array(pristine.buffer, pristine.byteOffset, pristine.length);
    return new Uint8Array(getClient().encodeStillPng(raw, width, height));
  };

  // The offscreen picture rather than the visible canvas, so what is saved is
  // the render rather than whichever pass happens to be on screen.
  const save = () => {
    try {
      const bytes = pngBytes();
      if (bytes) {
        download(new Blob([bytes], { type: "image/png" }), `${stem()}.png`);
        return;
      }
    } catch (e) {
      pushToast(e instanceof Error ? e.message : String(e), "error");
      return;
    }
    const canvas = beautyRef.current;
    if (!canvas) return;
    canvas.toBlob((blob) => {
      if (!blob) {
        pushToast("the image could not be encoded", "error");
        return;
      }
      download(blob, `${stem()}.png`);
    }, "image/png");
  };

  /// One name for a set of files, so a beauty and its passes sit together.
  const stem = () =>
    `solarxy_still_${new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19)}`;

  /** Every pass this render produced, in one archive.
   *
   * One action rather than one dialog per file, which is the question that
   * blocked auxiliary passes in the browser until now. The sibling naming is
   * the command line's, unchanged, so a set of passes globs the same way
   * wherever it came from, and the passes are floating point whatever the
   * beauty is, for the reason the command line gives: an eight-bit normal is
   * useless. */
  const saveAll = async () => {
    const produced = (["albedo", "normal", "depth"] as const).filter((p) =>
      passAvailable(p, passes),
    );
    if (produced.length === 0) return;
    const name = stem();
    const files: Record<string, Uint8Array> = {};
    try {
      for (const p of produced) {
        files[`${name}.${p}.exr`] = new Uint8Array(getClient().stillPassFile(p));
      }
      if (format === "exr") {
        files[`${name}.exr`] = new Uint8Array(getClient().saveStillExr());
      } else {
        const bytes = pngBytes();
        if (bytes) {
          files[`${name}.png`] = bytes;
        } else {
          const blob = await new Promise<Blob | null>((resolve) =>
            beautyRef.current?.toBlob(resolve, "image/png") ?? resolve(null),
          );
          if (blob) files[`${name}.png`] = new Uint8Array(await blob.arrayBuffer());
        }
      }
      download(new Blob([zipSync(files)], { type: "application/zip" }), `${name}.zip`);
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e);
      pushToast(message, "error");
    }
  };

  const saveExr = () => {
    try {
      const bytes = getClient().saveStillExr();
      // Copied out of the wasm heap before it becomes a Blob, for the reason
      // the tile drain gives: a view into that heap dies at the next
      // allocation the module makes.
      download(new Blob([new Uint8Array(bytes)], { type: "image/x-exr" }), `${stem()}.exr`);
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

  // What the render node asked for, read from the settings rather than from the
  // running job, so it reads correctly before Render is pressed too.
  const requested = (["albedo", "normal", "depth"] as const).filter(
    (p) =>
      request.settings[
        `aov${p.charAt(0).toUpperCase()}${p.slice(1)}` as
          | "aovAlbedo"
          | "aovNormal"
          | "aovDepth"
      ],
  );
  // Whether the chosen engine can write passes at all is a capability, read
  // from the backend's own constant rather than decided here by its name. The
  // engine names which capability to look at; it never answers the question.
  const caps = useViewState((s) => s.backendCaps);
  const engineWritesAovs =
    caps === null
      ? true
      : (request.settings.engine === "pathTraced" ? caps.traced : caps.raster).writesAovs;
  const producedNote = !engineWritesAovs
    ? "None: this engine writes no auxiliary passes"
    : requested.length === 0
      ? "None"
      : requested.map((p) => p.charAt(0).toUpperCase() + p.slice(1)).join(", ");

  // Offered only when there is a set to deliver. A render with no passes has
  // nothing an archive would add to the two buttons beside it.
  const anyPasses = (["albedo", "normal", "depth"] as const).some((p) =>
    passAvailable(p, passes),
  );

  const pct = job.tiles > 0 ? (job.tile + job.sample / Math.max(1, job.samples)) / job.tiles : 0;

  // The glass the picture sits on. Measured rather than assumed, because the
  // dialog is a percentage of the viewport and the fit has to follow it.
  useEffect(() => {
    const el = stageRef.current;
    if (!el) return undefined;
    const observer = new ResizeObserver(([entry]) => {
      const box = entry.contentRect;
      setStage({ w: Math.round(box.width), h: Math.round(box.height) });
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  const picture: Size = { w: request.settings.width, h: request.settings.height };
  const rect = viewRect(view, picture, stage);
  const zoom = zoomOf(view, picture, stage);

  // The transform applies to the element, never to the pixels: tiles keep
  // arriving into the same canvas at the same coordinates whatever the view is
  // doing, which is what makes panning during a render safe and what leaves the
  // save path alone.
  const onWheel = (e: ReactWheelEvent) => {
    const box = stageRef.current?.getBoundingClientRect();
    if (!box) return;
    // Exponential in the delta, so a trackpad's many small events and a
    // mouse's few large ones travel the same distance per unit scrolled.
    const factor = Math.exp(-e.deltaY * 0.002);
    setView((v) =>
      zoomAbout(v, { x: e.clientX - box.left, y: e.clientY - box.top }, factor, picture, stage),
    );
  };

  const onPointerDown = (e: ReactPointerEvent) => {
    dragRef.current = { x: e.clientX, y: e.clientY };
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: ReactPointerEvent) => {
    const from = dragRef.current;
    if (!from) return;
    dragRef.current = { x: e.clientX, y: e.clientY };
    setView((v) => pan(v, { x: e.clientX - from.x, y: e.clientY - from.y }, picture, stage));
  };

  const endDrag = (e: ReactPointerEvent) => {
    dragRef.current = null;
    if (e.currentTarget.hasPointerCapture(e.pointerId)) {
      e.currentTarget.releasePointerCapture(e.pointerId);
    }
  };

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
      {/* The strip. One row that reads left to right: what the window does,
          what it shows, what it will write, and how it is being looked at.
          What this window cannot change is a reading at the end rather than a
          disabled copy of a control that lives on the render node, because a
          dead input invites a click and answers nothing. */}
      <div className="render-strip">
        <button
          className="tbtn accent"
          disabled={!busy && oversizeForFloat}
          onClick={busy ? cancel : start}
        >
          {busy ? "Cancel" : started ? "Render again" : "Render"}
        </button>

        <span className="render-strip-sep" aria-hidden="true" />

        {passOptions(passes).length > 1 && (
          <label className="render-strip-field">
            <span className="render-strip-label">Showing</span>
            <Select
              ariaLabel="Pass shown"
              value={pass}
              width={130}
              options={passOptions(passes).map((o) => ({
                value: o.value,
                label: o.label,
                hint: o.unavailable ? "not produced" : undefined,
                disabled: o.unavailable !== undefined,
              }))}
              onChange={(v) => setPass(v as StillPass)}
            />
          </label>
        )}

        <label className="render-strip-field">
          <span className="render-strip-label">Format</span>
          <Select
            ariaLabel="Output format"
            value={format}
            width={110}
            options={[
              { value: "png", label: "PNG" },
              { value: "exr", label: "EXR" },
            ]}
            onChange={(v) => setFormat(v as StillFormat)}
            disabled={busy}
          />
        </label>

        {format === "exr" && (
          <label className="render-strip-field">
            <span className="render-strip-label">Space</span>
            <Select
              ariaLabel="Floating-point space"
              value={space}
              width={160}
              options={[
                { value: "sceneLinear", label: "Scene-linear" },
                { value: "display", label: "Display-referred" },
              ]}
              onChange={(v) => setSpace(v as StillSpace)}
              disabled={busy}
            />
          </label>
        )}

        <span className="render-strip-sep" aria-hidden="true" />

        <div className="render-strip-view">
          <button className="tbtn" onClick={() => setView(null)} title="Fit the picture to the window">
            Fit
          </button>
          <button
            className="tbtn"
            onClick={() => setView(actualSize(picture, stage))}
            title="One image pixel to one screen pixel"
          >
            100%
          </button>
          <span className="render-strip-zoom" aria-label={`Zoom ${Math.round(zoom * 100)} percent`}>
            {Math.round(zoom * 100)}%
          </span>
        </div>

        {/* Readings, not controls. First to go when the window is narrow. */}
        <div className="render-strip-readings">
          <span>
            {request.settings.width} x {request.settings.height}
          </span>
          <span>{request.settings.engine === "pathTraced" ? "Path traced" : "Rasterized"}</span>
          {request.settings.engine === "pathTraced" && (
            <span>
              {request.settings.samples} spp{request.settings.denoise ? ", denoised" : ""}
            </span>
          )}
          <span>Passes: {producedNote}</span>
        </div>
      </div>

      {/* The stage. The canvas keeps its intrinsic output size and is placed by
          CSS, so the picture can be moved without the pixels being touched. */}
      <div
        className="render-stage"
        ref={stageRef}
        onWheel={onWheel}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
      >
        <canvas
          ref={canvasRef}
          width={request.settings.width}
          height={request.settings.height}
          // The checker appears only behind a render that carries a matte:
          // behind an opaque one it would read as alpha the render does not
          // have, which is the ruling the plain ground records.
          className={
            request.settings.transparentBackground
              ? "render-canvas render-canvas-matte"
              : "render-canvas"
          }
          aria-label="The still being rendered"
          style={{
            left: `${rect.x}px`,
            top: `${rect.y}px`,
            width: `${rect.w}px`,
            height: `${rect.h}px`,
            // Past one to one the reader is looking at pixels and should see
            // them, rather than a smoothing of them.
            imageRendering: zoom >= 1 ? "pixelated" : "auto",
          }}
        />
      </div>

      {(oversizeForFloat || (started && previewNote) || megapixels >= CANVAS_WARN_MP) && (
        <div className="prefs-unit">
          {oversizeForFloat
            ? `A float still is limited to ${FLOAT_MAX_MP} megapixels and this one is ${megapixels.toFixed(1)}. Render it smaller, or take it from the command line, which writes the same image with no such limit.`
            : started && previewNote
              ? previewNote
              : `${megapixels.toFixed(0)} megapixels; this needs about ${Math.round(megapixels * 4)} MB of browser memory to hold.`}
        </div>
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
              ? `Tile ${Math.min(job.tile + 1, job.tiles)} of ${job.tiles}, sample ${job.sample} of ${job.samples} · ${formatDurationMs(job.elapsedMs)} elapsed${
                  // Nothing rather than a guess while the estimate has no rate
                  // to work from, and nothing once sampling is over, when the
                  // job is still assembling and a zero would read as finished.
                  job.remainingMs === null
                    ? ""
                    : `, ${formatDurationMs(job.remainingMs)} left`
                }`
              : finished
                ? `Done: ${job.tiles} tiles in ${formatDurationMs(job.elapsedMs)}`
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
        {anyPasses && (
          <button
            className="btn"
            disabled={busy || !finished}
            onClick={() => void saveAll()}
            title="The image and every pass it produced, in one archive"
          >
            Save all
          </button>
        )}
      </div>
    </Modal>
  );
}
