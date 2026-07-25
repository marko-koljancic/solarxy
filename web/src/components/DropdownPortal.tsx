// The shared portaled dropdown surface for overlays that must paint above
// every sibling overlay layer (viewport toolbar menus, the attr column's
// lane list, the viz settings panel). The pane toolbar anchor is its own
// stacking context (z 10) BELOW the attribute-pin layer (z 11), so any
// dropdown rendered inline is trapped underneath its sibling overlays no
// matter its own z-index; rendering into document.body at the dedicated
// dropdown layer (--z-dropdown) sidesteps sibling ordering for good.
//
// Owns the dismiss contract: outside pointerdown (the anchor and every
// panel sharing the same tree id count as inside, so a submenu flyout
// belongs to its parent menu), Escape, and window resize (pane rects
// move on resize; a stale fixed position is worse than closing).

import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";

const MARGIN = 8;
const GAP = 2;

export type DropdownPlacement = "below" | "side";
export type DropdownAlign = "left" | "right";

interface RectLike {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

interface SizeLike {
  width: number;
  height: number;
}

/** Pure placement math: below the anchor (left- or right-aligned) or to
 * its side (submenu flyouts), flipped and clamped against the viewport
 * once the panel size is known. Exported for tests. */
export function placeDropdown(
  anchor: RectLike,
  panel: SizeLike,
  viewport: SizeLike,
  placement: DropdownPlacement,
  align: DropdownAlign,
): { left: number; top: number } {
  let left: number;
  let top: number;
  if (placement === "side") {
    left = anchor.right + GAP;
    top = anchor.top - 4;
    if (left + panel.width > viewport.width - MARGIN) {
      left = anchor.left - panel.width - GAP;
    }
  } else {
    left = align === "right" ? anchor.right - panel.width : anchor.left;
    top = anchor.bottom + GAP;
    if (left + panel.width > viewport.width - MARGIN) {
      left = viewport.width - panel.width - MARGIN;
    }
    if (top + panel.height > viewport.height - MARGIN) {
      top = anchor.top - panel.height - GAP;
    }
  }
  if (top + panel.height > viewport.height - MARGIN) {
    top = viewport.height - panel.height - MARGIN;
  }
  return { left: Math.max(MARGIN, left), top: Math.max(MARGIN, top) };
}

export function DropdownPortal({
  anchorRef,
  onClose,
  placement = "below",
  align = "left",
  treeId,
  onPointerEnter,
  onPointerLeave,
  children,
}: {
  /** The trigger element the panel positions against (and which never
   * counts as an outside click, so the trigger's own toggle works). */
  anchorRef: RefObject<HTMLElement | null>;
  onClose: () => void;
  placement?: DropdownPlacement;
  align?: DropdownAlign;
  /** Panels sharing a tree id are one menu for outside-close purposes. */
  treeId?: string;
  onPointerEnter?: () => void;
  onPointerLeave?: () => void;
  children: ReactNode;
}) {
  const panelRef = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);

  // Hidden first paint, measured placement second: right-alignment and
  // edge flips need the panel's real size before it can be positioned.
  useLayoutEffect(() => {
    const anchor = anchorRef.current?.getBoundingClientRect();
    const panel = panelRef.current?.getBoundingClientRect();
    if (!anchor || !panel) return;
    setPos(
      placeDropdown(
        anchor,
        { width: panel.width, height: panel.height },
        { width: window.innerWidth, height: window.innerHeight },
        placement,
        align,
      ),
    );
  }, [anchorRef, placement, align]);

  useEffect(() => {
    const onDown = (e: PointerEvent) => {
      const t = e.target;
      if (!(t instanceof Element)) return;
      if (anchorRef.current?.contains(t)) return;
      const inside = treeId
        ? t.closest(`[data-dropdown-tree="${treeId}"]`)
        : panelRef.current?.contains(t);
      if (!inside) onClose();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onResize = () => onClose();
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("keydown", onKey, true);
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("keydown", onKey, true);
      window.removeEventListener("resize", onResize);
    };
  }, [anchorRef, treeId, onClose]);

  return createPortal(
    <div
      ref={panelRef}
      className="dropdown-portal"
      data-dropdown-tree={treeId}
      style={pos ? { left: pos.left, top: pos.top } : { visibility: "hidden", left: 0, top: 0 }}
      onPointerEnter={onPointerEnter}
      onPointerLeave={onPointerLeave}
    >
      {children}
    </div>,
    document.body,
  );
}
