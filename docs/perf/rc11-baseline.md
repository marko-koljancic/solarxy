# rc.11 — Performance baseline

Skeleton for the perf spike that lands alongside 0.5.0-rc.11. Measurements
are filled in by the maintainer on real hardware; this file documents the
scenarios, reproduction commands, and the metrics that each model/hardware
combination should produce.

> **Status: TODO — measure on maintainer hardware.**
> The code commits for rc.11 do not include performance fixes. Numbers
> captured here feed into the rc.11 → 0.6.0 backlog.

## Scope

Baseline the current cost of a single animated frame across three dimensions:

1. **Reference models** — vary by triangle count and material complexity.
2. **Hardware profiles** — laptop iGPU / mid-range discrete / Apple Silicon.
3. **Feature toggles** — full pipeline, SSAO off, bloom off, shadow off.

All measurements are taken from a release build
(`cargo build --release` or `cargo r --release --`) on the current branch
head at the time of measurement.

## Reproduction

```bash
# Full pipeline, default settings:
cargo r --release -- -m <model.obj>

# Verbose logs routed to the in-app console:
SOLARXY_CONSOLE_LOG=solarxy=debug cargo r --release -- -m <model.obj>

# Toggle each post-pass via the sidebar (Rendering panel):
#   - SSAO:   Shift+O
#   - Bloom:  Shift+D
#   - Shadow: Debug panel → "Shadow pass" off (or via sidebar)
```

FPS is read from the FPS HUD (enable via Window → FPS HUD). For CPU frame
time, inspect the value shown next to FPS (`{avg_ms:.1} ms {fps} fps`).

## Reference models

TODO: maintainer to pick and record.

| Slot | Model | Format | Tri count | Vert count | Materials | UVs |
|---|---|---|---|---|---|---|
| `small` | TBD | TBD | TODO | TODO | TODO | TODO |
| `medium` | TBD | TBD | TODO | TODO | TODO | TODO |
| `large` | TBD | TBD | TODO | TODO | TODO | TODO |

Suggested candidates from `res/models/`:
`xyzrgb_dragon.obj`, `happy_buddha.ply`, `stanford_bunny.obj`, plus one
glTF with multi-material / textured content.

## Hardware profiles

TODO: maintainer to measure.

| Profile | GPU | VRAM | CPU | OS | Driver |
|---|---|---|---|---|---|
| `integrated` | TBD | TBD | TBD | TBD | TBD |
| `discrete-midrange` | TBD | TBD | TBD | TBD | TBD |
| `apple-silicon` | TBD | TBD | TBD | TBD | TBD |

## Metrics grid

TODO for each (profile × model × scenario):

| Profile | Model | Scenario | Avg FPS | Avg frame time (ms) | GPU ms | Notes |
|---|---|---|---|---|---|---|
| integrated | small | default | TODO | TODO | TODO | |
| integrated | small | SSAO off | TODO | TODO | TODO | |
| integrated | small | bloom off | TODO | TODO | TODO | |
| integrated | small | shadow off | TODO | TODO | TODO | |
| integrated | medium | default | TODO | TODO | TODO | |
| integrated | medium | SSAO off | TODO | TODO | TODO | |
| integrated | medium | bloom off | TODO | TODO | TODO | |
| integrated | medium | shadow off | TODO | TODO | TODO | |
| integrated | large | default | TODO | TODO | TODO | |
| integrated | large | SSAO off | TODO | TODO | TODO | |
| integrated | large | bloom off | TODO | TODO | TODO | |
| integrated | large | shadow off | TODO | TODO | TODO | |
| discrete-midrange | small | default | TODO | TODO | TODO | |
| discrete-midrange | small | SSAO off | TODO | TODO | TODO | |
| discrete-midrange | small | bloom off | TODO | TODO | TODO | |
| discrete-midrange | small | shadow off | TODO | TODO | TODO | |
| discrete-midrange | medium | default | TODO | TODO | TODO | |
| discrete-midrange | medium | SSAO off | TODO | TODO | TODO | |
| discrete-midrange | medium | bloom off | TODO | TODO | TODO | |
| discrete-midrange | medium | shadow off | TODO | TODO | TODO | |
| discrete-midrange | large | default | TODO | TODO | TODO | |
| discrete-midrange | large | SSAO off | TODO | TODO | TODO | |
| discrete-midrange | large | bloom off | TODO | TODO | TODO | |
| discrete-midrange | large | shadow off | TODO | TODO | TODO | |
| apple-silicon | small | default | TODO | TODO | TODO | |
| apple-silicon | small | SSAO off | TODO | TODO | TODO | |
| apple-silicon | small | bloom off | TODO | TODO | TODO | |
| apple-silicon | small | shadow off | TODO | TODO | TODO | |
| apple-silicon | medium | default | TODO | TODO | TODO | |
| apple-silicon | medium | SSAO off | TODO | TODO | TODO | |
| apple-silicon | medium | bloom off | TODO | TODO | TODO | |
| apple-silicon | medium | shadow off | TODO | TODO | TODO | |
| apple-silicon | large | default | TODO | TODO | TODO | |
| apple-silicon | large | SSAO off | TODO | TODO | TODO | |
| apple-silicon | large | bloom off | TODO | TODO | TODO | |
| apple-silicon | large | shadow off | TODO | TODO | TODO | |

## GPU timings (optional)

`wgpu` can emit per-pass timestamps when the
`wgpu::Features::TIMESTAMP_QUERY` feature is available. Solarxy does not
enable this today; adding it to `solarxy-renderer` is itself a spike item
(tracked in `rc11-profiling-notes.md`). Once wired, measure:

- Shadow pass
- GBuffer pass
- Main (PBR) pass
- SSAO pass (+ blur)
- Bloom pass
- Composite pass

## Acceptance

This baseline is used to validate that **0.6.0** perf work produces a
real improvement. Rerun the full grid on the same hardware after each
change and record deltas in a follow-up file (`docs/perf/0.6.0-results.md`).
