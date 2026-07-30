import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import pkg from "./package.json" with { type: "json" };

// In production nginx serves each extensionless route from its HTML entry
// (the landing owns /). This mirrors that in `vite dev` and `vite preview`,
// so the landing's links work identically in every environment. A new public
// page needs its route here AND an exact-match location in the edge config.
function publicRoutesRewrite(): Plugin {
  const routes: Record<string, string> = {
    "/app": "/app.html",
    "/player": "/player.html",
    "/roadmap": "/roadmap.html",
    "/references": "/references.html",
  };
  const handler = (req: { url?: string }, _res: unknown, next: () => void) => {
    if (req.url) {
      const path = req.url.split("?")[0];
      const target = routes[path];
      if (target) req.url = target + req.url.slice(path.length);
    }
    next();
  };
  return {
    name: "solarxy-public-routes-rewrite",
    configureServer(server) {
      server.middlewares.use(handler);
    },
    configurePreviewServer(server) {
      server.middlewares.use(handler);
    },
  };
}

// The wasm-bindgen `--target web` output is imported as an ES module; the
// .wasm is loaded by URL (see src/engine/client.ts). No wasm plugin needed.
export default defineConfig({
  plugins: [react(), publicRoutesRewrite()],
  server: { port: 5175 },
  define: {
    // The app version, single-sourced from package.json. It is stamped into the
    // `generator` field of every `.slxy` the app writes; that string used to be
    // hardcoded in session.ts, where nothing tested it and it silently went
    // stale across a release.
    __APP_VERSION__: JSON.stringify(pkg.version),
    // Sentry tree-shaking flags. We use error reporting only: no performance
    // tracing, no debug logging. Setting these lets the minifier drop that code
    // entirely rather than shipping it dormant.
    __SENTRY_DEBUG__: false,
    __SENTRY_TRACING__: false,
  },
  // Keep the large wasm out of the dependency pre-bundle.
  optimizeDeps: { exclude: ["./src/wasm/pkg/solarxy_web.js"] },
  build: {
    // The wasm is the one legitimately huge asset; it is not a JS chunk and is
    // fetched separately, so the default 500 kB warning only adds noise.
    chunkSizeWarningLimit: 1000,
    rollupOptions: {
      // Multi-page build: the MPW landing owns index.html at the domain
      // root, the app is app.html (served at /app by nginx), the player is
      // player.html (served at /player), and the public roadmap and
      // references pages are served at /roadmap and /references.
      input: {
        landing: resolve(import.meta.dirname, "index.html"),
        app: resolve(import.meta.dirname, "app.html"),
        // The player is a real built page, not a template the exporter
        // stitches at runtime: it is typechecked, bundled and dev-servable
        // like the other two, and the export copies what the build produced.
        player: resolve(import.meta.dirname, "player.html"),
        roadmap: resolve(import.meta.dirname, "roadmap.html"),
        references: resolve(import.meta.dirname, "references.html"),
      },
      output: {
        // Split the heavy vendors so a change to app code does not invalidate
        // them in the browser cache.
        //
        // `elkjs` is deliberately ABSENT: it is dynamically imported inside
        // `computeElkLayout` (see web/src/flow/layout.ts). Naming it here would
        // pull it back into the eager graph and undo the largest single saving
        // available -- roughly 1.6 MB.
        manualChunks: {
          react: ["react", "react-dom", "react-dom/client"],
          flow: ["@xyflow/react"],
          dock: ["dockview-react", "dockview-core"],
          layout: ["@dagrejs/dagre"],
        },
      },
    },
  },
});
