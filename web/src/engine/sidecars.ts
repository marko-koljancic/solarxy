// Sidecar preflight for multi-file model imports. The browser sandbox
// means the app can only read files the user hands it, so a model that
// references companions (glTF external buffers and images, OBJ material
// libraries) imports correctly only when every companion is staged. These
// pure functions detect what a primary file references and what is still
// missing, so the import UI can prompt BEFORE the parse fails with
// "missing external asset". Matching mirrors the Rust TableResolver:
// percent-decoded URIs compared by trailing basename, case-sensitively.

/** Companion files a primary model references, by basename. `required`
 * companions hard-fail the parse when missing (glTF external buffers);
 * `optional` ones degrade quietly (glTF images, OBJ material libraries). */
export interface SidecarRefs {
  required: string[];
  optional: string[];
}

const NONE: SidecarRefs = { required: [], optional: [] };

/** The trailing file-name component (TableResolver's matching key). */
export function basename(path: string): string {
  return path.split(/[/\\]/).pop() ?? path;
}

/** Percent-decode a glTF URI the way the Rust loader does; undecodable
 * input falls back to the raw string. */
function percentDecode(uri: string): string {
  try {
    return decodeURIComponent(uri);
  } catch {
    return uri;
  }
}

function extOf(name: string): string {
  const dot = name.lastIndexOf(".");
  return dot >= 0 ? name.slice(dot).toLowerCase() : "";
}

function dedupe(names: string[]): string[] {
  return [...new Set(names)];
}

/** External URI entries of a glTF JSON array (`buffers` / `images`):
 * string `uri` fields that are not data URIs, percent-decoded, basenamed. */
function gltfUris(entries: unknown): string[] {
  if (!Array.isArray(entries)) return [];
  return entries
    .map((e) => (e as { uri?: unknown }).uri)
    .filter((u): u is string => typeof u === "string" && !u.startsWith("data:"))
    .map((u) => basename(percentDecode(u)));
}

/**
 * The companion basenames a primary model file references. `.gltf` parses
 * as JSON (buffers required, images optional); `.obj` scans `mtllib` lines
 * (optional; tobj treats the line remainder as one path, spaces included).
 * Self-contained formats (.glb, .stl, .ply) and anything unparseable
 * return nothing: the preflight must never block a file the real Rust
 * parser would accept.
 */
export function referencedSidecars(name: string, bytes: Uint8Array): SidecarRefs {
  const ext = extOf(name);
  try {
    if (ext === ".gltf") {
      const json = JSON.parse(new TextDecoder().decode(bytes)) as {
        buffers?: unknown;
        images?: unknown;
      };
      return {
        required: dedupe(gltfUris(json.buffers)),
        optional: dedupe(gltfUris(json.images)),
      };
    }
    if (ext === ".obj") {
      const text = new TextDecoder().decode(bytes);
      const libs: string[] = [];
      for (const line of text.split(/\r?\n/)) {
        const m = /^\s*mtllib\s+(.+?)\s*$/.exec(line);
        if (m) libs.push(basename(m[1]));
      }
      return { required: [], optional: dedupe(libs) };
    }
  } catch {
    return NONE;
  }
  return NONE;
}

/** The referenced companions not present in the staged set (staged names
 * compared by basename, exactly as the resolver will match them). */
export function missingSidecars(refs: SidecarRefs, stagedNames: Iterable<string>): SidecarRefs {
  const staged = new Set([...stagedNames].map(basename));
  return {
    required: refs.required.filter((n) => !staged.has(n)),
    optional: refs.optional.filter((n) => !staged.has(n)),
  };
}

/** Whether a diff still has anything missing. */
export function hasMissing(refs: SidecarRefs): boolean {
  return refs.required.length > 0 || refs.optional.length > 0;
}
