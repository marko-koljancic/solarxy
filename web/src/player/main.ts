// The player: a published Solarxy scene, running on any static host.
//
// This is the same wasm the editor runs, in player mode (decision M-10). A
// lean player-only crate would mean a second boot path and a second payload
// gate that could drift from the editor's, for a saving nobody has measured;
// that stays a follow-up with an explicit instruction to measure first.
//
// What the player is NOT: a viewer of `.slxy` as a delivery format. It is
// the engine, cooking the same document, which is why an animated scene
// animates rather than playing back a bake.
//
// Boot order matters. `set_player_mode` goes on BEFORE the scene loads, so
// no frame is ever drawn with a manipulator or a review pin on it.

import { SolarxyClient } from "../engine/client";
import type { PlayerConfig } from "../export/playerConfig";
import { DEFAULT_PLAYER_CONFIG } from "../export/playerConfig";

/// The UV checker the renderer wants at construction.
///
/// A 2x2 placeholder, not the editor's 1K checker: the player has no UV
/// pane, so the real asset would be 147 KB of bundle nobody can ever see.
/// The renderer only requires that it decode.
const PLACEHOLDER_CHECKER_PNG =
  "iVBORw0KGgoAAAANSUhEUgAAAAIAAAACCAYAAABytg0kAAAAFklEQVR42mM4ceLE/4qKiv8MIALEAQBlNAt9iai5SwAAAABJRU5ErkJggg==";

function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i += 1) out[i] = bin.charCodeAt(i);
  return out;
}

function fail(message: string, detail?: unknown): void {
  const el = document.getElementById("player-status");
  if (el) {
    el.textContent = message;
    el.classList.add("error");
  }
  if (detail) console.error(message, detail);
}

function setStatus(message: string | null): void {
  const el = document.getElementById("player-status");
  if (!el) return;
  if (message === null) {
    el.remove();
    return;
  }
  el.textContent = message;
}

async function loadConfig(): Promise<PlayerConfig> {
  try {
    const res = await fetch("./solarxy-player.json");
    if (!res.ok) return DEFAULT_PLAYER_CONFIG;
    return { ...DEFAULT_PLAYER_CONFIG, ...(await res.json()) };
  } catch {
    // A missing config is not an error: every field has a default, and the
    // scene is the payload that actually matters.
    return DEFAULT_PLAYER_CONFIG;
  }
}

async function main(): Promise<void> {
  const canvas = document.getElementById("player-canvas");
  if (!(canvas instanceof HTMLCanvasElement)) {
    fail("The player page is missing its canvas.");
    return;
  }

  if (!("gpu" in navigator)) {
    fail(
      "This browser has no WebGPU. Solarxy scenes need it; try current Chrome, Edge, or Safari 26+.",
    );
    return;
  }

  const config = await loadConfig();
  document.body.style.background = config.background;

  setStatus("Starting the engine...");
  let client: SolarxyClient;
  try {
    client = await SolarxyClient.create(canvas, decodeBase64(PLACEHOLDER_CHECKER_PNG));
  } catch (e) {
    fail("The engine failed to start. The page must be served over HTTP, not opened from a file.", e);
    return;
  }

  // Player mode first: no frame is ever drawn with editing chrome on it.
  client.setPlayerMode(true);

  setStatus("Loading the scene...");
  let sceneBytes: Uint8Array;
  try {
    const res = await fetch(config.scene);
    if (!res.ok) throw new Error(`${res.status} ${res.statusText}`);
    sceneBytes = new Uint8Array(await res.arrayBuffer());
  } catch (e) {
    fail(
      `Could not load ${config.scene}. Serve this folder over HTTP; opening index.html from disk will not work.`,
      e,
    );
    return;
  }

  try {
    client.loadSlxy(sceneBytes);
  } catch (e) {
    fail("The scene file could not be opened. It may have been written by a newer Solarxy.", e);
    return;
  }

  setStatus(null);
  resize(client, canvas);
  window.addEventListener("resize", () => resize(client, canvas));

  // A camera effect, not scene time: it spins whether or not the scene
  // animates, and it is session-only so it never touches the document.
  if (config.turntable) {
    const pane = client.viewState().paneSettings[0];
    if (pane) {
      // The rpm is scene-global and only reachable through the display
      // defaults; both apply flags are false so this changes the speed
      // without repainting the pane's own wireframe or background.
      client.setDisplayDefaults(
        { wireframeWeight: "Light", background: "Gradient", turntableRpm: config.turntableRpm },
        false,
        false,
      );
      client.setPaneSettings(0, { ...pane, turntableActive: true });
    }
  }

  if (config.transport) mountTransport(client);

  // Autoplay is the DOCUMENT's setting, not the export's: the same flag means
  // the same thing whether the scene is published or opened in the editor.
  // The config can only turn it off, never on for a scene that did not ask.
  if (client.autoplay() && config.autoplay) {
    client.dispatch({ type: "play" });
  }

  let last = performance.now();
  const loop = (now: number) => {
    const dt = now - last;
    last = now;
    try {
      client.frame(dt);
    } catch (e) {
      // One bad frame must not spin the loop at 60fps logging forever.
      console.error("frame failed", e);
      fail("The scene stopped rendering.");
      return;
    }
    requestAnimationFrame(loop);
  };
  requestAnimationFrame(loop);
}

/** A minimal transport for the published page.
 *
 * Deliberately NOT the editor's TransportBar: that one dispatches through
 * the session, which owns autosave, OPFS and the dirty flag, none of which a
 * player has any business running. Three buttons and a frame readout is the
 * whole surface a viewer needs.
 */
function mountTransport(client: SolarxyClient): void {
  const bar = document.createElement("div");
  bar.id = "player-transport";

  const button = (label: string, title: string, onClick: () => void) => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = label;
    b.title = title;
    b.setAttribute("aria-label", title);
    b.addEventListener("click", onClick);
    bar.appendChild(b);
    return b;
  };

  let playing = false;
  const playBtn = button("Play", "Play or pause", () => {
    playing = !playing;
    client.dispatch({ type: playing ? "play" : "pause" });
    playBtn.textContent = playing ? "Pause" : "Play";
  });
  button("Stop", "Stop and rewind", () => {
    playing = false;
    playBtn.textContent = "Play";
    client.dispatch({ type: "stop" });
  });

  const readout = document.createElement("span");
  readout.id = "player-frame";
  bar.appendChild(readout);
  document.body.appendChild(bar);

  // Polled once per second rather than driven by the event batch: a viewer
  // does not need frame-accurate chrome, and a per-frame DOM write is the
  // kind of cost the editor deleted its attribute pins to avoid.
  setInterval(() => {
    readout.textContent = `frame ${client.clockFrame()}`;
  }, 250);
}

function resize(client: SolarxyClient, canvas: HTMLCanvasElement): void {
  const dpr = window.devicePixelRatio || 1;
  const w = Math.max(1, Math.round(canvas.clientWidth * dpr));
  const h = Math.max(1, Math.round(canvas.clientHeight * dpr));
  canvas.width = w;
  canvas.height = h;
  client.resize(w, h, dpr);
}

void main();
