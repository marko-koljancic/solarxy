// The device gate (item 10, ratified "permissive" variant): Solarxy Web is
// designed for desktop and tablet. Only the smallest phone screens are blocked
// with a friendly MPW-styled message (before any wasm boots); larger coarse-
// pointer screens get one dismissible session warning and proceed. Everything
// else enters untouched.

import { useEffect, useState } from "react";

export type GateLevel = "ok" | "warn" | "blocked";

/** Width below which the full experience genuinely cannot render. */
const BLOCK_WIDTH = 560;
/** Coarse-pointer widths below this get the one-time warning. */
const WARN_WIDTH = 900;
const WARN_DISMISSED_KEY = "solarxy.ui.deviceWarnDismissed";

export function gateLevel(width: number, coarse: boolean): GateLevel {
  if (!coarse) return "ok";
  if (width < BLOCK_WIDTH) return "blocked";
  if (width < WARN_WIDTH) return "warn";
  return "ok";
}

function currentLevel(): GateLevel {
  const coarse = window.matchMedia?.("(pointer: coarse)").matches === true;
  return gateLevel(window.innerWidth, coarse);
}

/** The live gate level; re-evaluates on resize/rotation, so turning a phone
 * sideways can unblock it. */
export function useDeviceGate(): GateLevel {
  const [level, setLevel] = useState<GateLevel>(currentLevel);
  useEffect(() => {
    const onResize = () => setLevel(currentLevel());
    window.addEventListener("resize", onResize);
    window.addEventListener("orientationchange", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
      window.removeEventListener("orientationchange", onResize);
    };
  }, []);
  return level;
}

/** The full-screen block for the smallest phones. */
export function DeviceBlocked() {
  return (
    <div className="device-gate">
      <div className="device-gate-card">
        <div className="device-gate-mark">
          Solarxy<span className="device-gate-dot">.</span>
        </div>
        <h1>Built for bigger screens</h1>
        <p>
          Solarxy Web is a full 3D modeling and inspection studio, and a phone screen cannot
          hold the node graph, the viewport, and the panels at once. Open this page on a desktop
          or tablet for the real thing.
        </p>
        <p className="device-gate-hint">Rotating a larger phone to landscape may be enough.</p>
        <a className="device-gate-link" href="/">
          About Solarxy
        </a>
      </div>
    </div>
  );
}

/** The one-time dismissible warning for small-but-usable screens. */
export function DeviceWarning() {
  const [dismissed, setDismissed] = useState(
    () => sessionStorage.getItem(WARN_DISMISSED_KEY) === "1",
  );
  if (dismissed) return null;
  return (
    <div className="device-gate device-gate-soft">
      <div className="device-gate-card">
        <div className="device-gate-mark">
          Solarxy<span className="device-gate-dot">.</span>
        </div>
        <h1>Small screen ahead</h1>
        <p>
          Solarxy Web works best on a desktop or tablet. You can continue, but expect tight
          panels and a cozy node graph.
        </p>
        <button
          className="btn primary"
          onClick={() => {
            sessionStorage.setItem(WARN_DISMISSED_KEY, "1");
            setDismissed(true);
          }}
        >
          Continue anyway
        </button>
      </div>
    </div>
  );
}
