// The DOM marker overlay: one clip box per 3D pane (absolutely positioned
// from the pane rects, overflow hidden), holding a pin per top-level
// annotation. React renders pin STRUCTURE on review changes; the rAF loop
// patches pin POSITIONS imperatively through the marker registry, so
// orbiting never re-renders. Pins expand into a summary card on hover or
// selection (desktop review_overlay parity).

import { markerKey, registerMarker } from "../../engine/markers";
import type { Annotation } from "../../engine/types";
import { repliesOf, useReview } from "../../store/review";
import { useViewState } from "../../store/viewState";
import { CATEGORY_GLYPHS, relativeTime, shortPreview } from "./visuals";

function Pin({ a, pane, replies }: { a: Annotation; pane: number; replies: Annotation[] }) {
  const selected = useReview((s) => s.selected === a.id);
  const setSelected = useReview((s) => s.setSelected);
  const setPanelOpen = useReview((s) => s.setPanelOpen);
  const classes = [
    "review-pin",
    `cat-${a.category}`,
    a.resolved ? "resolved" : "",
    a.needsReanchor ? "stale" : "",
    selected ? "selected" : "",
  ]
    .filter(Boolean)
    .join(" ");
  return (
    <div
      key={markerKey(pane, a.id)}
      ref={(el) => registerMarker(markerKey(pane, a.id), el)}
      className={classes}
      onPointerDown={(e) => e.stopPropagation()}
      onClick={(e) => {
        e.stopPropagation();
        setSelected(selected ? null : a.id);
        if (!selected) setPanelOpen(true);
      }}
    >
      <span className="review-pin-dot">{CATEGORY_GLYPHS[a.category]}</span>
      <div className="review-pin-card">
        <div className="review-pin-meta">
          <span>{a.author ?? "Anonymous"}</span>
          <span>{relativeTime(a.updatedAt || a.createdAt)}</span>
        </div>
        <div className="review-pin-text">{shortPreview(a.text)}</div>
        {a.needsReanchor && <div className="review-pin-stale">Geometry changed — re-place</div>}
        {replies.length > 0 && (
          <div className="review-pin-replies">
            {replies.length} repl{replies.length === 1 ? "y" : "ies"}
          </div>
        )}
      </div>
    </div>
  );
}

export function ReviewOverlay() {
  const annotations = useReview((s) => s.annotations);
  const markersHidden = useReview((s) => s.markersHidden);
  const filters = useReview((s) => s.filters);
  const view = useViewState((s) => s.view);

  if (!view || markersHidden || annotations.length === 0) return null;

  const pins = annotations.filter(
    (a) =>
      (a.replyTo === null || a.replyTo === undefined) &&
      filters.categories[a.category] &&
      (!a.resolved || filters.showResolved),
  );
  if (pins.length === 0) return null;

  return (
    <>
      {view.paneRects.map((rect, pane) => {
        if (view.paneSettings[pane]?.paneMode === "UvMap") return null;
        return (
          <div
            key={pane}
            className="review-pane-clip"
            style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }}
          >
            {pins.map((a) => (
              <Pin key={a.id} a={a} pane={pane} replies={repliesOf(annotations, a.id)} />
            ))}
          </div>
        );
      })}
    </>
  );
}
