// OPFS autosave: a rotating ring of 3 slots holding the full `.slxy` archive
// bytes (document + camera + embedded assets), so kill-the-tab recovery
// restores a scene with its imports intact. Explicit save/open use the File
// System Access API with a download / file-input fallback.
//
// Storing the monolithic archive keeps recovery a single `load_slxy`. For
// scenes with very large embedded assets this re-zips on each autosave; the
// exploded content-addressed cache (document + write-once asset blobs) is the
// planned follow-up optimization.

const RING = 3;
const CURSOR_KEY = "solarxy.autosave.cursor";

function slotName(i: number): string {
  return `autosave-${i}.slxy`;
}

async function opfsRoot(): Promise<FileSystemDirectoryHandle | null> {
  try {
    return await navigator.storage.getDirectory();
  } catch {
    return null; // OPFS unavailable (insecure context / unsupported)
  }
}

/** Writes an autosave (`.slxy` bytes) into the next ring slot (best-effort). */
export async function writeAutosave(bytes: Uint8Array): Promise<void> {
  const root = await opfsRoot();
  if (!root) return;
  const cursor = (Number(localStorage.getItem(CURSOR_KEY) ?? "0") + 1) % RING;
  try {
    const handle = await root.getFileHandle(slotName(cursor), { create: true });
    const w = await handle.createWritable();
    await w.write(bytes as unknown as FileSystemWriteChunkType);
    await w.close();
    localStorage.setItem(CURSOR_KEY, String(cursor));
  } catch {
    /* quota or transient; drop this autosave */
  }
}

/** The newest autosave across the ring (by file mtime), or null. */
export async function readLatestAutosave(): Promise<{ bytes: Uint8Array; when: number } | null> {
  const root = await opfsRoot();
  if (!root) return null;
  let best: { bytes: Uint8Array; when: number } | null = null;
  for (let i = 0; i < RING; i++) {
    try {
      const handle = await root.getFileHandle(slotName(i));
      const file = await handle.getFile();
      if (!best || file.lastModified > best.when) {
        best = { bytes: new Uint8Array(await file.arrayBuffer()), when: file.lastModified };
      }
    } catch {
      /* slot absent */
    }
  }
  return best;
}

/** Clears all autosave slots (after a recovery decision or new scene). */
export async function clearAutosaves(): Promise<void> {
  const root = await opfsRoot();
  if (!root) return;
  for (let i = 0; i < RING; i++) {
    try {
      await root.removeEntry(slotName(i));
    } catch {
      /* absent */
    }
  }
  localStorage.removeItem(CURSOR_KEY);
}

/** Downloads bytes as a file (the explicit-save fallback). */
function downloadBytes(bytes: Uint8Array, filename: string): void {
  const blob = new Blob([bytes as unknown as BlobPart], { type: "application/octet-stream" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

interface SaveFilePickerWindow {
  showSaveFilePicker?: (opts: unknown) => Promise<{
    createWritable: () => Promise<{
      write: (d: Uint8Array) => Promise<void>;
      close: () => Promise<void>;
    }>;
  }>;
  showOpenFilePicker?: (opts: unknown) => Promise<Array<{ getFile: () => Promise<File> }>>;
}

const SLXY_TYPES = [
  { description: "Solarxy scene", accept: { "application/octet-stream": [".slxy"] } },
];

/** Explicit save: File System Access API when available, else download. */
export async function saveToFile(bytes: Uint8Array, filename: string): Promise<void> {
  const picker = (window as unknown as SaveFilePickerWindow).showSaveFilePicker;
  if (picker) {
    try {
      const handle = await picker({ suggestedName: filename, types: SLXY_TYPES });
      const w = await handle.createWritable();
      await w.write(bytes);
      await w.close();
      return;
    } catch (e) {
      if ((e as DOMException)?.name === "AbortError") return; // user cancelled
    }
  }
  downloadBytes(bytes, filename);
}

/** Explicit save for EXPORT bytes: like `saveToFile` but with
 * the export's own mime/extension instead of the `.slxy` type. */
export async function saveExportToFile(
  bytes: Uint8Array,
  filename: string,
  mime: string,
): Promise<void> {
  const ext = filename.includes(".") ? `.${filename.split(".").pop()}` : "";
  const picker = (window as unknown as SaveFilePickerWindow).showSaveFilePicker;
  if (picker && ext) {
    try {
      const handle = await picker({
        suggestedName: filename,
        types: [{ description: "Export", accept: { [mime]: [ext] } }],
      });
      const w = await handle.createWritable();
      await w.write(bytes);
      await w.close();
      return;
    } catch (e) {
      if ((e as DOMException)?.name === "AbortError") return; // user cancelled
    }
  }
  downloadBytes(bytes, filename);
}

/** Opens a `.slxy` via the File System Access API, or a hidden file input
 * fallback. Returns the file's bytes and name, or null if cancelled. */
export async function openSceneFile(): Promise<{ bytes: Uint8Array; name: string } | null> {
  const picker = (window as unknown as SaveFilePickerWindow).showOpenFilePicker;
  if (picker) {
    try {
      const [handle] = await picker({ types: SLXY_TYPES, multiple: false });
      const file = await handle.getFile();
      return { bytes: new Uint8Array(await file.arrayBuffer()), name: file.name };
    } catch (e) {
      if ((e as DOMException)?.name === "AbortError") return null;
    }
  }
  // Fallback: a transient file input.
  return new Promise((resolve) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = ".slxy";
    input.onchange = async () => {
      const file = input.files?.[0];
      resolve(file ? { bytes: new Uint8Array(await file.arrayBuffer()), name: file.name } : null);
    };
    input.click();
  });
}
