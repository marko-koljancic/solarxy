// Which dock panel the pointer is over, tracked imperatively so the keyboard
// dispatcher can resolve the hovered panel without a store subscription (the
// keymap reads it once per keydown, nothing renders from it).
//
// The clear is conditional on the id matching: pointerenter on the next panel
// can fire before pointerleave on the previous one, and an unconditional clear
// in that ordering would wipe the fresher hover.

let hoveredPanelId: string | null = null;

export function setHoveredPanel(id: string): void {
  hoveredPanelId = id;
}

/** Clears the hover only if `id` is still the current one. */
export function clearHoveredPanel(id: string): void {
  if (hoveredPanelId === id) hoveredPanelId = null;
}

export function getHoveredPanel(): string | null {
  return hoveredPanelId;
}
