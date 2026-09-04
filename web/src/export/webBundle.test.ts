// The bundle assembler's one risky move is the path rewrite: a built page
// references `/assets/x` from the origin root, and a bundle uploaded to a
// subdirectory has no origin root to speak of. These pin the transform
// against real Vite output rather than against a guess at its shape.

import { describe, expect, it } from "vitest";
import {
  extractAssetUrls,
  injectFileAddressNote,
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
  it("resolves against the document URL: dropping the slash is right for src and href", () => {
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
  it("resolves against the script's own URL, not the document's", () => {
    // The real shape, from the built client chunk. The script sits inside
    // assets/, so `import.meta.url` already ends in assets/, and merely
    // dropping the slash would ask for assets/assets/<wasm>.
    const js = `const e=new URL("/assets/solarxy_web_bg-G_0nEwUd.wasm",import.meta.url);`;
    expect(rewriteScript(js, "assets")).toBe(
      `const e=new URL("./solarxy_web_bg-G_0nEwUd.wasm",import.meta.url);`,
    );
  });

  it("reaches the bundled wasm from a host root and from a subdirectory alike", () => {
    // The subdirectory case is the one the rewrite exists to serve; resolve
    // with real URL semantics rather than comparing strings.
    const rewritten = rewriteScript(`new URL("/assets/engine.wasm",import.meta.url)`, "assets");
    const literal = /"([^"]+)"/.exec(rewritten)?.[1] ?? "";
    for (const scriptUrl of [
      "https://host/assets/player.js",
      "https://host/sub/dir/assets/player.js",
    ]) {
      expect(new URL(literal, scriptUrl).href).toBe(
        scriptUrl.replace(/player\.js$/, "engine.wasm"),
      );
    }
  });

  it("leaves unrelated strings alone", () => {
    const js = `const a="/api/thing";const b="assets/already.js";const c="/assets/x.js";`;
    expect(rewriteScript(js, "assets")).toBe(
      `const a="/api/thing";const b="assets/already.js";const c="./x.js";`,
    );
  });

  it("still reaches a sibling asset from a script at the archive root", () => {
    expect(rewriteScript(`"/assets/x.js"`, "")).toBe(`"./assets/x.js"`);
  });
});

describe("injectFileAddressNote", () => {
  it("injects a hidden note and the classic script that reveals it on file:", () => {
    const out = injectFileAddressNote(BUILT_HTML);
    expect(out).toContain('id="solarxy-file-note" hidden');
    expect(out).toContain('location.protocol==="file:"');
    expect(out).toContain("python3 -m http.server");
    // Before </body>, so the page parses it even though the module never runs.
    expect(out.indexOf("solarxy-file-note")).toBeLessThan(out.indexOf("</body>"));
  });

  it("leaves the served page's content untouched", () => {
    const out = injectFileAddressNote(BUILT_HTML);
    expect(out).toContain('<canvas id="player-canvas">');
    expect(out).toContain("<style>body { margin: 0 }</style>");
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
