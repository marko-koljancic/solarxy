# 0.5.0-rc.11 — Baseline performance snapshot

Skeletal as of rc.11. Numbers are filled in on maintainer hardware as part of
the 0.6.0 cycle; this file exists so the methodology and section structure are
agreed upfront and so callers from `CLAUDE.md` resolve.

## Hardware

TBD. Target reference machines for the 0.6.0 measurement pass:
- macOS: aarch64 (M-series), Metal backend
- Linux: x86_64 + dGPU, Vulkan backend
- Windows: x86_64 + dGPU, DirectX 12 backend

## Methodology

- Capture inputs: `res/models/xyzrgb_dragon.obj` (default), one mid-poly glTF,
  one heavy multi-material STL.
- Measure: average frame time across 600 frames (10 s @ 60 Hz) after a 60-frame
  warm-up; 99th-percentile frame time over the same window.
- Conditions: split-viewport off (F1), Shaded inspection mode, default IBL,
  SSAO + Bloom on, MSAA at the user's preference value.
- Tools: `tracing` spans for CPU phases, GPU timestamps via wgpu where the
  backend supports them.

## Results (TBD on maintainer hardware)

| Model | Backend | Avg ms / frame | p99 ms / frame |
|---|---|---|---|
| xyzrgb_dragon.obj | Metal (M-series) | TBD | TBD |
| xyzrgb_dragon.obj | Vulkan (Linux) | TBD | TBD |
| xyzrgb_dragon.obj | DX12 (Windows) | TBD | TBD |

## Known regressions

None tracked at rc.11. Add 0.6.0 cycle findings as bullet items below.

---

TODO 0.6.0 — populate the table on maintainer hardware and link any tracked
hot-path issues.
