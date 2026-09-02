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
  // The picture itself, off screen. The visible canvas shows whichever pass is
  // selected, so the beauty needs somewhere of its own to live: switching to
  // the albedo and back has to replay what already arrived rather than lose it,
  // and the passes are held on the Rust side while the beauty is only ever
  // here. It is also what Save writes, so what is saved does not depend on what
  // is being looked at.
  const beautyRef = useRef<HTMLCanvasElement | null>(null);
  const [started, setStarted] = useState(false);
  const [finished, setFinished] = useState(false);
  const [pass, setPass] = useState<StillPass>("beauty");
  const [passes, setPasses] = useState<StillPasses | undefined>(undefined);
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
      const beauty = beautyRef.current?.getContext("2d");
      const view = canvasRef.current?.getContext("2d");
      // Painted into the visible canvas as well as the offscreen one, rather
      // than blitting the whole picture every frame: a tile is a small
      // rectangle and the whole picture is not.
      const showing = pass === "beauty" ? view : null;
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
          landed = true;
        }
        // A pass only exists for tiles that finished, so it is worth remapping
        // exactly when one has. A preview carries no passes and changes nothing
        // here.
        if (landed && pass !== "beauty" && view) showPass(view, pass);
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

  // The offscreen picture rather than the visible canvas, so what is saved is
  // the render rather than whichever pass happens to be on screen.
  const save = () => {
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
        const blob = await new Promise<Blob | null>((resolve) =>
          beautyRef.current?.toBlob(resolve, "image/png") ?? resolve(null),
        );
        if (blob) files[`${name}.png`] = new Uint8Array(await blob.arrayBuffer());
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
        <Row
          label="Passes"
          doc="Set on the render node. A pass is written beside the image for a compositor; which one this window shows is the separate control below."
        >
          <span className="prefs-unit">{producedNote}</span>
        </Row>
      </Section>

      {started && passOptions(passes).length > 1 && (
        <Row
          label="Showing"
          doc="Which pass this window displays. Switching replays what has already arrived; it never re-renders, and it never changes what is saved."
        >
          <Select
            ariaLabel="Pass shown"
            value={pass}
            options={passOptions(passes).map((o) => ({
              value: o.value,
              label: o.label,
              hint: o.unavailable ? "not produced" : undefined,
              disabled: o.unavailable !== undefined,
            }))}
            onChange={(v) => setPass(v as StillPass)}
          />
        </Row>
      )}

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
        <button className="btn primary" disabled={busy || oversizeForFloat} onClick={start}>
          {started ? "Render again" : "Render"}
        </button>
      </div>
    </Modal>
  );
}
