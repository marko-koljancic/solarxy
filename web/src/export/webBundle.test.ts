// The bundle assembler's one risky move is the path rewrite: a built page
// references `/assets/x` from the origin root, and a bundle uploaded to a
// subdirectory has no origin root to speak of. These pin the transform
// against real Vite output rather than against a guess at its shape.

import { describe, expect, it } from "vitest";
import {
  extractAssetUrls,
  rewriteHtml,
  rewriteScript,
  toRelativeAssetPath,
} from "./webBundle";
import { DEFAULT_PLAYER_CONFIG, bundleReadme } from "./playerConfig";

/** A trimmed copy of what `vite build` actually emitted for player.html. */
const BUILT_HTML = `<!doctype html>
<html lang="en">
  <head>
    <title>Solarxy scene</title>
    <style>body { margin: 0 }</style>
    <script type="module" crossorigin src="/assets/player-9ly-yC9-.js"></script>
    <link rel="modulepreload" crossorigin href="/assets/modulepreload-polyfill-B5Qt9EMX.js">
    <link rel="modulepreload" crossorigin href="/assets/client-CqIDWykD.js">
  </head>
  <body>
    <canvas id="player-canvas"></canvas>
  </body>
</html>`;

describe("toRelativeAssetPath", () => {
  it("drops the leading slash so the reference resolves against the page", () => {
    expect(toRelativeAssetPath("/assets/client-abc.js")).toBe("assets/client-abc.js");
  });

  it("leaves an already-relative path alone", () => {
    expect(toRelativeAssetPath("assets/x.js")).toBe("assets/x.js");
    expect(toRelativeAssetPath("./scene.slxy")).toBe("./scene.slxy");
  });
});

describe("extractAssetUrls", () => {
  it("finds every script and modulepreload the built page references", () => {
    expect(extractAssetUrls(BUILT_HTML).sort()).toEqual([
      "/assets/client-CqIDWykD.js",
      "/assets/modulepreload-polyfill-B5Qt9EMX.js",
      "/assets/player-9ly-yC9-.js",
    ]);
  });

  it("ignores references to other origins and to non-asset paths", () => {
    const html = `<link href="https://example.com/assets/x.css"><img src="/favicon.ico">`;
    expect(extractAssetUrls(html)).toEqual([]);
  });

  it("deduplicates a URL referenced twice", () => {
    const html = `<script src="/assets/a.js"></script><link href="/assets/a.js">`;
    expect(extractAssetUrls(html)).toEqual(["/assets/a.js"]);
  });
});

describe("rewriteHtml", () => {
  it("makes every asset reference document-relative", () => {
    const out = rewriteHtml(BUILT_HTML);
    expect(out).toContain('src="assets/player-9ly-yC9-.js"');
    expect(out).toContain('href="assets/client-CqIDWykD.js"');
    expect(out).not.toContain('"/assets/');
  });

  it("leaves the rest of the document untouched", () => {
    const out = rewriteHtml(BUILT_HTML);
    expect(out).toContain('<canvas id="player-canvas">');
    expect(out).toContain("<style>body { margin: 0 }</style>");
  });

  it("does not touch an absolute reference outside /assets", () => {
    const html = `<link rel="icon" href="/favicon.ico">`;
    expect(rewriteHtml(html)).toBe(html);
  });
});

describe("rewriteScript", () => {
  it("rewrites the wasm URL, which is the only absolute asset the glue holds", () => {
    // The real shape, from the built client chunk.
    const js = `const e=new URL("/assets/solarxy_web_bg-G_0nEwUd.wasm",import.meta.url);`;
    expect(rewriteScript(js)).toBe(
      `const e=new URL("assets/solarxy_web_bg-G_0nEwUd.wasm",import.meta.url);`,
    );
  });

  it("leaves unrelated strings alone", () => {
    const js = `const a="/api/thing";const b="assets/already.js";const c="/assets/x.js";`;
    expect(rewriteScript(js)).toBe(
      `const a="/api/thing";const b="assets/already.js";const c="assets/x.js";`,
    );
  });
});

describe("player config", () => {
  it("defaults to a scene beside the page", () => {
    expect(DEFAULT_PLAYER_CONFIG.scene).toBe("./scene.slxy");
  });

  it("defaults the transport off, because a published scene is not an editor", () => {
    expect(DEFAULT_PLAYER_CONFIG.transport).toBe(false);
  });

  it("states the file:// limitation in the README rather than hiding it", () => {
    const readme = bundleReadme("scene.slxy");
    expect(readme).toContain("file://");
    expect(readme).toContain("http.server");
  });
});
