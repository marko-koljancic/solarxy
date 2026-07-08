import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The wasm-bindgen `--target web` output is imported as an ES module; the
// .wasm is loaded by URL (see src/engine/client.ts). No wasm plugin needed.
export default defineConfig({
  plugins: [react()],
  server: { port: 5175 },
  // Keep the large wasm out of the dependency pre-bundle.
  optimizeDeps: { exclude: ["./src/wasm/pkg/solarxy_web.js"] },
});
