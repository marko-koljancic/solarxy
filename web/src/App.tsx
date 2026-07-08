// The application shell: header, the 3D viewport (left), and the
// registry-driven node editor (right) with a subflow breadcrumb.

import { useEffect, useState } from "react";
import { ReactFlowProvider } from "@xyflow/react";
import { NodePalette } from "./components/NodePalette";
import { ParameterPanel } from "./components/ParameterPanel";
import { RecoveryPrompt } from "./components/RecoveryPrompt";
import { Toasts } from "./components/Toasts";
import { Toolbar } from "./components/Toolbar";
import { Viewport } from "./components/Viewport";
import { explicitSave, importDroppedFiles, openScene } from "./engine/session";
import { NodeCanvas } from "./flow/NodeCanvas";
import { useKeyboard } from "./hooks/useKeyboard";
import { descriptorFor } from "./registry/datatypes";
import { selectGraph, useMirror } from "./store/mirror";

function Breadcrumb() {
  const current = useMirror((s) => s.current);
  const registry = useMirror((s) => s.registry);
  const root = useMirror((s) => selectGraph(s, "root"));
  const setCurrent = useMirror((s) => s.setCurrent);

  if (current === "root") return <div className="breadcrumb">Scene</div>;
  const owner = root.nodes.find((n) => n.id === current.subflow);
  const name = owner ? descriptorFor(registry, owner.typeId)?.displayName ?? owner.typeId : "subflow";
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

export function App() {
  const registry = useMirror((s) => s.registry);
  const dirty = useMirror((s) => s.dirty);
  const [dropActive, setDropActive] = useState(false);
  useKeyboard();

  const hasFiles = (e: React.DragEvent) => Array.from(e.dataTransfer.types).includes("Files");
  const onDragOver = (e: React.DragEvent) => {
    if (!hasFiles(e)) return;
    e.preventDefault();
    setDropActive(true);
  };
  const onDrop = (e: React.DragEvent) => {
    if (e.dataTransfer.files.length === 0) return;
    e.preventDefault();
    setDropActive(false);
    void importDroppedFiles(Array.from(e.dataTransfer.files));
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
        <span className="brand">Solarxy</span>
        <span className="sub">Web</span>
        {dirty && <span className="dirty-dot" title="unsaved changes" />}
        <Toolbar />
        <span className="spacer" />
        <button className="tbtn" title="Open a .slxy scene" onClick={() => void openScene()}>
          Open
        </button>
        <button className="tbtn" title="Save scene as .slxy" onClick={() => void explicitSave()}>
          Save
        </button>
        <span className="stat">
          {registry ? `${registry.nodes.length} node types` : "loading engine..."}
        </span>
      </header>
      <div className="app-body">
        <div className="viewport-pane">
          <Viewport />
        </div>
        <div className="node-pane">
          <div className="node-toolbar">
            <Breadcrumb />
            <span className="spacer" />
            <NodePalette />
          </div>
          <div className="node-canvas-host">
            <ReactFlowProvider>
              <NodeCanvas />
            </ReactFlowProvider>
          </div>
          <ParameterPanel />
        </div>
      </div>
      <Toasts />
      <RecoveryPrompt />
    </div>
  );
}
