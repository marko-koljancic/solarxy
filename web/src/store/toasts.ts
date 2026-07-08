// A tiny toast queue for transient feedback (rejected connections, skipped
// pastes, save/recovery notices).

import { create } from "zustand";

export type ToastSeverity = "info" | "warn" | "error";

export interface Toast {
  id: number;
  message: string;
  severity: ToastSeverity;
}

interface ToastState {
  toasts: Toast[];
  dismiss: (id: number) => void;
  _push: (t: Toast) => void;
}

let nextId = 1;

export const useToasts = create<ToastState>((set) => ({
  toasts: [],
  dismiss: (id) => set((s) => ({ toasts: s.toasts.filter((t) => t.id !== id) })),
  _push: (t) => set((s) => ({ toasts: [...s.toasts.slice(-4), t] })),
}));

/** Pushes a toast that auto-dismisses after a few seconds. */
export function pushToast(message: string, severity: ToastSeverity = "info"): void {
  const id = nextId++;
  useToasts.getState()._push({ id, message, severity });
  setTimeout(() => useToasts.getState().dismiss(id), severity === "error" ? 6000 : 3500);
}
