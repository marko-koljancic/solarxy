// The application shell: a slim menu-bar header over a dockview dock.
// The four panels (viewport, nodes, properties, review) dock, float, tab and
// maximize freely; the arrangement persists and Desks capture it. The 3D canvas
// is a module-level DOM node the viewport panel adopts (engine/canvas.ts), so no
// dock gesture can ever remount it.

import { useEffect, useState } from "react";
import { DeviceBlocked, DeviceWarning, useDeviceGate } from "./components/DeviceGate";
import { Dock } from "./dock/Dock";
import { MenuBar } from "./components/menu/MenuBar";
import { RecoveryPrompt } from "./components/RecoveryPrompt";
import { PreferencesModal } from "./components/preferences/PreferencesModal";
import { ScreenshotModal } from "./components/ScreenshotModal";
import { TurntableExportModal } from "./components/TurntableExportModal";
import { ShortcutsModal } from "./components/ShortcutsModal";
import { Toasts } from "./components/Toasts";
import { Toolbar } from "./components/Toolbar";
import { NodeInfoModal } from "./components/NodeInfoModal";
import { FloatingProperties } from "./components/FloatingProperties";
import { Tour } from "./components/tour/Tour";
import { MissingSidecarsModal } from "./components/MissingSidecarsModal";
import { bootSession, importDroppedFiles } from "./engine/session";
import { viewportCanvas } from "./engine/canvas";
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
  const turntableOpen = useUi((s) => s.turntableOpen);
  const bootError = useUi((s) => s.bootError);
  const reviewMode = useReview((s) => s.reviewMode);
  const [dropActive, setDropActive] = useState(false);
  const gate = useDeviceGate();
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

  // Boot the engine from the shell, not from the viewport panel.
  //
  // It used to start inside that panel's effect, which quietly made booting
  // conditional on a restored layout still containing the panel. One that did
  // not left the app on the spinner forever with nothing logged, because
  // nothing had failed: boot simply never began. The canvas is a module
  // singleton the panel merely adopts (engine/canvas.ts), so the surface does
  // not belong to the component and starting the engine never needed it.
  //
  // `bootSession` is idempotent, so the viewport panel awaiting the same
  // promise to start its render loop costs nothing.
  useEffect(() => {
    // The smallest phones get the friendly message INSTEAD of the app, and
    // must not pay for a WebGPU/wasm boot. The gate re-evaluates on rotation,
    // hence the dependency: turning a phone sideways can unblock it, and this
    // effect is what starts the engine when it does.
    if (gate === "blocked") return;
    bootSession(viewportCanvas()).catch((err: unknown) => {
      // WebGPU unavailable or wasm init failed; the boot overlay renders the
      // message (the full unsupported-browser page is separate).
      useUi.getState().setBootError(err instanceof Error ? err.message : String(err));
    });
  }, [gate]);

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

  // The smallest phones get the friendly message INSTEAD of the app, before
  // any WebGPU/wasm boot begins (the Dock mounts the viewport which boots).
  if (gate === "blocked") return <DeviceBlocked />;

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
      <FloatingProperties />
      <Tour />
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
      {turntableOpen && (
        <TurntableExportModal onClose={() => useUi.getState().setTurntableOpen(false)} />
      )}
      {gate === "warn" && <DeviceWarning />}
    </div>
  );
}
