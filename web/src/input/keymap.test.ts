// Keymap integrity: the table is the single source of truth, so it must
// stay unambiguous (no duplicate keys within a context) and documented.

import { describe, expect, it } from "vitest";
import { KEYMAP, lookupBinding } from "./keymap";

describe("keymap table", () => {
  it("has no duplicate (context, keys) pairs", () => {
    const seen = new Set<string>();
    for (const b of KEYMAP) {
      const sig = `${b.context}:${b.keys}`;
      expect(seen.has(sig), `duplicate binding ${sig}`).toBe(false);
      seen.add(sig);
    }
  });

  it("has a description and id on every entry", () => {
    for (const b of KEYMAP) {
      expect(b.id.length).toBeGreaterThan(0);
      expect(b.description.length).toBeGreaterThan(0);
    }
  });

  it("normalizes modifier order as mod+shift+alt+key", () => {
    for (const b of KEYMAP) {
      const parts = b.keys.split("+");
      const mods = parts.slice(0, -1);
      const order = ["mod", "shift", "alt"];
      const idx = mods.map((m) => order.indexOf(m));
      expect([...idx].sort((a, z) => a - z)).toEqual(idx);
      expect(idx.includes(-1)).toBe(false);
    }
  });

  it("resolves context bindings with global fallback", () => {
    expect(lookupBinding("mod+z", "canvas")?.id).toBe("undo");
    expect(lookupBinding("mod+z", "global")?.id).toBe("undo");
    expect(lookupBinding("q", "canvas")).toBeNull();
  });
});
