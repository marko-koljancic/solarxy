// The application shell (Minimystix layout, phase-6 design adoption): a
// menu-bar header, the 3D viewport (left) and the registry-driven node
// editor (right) behind a draggable 20-80 percent split, with the
// parameter panel in a collapsible bottom properties drawer. The viewport
// can maximize to the full window from the View menu.

import { useEffect, useState } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { PropertiesDrawer } from "./components/layout/PropertiesDrawer";
import { PropertiesSide } from "./components/layout/PropertiesSide";
import { SplitPane } from "./components/layout/SplitPane";
import { MenuBar } from "./components/menu/MenuBar";
import { NodesMenu } from "./components/menu/NodesMenu";
import { NodePalette } from "./components/NodePalette";
import { ParameterPanel } from "./components/ParameterPanel";
import { RecoveryPrompt } from "./components/RecoveryPrompt";
import { PreferencesModal } from "./components/preferences/PreferencesModal";
import { ScreenshotModal } from "./components/ScreenshotModal";
import { ShortcutsModal } from "./components/ShortcutsModal";
import { Toasts } from "./components/Toasts";
import { Toolbar } from "./components/Toolbar";
import { Viewport } from "./components/Viewport";
import { NodeInfoModal } from "./components/NodeInfoModal";
import { importDroppedFiles } from "./engine/session";
import { ctxKey } from "./engine/types";
import { collectDroppedFiles } from "./persistence/dropEntries";
import { FlowListView } from "./flow/FlowListView";
import { NodeCanvas } from "./flow/NodeCanvas";
import { RadialMenu } from "./flow/RadialMenu";
import { useKeyboard } from "./hooks/useKeyboard";
import { descriptorFor } from "./registry/datatypes";
import { nodeLabel } from "./flow/nodeLabel";
import { selectGraph, useMirror } from "./store/mirror";
import { useUi } from "./store/ui";

function Breadcrumb() {
  const current = useMirror((s) => s.current);
  const registry = useMirror((s) => s.registry);
  const root = useMirror((s) => selectGraph(s, "root"));
  const setCurrent = useMirror((s) => s.setCurrent);

  if (current === "root") return <div className="breadcrumb">Scene</div>;
  const owner = root.nodes.find((n) => n.id === current.subflow);
  const name = owner ? nodeLabel(owner, descriptorFor(registry, owner.typeId)) : "subflow";
  return (
    <div className="breadcrumb">
      <button className="crumb-link" onClick={() => setCurrent("root")}>
        Scene
      </button>
      <span className="crumb-sep">›</span>
      <span>{name}</span>
    </div>
  );
}

/** The selected node's display label (its `name` param when renamed, else
 * the type display name), for the properties drawer header. */
function useSelectedNodeName(): string {
  const registry = useMirror((s) => s.registry);
  const graph = useMirror((s) => selectGraph(s, s.current));
  const selected = graph.nodes.find((n) => n.id === graph.selection[0]);
  if (!selected) return "";
  return nodeLabel(selected, descriptorFor(registry, selected.typeId));
}

export function App() {
  const registry = useMirror((s) => s.registry);
  const dirty = useMirror((s) => s.dirty);
  const current = useMirror((s) => s.current);
  const selectedName = useSelectedNodeName();
  const shortcutsOpen = useUi((s) => s.shortcutsOpen);
  const prefsOpen = useUi((s) => s.prefsOpen);
  const screenshotOpen = useUi((s) => s.screenshotOpen);
  const bootError = useUi((s) => s.bootError);
  const flowView = useUi((s) => s.flowView[ctxKey(current)] ?? "graph");
  const viewportSide = useUi((s) => s.viewportSide);
  const propertiesDock = useUi((s) => s.propertiesDock);
  const [dropActive, setDropActive] = useState(false);
  useKeyboard();

  const hasFiles = (e: React.DragEvent) => Array.from(e.dataTransfer.types).includes("Files");
  const onDragOver = (e: React.DragEvent) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    setDropActive(true);
  };
  const onDrop = (e: React.DragEvent) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    setDropActive(false);
    // Folder drops expand recursively (FlightHelmet-style layouts); the
    // DataTransfer must be walked before the event is recycled, so collect
    // first and import when the walk resolves.
    const dt = e.dataTransfer;
    void collectDroppedFiles(dt).then((files) => {
      if (files.length > 0) void importDroppedFiles(files);
    });
  };

  // Warn before leaving with unsaved changes (the autosave still runs).
  useEffect(() => {
    const guard = (e: BeforeUnloadEvent) => {
      if (useMirror.getState().dirty) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", guard);
    return () => window.removeEventListener("beforeunload", guard);
  }, []);

  return (
    <div
      className="app"
      onDragOver={onDragOver}
      onDragLeave={() => setDropActive(false)}
      onDrop={onDrop}
    >
      {dropActive && (
        <div className="drop-overlay">Drop a model to import (.obj .gltf .glb .stl .ply)</div>
      )}
      <header className="app-header">
        <MenuBar />
        {dirty && <span className="dirty-dot" title="unsaved changes" />}
        <span className="spacer" />
        <Toolbar />
      </header>
      <div className="app-body">
        <SplitPane
          side={viewportSide}
          viewport={
            <div className="viewport-pane">
              <Viewport />
            </div>
          }
          panel={
            <div className="node-pane">
              <div className="node-toolbar">
                <Breadcrumb />
                <NodesMenu />
                <button
                  className="btn view-toggle"
                  title={flowView === "graph" ? "Switch to list view" : "Switch to graph view"}
                  onClick={() =>
                    useUi
                      .getState()
                      .setFlowView(ctxKey(current), flowView === "graph" ? "list" : "graph")
                  }
                >
                  {flowView === "graph" ? "List" : "Graph"}
                </button>
                <span className="spacer" />
                <NodePalette />
              </div>
              <div className="node-canvas-host">
                {flowView === "list" ? (
                  <FlowListView />
                ) : (
                  <ReactFlowProvider>
                    <NodeCanvas />
                  </ReactFlowProvider>
                )}
              </div>
              {propertiesDock === "bottom" && (
                <PropertiesDrawer title={selectedName}>
                  <ParameterPanel />
                </PropertiesDrawer>
              )}
            </div>
          }
        />
        {propertiesDock === "right" && (
          <PropertiesSide title={selectedName}>
            <ParameterPanel />
          </PropertiesSide>
        )}
      </div>
      <Toasts />
      <RadialMenu />
      <NodeInfoModal />
      <RecoveryPrompt />
      {(bootError !== null || !registry) && (
        <div className="boot-overlay">
          <div className="boot-card">
            <div className="boot-title">Solarxy Web</div>
            {bootError === null ? (
              <>
                <div className="boot-spinner" />
                <div className="boot-note">Initializing WebGPU...</div>
              </>
            ) : (
              <div className="boot-note boot-error">
                This browser could not start WebGPU: {bootError}. Solarxy Web needs a
                WebGPU-capable browser (current Chrome or Edge).
              </div>
            )}
          </div>
        </div>
      )}
      {shortcutsOpen && <ShortcutsModal onClose={() => useUi.getState().setShortcutsOpen(false)} />}
      {prefsOpen && <PreferencesModal onClose={() => useUi.getState().setPrefsOpen(false)} />}
      {screenshotOpen && <ScreenshotModal onClose={() => useUi.getState().setScreenshotOpen(false)} />}
    </div>
  );
}
