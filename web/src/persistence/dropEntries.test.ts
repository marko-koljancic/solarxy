// Folder-drop traversal: directories expand recursively, batched
// readEntries drains fully, and non-entry environments fall back flat.

import { describe, expect, it } from "vitest";
import { collectDroppedFiles } from "./dropEntries";

type AnyEntry = {
  isFile: boolean;
  isDirectory: boolean;
  name: string;
  file?: (ok: (f: File) => void) => void;
  createReader?: () => { readEntries: (ok: (e: AnyEntry[]) => void) => void };
};

function fileEntry(name: string): AnyEntry {
  return {
    isFile: true,
    isDirectory: false,
    name,
    file: (ok) => ok(new File(["x"], name)),
  };
}

function dirEntry(name: string, children: AnyEntry[], batchSize = 2): AnyEntry {
  let cursor = 0;
  return {
    isFile: false,
    isDirectory: true,
    name,
    createReader: () => ({
      readEntries: (ok) => {
        const batch = children.slice(cursor, cursor + batchSize);
        cursor += batch.length;
        ok(batch);
      },
    }),
  };
}

function dt(entries: (AnyEntry | null)[], flat: File[] = []): DataTransfer {
  return {
    items: entries.map((e) => ({ webkitGetAsEntry: () => e })),
    files: flat,
  } as unknown as DataTransfer;
}

describe("collectDroppedFiles", () => {
  it("expands a nested folder drop recursively, draining batched reads", async () => {
    const helmet = dirEntry("FlightHelmet", [
      fileEntry("FlightHelmet.gltf"),
      fileEntry("FlightHelmet.bin"),
      dirEntry("textures", [fileEntry("a.png"), fileEntry("b.png"), fileEntry("c.png")]),
    ]);
    const files = await collectDroppedFiles(dt([helmet]));
    expect(files.map((f) => f.name).sort()).toEqual([
      "FlightHelmet.bin",
      "FlightHelmet.gltf",
      "a.png",
      "b.png",
      "c.png",
    ]);
  });

  it("passes plain multi-file drops through", async () => {
    const files = await collectDroppedFiles(dt([fileEntry("m.obj"), fileEntry("m.mtl")]));
    expect(files.map((f) => f.name)).toEqual(["m.obj", "m.mtl"]);
  });

  it("falls back to the flat list when the entry API yields nothing", async () => {
    const flat = [new File(["x"], "fallback.obj")];
    const files = await collectDroppedFiles(dt([null], flat));
    expect(files.map((f) => f.name)).toEqual(["fallback.obj"]);
  });
});
