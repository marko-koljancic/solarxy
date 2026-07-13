// The import Web Worker: a second, headless instantiation of the same
// solarxy_web.wasm. It runs `parse_model_job` (parsing model files off the
// main thread, plus the implicit load validation) and
// `validate_geometry_job` (the validate node above its inline threshold);
// no wgpu device is ever created. Results transfer back as a compact
// geometry blob and/or a JSON validation payload; the main instance
// commits them under the generation guard.
//
// Draco-compressed glTF is rejected with a clear message before the Rust
// parse (the decision-18 cut-line); see `draco.ts`.

import init, {
  parse_model_job,
  prepare_hdri_job,
  validate_geometry_job,
} from "../wasm/pkg/solarxy_web.js";
import wasmUrl from "../wasm/pkg/solarxy_web_bg.wasm?url";
import { maybeInflateDraco } from "./draco";
import type { GraphContext } from "./types";

/** A file handed to the parser: original name + raw bytes. */
interface WorkerFile {
  name: string;
  bytes: Uint8Array;
}

/** The parse request the main thread posts. */
interface ParseRequest {
  kind: "parse";
  jobId: number;
  ctx: GraphContext;
  format: string;
  optionsJson: string;
  files: WorkerFile[];
}

/** The HDRI-preparation request the main thread posts (decode + the CPU
 * IBL stages; the GPU finish happens on the main thread). */
interface HdriRequest {
  kind: "hdri";
  jobId: number;
  ctx: GraphContext;
  bytes: Uint8Array;
  /** Lowercase extension without the dot; empty sniffs the magic. */
  format: string;
}

/** The geometry-validation request the main thread posts. */
interface ValidateRequest {
  kind: "validate";
  jobId: number;
  ctx: GraphContext;
  /** The geometry transfer blob (packed by the host at drain time). */
  blob: Uint8Array;
  /** JSON `ValidationConfig`. */
  configJson: string;
  budget?: number;
}

/** The image-decode request (`import_image`, Phase 13). Decoded entirely
 * by the browser (`createImageBitmap` + OffscreenCanvas readback): zero
 * wasm involvement, native codec speed, free format support. */
interface DecodeImageRequest {
  kind: "decodeImage";
  jobId: number;
  ctx: GraphContext;
  name: string;
  bytes: Uint8Array;
}

/** Decode encoded image bytes to raw RGBA8. `premultiplyAlpha: "none"` and
 * `colorSpaceConversion: "none"` keep the readback byte-faithful to the
 * file (no fringing, no ICC drift); the 2d readback path is the standard
 * worker-safe route to pixels. */
async function decodeImageBytes(
  bytes: Uint8Array,
): Promise<{ width: number; height: number; pixels: Uint8Array }> {
  const bitmap = await createImageBitmap(new Blob([bytes as BlobPart]), {
    premultiplyAlpha: "none",
    colorSpaceConversion: "none",
  });
  try {
    const canvas = new OffscreenCanvas(bitmap.width, bitmap.height);
    const c2d = canvas.getContext("2d", { willReadFrequently: true });
    if (!c2d) throw new Error("no 2d context for image readback");
    c2d.drawImage(bitmap, 0, 0);
    const data = c2d.getImageData(0, 0, bitmap.width, bitmap.height);
    return {
      width: bitmap.width,
      height: bitmap.height,
      pixels: new Uint8Array(data.data.buffer),
    };
  } finally {
    bitmap.close();
  }
}

/** What `parse_model_job` returns: the geometry transfer blob plus the
 * implicit load validation as JSON. */
interface ParseOutput {
  blob: Uint8Array;
  validation: string;
}

let ready: Promise<void> | null = null;
function ensureReady(): Promise<void> {
  if (!ready) ready = init({ module_or_path: wasmUrl }).then(() => undefined);
  return ready;
}

const ctx = self as unknown as Worker;

ctx.onmessage = async (
  event: MessageEvent<ParseRequest | ValidateRequest | HdriRequest | DecodeImageRequest>,
) => {
  const req = event.data;
  try {
    // Image decode is pure browser API; skip the wasm init entirely.
    if (req.kind === "decodeImage") {
      const { width, height, pixels } = await decodeImageBytes(req.bytes);
      ctx.postMessage({ kind: "decodeImage", jobId: req.jobId, ctx: req.ctx, width, height, pixels }, [
        pixels.buffer,
      ]);
      return;
    }
    await ensureReady();
    if (req.kind === "hdri") {
      const prepared = prepare_hdri_job(req.bytes, req.format);
      ctx.postMessage({ kind: "hdri", jobId: req.jobId, ctx: req.ctx, prepared }, [
        prepared.buffer,
      ]);
      return;
    }
    if (req.kind === "validate") {
      const validation = validate_geometry_job(req.blob, req.configJson, req.budget);
      ctx.postMessage({ kind: "validate", jobId: req.jobId, ctx: req.ctx, validation });
      return;
    }
    // Draco lives only inside glTF; de-compress the primary file first so
    // the Rust glTF parser sees uncompressed buffers.
    const files =
      req.format === "gltf" || req.format === "glb"
        ? await maybeInflateDraco(req.files)
        : req.files;

    const out = parse_model_job(req.format, req.optionsJson, files) as ParseOutput;
    ctx.postMessage(
      { kind: "parse", jobId: req.jobId, ctx: req.ctx, blob: out.blob, validation: out.validation },
      [out.blob.buffer],
    );
  } catch (err) {
    ctx.postMessage({
      kind: req.kind,
      jobId: req.jobId,
      ctx: req.ctx,
      error: err instanceof Error ? err.message : String(err),
    });
  }
};
