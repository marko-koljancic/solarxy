// Global editor keyboard shortcuts. A single typed table is the source of
// truth (the generated shortcuts modal is Phase 7); here it drives the
// dispatcher directly. Ignored while typing in a field.

import { useEffect } from "react";
import { copySelection, dispatch, duplicateSelection, paste } from "../engine/session";
import { useMirror } from "../store/mirror";

function typing(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  return !!el?.tagName?.match(/INPUT|TEXTAREA|SELECT/) || el?.isContentEditable === true;
}

export function useKeyboard(): void {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typing(e.target)) return;
      const mod = e.metaKey || e.ctrlKey;
      const key = e.key.toLowerCase();
      const s = useMirror.getState();
      const ctx = s.current;
      const selection = s.contexts[ctx === "root" ? "root" : `sub:${ctx.subflow}`]?.selection ?? [];

      if (mod && key === "z") {
        e.preventDefault();
        dispatch({ type: e.shiftKey ? "redo" : "undo" });
      } else if (mod && key === "y") {
        e.preventDefault();
        dispatch({ type: "redo" });
      } else if (mod && key === "c") {
        copySelection();
      } else if (mod && key === "v") {
        paste();
      } else if (mod && key === "d") {
        e.preventDefault();
        duplicateSelection();
      } else if (mod && key === "enter") {
        e.preventDefault();
        dispatch({ type: "cookNow" });
      } else if (key === "b" && !mod && selection.length) {
        // Toggle bypass on the selection (using the first node's state).
        const first = s.contexts[ctx === "root" ? "root" : `sub:${ctx.subflow}`]?.nodes.find(
          (n) => n.id === selection[0],
        );
        const bypassed = !(first?.bypassed ?? false);
        for (const id of selection) dispatch({ type: "setBypass", ctx, node: id, bypassed });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}
