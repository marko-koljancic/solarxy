// The screenshot modal (/ section 11, Minimystix reference):
// resolution presets over the active pane, GPU overlay toggles, capture,
// preview, and PNG download. The capture renders offscreen Rust-side at
// the requested resolution; the RGBA readback encodes to PNG here via
// OffscreenCanvas (no image codec in the wasm payload).

import { useEffect, useRef, useState } from "react";
import { getClient } from "../engine/session";
import type { ScreenshotResult } from "../engine/types";
import { usePrefs, type ScreenshotResolution } from "../store/prefs";
import { pushToast } from "../store/toasts";
import { useUi } from "../store/ui";
import { Modal } from "./Modal";
import { Row, Section } from "./DialogRow";
import { Popover, renderDoc } from "./Popover";
import { useViewState } from "../store/viewState";
import { Select } from "./Select";

/** Capture dimensions (physical px) for a preset over the active pane's
 * CSS rect. Pure for tests. */
export function screenshotDims(
  resolution: ScreenshotResolution,
  paneCss: { width: number; height: number },
  dpr: number,
  customWidth: number,
  customHeight: number,
): { width: number; height: number } {
  if (resolution === "custom") {
    return { width: Math.max(16, Math.round(customWidth)), height: Math.max(16, Math.round(customHeight)) };
  }
  const factor = resolution === "1.5x" ? 1.5 : resolution === "2x" ? 2 : resolution === "4x" ? 4 : 1;
  return {
    width: Math.max(16, Math.round(paneCss.width * dpr * factor)),
    height: Math.max(16, Math.round(paneCss.height * dpr * factor)),
  };
}

async function encodePng(result: ScreenshotResult): Promise<Blob> {
  const canvas = new OffscreenCanvas(result.width, result.height);
  const ctx = canvas.getContext("2d");
  if (!ctx) throw new Error("2d context unavailable");
  // Copy into a fresh ArrayBuffer-backed clamped array (the wasm-boundary
  // Uint8Array types as ArrayBufferLike, which ImageData rejects).
  const clamped = new Uint8ClampedArray(result.pixels.length);
  clamped.set(result.pixels);
  const image = new ImageData(clamped, result.width, result.height);
  ctx.putImageData(image, 0, 0);
  return canvas.convertToBlob({ type: "image/png" });
}

function filename(): string {
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, "0");
  return `solarxy_${d.getFullYear()}${pad(d.getMonth() + 1)}${pad(d.getDate())}-${pad(d.getHours())}${pad(d.getMinutes())}${pad(d.getSeconds())}.png`;
}

export function ScreenshotModal({ onClose }: { onClose: () => void }) {
  const defaults = usePrefs((s) => s.prefs.screenshot);
  const view = useViewState((s) => s.view);
  // A render node's Render button presets a custom resolution
  // for exactly one open; consumed here.
  const preset = useUi.getState().screenshotPreset;
  if (preset) useUi.getState().setScreenshotPreset(null);
  const [resolution, setResolution] = useState<ScreenshotResolution>(
    preset ? "custom" : defaults.resolution,
  );
  const [custom, setCustom] = useState(
    preset ? { w: preset.width, h: preset.height } : { w: defaults.customWidth, h: defaults.customHeight },
  );
  const [overlays, setOverlays] = useState({ ...defaults.overlays });
  const [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<{ url: string; blob: Blob; dims: string } | null>(null);
  const pollRef = useRef<number | null>(null);

  // Esc/backdrop close through the shared shell; this cleanup only stops
  // the poll and frees the preview URL.
  useEffect(() => {
    return () => {
      if (pollRef.current !== null) cancelAnimationFrame(pollRef.current);
      if (preview) URL.revokeObjectURL(preview.url);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const capture = () => {
    if (!view || busy) return;
    const rect = view.paneRects[view.activePane];
    if (!rect) return;
    const dims = screenshotDims(
      resolution,
      { width: rect.width, height: rect.height },
      window.devicePixelRatio || 1,
      custom.w,
      custom.h,
    );
    try {
      getClient().requestScreenshot({ width: dims.width, height: dims.height, overlays });
    } catch (err) {
      pushToast(`Screenshot failed: ${err instanceof Error ? err.message : err}`, "error");
      return;
    }
    setBusy(true);
    const deadline = performance.now() + 20_000;
    const poll = () => {
      let result: ScreenshotResult | undefined;
      try {
        result = getClient().pollScreenshot();
      } catch (err) {
        setBusy(false);
        pushToast(`Screenshot failed: ${err instanceof Error ? err.message : err}`, "error");
        return;
      }
      if (!result) {
        // A capture that never resolves (a lost GPU device) must not hang
        // the modal forever.
        if (performance.now() > deadline) {
          setBusy(false);
          pushToast(
            "Screenshot timed out (GPU limit reached). Try a smaller preset; reload if the viewport stopped rendering.",
            "error",
          );
          return;
        }
        pollRef.current = requestAnimationFrame(poll);
        return;
      }
      void encodePng(result).then((blob) => {
        setBusy(false);
        setPreview((old) => {
          if (old) URL.revokeObjectURL(old.url);
          return {
            url: URL.createObjectURL(blob),
            blob,
            dims: `${result.width} x ${result.height}`,
          };
        });
      });
    };
    pollRef.current = requestAnimationFrame(poll);
  };

  const save = () => {
    if (!preview) return;
    const a = document.createElement("a");
    a.href = preview.url;
    a.download = filename();
    a.click();
  };

  return (
    <Modal
      id="screenshot"
      title="Screenshot"
      onClose={onClose}
      className="modal-wide screenshot-modal"
      // Fixed height with a growing preview and a pinned action row: the
      // body has to be a flex column for `.screenshot-preview`'s `flex: 1`
      // to mean anything.
      bodyLayout="column"
    >
        <Section title="Capture">
        <Row
          label="Resolution"
          doc="Image size, relative to the pane's current on-screen size. Captures are budgeted at about 4 megapixels: past that the browser can lose the graphics device and the whole viewport goes with it."
        >
          <Select
            ariaLabel="Resolution"
            value={resolution}
            options={[
              { value: "viewport", label: "Viewport" },
              { value: "1.5x", label: "1.5x" },
              { value: "2x", label: "2x" },
              { value: "4x", label: "4x" },
              { value: "custom", label: "Custom" },
            ]}
            onChange={(v) => setResolution(v as ScreenshotResolution)}
          />
          {resolution === "custom" && (
            <>
              <input
                className="input-field prefs-dim"
                type="number"
                min={16}
                value={custom.w}
                onChange={(e) => setCustom({ ...custom, w: Number(e.target.value) || custom.w })}
              />
              <span className="prefs-unit">x</span>
              <input
                className="input-field prefs-dim"
                type="number"
                min={16}
                value={custom.h}
                onChange={(e) => setCustom({ ...custom, h: Number(e.target.value) || custom.h })}
              />
            </>
          )}
        </Row>
        </Section>
        <Section title="Include in the image">
        <div className="screenshot-controls">
          {(
            [
              [
                "grid",
                "Grid",
                "Whether the ground grid appears in the image. Off for a presentation frame, on when the image is meant to convey scale.",
              ],
              [
                "axes",
                "Axes",
                "Whether the corner axis gizmo appears in the image.",
              ],
              [
                "validation",
                "Validation",
                "Whether validation highlights are baked in. On, a screenshot doubles as a bug report showing exactly which faces are flagged.",
              ],
            ] as const
          ).map(([key, label, doc]) => (
            <Popover key={key} title={label} content={renderDoc(doc)}>
              <label className="review-complete">
                <input
                  type="checkbox"
                  checked={overlays[key]}
                  onChange={(e) => setOverlays({ ...overlays, [key]: e.target.checked })}
                />
                {label}
              </label>
            </Popover>
          ))}
          <button className="btn primary" disabled={busy} onClick={capture}>
            {busy ? "Capturing..." : "Capture"}
          </button>
        </div>
        </Section>
        <div className="screenshot-preview">
          {preview ? (
            <img src={preview.url} alt={`Screenshot ${preview.dims}`} />
          ) : (
            <div className="screenshot-placeholder">
              {busy ? "Rendering..." : "Capture the active pane to preview it here."}
            </div>
          )}
        </div>
        <div className="modal-actions">
          {preview && <span className="prefs-unit">{preview.dims}</span>}
          <button className="btn" onClick={onClose}>
            Close
          </button>
          <button className="btn primary" disabled={!preview} onClick={save}>
            Save PNG
          </button>
        </div>
    </Modal>
  );
}
