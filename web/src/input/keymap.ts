// The keyboard map: a single typed table (UX spec section 16) feeding the
// dispatcher AND the generated shortcuts modal, so the two can never drift.
// Contexts follow the cursor-hover focus model: "global" fires anywhere
// (outside text fields), "canvas" only while the pointer is over the node
// canvas, "viewport" only over the 3D viewport region.

export type KeyContext = "global" | "canvas" | "viewport";

/** The shortcuts modal's section headings (display order). */
export const KEY_GROUPS = [
  "File",
  "Edit",
  "Node Canvas",
  "Viewport & Layout",
  "Inspection",
  "Review",
] as const;

export type KeyGroup = (typeof KEY_GROUPS)[number];

export interface KeyBinding {
  id: string;
  /** "mod" = Cmd on macOS / Ctrl elsewhere; "+"-joined, lowercase key. */
  keys: string;
  context: KeyContext;
  description: string;
  /** The shortcuts modal section this binding lists under. */
  group: KeyGroup;
  /** Browser-conflict or scoping note shown in the modal (spec sec. 16). */
  note?: string;
}

export const KEYMAP: readonly KeyBinding[] = [
  { id: "save", keys: "mod+s", context: "global", group: "File", description: "Save the scene (.slxy)", note: "Intercepted from the browser; Cmd/Ctrl+W cannot be (autosave covers it)" },
  { id: "undo", keys: "mod+z", context: "global", group: "Edit", description: "Undo" },
  { id: "redo", keys: "mod+shift+z", context: "global", group: "Edit", description: "Redo" },
  { id: "redo-alt", keys: "mod+y", context: "global", group: "Edit", description: "Redo" },
  { id: "copy", keys: "mod+c", context: "global", group: "Edit", description: "Copy selection" },
  { id: "paste", keys: "mod+v", context: "global", group: "Edit", description: "Paste" },
  { id: "duplicate", keys: "mod+d", context: "global", group: "Edit", description: "Duplicate selection" },
  { id: "cook", keys: "mod+enter", context: "global", group: "Edit", description: "Cook now (manual mode)" },
  { id: "bypass", keys: "b", context: "global", group: "Node Canvas", description: "Toggle bypass on selection" },
  { id: "palette", keys: "tab", context: "global", group: "Node Canvas", description: "Open the node palette", note: "When the canvas has focus" },
  // Narrowed from global to canvas in Phase 11: that is precisely what frees E
  // over the viewport for the Rotate tool. It was always a Node Canvas action.
  { id: "display-flag", keys: "e", context: "canvas", group: "Node Canvas", description: "Set the display flag on the selection (subflow)" },
  { id: "rename", keys: "f2", context: "canvas", group: "Node Canvas", description: "Rename the first selected node (inline)" },
  { id: "flow-grid", keys: "g", context: "canvas", group: "Node Canvas", description: "Toggle the canvas grid" },
  { id: "flow-minimap", keys: "m", context: "canvas", group: "Node Canvas", description: "Toggle the minimap" },
  { id: "flow-controls", keys: "c", context: "canvas", group: "Node Canvas", description: "Toggle the zoom controls" },
  { id: "layout-cycle", keys: "l", context: "canvas", group: "Node Canvas", description: "Auto-layout the graph (cycles Dagre / ELK)" },
  { id: "edge-style-cycle", keys: "s", context: "canvas", group: "Node Canvas", description: "Cycle the connection style" },
  { id: "shortcuts", keys: "?", context: "global", group: "File", description: "Show keyboard shortcuts" },
  { id: "preferences", keys: "mod+,", context: "global", group: "File", description: "Open preferences" },
  // Viewport context (cursor over the 3D region). The 1-7 assignments and
  // F1-F5 layouts mirror the desktop bindings exactly.
  { id: "inspect-shaded", keys: "1", context: "viewport", group: "Inspection", description: "Inspection: Shaded" },
  { id: "inspect-material", keys: "2", context: "viewport", group: "Inspection", description: "Inspection: Material ID" },
  { id: "uv-pane-toggle", keys: "3", context: "viewport", group: "Inspection", description: "Toggle the UV pane" },
  { id: "inspect-texel", keys: "4", context: "viewport", group: "Inspection", description: "Inspection: Texel Density" },
  { id: "inspect-depth", keys: "5", context: "viewport", group: "Inspection", description: "Inspection: Depth" },
  { id: "inspect-overdraw", keys: "6", context: "viewport", group: "Inspection", description: "Inspection: Overdraw" },
  { id: "inspect-ao", keys: "7", context: "viewport", group: "Inspection", description: "Inspection: AO Preview" },
  { id: "layout-single", keys: "f1", context: "viewport", group: "Viewport & Layout", description: "Layout: Single" },
  { id: "layout-split-v", keys: "f2", context: "viewport", group: "Viewport & Layout", description: "Layout: Split Vertical" },
  { id: "layout-split-h", keys: "f3", context: "viewport", group: "Viewport & Layout", description: "Layout: Split Horizontal" },
  { id: "layout-quad", keys: "f4", context: "viewport", group: "Viewport & Layout", description: "Layout: Quad" },
  { id: "layout-three", keys: "f5", context: "viewport", group: "Viewport & Layout", description: "Layout: Three Left Big" },
  { id: "fit", keys: "f", context: "viewport", group: "Viewport & Layout", description: "Fit view to the scene" },
  { id: "screenshot", keys: "c", context: "viewport", group: "Viewport & Layout", description: "Screenshot the active pane" },
  { id: "uv-overlap-toggle", keys: "o", context: "viewport", group: "Inspection", description: "Toggle the UV overlap display (UV pane)" },
  // Viewport tools (Maya-style Q/W/E/R; Blender's G/R/S collide with the grid,
  // connection-style and review bindings). Rotate (E) and Scale (R) arrive in
  // Phase 12 and are deliberately UNBOUND until then, matching their disabled
  // buttons: a key that silently does nothing is worse than no key.
  { id: "tool-select", keys: "q", context: "viewport", group: "Viewport & Layout", description: "Tool: Select" },
  { id: "tool-move", keys: "w", context: "viewport", group: "Viewport & Layout", description: "Tool: Move (translate gizmo)" },
  // Review (Phase 7): mode toggle over the viewport, panel anywhere; Esc
  // walks the cancel ladder (gizmo drag > draft > re-anchor > review mode >
  // maximized panel).
  { id: "review-mode", keys: "shift+r", context: "viewport", group: "Review", description: "Toggle review mode (click geometry to pin a note)" },
  { id: "review-panel", keys: "n", context: "global", group: "Review", description: "Toggle the review panel" },
  { id: "review-cancel", keys: "escape", context: "global", group: "Review", description: "Cancel the note editor / re-anchor / review mode, or restore a maximized panel" },
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
 * global), or null. `?` is stored bare: the shift that produces it on most
 * layouts is folded away so lookups match. */
export function lookupBinding(keys: string, context: KeyContext): KeyBinding | null {
  const canonical = keys === "shift+?" ? "?" : keys;
  const exact = KEYMAP.find((b) => b.keys === canonical && b.context === context);
  if (exact) return exact;
  if (context !== "global") {
    return KEYMAP.find((b) => b.keys === canonical && b.context === "global") ?? null;
  }
  return null;
}

const IS_MAC =
  typeof navigator !== "undefined" && navigator.platform.toLowerCase().includes("mac");

const KEY_LABELS: Record<string, string> = {
  enter: "⏎",
  escape: "Esc",
  tab: "Tab",
  backspace: "⌫",
  delete: "⌦",
  arrowup: "↑",
  arrowdown: "↓",
  arrowleft: "←",
  arrowright: "→",
};

/** Human-readable key chips for the shortcuts modal and menus: "mod" maps
 * to the platform modifier, single letters uppercase, specials get their
 * conventional symbols. */
export function formatKeys(keys: string): string[] {
  return keys.split("+").map((part) => {
    if (part === "mod") return IS_MAC ? "⌘" : "Ctrl";
    if (part === "shift") return IS_MAC ? "⇧" : "Shift";
    if (part === "alt") return IS_MAC ? "⌥" : "Alt";
    const label = KEY_LABELS[part];
    if (label) return label;
    return part.length === 1 ? part.toUpperCase() : part.toUpperCase();
  });
}
