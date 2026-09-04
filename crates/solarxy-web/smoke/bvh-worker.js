// The harness worker: a second, headless instantiation of the same
// solarxy_web.wasm, running exactly the `build_bvh_job` export the app's
// import worker runs.
//
// Deliberately not the app's `importWorker.ts`: that one lives under `web/`
// and is built by Vite, and this page is served as plain files beside the wasm.
// What has to be the same is the export being called, and it is.

let ready = null;
let mod = null;

async function ensureReady() {
  if (!ready) {
    ready = (async () => {
      mod = await import("./pkg/solarxy_web.js");
      await mod.default();
    })();
  }
  return ready;
}

self.onmessage = async (event) => {
  const req = event.data;
  try {
    await ensureReady();
    const started = performance.now();
    const bvh = mod.build_bvh_job(req.positions, req.indices);
    const elapsed = performance.now() - started;
    self.postMessage({ id: req.id, bvh, elapsed }, [bvh.buffer]);
  } catch (err) {
    self.postMessage({ id: req.id, error: String(err && err.message ? err.message : err) });
  }
};
