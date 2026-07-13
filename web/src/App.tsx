// The application shell: a slim menu-bar header over a dockview dock (Phase 10).
// The four panels (viewport, nodes, properties, review) dock, float, tab and
// maximize freely; the arrangement persists and Desks capture it. The 3D canvas
// is a module-level DOM node the viewport panel adopts (engine/canvas.ts), so no
// dock gesture can ever remount it.

import { useEffect, useState } from "react";
import { Dock } from "./dock/Dock";
import { MenuBar } from "./components/menu/MenuBar";
import { RecoveryPrompt } from "./components/RecoveryPrompt";
import { PreferencesModal } from "./components/preferences/PreferencesModal";
import { ScreenshotModal } from "./components/ScreenshotModal";
import { ShortcutsModal } from "./components/ShortcutsModal";
import { Toasts } from "./components/Toasts";
import { Toolbar } from "./components/Toolbar";
import { NodeInfoModal } from "./components/NodeInfoModal";
import { MissingSidecarsModal } from "./components/MissingSidecarsModal";
import { importDroppedFiles } from "./engine/session";
import { collectDroppedFiles } from "./persistence/dropEntries";
import { useKeyboard } from "./hooks/useKeyboard";
import { useMirror } from "./store/mirror";
import { useReview } from "./store/review";
import { useUi } from "./store/ui";

export function App() {
  const registry = useMirror((s) => s.registry);
  const dirty = useMirror((s) => s.dirty);
  const shortcutsOpen = useUi((s) => s.shortcutsOpen);
  const prefsOpen = useUi((s) => s.prefsOpen);
  const screenshotOpen = useUi((s) => s.screenshotOpen);
  const bootError = useUi((s) => s.bootError);
  const reviewMode = useReview((s) => s.reviewMode);
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
        {reviewMode && (
          <button
            className="review-pill"
            title="Review mode is active; click to exit"
            onClick={() => useReview.getState().setReviewMode(false)}
          >
            Review
          </button>
        )}
        <span className="spacer" />
        <Toolbar />
      </header>
      <div className="app-body">
        <Dock />
      </div>
      <Toasts />
      <NodeInfoModal />
      <MissingSidecarsModal />
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
