// `solarxy-player.json`: the exported bundle's one knob file.
//
// Shared by the exporter that writes it and the player that reads it, so the
// two cannot disagree about a field name. Every field has a default, and the
// player treats a missing or unparseable config as "all defaults": the scene
// is the payload that matters, and a typo in a config should not cost
// somebody their published page.
//
// Deliberately NOT the scene's own runtime settings. `fps`, the frame range
// and loop mode live in the `.slxy` because they are what the scene means;
// these are what the PAGE does with it.

export interface PlayerConfig {
  /** The scene file, relative to the page. */
  scene: string;
  /** Allows autoplay. Can only ever turn the document's `autoplay` OFF: a
   * scene that did not ask to play is never started by a config. */
  autoplay: boolean;
  /** Shows the transport strip under the canvas. */
  transport: boolean;
  /** Page background behind the canvas, any CSS color. */
  background: string;
  /** Spins the camera, the same session-only effect the editor offers.
   * Not document time: a turntable is a camera effect, and folding it into
   * the clock would change what a saved scene means. */
  turntable: boolean;
  /** Turntable speed in revolutions per minute. */
  turntableRpm: number;
}

export const DEFAULT_PLAYER_CONFIG: PlayerConfig = {
  scene: "./scene.slxy",
  autoplay: true,
  transport: false,
  background: "#101014",
  turntable: false,
  turntableRpm: 6,
};

/** The README that ships in the archive.
 *
 * States the one real limitation rather than engineering around it: the page
 * loads an ES module and fetches the wasm by URL, both of which a `file://`
 * origin refuses. That is a browser rule, not something a bundle can fix.
 */
export function bundleReadme(sceneName: string): string {
  return `Solarxy scene bundle
====================

This folder is a self-contained Solarxy scene. It carries the engine, so it
needs no install, no account and no network once served.

  ${sceneName}

HOW TO VIEW IT
--------------

Serve this folder over HTTP and open the page. Opening index.html directly
from disk will NOT work: the page loads an ES module and fetches the engine
by URL, and browsers refuse both from a file:// address. That is a browser
security rule, not a Solarxy limitation.

The quickest local option, from inside this folder:

  python3 -m http.server 8000

then visit http://localhost:8000

To publish it, upload the whole folder to any static host (GitHub Pages,
Netlify, S3, nginx, or a folder on your own server). There is no build step.

REQUIREMENTS
------------

A browser with WebGPU: current Chrome or Edge, or Safari 26 and later.

WHAT IS IN HERE
---------------

  index.html               the page
  assets/                  the player and the Solarxy engine (wasm)
  ${sceneName}             your scene, exactly as the editor saved it
  solarxy-player.json      playback and appearance settings
  README.txt               this file

Editing solarxy-player.json changes how the page behaves without re-exporting:
turn the transport strip on, change the background, or stop it autoplaying.

Made with Solarxy - https://solarxy.koljam.com
`;
}
