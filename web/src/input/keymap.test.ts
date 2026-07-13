// Keymap integrity: the table is the single source of truth, so it must
// stay unambiguous (no duplicate keys within a context) and documented.

import { describe, expect, it } from "vitest";
import { formatKeys, KEY_GROUPS, KEYMAP, lookupBinding } from "./keymap";

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

  it("assigns every binding to a known shortcuts-modal group", () => {
    for (const b of KEYMAP) {
      expect(KEY_GROUPS.includes(b.group), `${b.id} group '${b.group}'`).toBe(true);
    }
  });

  it("folds the shift that produces '?' so the lookup matches", () => {
    expect(lookupBinding("shift+?", "global")?.id).toBe("shortcuts");
    expect(lookupBinding("?", "global")?.id).toBe("shortcuts");
  });

  it("formats keys for display (platform modifier, symbols, uppercase)", () => {
    // The platform modifier follows the test host's navigator.platform.
    const mac =
      typeof navigator !== "undefined" && navigator.platform.toLowerCase().includes("mac");
    expect(formatKeys("mod+shift+z")).toEqual(
      mac ? ["⌘", "⇧", "Z"] : ["Ctrl", "Shift", "Z"],
    );
    expect(formatKeys("mod+enter")).toEqual(mac ? ["⌘", "⏎"] : ["Ctrl", "⏎"]);
    expect(formatKeys("escape")).toEqual(["Esc"]);
    expect(formatKeys("f4")).toEqual(["F4"]);
    expect(formatKeys("?")).toEqual(["?"]);
  });
});

describe("viewport tools (phase 11)", () => {
  it("binds Q and W over the viewport", () => {
    expect(lookupBinding("q", "viewport")?.id).toBe("tool-select");
    expect(lookupBinding("w", "viewport")?.id).toBe("tool-move");
  });

  it("leaves E and R unbound until Phase 12 wires rotate and scale", () => {
    // A key that silently does nothing is worse than no key; their buttons ship
    // disabled for the same reason.
    expect(lookupBinding("e", "viewport")).toBeNull();
    expect(lookupBinding("r", "viewport")).toBeNull();
  });

  it("narrows the display flag to the canvas, which is what frees E", () => {
    // It was always a Node Canvas action. Before Phase 11 it was `global`, so it
    // fired over the viewport too and E could never become a tool key.
    const flag = KEYMAP.find((b) => b.id === "display-flag");
    expect(flag?.context).toBe("canvas");
    expect(lookupBinding("e", "canvas")?.id).toBe("display-flag");
    // And it no longer fires when the pointer is over neither pane.
    expect(lookupBinding("e", "global")).toBeNull();
  });
});
