// Draco handling for glTF imports, run inside the import worker before the
// Rust glTF parse (the Rust `gltf` crate cannot decode
// KHR_draco_mesh_compression).
//
// Decision-18 cut-line (invoked 2026-07-08, maintainer sign-off): Solarxy Web
// ships without a Draco decoder for now. Rather than let the Rust parser fail
// with a cryptic message, this module detects the extension up front and
// throws a clear, actionable error, which the import node badges while
// keep-last-good holds the viewport. Re-enabling Draco (the draco3d decoder in
// the worker) is a reversible post-beta follow-up.

interface WorkerFile {
  name: string;
  bytes: Uint8Array;
}

// "glTF" and "JSON" as little-endian u32s, read via byte indexing (no
// DataView, which can throw on a transferred Uint8Array whose buffer is
// aliased or offset — that RangeError would otherwise be swallowed and skip
// detection entirely).
const u32le = (b: Uint8Array, o: number) =>
  (b[o] | (b[o + 1] << 8) | (b[o + 2] << 16) | (b[o + 3] << 24)) >>> 0;
const GLB_MAGIC = 0x46546c67; // "glTF"
const CHUNK_JSON = 0x4e4f534a; // "JSON"

/** Reads the glTF JSON out of a file: the JSON chunk of a GLB, or the whole
 * text of a `.gltf`. Returns null if it does not look like glTF JSON. */
function gltfJson(bytes: Uint8Array): unknown | null {
  try {
    if (bytes.byteLength >= 20 && u32le(bytes, 0) === GLB_MAGIC) {
      const chunkLen = u32le(bytes, 12);
      if (u32le(bytes, 16) !== CHUNK_JSON) return null;
      return JSON.parse(new TextDecoder().decode(bytes.subarray(20, 20 + chunkLen)));
    }
    // Plain .gltf text (guard against binary STL/PLY that start with other bytes).
    return JSON.parse(new TextDecoder().decode(bytes));
  } catch {
    return null;
  }
}

/** Whether a glTF file uses KHR_draco_mesh_compression. */
function usesDraco(file: WorkerFile): boolean {
  const doc = gltfJson(file.bytes);
  if (!doc || typeof doc !== "object") return false;
  const used = (doc as { extensionsUsed?: unknown }).extensionsUsed;
  return Array.isArray(used) && used.includes("KHR_draco_mesh_compression");
}

/** For glTF imports, rejects Draco-compressed files with a clear message
 * (the decision-18 cut-line); everything else passes through unchanged. The
 * primary model file is `files[0]`. */
export async function maybeInflateDraco(files: WorkerFile[]): Promise<WorkerFile[]> {
  if (files.length > 0 && usesDraco(files[0])) {
    throw new Error(
      "This glTF uses Draco mesh compression, which Solarxy Web does not support yet. " +
        "Please re-export the model without Draco compression.",
    );
  }
  return files;
}
