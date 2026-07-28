// The mirror store: React's read-only view of the Rust-owned document.
//
// It is fed exclusively by `EventBatch`es (from dispatch and from the frame
// cook loop) and never mutates document state directly. The monotonic
// `revision` detects desync: a command batch advances it by one and a cook
// batch leaves it unchanged, so a jump of more than one (or a
// `documentReplaced` event) means the mirror missed something and must
// rebuild from a full `snapshot()`. Geometry never appears here.

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import {
  ctxKey,
  type CookMode,
  type RuntimeSettings,
  type CookStatus,
  type DocumentSnapshot,
  type EdgeMirror,
  type EngineEvent,
  type EventBatch,
  type GraphContext,
  type GraphMirror,
  type RegistrySnapshot,
  type ValidationIssue,
} from "../engine/types";

export interface NodeCook {
  status?: CookStatus;
  points?: number;
  prims?: number;
  meshes?: number;
  /** `[width, height]` of the default image output, for image nodes. */
  image?: [number, number] | null;
  /** Validation badge counts (validate node, import load validation).
   * Zero counts mean clean or cleared; the badge renders only when > 0. */
  validation?: { errors: number; warnings: number };
}

/** The full issue list behind a node's validation badge (capped
 * engine-side at 2000 rows; `truncated` flags the cut). */
export interface ValidationReportData {
  errors: number;
  warnings: number;
  truncated: boolean;
  issues: ValidationIssue[];
}

interface MirrorState {
  registry: RegistrySnapshot | null;
  revision: number;
  /** Per-context graphs, keyed by `ctxKey`. */
  contexts: Record<string, GraphMirror>;
  /** Per-node cook status + stats. */
  cook: Record<number, NodeCook>;
  /** Per-node validation reports (the report panel's data). */
  reports: Record<number, ValidationReportData>;
  cookMode: CookMode;
  /** Transport: whether the scene clock is running. Session state. */
  playing: boolean;
  /** Transport: the current frame. Session state. */
  frame: number;
  /** The persisted half of the clock (fps, range, loop, autoplay). */
  runtime: RuntimeSettings;
  /** The graph the node canvas is currently showing. */
  current: GraphContext;
  /** Node ids that are stale (dirty), for manual-mode badges + header count. */
  stale: number[];
  /** Unsaved changes since the last explicit save (drives the dirty dot +
   * beforeunload guard). Autosave does not clear it. */
  dirty: boolean;

  setRegistry: (reg: RegistrySnapshot) => void;
  setCurrent: (ctx: GraphContext) => void;
  setStale: (ids: number[]) => void;
  setDirty: (dirty: boolean) => void;
  /** Applies a batch; returns whether a full resnapshot is required. */
  applyBatch: (batch: EventBatch) => boolean;
  /** Rebuilds the whole mirror from a fresh snapshot. */
  replaceFromSnapshot: (snap: DocumentSnapshot, revision: number) => void;
}

function emptyGraph(): GraphMirror {
  return { nodes: [], edges: [], activeOutput: null, selection: [] };
}

/** Ensures a context exists in the map and returns it. */
function graphFor(contexts: Record<string, GraphMirror>, ctx: GraphContext): GraphMirror {
  const key = ctxKey(ctx);
  if (!contexts[key]) contexts[key] = emptyGraph();
  return contexts[key];
}

/** The session-state writers `applyEvent` needs, so it can stay free of the
 * whole store while still reporting transport and cook-mode changes. */
interface SessionSetters {
  setCookMode: (m: CookMode) => void;
  setPlaying: (p: boolean) => void;
  setFrame: (f: number) => void;
  setRuntime: (r: RuntimeSettings) => void;
}

/** Mirrors `solarxy_graph::runtime`'s defaults. A scene that has never
 * touched the transport still has to show something coherent. */
export const DEFAULT_RUNTIME: RuntimeSettings = Object.freeze({
  fps: 24,
  frameStart: 1,
  frameEnd: 240,
  loopMode: "loop",
  autoplay: false,
});

/** Applies one event to the draft state. */
function applyEvent(
  contexts: Record<string, GraphMirror>,
  cook: Record<number, NodeCook>,
  reports: Record<number, ValidationReportData>,
  ev: EngineEvent,
  session: SessionSetters,
): void {
  switch (ev.type) {
    case "nodeAdded": {
      const g = graphFor(contexts, ev.ctx);
      if (!g.nodes.some((n) => n.id === ev.node.id)) g.nodes.push(ev.node);
      break;
    }
    case "nodeRemoved": {
      const g = graphFor(contexts, ev.ctx);
      g.nodes = g.nodes.filter((n) => n.id !== ev.id);
      g.edges = g.edges.filter((e) => e.from !== ev.id && e.to !== ev.id);
      if (g.activeOutput === ev.id) g.activeOutput = null;
      break;
    }
    case "edgeAdded": {
      const g = graphFor(contexts, ev.ctx);
      if (!g.edges.some((e) => e.id === ev.edge.id)) g.edges.push(ev.edge as EdgeMirror);
      break;
    }
    case "edgeRemoved": {
      const g = graphFor(contexts, ev.ctx);
      g.edges = g.edges.filter((e) => e.id !== ev.id);
      break;
    }
    case "paramChanged": {
      const g = graphFor(contexts, ev.ctx);
      const n = g.nodes.find((n) => n.id === ev.node);
      if (n) n.params[ev.key] = ev.value;
      break;
    }
    case "nodesMoved": {
      const g = graphFor(contexts, ev.ctx);
      for (const [id, pos] of ev.moves) {
        const n = g.nodes.find((n) => n.id === id);
        if (n) n.position = pos;
      }
      break;
    }
    case "activeOutputChanged": {
      graphFor(contexts, ev.ctx).activeOutput = ev.node;
      break;
    }
    case "selectionChanged": {
      graphFor(contexts, ev.ctx).selection = ev.ids;
      break;
    }
    case "bypassChanged": {
      const g = graphFor(contexts, ev.ctx);
      const n = g.nodes.find((n) => n.id === ev.node);
      if (n) n.bypassed = ev.bypassed;
      break;
    }
    case "cookStatus": {
      (cook[ev.node] ??= {}).status = ev.status;
      break;
    }
    case "nodeStats": {
      const c = (cook[ev.node] ??= {});
      c.points = ev.points;
      c.prims = ev.prims;
      c.meshes = ev.meshes;
      c.image = ev.image;
      break;
    }
    case "validationSummary": {
      (cook[ev.node] ??= {}).validation = { errors: ev.errors, warnings: ev.warnings };
      // A zeroed summary is also the clear signal (bypass, lost input);
      // drop the stale report so the panel section disappears. A clean
      // recook re-emits an empty report right after.
      if (ev.errors === 0 && ev.warnings === 0) delete reports[ev.node];
      break;
    }
    case "validationReport": {
      reports[ev.node] = {
        errors: ev.errors,
        warnings: ev.warnings,
        truncated: ev.truncated,
        issues: ev.issues,
      };
      break;
    }
    case "cookModeChanged": {
      session.setCookMode(ev.mode);
      break;
    }
    case "playbackChanged": {
      session.setPlaying(ev.playing);
      break;
    }
    case "frameChanged": {
      session.setFrame(ev.frame);
      break;
    }
    case "runtimeSettingsChanged": {
      session.setRuntime(ev.settings);
      break;
    }
    // variadicReordered, reviewChanged, documentReplaced: handled elsewhere
    // (documentReplaced triggers resnapshot; the others are no-ops here).
    default:
      break;
  }
}

export const useMirror = create<MirrorState>()(
  immer((set) => ({
    registry: null,
    revision: 0,
    contexts: { root: emptyGraph() },
    cook: {},
    reports: {},
    cookMode: "auto",
    playing: false,
    frame: DEFAULT_RUNTIME.frameStart,
    runtime: DEFAULT_RUNTIME,
    current: "root",
    stale: [],
    dirty: false,

    setRegistry: (reg) =>
      set((s) => {
        s.registry = reg;
      }),

    setDirty: (dirty) =>
      set((s) => {
        s.dirty = dirty;
      }),

    setCurrent: (ctx) =>
      set((s) => {
        s.current = ctx;
      }),

    setStale: (ids) =>
      set((s) => {
        s.stale = ids;
      }),

    applyBatch: (batch) => {
      // Desync: a gap of more than one revision, or an explicit replace.
      let needsResnapshot = batch.events.some((e) => e.type === "documentReplaced");
      set((s) => {
        if (batch.revision > s.revision + 1) needsResnapshot = true;
        if (!needsResnapshot) {
          for (const ev of batch.events) {
            applyEvent(s.contexts, s.cook, s.reports, ev, {
              setCookMode: (m) => {
                s.cookMode = m;
              },
              setPlaying: (p) => {
                s.playing = p;
              },
              setFrame: (f) => {
                s.frame = f;
              },
              setRuntime: (r) => {
                s.runtime = r;
              },
            });
          }
        }
        s.revision = Math.max(s.revision, batch.revision);
      });
      return needsResnapshot;
    },

    replaceFromSnapshot: (snap, revision) =>
      set((s) => {
        const contexts: Record<string, GraphMirror> = { root: snap.root };
        for (const [owner, g] of Object.entries(snap.subflows)) {
          contexts[`sub:${owner}`] = g;
        }
        s.contexts = contexts;
        s.revision = revision;
      }),
  })),
);

/** A stable empty graph, so `selectGraph` on an absent context returns the
 * same reference each call (no React re-render loops). */
const EMPTY_GRAPH: GraphMirror = Object.freeze({
  nodes: [],
  edges: [],
  activeOutput: null,
  selection: [],
}) as GraphMirror;

/** Reads one context's graph from the store (a stable empty if absent). */
export function selectGraph(s: MirrorState, ctx: GraphContext): GraphMirror {
  return s.contexts[ctxKey(ctx)] ?? EMPTY_GRAPH;
}
