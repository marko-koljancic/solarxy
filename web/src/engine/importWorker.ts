// The import Web Worker: a second, headless instantiation of the same
// solarxy_web.wasm. It runs only `parse_model_job` (no wgpu device is ever
// created), parsing model files off the main thread and transferring the
// result back as a compact geometry blob. The main instance reconstructs the
// GeometrySet with one memcpy and commits it under the generation guard.
//
// Draco-compressed glTF is rejected with a clear message before the Rust
// parse (the decision-18 cut-line); see `draco.ts`.

import init, { parse_model_job } from "../wasm/pkg/solarxy_web.js";
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
  jobId: number;
  ctx: GraphContext;
  format: string;
  optionsJson: string;
  files: WorkerFile[];
}

let ready: Promise<void> | null = null;
function ensureReady(): Promise<void> {
  if (!ready) ready = init({ module_or_path: wasmUrl }).then(() => undefined);
  return ready;
}

const ctx = self as unknown as Worker;

ctx.onmessage = async (event: MessageEvent<ParseRequest>) => {
  const req = event.data;
  try {
    await ensureReady();
    // Draco lives only inside glTF; de-compress the primary file first so
    // the Rust glTF parser sees uncompressed buffers.
    const files =
      req.format === "gltf" || req.format === "glb"
        ? await maybeInflateDraco(req.files)
        : req.files;

    const blob = parse_model_job(req.format, req.optionsJson, files);
    ctx.postMessage({ jobId: req.jobId, ctx: req.ctx, blob }, [blob.buffer]);
  } catch (err) {
    ctx.postMessage({
      jobId: req.jobId,
      ctx: req.ctx,
      error: err instanceof Error ? err.message : String(err),
    });
  }
};
