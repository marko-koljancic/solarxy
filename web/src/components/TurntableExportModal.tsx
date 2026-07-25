// The turntable export modal (item 9b): renders a 360-degree sweep of the
// active pane through its render-through camera, one deterministic offscreen
// frame at a time (host `request_turntable_frame`), then encodes to WebM / MP4
// (WebCodecs) or a PNG sequence (ZIP). Mirrors the ScreenshotModal capture
// pattern; the camera and the 4 MP clamp are the host's concern.

import { useEffect, useRef, useState } from "react";
import { getClient } from "../engine/session";
import {
  encodeTurntable,
  formatExtension,
  videoExportSupported,
  type TurntableFormat,
} from "../export/turntable";
import type { ScreenshotResult } from "../engine/types";
import { type ScreenshotResolution } from "../store/prefs";
import { pushToast } from "../store/toasts";
import { useViewState } from "../store/viewState";
import { Modal } from "./Modal";
import { screenshotDims } from "./ScreenshotModal";
import { Select } from "./Select";

/** Requests one turntable frame and resolves when the readback is ready. */
function captureFrame(
  pane: number,
  azimuthDeg: number,
  opts: { width: number; height: number; overlays: { grid: boolean; axes: boolean; validation: boolean } },
): Promise<ScreenshotResult> {
  return new Promise((resolve, reject) => {
    try {
      getClient().requestTurntableFrame(pane, azimuthDeg, opts);
    } catch (e) {
      reject(e instanceof Error ? e : new Error(String(e)));
      return;
    }
    const deadline = performance.now() + 20_000;
    const poll = () => {
      let r: ScreenshotResult | undefined;
      try {
        r = getClient().pollScreenshot();
      } catch (e) {
        reject(e instanceof Error ? e : new Error(String(e)));
        return;
      }
      if (r) {
        resolve(r);
        return;
      }
      if (performance.now() > deadline) {
        reject(new Error("a frame timed out (GPU limit reached)"));
        return;
      }
      requestAnimationFrame(poll);
    };
    requestAnimationFrame(poll);
  });
}

function download(blob: Blob, name: string): void {
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

export function TurntableExportModal({ onClose }: { onClose: () => void }) {
  const view = useViewState((s) => s.view);
  const canVideo = videoExportSupported();
  const [format, setFormat] = useState<TurntableFormat>(canVideo ? "webm" : "pngZip");
  const [resolution, setResolution] = useState<ScreenshotResolution>("viewport");
  const [fps, setFps] = useState(30);
  const [duration, setDuration] = useState(4);
  const [overlays, setOverlays] = useState({ grid: true, axes: false, validation: false });
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState(0);
  const [status, setStatus] = useState("");
  const cancelRef = useRef(false);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        if (busy) cancelRef.current = true;
        else onClose();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [busy, onClose]);

  const run = async () => {
    if (!view || busy) return;
    const pane = view.activePane;
    const rect = view.paneRects[pane];
    if (!rect) return;
    const dims = screenshotDims(
      resolution,
      { width: rect.width, height: rect.height },
      window.devicePixelRatio || 1,
      1920,
      1080,
    );
    const frameCount = Math.max(2, Math.round(fps * duration));
    cancelRef.current = false;
    setBusy(true);
    setProgress(0);
    setStatus(`Rendering 0 / ${frameCount}`);
    const frames: ScreenshotResult[] = [];
    try {
      for (let k = 0; k < frameCount; k++) {
        if (cancelRef.current) break;
        const azimuth = (360 * k) / frameCount;
        frames.push(
          await captureFrame(pane, azimuth, {
            width: dims.width,
            height: dims.height,
            overlays,
          }),
        );
        setProgress((k + 1) / frameCount);
        setStatus(`Rendering ${k + 1} / ${frameCount}`);
      }
      if (cancelRef.current) {
        setStatus("Cancelled");
        setBusy(false);
        return;
      }
      setStatus("Encoding...");
      const blob = await encodeTurntable(frames, fps, format);
      download(blob, `solarxy_turntable.${formatExtension(format)}`);
      setStatus("Done");
    } catch (e) {
      pushToast(`Turntable export failed: ${e instanceof Error ? e.message : e}`, "error");
      setStatus("Failed");
    }
    setBusy(false);
  };

  return (
    <Modal
      id="turntable"
      title="Export Turntable"
      onClose={onClose}
      className="modal-wide"
      // While an export runs, Esc CANCELS the run (the dialog's own
      // listener below) and the backdrop is inert.
      closeOnEsc={false}
      closeOnBackdrop={!busy}
    >
        <div className="screenshot-controls">
          <label className="prefs-unit">Format</label>
          <Select
            ariaLabel="Format"
            value={format}
            options={[
              { value: "webm", label: "WebM", hint: canVideo ? undefined : "needs WebCodecs", disabled: !canVideo },
              { value: "mp4", label: "MP4", hint: canVideo ? undefined : "needs WebCodecs", disabled: !canVideo },
              { value: "pngZip", label: "PNG sequence (ZIP)" },
            ]}
            onChange={(v) => setFormat(v as TurntableFormat)}
          />
          <label className="prefs-unit">Resolution</label>
          <Select
            ariaLabel="Resolution"
            value={resolution}
            options={[
              { value: "viewport", label: "Viewport" },
              { value: "1.5x", label: "1.5x" },
              { value: "2x", label: "2x" },
            ]}
            onChange={(v) => setResolution(v as ScreenshotResolution)}
          />
          <label className="prefs-unit">FPS</label>
          <input
            className="input-field prefs-dim"
            type="number"
            min={1}
            max={60}
            value={fps}
            onChange={(e) => setFps(Math.max(1, Math.min(60, Number(e.target.value) || fps)))}
          />
          <label className="prefs-unit">Seconds</label>
          <input
            className="input-field prefs-dim"
            type="number"
            min={1}
            max={30}
            value={duration}
            onChange={(e) =>
              setDuration(Math.max(1, Math.min(30, Number(e.target.value) || duration)))
            }
          />
        </div>
        <div className="screenshot-controls">
          {(
            [
              ["grid", "Grid"],
              ["axes", "Axes"],
              ["validation", "Validation"],
            ] as const
          ).map(([key, label]) => (
            <label key={key} className="review-complete">
              <input
                type="checkbox"
                checked={overlays[key]}
                onChange={(e) => setOverlays({ ...overlays, [key]: e.target.checked })}
              />
              {label}
            </label>
          ))}
          <span className="prefs-unit">
            {Math.max(2, Math.round(fps * duration))} frames, one 360 rotation
          </span>
        </div>
        {busy && (
          <div className="turntable-progress">
            <div className="turntable-progress-bar" style={{ width: `${Math.round(progress * 100)}%` }} />
          </div>
        )}
        <div className="modal-actions">
          <span className="prefs-unit">{status}</span>
          <button className="btn" onClick={busy ? () => (cancelRef.current = true) : onClose}>
            {busy ? "Cancel" : "Close"}
          </button>
          <button className="btn primary" disabled={busy} onClick={run}>
            {busy ? "Exporting..." : "Export"}
          </button>
        </div>
    </Modal>
  );
}
