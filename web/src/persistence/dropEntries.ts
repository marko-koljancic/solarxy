// Folder-aware drop collection: expands directory entries recursively via
// the Chromium webkitGetAsEntry API so dropping a model FOLDER (the Khronos
// FlightHelmet layout: gltf + bin + a textures/ subdirectory) stages every
// file. Plain multi-file drops pass straight through. Directory structure
// is discarded deliberately: the import sidecar resolver matches companions
// by basename.

interface FileSystemEntryLike {
  isFile: boolean;
  isDirectory: boolean;
  name: string;
  file?: (ok: (f: File) => void, err: (e: unknown) => void) => void;
  createReader?: () => {
    readEntries: (ok: (entries: FileSystemEntryLike[]) => void, err: (e: unknown) => void) => void;
  };
}

function entryFile(entry: FileSystemEntryLike): Promise<File | null> {
  return new Promise((resolve) => {
    if (!entry.file) {
      resolve(null);
      return;
    }
    entry.file(
      (f) => resolve(f),
      () => resolve(null),
    );
  });
}

async function readAllEntries(entry: FileSystemEntryLike): Promise<FileSystemEntryLike[]> {
  const reader = entry.createReader?.();
  if (!reader) return [];
  // readEntries returns results in batches (100 per call in Chromium);
  // drain until an empty batch.
  const all: FileSystemEntryLike[] = [];
  for (;;) {
    const batch = await new Promise<FileSystemEntryLike[]>((resolve) => {
      reader.readEntries(
        (entries) => resolve(entries),
        () => resolve([]),
      );
    });
    if (batch.length === 0) return all;
    all.push(...batch);
  }
}

async function collectEntry(entry: FileSystemEntryLike, out: File[]): Promise<void> {
  if (entry.isFile) {
    const f = await entryFile(entry);
    if (f) out.push(f);
    return;
  }
  if (entry.isDirectory) {
    for (const child of await readAllEntries(entry)) {
      await collectEntry(child, out);
    }
  }
}

/** Every file in a drop, folders expanded recursively. Falls back to the
 * flat file list when the entry API is unavailable. */
export async function collectDroppedFiles(dt: DataTransfer): Promise<File[]> {
  const items = Array.from(dt.items ?? []);
  const entries = items
    .map((item) => (item.webkitGetAsEntry ? item.webkitGetAsEntry() : null))
    .filter((e): e is NonNullable<typeof e> => e !== null);
  if (entries.length === 0) {
    return Array.from(dt.files);
  }
  const out: File[] = [];
  for (const entry of entries) {
    await collectEntry(entry as unknown as FileSystemEntryLike, out);
  }
  // An entry-API walk that yields nothing (odd drags) still falls back.
  return out.length > 0 ? out : Array.from(dt.files);
}
