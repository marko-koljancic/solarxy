// The ui store's pure pieces: the dock-layout loader (Phase 10), the legacy
// arrangement reader that migrates a pre-docking user forward, and the
// connection-style state. (Theme resolution moved to the preferences store; see
// prefs.test.ts. The layout clamps went with the hand-rolled SplitPane: dockview
// owns the shell's geometry now, and the recipe clamp lives in dock/layouts.ts.)

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  EDGE_STYLES,
  loadDockLayout,
  loadEdgeStyle,
  loadFlowView,
  loadLegacyArrangement,
  useUi,
} from "./ui";

/** The node test environment has no localStorage; a Map-backed stub lets the
 * loaders and the setters' persistence writes be asserted directly. */
function stubStorage(seed: Record<string, string> = {}): void {
  const store = new Map<string, string>(Object.entries(seed));
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
  });
}

afterEach(() => vi.unstubAllGlobals());

describe("dock layout persistence", () => {
  it("returns null when nothing is stored", () => {
    stubStorage();
    expect(loadDockLayout()).toBeNull();
  });

  it("round-trips a stored layout", () => {
    const layout = {
      grid: { root: {}, width: 100, height: 100, orientation: "HORIZONTAL" },
      panels: {},
    };
    stubStorage({ "solarxy.ui.dockLayout": JSON.stringify(layout) });
    expect(loadDockLayout()).toEqual(layout);
  });

  it("discards a corrupt blob rather than handing it to fromJSON", () => {
    // fromJSON THROWS on a bad layout and leaves the dock with zero panels
    // (dockview #341, reproduced in the spike), so junk is rejected up front.
    stubStorage({ "solarxy.ui.dockLayout": "{not json" });
    expect(loadDockLayout()).toBeNull();

    stubStorage({ "solarxy.ui.dockLayout": JSON.stringify({ nope: true }) });
    expect(loadDockLayout()).toBeNull();
  });
});

describe("legacy arrangement migration", () => {
  it("returns null for a user who never had the pre-docking shell", () => {
    stubStorage();
    expect(loadLegacyArrangement()).toBeNull();
  });

  it("reads the retired keys so a returning user's arrangement is preserved", () => {
    stubStorage({
      "solarxy.ui.arrangement": JSON.stringify({
        viewportSide: "right",
        propertiesDock: "right",
      }),
      "solarxy.ui.splitPct": "63",
    });
    expect(loadLegacyArrangement()).toEqual({
      viewportSide: "right",
      propertiesDock: "right",
      splitPct: 63,
    });
  });

  it("survives a corrupt legacy blob", () => {
    stubStorage({ "solarxy.ui.arrangement": "{nope" });
    expect(loadLegacyArrangement()).toBeNull();
  });
});

describe("connection style", () => {
  beforeEach(() => stubStorage());

  it("falls back to bezier on a missing or invalid persisted value", () => {
    expect(loadEdgeStyle()).toBe("bezier");
    localStorage.setItem("solarxy.ui.edgeStyle", "zigzag");
    expect(loadEdgeStyle()).toBe("bezier");
    localStorage.setItem("solarxy.ui.edgeStyle", "smoothStep");
    expect(loadEdgeStyle()).toBe("smoothStep");
  });

  it("cycles through all four styles in order and wraps, persisting each", () => {
    useUi.getState().setEdgeStyle("bezier");
    const seen = [useUi.getState().edgeStyle];
    for (let i = 0; i < EDGE_STYLES.length; i += 1) {
      useUi.getState().cycleEdgeStyle();
      seen.push(useUi.getState().edgeStyle);
    }
    expect(seen).toEqual(["bezier", "straight", "simpleBezier", "smoothStep", "bezier"]);
    expect(localStorage.getItem("solarxy.ui.edgeStyle")).toBe("bezier");
  });
});

describe("canvas chrome: snap to grid (D-24)", () => {
  beforeEach(() => stubStorage());

  it("defaults off and persists into the flow-chrome blob when toggled", () => {
    useUi.setState({ snapToGrid: false });
    useUi.getState().toggleFlowChrome("snapToGrid");
    expect(useUi.getState().snapToGrid).toBe(true);
    const blob = JSON.parse(localStorage.getItem("solarxy.ui.flowChrome") ?? "{}") as {
      snapToGrid?: boolean;
    };
    expect(blob.snapToGrid).toBe(true);
  });
});

describe("flow view persistence (D-24)", () => {
  beforeEach(() => stubStorage());

  it("returns an empty record when nothing is stored or the blob is corrupt", () => {
    expect(loadFlowView()).toEqual({});
    localStorage.setItem("solarxy.ui.flowView", "{nope");
    expect(loadFlowView()).toEqual({});
  });

  it("keeps only well-formed graph/list entries", () => {
    localStorage.setItem(
      "solarxy.ui.flowView",
      JSON.stringify({ root: "list", "sub:3": "graph", "sub:4": "sideways", junk: 7 }),
    );
    expect(loadFlowView()).toEqual({ root: "list", "sub:3": "graph" });
  });

  it("setFlowView writes the choice through to localStorage", () => {
    useUi.setState({ flowView: {} });
    useUi.getState().setFlowView("root", "list");
    expect(JSON.parse(localStorage.getItem("solarxy.ui.flowView") ?? "{}")).toEqual({
      root: "list",
    });
  });
});
