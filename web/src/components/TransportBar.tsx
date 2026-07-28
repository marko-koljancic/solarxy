// The playbar: a compact scene-clock strip under the viewport.
//
// Named "Playbar" everywhere a user reads it (Houdini's term). The
// component, CSS and preference key keep `transport` for continuity.
//
// Scene-global, not per pane, so it lives below the whole viewport region
// rather than on a pane toolbar: there is one clock, and a control that
// appeared four times in a quad layout would imply four.
//
// A pure mirror consumer. Every button dispatches a Command and the state
// comes back through the event batch, so an undo of a frame-range edit moves
// these fields exactly as a direct edit does.

import { dispatch } from "../engine/session";
import { useMirror } from "../store/mirror";
import type { LoopMode } from "../engine/types";
import { NumberField } from "./inputs/NumberField";
import { Select } from "./Select";
import { TransportTrack } from "./TransportTrack";

/** The range and rate fields deliberately do NOT preview: each commit is an
 * undo step, and a precision drag would otherwise mint one per pointer tick. */
const noPreview = () => {};

const LOOP_LABEL: Record<LoopMode, string> = {
  once: "Once",
  loop: "Loop",
  pingPong: "Ping-pong",
};

export function TransportBar() {
  const playing = useMirror((s) => s.playing);
  const frame = useMirror((s) => s.frame);
  const runtime = useMirror((s) => s.runtime);

  const seconds = runtime.fps > 0 ? (frame / runtime.fps).toFixed(2) : "0.00";

  return (
    <div className="transport-bar" role="group" aria-label="Playback">
      <TransportTrack />
      <div className="transport-row">
      <div className="transport-buttons">
        <button
          type="button"
          className="transport-button"
          title="Stop and rewind to the range start"
          aria-label="Stop"
          onClick={() => dispatch({ type: "stop" })}
        >
          <StopGlyph />
        </button>
        <button
          type="button"
          className="transport-button"
          title="Step back one frame (,)"
          aria-label="Step back"
          onClick={() => dispatch({ type: "stepFrame", delta: -1 })}
        >
          <StepGlyph back />
        </button>
        <button
          type="button"
          className={`transport-button transport-play${playing ? " active" : ""}`}
          title={playing ? "Pause (Space)" : "Play (Space)"}
          aria-label={playing ? "Pause" : "Play"}
          aria-pressed={playing}
          onClick={() => dispatch({ type: playing ? "pause" : "play" })}
        >
          {playing ? <PauseGlyph /> : <PlayGlyph />}
        </button>
        <button
          type="button"
          className="transport-button"
          title="Step forward one frame (.)"
          aria-label="Step forward"
          onClick={() => dispatch({ type: "stepFrame", delta: 1 })}
        >
          <StepGlyph />
        </button>
      </div>

      <label className="transport-field">
        <span>Frame</span>
        <NumberField
          value={frame}
          int
          min={0}
          className="transport-number"
          /* Scrubbing the frame IS meaningful live, and `setFrame` records no
           * undo step, so the preview lane can dispatch it directly. */
          onPreview={(v) => dispatch({ type: "setFrame", frame: Math.round(v) })}
          onCommit={(v) => dispatch({ type: "setFrame", frame: Math.round(v) })}
        />
      </label>

      {/* The seconds readout is the one place `$T` is visible without
        * writing an expression, which is what makes `frame / fps` legible. */}
      <span className="transport-seconds" title="Scene seconds, the value of $T">
        {seconds}s
      </span>

      <div className="transport-spacer" />

      {/* Two fields, two labels: one label naming two inputs leaves a
        * screen reader unable to say which end it is on.
        *
        * `min={0}` on both: a negative frame has no meaning here. `$T` is
        * `frame / fps`, so a negative range runs the clock before time
        * zero, which every expression in the scene then has to defend
        * against. The engine clamps too (`SceneClock::set_range`), so a
        * hand-edited `.slxy` cannot smuggle one in either. */}
      <label className="transport-field">
        <span>Start</span>
        <NumberField
          value={runtime.frameStart}
          int
          min={0}
          className="transport-number"
          onPreview={noPreview}
          onCommit={(v) =>
            dispatch({
              type: "setFrameRange",
              start: Math.round(v),
              end: runtime.frameEnd,
            })
          }
        />
      </label>

      <label className="transport-field">
        <span>End</span>
        <NumberField
          value={runtime.frameEnd}
          int
          min={0}
          className="transport-number"
          onPreview={noPreview}
          onCommit={(v) =>
            dispatch({
              type: "setFrameRange",
              start: runtime.frameStart,
              end: Math.round(v),
            })
          }
        />
      </label>

      <label className="transport-field">
        <span>FPS</span>
        <NumberField
          value={runtime.fps}
          int
          min={1}
          max={240}
          className="transport-number"
          onPreview={noPreview}
          onCommit={(v) => dispatch({ type: "setFps", fps: v })}
        />
      </label>

      {/* The themed dropdown, not the native element: that one draws an OS
        * popup no theme token can reach, and a drift gate enforces it.
        *
        * Portaled, unlike every other Select in the app: this bar is the
        * last child of the viewport pane, so an inline list would open
        * below the window edge, and it is narrow enough that any clipping
        * ancestor swallows it whole. The portal flips it upward. */}
      <div className="transport-field">
        <span>Loop</span>
        <Select<LoopMode>
          portal
          width={110}
          ariaLabel="Loop mode"
          value={runtime.loopMode}
          options={(Object.keys(LOOP_LABEL) as LoopMode[]).map((m) => ({
            value: m,
            label: LOOP_LABEL[m],
          }))}
          onChange={(mode) => dispatch({ type: "setLoopMode", mode })}
        />
      </div>
      </div>
    </div>
  );
}

// Glyphs are inline SVG for the same reason the node glyphs are: no icon
// font to load, and they inherit `currentColor` so the theme drives them.

function PlayGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path d="M4 2.5l9 5.5-9 5.5z" fill="currentColor" />
    </svg>
  );
}

function PauseGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path d="M4 2.5h3.2v11H4z M8.8 2.5H12v11H8.8z" fill="currentColor" />
    </svg>
  );
}

function StopGlyph() {
  return (
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
      <path d="M3.5 3.5h9v9h-9z" fill="currentColor" />
    </svg>
  );
}

function StepGlyph({ back = false }: { back?: boolean }) {
  return (
    <svg
      viewBox="0 0 16 16"
      width="12"
      height="12"
      aria-hidden="true"
      style={back ? { transform: "scaleX(-1)" } : undefined}
    >
      <path d="M4 3l7 5-7 5z M11.5 3H13v10h-1.5z" fill="currentColor" />
    </svg>
  );
}
