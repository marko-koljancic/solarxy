import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App";
import { ErrorBoundary } from "./components/ErrorBoundary";
import { UnsupportedBrowser } from "./components/UnsupportedBrowser";
import { initTelemetry, installSmokeHooks } from "./telemetry";
// dockview's base CSS first; styles.css then maps every --dv-* colour onto the
// token theme, so the dock re-themes with the app and no dockview preset ships.
import "dockview-react/dist/styles/dockview.css";
import "./styles/tokens.css";
import "./styles.css";

// First, so that a crash during boot is still reported.
initTelemetry();
installSmokeHooks();

const root = document.getElementById("root");

if (root) {
  // The WebGPU gate. Checked HERE, before <App/> mounts, because the engine (and
  // therefore the 3.4 MB wasm) is only fetched from inside the app tree. A
  // browser that cannot run Solarxy is told so without first downloading a
  // renderer it could never use.
  //
  // The mere presence of `navigator.gpu` is the right test. Actually requesting
  // an adapter is async and can fail for reasons that are NOT "unsupported" (a
  // busy GPU, a headless VM, a driver hiccup); treating those as unsupported
  // would turn people away from an app that would have worked on a retry. A real
  // failure to acquire a device later surfaces through the existing boot-error
  // path instead.
  const supported = typeof navigator !== "undefined" && "gpu" in navigator;

  createRoot(root).render(
    <StrictMode>
      <ErrorBoundary>{supported ? <App /> : <UnsupportedBrowser />}</ErrorBoundary>
    </StrictMode>,
  );
}
