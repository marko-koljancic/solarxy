import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import pkg from "./package.json" with { type: "json" };

// The wasm-bindgen `--target web` output is imported as an ES module; the
// .wasm is loaded by URL (see src/engine/client.ts). No wasm plugin needed.
export default defineConfig({
  plugins: [react()],
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
