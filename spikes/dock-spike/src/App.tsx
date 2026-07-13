// The dock-spike harness (Phase 10 W1 gate).
//
// Option A (the hypothesis): the WebGPU canvas is a module-level DOM node that
// React never owns. The viewport panel renders an empty host div and appends the
// canvas into it. Every dockview gesture -- drag, float, tab-move, maximize,
// fromJSON -- re-parents the node instead of recreating it.
//
// Option B (the ratified fallback, section 9): the canvas lives OUTSIDE dockview
// and is absolutely positioned onto a rect the panel host publishes.
//
// Switch with ?mode=b. The status bar reports the numbers that decide it.

// v7 packaging (settled by this spike): `dockview` re-exports `dockview-core`
// (vanilla) only; the React bindings live in `dockview-react`, which itself
// re-exports all of `dockview`. So `dockview-react` is the single import, and
// `dockview` need not be a direct dependency at all.
import {
  DockviewReact,
  type DockviewApi,
  type DockviewReadyEvent,
  type IDockviewPanelProps,
} from "dockview-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { attachCanvas, bootSurface, canvasEl, renderOnce, surfaceStats } from "./surface";

// Spike instrumentation: the harness is driven from the console/automation, not
// by clicking, because Chrome freezes rAF AND paint in a hidden tab, so a
// screenshot of a background tab reports stale numbers. Forcing a frame after
// each gesture answers the real question deterministically.
declare global {
  interface Window {
    __spike: {
      api: DockviewApi | null;
      stats: typeof surfaceStats;
      render: typeof renderOnce;
      canvas: HTMLCanvasElement;
    };
  }
}

const LAYOUT_KEY = "dock-spike.layout";
const MODE: "a" | "b" = new URLSearchParams(location.search).get("mode") === "b" ? "b" : "a";

// ---------------------------------------------------------------- option A

/** The viewport panel under option A: an empty host that adopts the one canvas. */
function ViewportPanelA(_props: IDockviewPanelProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (hostRef.current) attachCanvas(hostRef.current);
  });
  return <div className="canvas-host" id="canvas-host" ref={hostRef} />;
}

// ---------------------------------------------------------------- option B

/** Under option B the panel publishes its rect and stays empty; the canvas is a
 * sibling of the dock, absolutely positioned to match. */
function ViewportPanelB(_props: IDockviewPanelProps) {
  const hostRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const host = hostRef.current;
    if (!host) return;
    const publish = () => {
      const r = host.getBoundingClientRect();
      canvasEl.style.position = "absolute";
      canvasEl.style.left = `${r.left}px`;
      canvasEl.style.top = `${r.top}px`;
      canvasEl.style.width = `${r.width}px`;
      canvasEl.style.height = `${r.height}px`;
    };
    publish();
    const ro = new ResizeObserver(publish);
    ro.observe(host);
    // Layout changes move the host without resizing it (a tab drag, a maximize),
    // so the rect must also be re-published on scroll/resize of the window.
    window.addEventListener("resize", publish);
    const raf = setInterval(publish, 16); // brute force: measures rect-sync lag
    return () => {
      ro.disconnect();
      window.removeEventListener("resize", publish);
      clearInterval(raf);
    };
  }, []);
  return <div className="canvas-host transparent" id="canvas-host" ref={hostRef} />;
}

function Filler({ api }: IDockviewPanelProps) {
  return (
    <div className="filler">
      <p>
        <b>{api.title}</b>
      </p>
      <p>Drag my tab. Float me (drag out while holding the modifier). Tab me onto another group.</p>
    </div>
  );
}

const components = {
  viewport: MODE === "a" ? ViewportPanelA : ViewportPanelB,
  nodes: Filler,
  properties: Filler,
  review: Filler,
};

function defaultLayout(api: DockviewApi): void {
  const viewport = api.addPanel({ id: "viewport", component: "viewport", title: "Viewport" });
  api.addPanel({
    id: "nodes",
    component: "nodes",
    title: "Nodes",
    position: { direction: "right", referencePanel: viewport },
  });
  api.addPanel({
    id: "properties",
    component: "properties",
    title: "Properties",
    position: { direction: "below", referencePanel: "nodes" },
  });
}

export function App() {
  const [api, setApi] = useState<DockviewApi | null>(null);
  const [stats, setStats] = useState(surfaceStats());
  const [log, setLog] = useState<string[]>([]);
  const [boot, setBoot] = useState<string>("booting");

  const note = useCallback((m: string) => {
    setLog((l) => [`${new Date().toISOString().slice(11, 23)} ${m}`, ...l].slice(0, 12));
  }, []);

  useEffect(() => {
    bootSurface().then(
      () => setBoot("ok"),
      (e: unknown) => setBoot(`FAILED: ${e instanceof Error ? e.message : String(e)}`),
    );
    const t = setInterval(() => setStats(surfaceStats()), 250);
    return () => clearInterval(t);
  }, []);

  const onReady = (event: DockviewReadyEvent) => {
    setApi(event.api);
    window.__spike = {
      api: event.api,
      stats: surfaceStats,
      render: renderOnce,
      canvas: canvasEl,
    };
    if (MODE === "b") document.body.appendChild(canvasEl);
    defaultLayout(event.api);

    // The pin: the viewport must not be draggable out of the dock. Cancelling
    // the native drag on its tab is the only hook dockview gives us.
    event.api.onWillDragPanel((e) => {
      if (e.panel.id === "viewport") {
        e.nativeEvent.preventDefault();
        e.nativeEvent.stopPropagation();
        note("viewport drag PREVENTED (pin works)");
      }
    });
    event.api.onDidLayoutChange(() => note("onDidLayoutChange"));
  };

  const act = {
    maximize: () => {
      const p = api?.getPanel("viewport");
      if (p) api?.maximizeGroup(p);
      note(`maximizeGroup(viewport) -> hasMaximizedGroup=${api?.hasMaximizedGroup()}`);
    },
    exitMax: () => {
      api?.exitMaximizedGroup();
      note(`exitMaximizedGroup -> hasMaximizedGroup=${api?.hasMaximizedGroup()}`);
    },
    maxNodes: () => {
      const p = api?.getPanel("nodes");
      if (p) api?.maximizeGroup(p);
      note("maximizeGroup(nodes) [viewport is now hidden]");
    },
    floatNodes: () => {
      const p = api?.getPanel("nodes");
      if (!p) return;
      api?.addFloatingGroup(p, { position: { top: 120, left: 220 }, width: 420, height: 320 });
      note("addFloatingGroup(nodes)");
    },
    addReview: () => {
      if (api?.getPanel("review")) {
        api.removePanel(api.getPanel("review")!);
        note("removePanel(review)");
        return;
      }
      api?.addPanel({ id: "review", component: "review", title: "Review" });
      note("addPanel(review)");
    },
    save: () => {
      if (!api) return;
      // Maximize must not leak into a saved desk: check whether toJSON carries it.
      const wasMax = api.hasMaximizedGroup();
      const json = api.toJSON();
      const carries = JSON.stringify(json).includes("maximized");
      localStorage.setItem(LAYOUT_KEY, JSON.stringify(json));
      note(`toJSON saved (maximized active=${wasMax}, json mentions "maximized"=${carries})`);
    },
    load: () => {
      const raw = localStorage.getItem(LAYOUT_KEY);
      if (!api || !raw) return note("no saved layout");
      const before = surfaceStats();
      try {
        // THE KILLER CASE: fromJSON rebuilds the panel tree. Under option A the
        // canvas must be re-parented, not recreated.
        api.fromJSON(JSON.parse(raw));
        const after = surfaceStats();
        note(
          `fromJSON OK (reparents ${before.reparents} -> ${after.reparents}, configures ${before.configures} -> ${after.configures})`,
        );
      } catch (e) {
        note(`fromJSON THREW: ${e instanceof Error ? e.message : String(e)} (instance may be wedged)`);
      }
    },
    reset: () => {
      if (!api) return;
      api.clear();
      defaultLayout(api);
      note("clear + rebuild default");
    },
  };

  const alive = boot === "ok" && stats.errors === 0 && !stats.deviceLost;

  return (
    <div className="app">
      <div className="bar">
        <b>dock-spike</b>
        <span className="pill">mode {MODE.toUpperCase()}</span>
        <button onClick={act.maximize}>Maximize viewport</button>
        <button onClick={act.maxNodes}>Maximize nodes</button>
        <button onClick={act.exitMax}>Exit maximize</button>
        <button onClick={act.floatNodes}>Float nodes</button>
        <button onClick={act.addReview}>Toggle review</button>
        <button onClick={act.save}>toJSON</button>
        <button onClick={act.load}>fromJSON</button>
        <button onClick={act.reset}>Reset</button>
      </div>

      <div className="dock">
        <DockviewReact components={components} onReady={onReady} className="dockview-theme-abyss" />
      </div>

      <div className={`status ${alive ? "ok" : "bad"}`}>
        <span>
          boot <b>{boot}</b>
        </span>
        <span>
          frames <b>{stats.frames}</b>
        </span>
        <span>
          errors <b>{stats.errors}</b>
        </span>
        <span>
          reparents <b>{stats.reparents}</b>
        </span>
        <span>
          configures <b>{stats.configures}</b>
        </span>
        <span>
          deviceLost <b>{String(stats.deviceLost)}</b>
        </span>
        <span>
          parent <b>{stats.parent}</b>
        </span>
        <span className="err">{stats.lastError ?? ""}</span>
      </div>

      <pre className="log">{log.join("\n")}</pre>
    </div>
  );
}
