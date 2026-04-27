# 0.5.0-rc.11 — Profiling notes

Skeletal companion to `rc11-baseline.md`. Captures the qualitative observations
from the rc.11 perf spike before quantitative numbers land in 0.6.0.

## Frame budget breakdown

Phases the renderer touches per frame (see
`crates/solarxy-renderer/src/frame.rs`):
1. Shadow pass
2. GBuffer pass (when SSAO is on)
3. Background pass
4. Main PBR pass
5. Floor pass
6. Wireframe / ghosted overlays
7. Visualization (grid, axes, normals)
8. Validation overlay
9. SSAO + Bloom post-processing
10. Composite (tone mapping, scissor)
11. UV map passes (UV inspection panes)
12. egui overlay

TBD for 0.6.0: time each phase under the standard methodology and surface the
top 3 contributors as concrete issues.

## Hot paths identified (qualitative)

- `rebuild_light_bind_group` (`solarxy-app/src/state/update.rs`) is touched on
  every IBL change, background change, and HDRI drop — verify the partial
  `queue.write_buffer` is actually a partial update and not pushing the full
  uniform.
- `lights_from_camera` (`solarxy-renderer/src/scene.rs`) is called from three
  sites; check whether any are redundant per frame.
- Split-viewport mode (F2/F3) doubles or triples most pass costs — confirm
  pipeline reuse across panes.

## Tools used

- `tracing` + `tracing-subscriber` with `RUST_LOG=solarxy=debug` for CPU phases.
- wgpu timestamp queries (where the backend supports them) for GPU phases.
- Native OS profilers (Instruments on macOS, RenderDoc on Linux/Windows) for
  GPU pass attribution when wgpu timestamps are insufficient.

## Action items for 0.6.0

- Populate `rc11-baseline.md` on each reference machine.
- File issues for the top 3 GPU-time contributors and top 3 CPU-time
  contributors.
- Re-evaluate SSAO and Bloom defaults if either dominates the budget on the
  Linux / Windows reference machines.

---

TODO 0.6.0 — promote findings into tracked issues, link them from this file.
