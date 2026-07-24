// The attribute-pin position channel, the review-marker pattern with a
// POOL twist: React renders a fixed pool of pin elements per pane once
// (per settings change) and this module patches position, text, and
// visibility imperatively each frame. The host stride-samples a stable
// candidate set and each pin carries its candidate SLOT, so slot j of a
// pane always shows the same point; a camera move only flips per-slot
// visibility, never re-binds content. Unused slots hide.

import { create } from "zustand";
import type { AttrPin } from "./types";

const slots = new Map<string, HTMLElement>();

interface AttrPinStats {
  /** Pool slots the host is sampling into (0 while pins are off). */
  capacity: number;
  /** Displayed points in the scene; capacity < total means sampling. */
  total: number;
  set: (capacity: number, total: number) => void;
}

/** The per-frame sampling facts, published only on change so the React
 * pool and the strip's notice re-render on cooks, not on every frame. */
export const useAttrPinStats = create<AttrPinStats>((set, get) => ({
  capacity: 0,
  total: 0,
  set: (capacity, total) => {
    const s = get();
    if (s.capacity !== capacity || s.total !== total) set({ capacity, total });
  },
}));

export function attrPinKey(pane: number, slot: number): string {
  return `${pane}:${slot}`;
}

/** Registers (or unregisters, with null) a pool element. Fresh slots
 * start hidden until a frame assigns them a pin. */
export function registerAttrPin(key: string, el: HTMLElement | null): void {
  if (el) {
    el.style.visibility = "hidden";
    slots.set(key, el);
  } else {
    slots.delete(key);
  }
}

/** Compact pin value text: up to three significant decimals per
 * component, joined for vectors. Pure, exported for tests. */
export function fmtPinValue(value: number[]): string {
  return value
    .map((x) => {
      const r = Math.round(x * 100) / 100;
      return String(Object.is(r, -0) ? 0 : r);
    })
    .join(", ");
}

/** The text one pin shows: the point number in points mode, the value in
 * labels mode (falling back to the point number when the lane is absent),
 * both when both modes are on. Pure, exported for tests. */
export function pinText(pin: AttrPin, labels: boolean, points: boolean): string {
  const value = labels && pin.value && pin.value.length > 0 ? fmtPinValue(pin.value) : null;
  if (points && value) return `${pin.ptnum}: ${value}`;
  if (value) return value;
  return String(pin.ptnum);
}

/** Applies one frame's pins, keyed by each pin's stable candidate slot;
 * leftover slots hide. */
export function applyAttrPins(pins: AttrPin[], labels: boolean, points: boolean): void {
  if (slots.size === 0) return;
  const used = new Set<string>();
  for (const pin of pins) {
    const key = attrPinKey(pin.pane, pin.slot);
    const el = slots.get(key);
    if (!el) continue;
    el.style.transform = `translate3d(${pin.x}px, ${pin.y}px, 0)`;
    el.style.visibility = "visible";
    const text = el.querySelector(".attr-pin-text");
    if (text) text.textContent = pinText(pin, labels, points);
    used.add(key);
  }
  for (const [key, el] of slots) {
    if (!used.has(key)) el.style.visibility = "hidden";
  }
}

/** Hides every pooled pin (viz off, canvas hidden). */
export function hideAttrPins(): void {
  for (const el of slots.values()) el.style.visibility = "hidden";
}
