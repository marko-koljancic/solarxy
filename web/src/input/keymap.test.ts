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

  it("binds E and R for rotate and scale", () => {
 // left these deliberately unbound, because a key that silently does
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

describe("view presets (feedback wave 4)", () => {
  it("binds the Max-style axis views over the viewport", () => {
    expect(lookupBinding("t", "viewport")?.id).toBe("view-top");
    expect(lookupBinding("f", "viewport")?.id).toBe("view-front");
    expect(lookupBinding("l", "viewport")?.id).toBe("view-left");
    expect(lookupBinding("b", "viewport")?.id).toBe("view-bottom");
    expect(lookupBinding("p", "viewport")?.id).toBe("view-perspective");
    expect(lookupBinding("o", "viewport")?.id).toBe("view-ortho");
  });

  it("moves viewport fit to Z and gives the canvas its own F fit", () => {
    // F over the viewport is now the Front view (Max muscle memory); the fit
    // action lives on Z there, while F over the node canvas fits the graph.
    expect(lookupBinding("z", "viewport")?.id).toBe("fit");
    expect(lookupBinding("f", "canvas")?.id).toBe("canvas-fit");
  });

  it("splits P between the canvas and the viewport, and says so on both", () => {
    // The same key doing two things depending on where the cursor sits is
    // defensible only if both bindings admit it: the shortcuts modal
    // renders these notes, so this is what makes the split discoverable
    // rather than a trap.
    expect(lookupBinding("p", "canvas")?.id).toBe("floating-props");
    expect(lookupBinding("p", "viewport")?.id).toBe("view-perspective");

    for (const id of ["floating-props", "view-perspective"]) {
      const binding = KEYMAP.find((b) => b.id === id);
      expect(binding?.note, `${id} must name the other P`).toBeTruthy();
    }
  });

  it("keeps F and L meaning different things per context", () => {
    expect(lookupBinding("f", "viewport")?.id).toBe("view-front");
    expect(lookupBinding("l", "canvas")?.id).toBe("layout-cycle");
    expect(lookupBinding("l", "viewport")?.id).toBe("view-left");
  });

  it("narrows bypass to the canvas so B can be the Bottom view", () => {
    const bypass = KEYMAP.find((b) => b.id === "bypass");
    expect(bypass?.context).toBe("canvas");
    expect(lookupBinding("b", "canvas")?.id).toBe("bypass");
    expect(lookupBinding("b", "viewport")?.id).toBe("view-bottom");
    expect(lookupBinding("b", "global")).toBeNull();
  });
});
