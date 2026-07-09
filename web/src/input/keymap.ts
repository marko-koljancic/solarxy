// The keyboard map: a single typed table (UX spec section 16) feeding the
// dispatcher today and the generated shortcuts modal in phase 7. Contexts
// follow the cursor-hover focus model: "global" fires anywhere (outside
// text fields), "canvas" only while the pointer is over the node canvas,
// "viewport" only over the 3D viewport region. Phase-6 W2 activates the
// viewport bindings (1-7 inspection, F1-F5 layouts, F fit).

export type KeyContext = "global" | "canvas" | "viewport";

export interface KeyBinding {
  id: string;
  /** "mod" = Cmd on macOS / Ctrl elsewhere; "+"-joined, lowercase key. */
  keys: string;
  context: KeyContext;
  description: string;
}

export const KEYMAP: readonly KeyBinding[] = [
  { id: "undo", keys: "mod+z", context: "global", description: "Undo" },
  { id: "redo", keys: "mod+shift+z", context: "global", description: "Redo" },
  { id: "redo-alt", keys: "mod+y", context: "global", description: "Redo" },
  { id: "copy", keys: "mod+c", context: "global", description: "Copy selection" },
  { id: "paste", keys: "mod+v", context: "global", description: "Paste" },
  { id: "duplicate", keys: "mod+d", context: "global", description: "Duplicate selection" },
  { id: "cook", keys: "mod+enter", context: "global", description: "Cook now (manual mode)" },
  { id: "bypass", keys: "b", context: "global", description: "Toggle bypass on selection" },
  { id: "palette", keys: "tab", context: "global", description: "Open the node palette" },
  { id: "display-flag", keys: "e", context: "global", description: "Set the display flag on the selection (subflow)" },
  // Viewport context (cursor over the 3D region). The 1-7 assignments and
  // F1-F5 layouts mirror the desktop bindings exactly.
  { id: "inspect-shaded", keys: "1", context: "viewport", description: "Inspection: Shaded" },
  { id: "inspect-material", keys: "2", context: "viewport", description: "Inspection: Material ID" },
  { id: "uv-pane-toggle", keys: "3", context: "viewport", description: "Toggle the UV pane" },
  { id: "inspect-texel", keys: "4", context: "viewport", description: "Inspection: Texel Density" },
  { id: "inspect-depth", keys: "5", context: "viewport", description: "Inspection: Depth" },
  { id: "inspect-overdraw", keys: "6", context: "viewport", description: "Inspection: Overdraw" },
  { id: "inspect-ao", keys: "7", context: "viewport", description: "Inspection: AO Preview" },
  { id: "layout-single", keys: "f1", context: "viewport", description: "Layout: Single" },
  { id: "layout-split-v", keys: "f2", context: "viewport", description: "Layout: Split Vertical" },
  { id: "layout-split-h", keys: "f3", context: "viewport", description: "Layout: Split Horizontal" },
  { id: "layout-quad", keys: "f4", context: "viewport", description: "Layout: Quad" },
  { id: "layout-three", keys: "f5", context: "viewport", description: "Layout: Three Left Big" },
  { id: "fit", keys: "f", context: "viewport", description: "Fit view to the scene" },
  { id: "uv-overlap-toggle", keys: "o", context: "viewport", description: "Toggle the UV overlap display (UV pane)" },
] as const;

/** The canonical key string for a keyboard event ("mod+shift+z" form). */
export function eventKeys(e: KeyboardEvent): string {
  const parts: string[] = [];
  if (e.metaKey || e.ctrlKey) parts.push("mod");
  if (e.shiftKey) parts.push("shift");
  if (e.altKey) parts.push("alt");
  const key = e.key.toLowerCase();
  if (!["meta", "control", "shift", "alt"].includes(key)) parts.push(key);
  return parts.join("+");
}

/** The binding matching an event in a context (contexts fall back to
 * global), or null. */
export function lookupBinding(keys: string, context: KeyContext): KeyBinding | null {
  const exact = KEYMAP.find((b) => b.keys === keys && b.context === context);
  if (exact) return exact;
  if (context !== "global") {
    return KEYMAP.find((b) => b.keys === keys && b.context === "global") ?? null;
  }
  return null;
}
