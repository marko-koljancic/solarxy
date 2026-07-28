// The shared help popover (sections 7/9/18): hover or focus opens
// after a short delay, Escape dismisses (spec 19), position flips at the
// viewport edges. Rendered in a portal so overflow containers (palette,
// parameter panel) never clip it. The doc renderer handles the descriptor
// markdown subset (paragraphs, `code`, **bold**) without a dependency.

import { useEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { claimEscape, releaseEscape } from "./escapeClaim";

const OPEN_DELAY_MS = 400;
const MARGIN = 8;

/** Renders a descriptor doc string: blank-line paragraphs with `code` and
 * **bold** spans. Deliberately tiny; the catalog's docs use nothing else. */
export function renderDoc(doc: string): ReactNode {
  return doc
    .split(/\n\s*\n/)
    .filter((p) => p.trim().length > 0)
    .map((para, pi) => (
      <p key={pi}>
        {para.split(/(`[^`]+`|\*\*[^*]+\*\*)/).map((chunk, ci) => {
          if (chunk.startsWith("`") && chunk.endsWith("`")) {
            return <code key={ci}>{chunk.slice(1, -1)}</code>;
          }
          if (chunk.startsWith("**") && chunk.endsWith("**")) {
            return <strong key={ci}>{chunk.slice(2, -2)}</strong>;
          }
          return chunk;
        })}
      </p>
    ));
}

export function Popover({
  title,
  children,
  content,
}: {
  /** Bold first line of the popover (e.g. the param label or node name). */
  title?: string;
  /** The trigger element(s); hover/focus opens, leave/blur closes. */
  children: ReactNode;
  /** The popover body (typically renderDoc output plus meta lines). */
  content: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const anchorRef = useRef<HTMLSpanElement>(null);
  const panelRef = useRef<HTMLDivElement>(null);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // The anchor span is display:contents (layout-neutral), so it has no box
  // of its own; measure the real trigger element inside it.
  const anchorRect = () => {
    const anchor = anchorRef.current;
    const el = (anchor?.firstElementChild as HTMLElement | null) ?? anchor;
    return el?.getBoundingClientRect() ?? null;
  };

  const show = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const r = anchorRect();
      if (!r) return;
      // Default to the right of the trigger; measured flip happens after
      // the first paint via the effect below.
      setPos({ left: r.right + MARGIN, top: r.top });
      setOpen(true);
    }, OPEN_DELAY_MS);
  };

  const hide = () => {
    if (timer.current) clearTimeout(timer.current);
    timer.current = null;
    setOpen(false);
  };

  // Flip/clamp once the panel has a size; Escape closes (spec 19).
  useEffect(() => {
    if (!open) return;
    const panel = panelRef.current;
    const ar = anchorRect();
    if (panel && ar) {
      const pr = panel.getBoundingClientRect();
      let left = ar.right + MARGIN;
      let top = ar.top;
      if (left + pr.width > window.innerWidth - MARGIN) {
        left = Math.max(MARGIN, ar.left - pr.width - MARGIN);
      }
      if (top + pr.height > window.innerHeight - MARGIN) {
        top = Math.max(MARGIN, window.innerHeight - pr.height - MARGIN);
      }
      setPos({ left, top });
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      // Escape closes THIS popover and goes no further.
      //
      // Without this, a popover open inside a dialog let Escape dismiss the
      // Preferences window around it -- losing unsaved edits to answer
      // "what does this row do?". The popover is the innermost thing on
      // screen, so it is the thing Escape means.
      //
      // `stopPropagation` alone does NOT achieve that: `Modal` listens on
      // the same target in the same phase and runs first, so it would have
      // closed already by the time this fires. `escapeClaim` is how it
      // knows to stand down; the stop here is belt-and-braces for anything
      // listening further down the tree.
      e.stopPropagation();
      hide();
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open]);

  // Claim Escape for as long as the panel is up (see `escapeClaim`).
  useEffect(() => {
    if (!open) return undefined;
    claimEscape();
    return releaseEscape;
  }, [open]);

  useEffect(() => () => hide(), []); // eslint-disable-line react-hooks/exhaustive-deps

  return (
    <span
      ref={anchorRef}
      className="popover-anchor"
      onMouseEnter={show}
      onMouseLeave={hide}
      onFocus={show}
      onBlur={hide}
    >
      {children}
      {open &&
        pos &&
        createPortal(
          <div ref={panelRef} className="doc-popover" style={{ left: pos.left, top: pos.top }}>
            {title && <div className="doc-popover-title">{title}</div>}
            <div className="doc-popover-body">{content}</div>
          </div>,
          document.body,
        )}
    </span>
  );
}
