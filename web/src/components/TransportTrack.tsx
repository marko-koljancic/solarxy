// The frame scrubber: a ticked track above the transport's buttons and
// fields.
//
// Discrete on purpose. The clock advances a whole frame at a time (fixed
// step, so a frame is reproducible), and a scrubber that let you sit between
// frames would promise something the engine does not offer. Every pointer
// position therefore rounds to a frame before it is dispatched.
//
// A pure mirror consumer like the rest of the bar: dragging dispatches
// `setFrame`, which records no undo step, and the playhead moves only once
// the frame has come back through the event batch.

import { memo, useCallback, useEffect, useRef, useState } from "react";
import { dispatch } from "../engine/session";
import { useMirror } from "../store/mirror";

/** Minimum on-screen gap between ticks, in CSS px. Below this the ticks
 * stop reading as a scale and start reading as a texture. */
const MIN_TICK_PX = 7;

/** Approximate width a tick label needs, in CSS px, including its air. */
const LABEL_PX = 44;

/** The tick ladder, in frames. Standard editorial increments: a scrubber
 * whose labels read 1 / 25 / 50 is legible in a way one reading 1 / 37 / 74
 * is not, so the step is chosen from this list rather than computed. */
const TICK_STEPS = [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000] as const;

/** The smallest ladder step keeping ticks at least `MIN_TICK_PX` apart.
 * Falls back to the coarsest rung rather than returning nothing, so a
 * pathologically narrow pane still draws something sane. */
export function tickStep(frameCount: number, widthPx: number): number {
  if (!(widthPx > 0) || frameCount <= 0) return TICK_STEPS[TICK_STEPS.length - 1];
  for (const step of TICK_STEPS) {
    if ((step / frameCount) * widthPx >= MIN_TICK_PX) return step;
  }
  return TICK_STEPS[TICK_STEPS.length - 1];
}

/** Every tick frame in `[start, end]` on the given step, anchored so ticks
 * land on multiples of the step rather than on the range start: a range
 * beginning at 7 should still tick at 10, 20, 30. Both ends are always
 * included, because those are the two frames worth naming. */
export function tickFrames(start: number, end: number, step: number): number[] {
  if (end <= start) return [start];
  const out: number[] = [start];
  for (let f = Math.ceil(start / step) * step; f < end; f += step) {
    if (f > start) out.push(f);
  }
  out.push(end);
  return out;
}

/** How many ticks apart labels may be drawn without colliding. */
export function labelStride(tickGapPx: number): number {
  if (!(tickGapPx > 0)) return 1;
  return Math.max(1, Math.ceil(LABEL_PX / tickGapPx));
}

/** The frame under a pointer at `x` within a track of `width`, clamped to
 * the range and rounded to a whole frame. Pure, so the mapping is testable
 * without a DOM. */
export function frameAtX(x: number, width: number, start: number, end: number): number {
  if (!(width > 0) || end <= start) return start;
  const t = Math.min(Math.max(x / width, 0), 1);
  return Math.round(start + t * (end - start));
}

/** A frame's offset along the track, 0 to 1. */
function frameToFraction(frame: number, start: number, end: number): number {
  if (end <= start) return 0;
  return Math.min(Math.max((frame - start) / (end - start), 0), 1);
}

/** The element's width in CSS px, remeasured whenever it resizes (the
 * transport spans the viewport region, which moves with every dock drag). */
function useElementWidth(ref: React.RefObject<HTMLElement | null>): number {
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    setWidth(el.getBoundingClientRect().width);
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width));
    ro.observe(el);
    return () => ro.disconnect();
  }, [ref]);
  return width;
}

/** The tick scale, memoized away from the playhead.
 *
 * The playhead moves every frame during playback; the ticks move only when
 * the range or the pane width changes. Without this split React would
 * re-diff every tick sixty times a second to redraw one 2px line, which on
 * a wide pane is a couple of hundred elements per frame for nothing. */
const Ticks = memo(function Ticks({
  start,
  end,
  width,
}: {
  start: number;
  end: number;
  width: number;
}) {
  const span = Math.max(end - start, 1);
  const step = tickStep(end - start, width);
  const stride = labelStride((step / span) * width);
  return (
    <>
      {tickFrames(start, end, step).map((f, i) => {
        const labelled = i % stride === 0 || f === end;
        return (
          <span
            key={f}
            className={`transport-tick${labelled ? " major" : ""}`}
            style={{ left: `${frameToFraction(f, start, end) * 100}%` }}
            aria-hidden
          >
            {labelled && <span className="transport-tick-label">{f}</span>}
          </span>
        );
      })}
    </>
  );
});

export function TransportTrack() {
  const frame = useMirror((s) => s.frame);
  const runtime = useMirror((s) => s.runtime);
  const trackRef = useRef<HTMLDivElement>(null);
  const width = useElementWidth(trackRef);

  const start = runtime.frameStart;
  const end = runtime.frameEnd;

  const seekTo = useCallback(
    (clientX: number) => {
      const rect = trackRef.current?.getBoundingClientRect();
      if (!rect) return;
      dispatch({ type: "setFrame", frame: frameAtX(clientX - rect.left, rect.width, start, end) });
    },
    [start, end],
  );

  return (
    <div
      ref={trackRef}
      className="transport-track"
      role="slider"
      tabIndex={0}
      aria-label="Frame"
      aria-valuemin={start}
      aria-valuemax={end}
      aria-valuenow={frame}
      onPointerDown={(e) => {
        // Capture on the track so a drag that leaves the strip keeps
        // scrubbing instead of stopping dead at the edge.
        e.currentTarget.setPointerCapture(e.pointerId);
        seekTo(e.clientX);
      }}
      onPointerMove={(e) => {
        if (e.currentTarget.hasPointerCapture(e.pointerId)) seekTo(e.clientX);
      }}
      onPointerUp={(e) => e.currentTarget.releasePointerCapture(e.pointerId)}
      onKeyDown={(e) => {
        // Scoped to the focused track. The global comma / period bindings do
        // the same job everywhere else, and Home is already "stop and
        // rewind", so neither is re-bound here.
        const delta = e.key === "ArrowLeft" ? -1 : e.key === "ArrowRight" ? 1 : 0;
        if (delta === 0) return;
        e.preventDefault();
        e.stopPropagation();
        dispatch({ type: "stepFrame", delta: e.shiftKey ? delta * 10 : delta });
      }}
    >
      <Ticks start={start} end={end} width={width} />
      <span
        className="transport-playhead"
        style={{ left: `${frameToFraction(frame, start, end) * 100}%` }}
        aria-hidden
      />
    </div>
  );
}
