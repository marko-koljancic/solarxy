// OPFS autosave: a rotating ring of 3 slots holding the JSON DocumentFile.
// Phase 4 persists the document only (no assets); Phase 5 upgrades to a full
// `.slxy` with asset payloads. Explicit save uses the File System Access API
// with a download fallback.

const RING = 3;
const CURSOR_KEY = "solarxy.autosave.cursor";

function slotName(i: number): string {
  return `autosave-${i}.json`;
}

async function opfsRoot(): Promise<FileSystemDirectoryHandle | null> {
  try {
    return await navigator.storage.getDirectory();
  } catch {
    return null; // OPFS unavailable (insecure context / unsupported)
  }
}

/** Writes an autosave into the next ring slot (best-effort). */
export async function writeAutosave(json: string): Promise<void> {
  const root = await opfsRoot();
  if (!root) return;
  const cursor = (Number(localStorage.getItem(CURSOR_KEY) ?? "0") + 1) % RING;
  try {
    const handle = await root.getFileHandle(slotName(cursor), { create: true });
    const w = await handle.createWritable();
    await w.write(json);
    await w.close();
    localStorage.setItem(CURSOR_KEY, String(cursor));
  } catch {
    /* quota or transient; drop this autosave */
  }
}

/** The newest autosave across the ring (by file mtime), or null. */
export async function readLatestAutosave(): Promise<{ json: string; when: number } | null> {
  const root = await opfsRoot();
  if (!root) return null;
  let best: { json: string; when: number } | null = null;
  for (let i = 0; i < RING; i++) {
    try {
      const handle = await root.getFileHandle(slotName(i));
      const file = await handle.getFile();
      if (!best || file.lastModified > best.when) {
        best = { json: await file.text(), when: file.lastModified };
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

/** Downloads a JSON string as a file (the explicit-save fallback). */
export function downloadJson(json: string, filename: string): void {
  const blob = new Blob([json], { type: "application/json" });
  const url = URL.createObjectURL(blob);
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  a.click();
  URL.revokeObjectURL(url);
}

interface SaveFilePickerWindow {
  showSaveFilePicker?: (opts: unknown) => Promise<{
    createWritable: () => Promise<{ write: (d: string) => Promise<void>; close: () => Promise<void> }>;
  }>;
}

/** Explicit save: File System Access API when available, else download. */
export async function saveToFile(json: string, filename: string): Promise<void> {
  const picker = (window as unknown as SaveFilePickerWindow).showSaveFilePicker;
  if (picker) {
    try {
      const handle = await picker({
        suggestedName: filename,
        types: [{ description: "Solarxy scene", accept: { "application/json": [".json"] } }],
      });
      const w = await handle.createWritable();
      await w.write(json);
      await w.close();
      return;
    } catch (e) {
      if ((e as DOMException)?.name === "AbortError") return; // user cancelled
    }
  }
  downloadJson(json, filename);
}
