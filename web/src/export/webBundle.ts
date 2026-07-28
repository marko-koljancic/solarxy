// Assembles a published scene bundle: a zip that runs on any static host.
//
// The bundle carries the ENGINE, not a bake. It is the same wasm the editor
// runs, so an expression-driven or wrangle-driven scene animates in the
// bundle exactly as it does in the editor, because it is being cooked rather
// than replayed.
//
// **What gets copied, and why it is fetched rather than templated.** The
// player is a real Vite entry (`player.html`), so the export copies what the
// build actually produced instead of stitching a hand-written shell that
// could drift from the source. At export time the running app fetches its
// own built player page, reads the asset URLs out of it, and fetches those.
//
// **The one rewrite.** A built page references `/assets/x` from the origin
// root, which breaks the moment somebody uploads the folder to a
// subdirectory. Every such reference is made document-relative by dropping
// the leading slash. That is the whole transform: it applies to the HTML's
// src/href attributes and to exactly one string literal in the engine glue
// (the wasm URL), which is the only absolute asset reference the player's
// JS contains. It is verified by a test rather than assumed.

import { zipSync } from "fflate";
import { DEFAULT_PLAYER_CONFIG, bundleReadme, type PlayerConfig } from "./playerConfig";

/** Where the built player page lives on the app's own origin. */
const PLAYER_PAGE = "/player.html";

/** The scene's name inside the archive. */
const SCENE_NAME = "scene.slxy";

export interface BundleOptions {
  /** The saved `.slxy` bytes, from the engine's own writer. */
  scene: Uint8Array;
  /** Overrides on top of the defaults. */
  config?: Partial<PlayerConfig>;
}

export interface BundleResult {
  zip: Uint8Array;
  /** Archive member names, for the modal's summary and for tests. */
  files: string[];
}

/** Rewrites an origin-absolute asset reference to a document-relative one.
 *
 * `/assets/x` becomes `assets/x`, which resolves against the bundle's own
 * index.html wherever the folder is uploaded. Anything already relative, or
 * pointing at another origin, is left alone.
 */
export function toRelativeAssetPath(url: string): string {
  return url.startsWith("/") ? url.slice(1) : url;
}

/** Every `/assets/...` reference in a built page's src/href attributes. */
export function extractAssetUrls(html: string): string[] {
  const out = new Set<string>();
  const attr = /(?:src|href)\s*=\s*"([^"]+)"/g;
  let m: RegExpExecArray | null;
  while ((m = attr.exec(html)) !== null) {
    const url = m[1];
    if (url.startsWith("/assets/")) out.add(url);
  }
  return [...out];
}

/** Makes a built page's asset references document-relative. */
export function rewriteHtml(html: string): string {
  return html.replace(
    /((?:src|href)\s*=\s*")(\/assets\/[^"]+)(")/g,
    (_all, lead: string, url: string, tail: string) =>
      `${lead}${toRelativeAssetPath(url)}${tail}`,
  );
}

/** Makes a built script's absolute asset references document-relative.
 *
 * In practice this is the wasm URL and nothing else, but the rewrite is
 * written generally so a future asset import does not silently ship a broken
 * bundle. Only quoted `/assets/...` literals are touched.
 */
export function rewriteScript(js: string): string {
  return js.replace(
    /"(\/assets\/[^"]+)"/g,
    (_all, url: string) => `"${toRelativeAssetPath(url)}"`,
  );
}

async function fetchBytes(url: string): Promise<Uint8Array> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status} ${res.statusText}`);
  return new Uint8Array(await res.arrayBuffer());
}

async function fetchText(url: string): Promise<string> {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`${url}: ${res.status} ${res.statusText}`);
  return res.text();
}

const encoder = new TextEncoder();

/** Builds the archive.
 *
 * @throws when the built player page cannot be fetched, which is what a dev
 * server looks like: `player.html` there serves the SOURCE page pointing at
 * `/src/player/main.ts`, and bundling that would produce an archive that
 * only works against a running Vite. The caller surfaces this rather than
 * shipping something broken.
 */
export async function buildWebBundle(opts: BundleOptions): Promise<BundleResult> {
  const config: PlayerConfig = {
    ...DEFAULT_PLAYER_CONFIG,
    ...opts.config,
    scene: `./${SCENE_NAME}`,
  };

  const html = await fetchText(PLAYER_PAGE);
  if (html.includes("/src/player/main.ts")) {
    throw new Error(
      "This is a development build, so the player page is unbundled source. " +
        "Export from a production build (npm run build) to get a bundle that " +
        "runs anywhere.",
    );
  }

  const assetUrls = extractAssetUrls(html);
  if (assetUrls.length === 0) {
    throw new Error("The built player page referenced no assets; the build looks wrong.");
  }

  const files: Record<string, Uint8Array> = {
    "index.html": encoder.encode(rewriteHtml(html)),
    [SCENE_NAME]: opts.scene,
    "solarxy-player.json": encoder.encode(`${JSON.stringify(config, null, 2)}\n`),
    "README.txt": encoder.encode(bundleReadme(SCENE_NAME)),
  };

  // The wasm is not referenced by the HTML; it is fetched by the engine glue
  // from a URL baked into the script, so it is collected from the rewritten
  // scripts rather than from the page.
  const wasmUrls = new Set<string>();

  for (const url of assetUrls) {
    const name = toRelativeAssetPath(url);
    if (url.endsWith(".js")) {
      const js = await fetchText(url);
      for (const m of js.matchAll(/"(\/assets\/[^"]+\.wasm)"/g)) wasmUrls.add(m[1]);
      files[name] = encoder.encode(rewriteScript(js));
    } else {
      files[name] = await fetchBytes(url);
    }
  }

  if (wasmUrls.size === 0) {
    throw new Error(
      "No engine wasm was referenced by the player scripts, so the bundle would not run.",
    );
  }
  for (const url of wasmUrls) {
    files[toRelativeAssetPath(url)] = await fetchBytes(url);
  }

  // Compressed, unlike the turntable exporter: that one ships PNGs, which are
  // already compressed, while this is several megabytes of wasm and text that
  // deflate to roughly a third the size.
  const zip = zipSync(files, { level: 6 });
  return { zip, files: Object.keys(files).sort() };
}
