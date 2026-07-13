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

  it("binds E and R now that Phase 12 has wired rotate and scale", () => {
    // Phase 11 left these deliberately unbound, because a key that silently does
    // nothing is worse than no key. Their gizmos exist now, so the keys do too.
    expect(lookupBinding("e", "viewport")?.id).toBe("tool-rotate");
    expect(lookupBinding("r", "viewport")?.id).toBe("tool-scale");
  });

  it("keeps E meaning two different things in two different contexts", () => {
    // The whole reason the display flag narrowed from global to canvas in Phase
    // 11: it freed E over the viewport for the Rotate tool. Both must resolve,
    // and to different actions, or the narrowing bought nothing.
    const flag = KEYMAP.find((b) => b.id === "display-flag");
    expect(flag?.context).toBe("canvas");
    expect(lookupBinding("e", "canvas")?.id).toBe("display-flag");
    expect(lookupBinding("e", "viewport")?.id).toBe("tool-rotate");
    // And it still fires nowhere when the pointer is over neither pane.
    expect(lookupBinding("e", "global")).toBeNull();
  });

  it("binds X to the gizmo orientation toggle", () => {
    expect(lookupBinding("x", "viewport")?.id).toBe("gizmo-orientation");
  });
});
