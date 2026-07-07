# web-spike (Phase 0 WebGPU spike)

THROWAWAY harness for the web-expansion milestone's Phase 0. It compiles the
real `solarxy-renderer` to wasm32, renders the runtime-fetched dragon through
the real shadow + PBR main + composite passes with the real orbit camera, and
reports FPS and wasm sizes. Deleted (or archived) once the go/no-go is
recorded.

This crate declares its own `[workspace]`, so the parent workspace's
`--workspace` build/test/clippy/doc jobs and CI never see it.

## Build

```bash
./build.sh
```

Prints the four size numbers (raw, wasm-bindgen, wasm-opt -Oz, brotli -q 11)
and leaves the optimized artifact in `dist/`.

## Run

Serve the SOLARXY REPO ROOT (so `/res/models/xyzrgb_dragon.obj` is
reachable), then open the spike page:

```bash
cd ../..   # solarxy repo root
python3 -m http.server 8321
# open http://localhost:8321/spikes/web-spike/index.html
```

Watch the console for `SPIKE_*` lines: `SPIKE_ADAPTER`, `SPIKE_SURFACE`,
`SPIKE_MODEL`, `SPIKE_FIRST_FRAME_OK`, then `SPIKE_FPS` once per second.
`SPIKE_DEVICE_LOST` / `SPIKE_UNCAPTURED_ERROR` are wired to console.error.

## Deliberate deviations from the desktop path (recorded for the go/no-go)

- `BrdfLut::fallback` + `IblState::fallback` instead of `generate` /
  `from_sky_colors`: the heavy single-threaded convolution loops would stall
  the tab; lighting comes from the 3-light rig. IBL quality is NOT part of
  this spike's claim.
- Surface usage is `RENDER_ATTACHMENT` only (desktop adds `COPY_SRC` for
  screenshot capture); `view_formats` is empty (desktop adds the non-sRGB
  view for egui).
- No model is embedded in the wasm; the dragon is fetched at runtime, so the
  size numbers are code, not asset.
- The size number is a fair lower bound protected against DCE: the harness
  links the full `Pipelines::new` (every pipeline + all 21 shaders),
  `BindGroupLayouts`, SSAO/bloom/composite/overdraw state, and the real
  model-upload path.
