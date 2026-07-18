import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import { resolve } from "node:path";
import pkg from "./package.json" with { type: "json" };

// In production nginx serves app.html at /app (the landing owns /). This
// mirrors that in `vite dev` and `vite preview`, so the landing's Launch CTA
// works identically in every environment.
function appRewrite(): Plugin {
  const handler = (req: { url?: string }, _res: unknown, next: () => void) => {
    if (req.url === "/app" || req.url?.startsWith("/app?")) req.url = "/app.html";
    next();
  };
  return {
    name: "solarxy-app-rewrite",
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
  plugins: [react(), appRewrite()],
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
      // Two-page build: the MPW landing owns index.html at the
      // domain root; the app moved to app.html (served at /app by nginx).
      input: {
        landing: resolve(import.meta.dirname, "index.html"),
        app: resolve(import.meta.dirname, "app.html"),
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
