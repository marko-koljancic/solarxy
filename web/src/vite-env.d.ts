/// <reference types="vite/client" />

declare module "*.wasm?url" {
  const url: string;
  export default url;
}

/** The app version, injected by Vite from package.json (see vite.config.ts).
 * Stamped into every `.slxy`'s `generator` field. */
declare const __APP_VERSION__: string;
