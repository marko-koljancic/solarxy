// The transport half of the mirror: playback and the persisted clock arrive
// as ordinary engine events, so the bar is a pure mirror consumer and an undo
// of a range edit moves it exactly as a direct edit does.

import { beforeEach, describe, expect, it } from "vitest";
import type { EventBatch, GraphMirror, RuntimeSettings } from "../engine/types";
import { DEFAULT_RUNTIME, useMirror } from "./mirror";

function emptyGraph(): GraphMirror {
  return { nodes: [], edges: [], activeOutput: null, selection: [] };
}

beforeEach(() => {
  useMirror.setState({
    revision: 0,
    contexts: { root: emptyGraph() },
    cook: {},
    cookMode: "auto",
    playing: false,
    frame: DEFAULT_RUNTIME.frameStart,
    runtime: DEFAULT_RUNTIME,
  });
});

function apply(events: EventBatch["events"], revision = 1): void {
  useMirror.getState().applyBatch({ revision, events });
}

describe("transport mirror", () => {
  it("starts stopped at the range start", () => {
    const s = useMirror.getState();
    expect(s.playing).toBe(false);
    expect(s.frame).toBe(1);
    expect(s.runtime.fps).toBe(24);
    expect(s.runtime.loopMode).toBe("loop");
  });

  it("follows playback and frame events", () => {
    apply([
      { type: "playbackChanged", playing: true },
      { type: "frameChanged", frame: 7 },
    ]);
    expect(useMirror.getState().playing).toBe(true);
    expect(useMirror.getState().frame).toBe(7);
  });

  it("follows the persisted clock settings", () => {
    const settings: RuntimeSettings = {
      fps: 30,
      frameStart: 5,
      frameEnd: 60,
      loopMode: "pingPong",
      autoplay: true,
    };
    apply([{ type: "runtimeSettingsChanged", settings }]);
    expect(useMirror.getState().runtime).toEqual(settings);
  });

  it("follows an undo, because an undo emits the same event", () => {
    // The reason the bar reads its state from events rather than holding its
    // own: undoing a range edit has to move the fields.
    apply([
      {
        type: "runtimeSettingsChanged",
        settings: { ...DEFAULT_RUNTIME, frameStart: 10, frameEnd: 20 },
      },
    ]);
    expect(useMirror.getState().runtime.frameEnd).toBe(20);

    apply([{ type: "runtimeSettingsChanged", settings: DEFAULT_RUNTIME }], 2);
    expect(useMirror.getState().runtime.frameEnd).toBe(240);
  });

  it("clears playing when the clock stops itself at the end of a once range", () => {
    apply([{ type: "playbackChanged", playing: true }]);
    apply([{ type: "playbackChanged", playing: false }], 2);
    expect(useMirror.getState().playing).toBe(false);
  });

  it("leaves transport untouched by unrelated events", () => {
    apply([{ type: "cookModeChanged", mode: "manual" }]);
    const s = useMirror.getState();
    expect(s.cookMode).toBe("manual");
    expect(s.playing).toBe(false);
    expect(s.frame).toBe(1);
  });
});
