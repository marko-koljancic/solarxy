# rc.11 — Profiling notes

Companion to `rc11-baseline.md`. Captures the hot paths a profiler run
surfaced and the concrete fix candidates. No code changes in rc.11 itself;
each slot below turns into a tracked issue for 0.6.0 or 0.7.0.

> **Status: TODO — run profiler on maintainer hardware.**

## Reproduction

### Linux / flamegraph (perf_events)

```bash
# Build once with frame pointers:
RUSTFLAGS="-C force-frame-pointers=yes" \
  cargo build --release --bin solarxy
cargo install --locked flamegraph
sudo sysctl -w kernel.perf_event_paranoid=1
cargo flamegraph -p solarxy --release -- -m <large-model>
```

The resulting `flamegraph.svg` lives in the workspace root. Navigate it
to identify the widest boxes inside `State::update`,
`State::render_pane`, and `EguiRenderer::render_ui`.

### macOS / samply (cross-platform)

```bash
cargo install --locked samply
samply record -- target/release/solarxy -m <large-model>
# Samply opens a Firefox profiler view in your browser.
```

### macOS / Instruments (native)

```bash
xcrun xctrace record --template 'Time Profiler' \
  --launch target/release/solarxy -- -m <large-model> \
  --output rc11.trace
open rc11.trace
```

## Hot path slots

### Slot 1 — TBD

- **Observation:** TODO
- **% of CPU frame:** TODO
- **Proposed fix:** TODO
- **Estimated effort:** TODO (S/M/L)
- **Milestone target:** TBD
- **Notes:** TODO

### Slot 2 — TBD

- **Observation:** TODO
- **% of CPU frame:** TODO
- **Proposed fix:** TODO
- **Estimated effort:** TODO
- **Milestone target:** TBD
- **Notes:** TODO

### Slot 3 — TBD

- **Observation:** TODO
- **% of CPU frame:** TODO
- **Proposed fix:** TODO
- **Estimated effort:** TODO
- **Milestone target:** TBD
- **Notes:** TODO

### Slot 4 — TBD

- **Observation:** TODO
- **% of CPU frame:** TODO
- **Proposed fix:** TODO
- **Estimated effort:** TODO
- **Milestone target:** TBD
- **Notes:** TODO

### Slot 5 — TBD

- **Observation:** TODO
- **% of CPU frame:** TODO
- **Proposed fix:** TODO
- **Estimated effort:** TODO
- **Milestone target:** TBD
- **Notes:** TODO

## GPU hot path slots

Requires timestamp queries (see `rc11-baseline.md` §GPU timings).

### Slot G1 — TBD

- **Pass:** TODO (shadow / gbuffer / main / ssao / bloom / composite)
- **Avg duration (µs):** TODO
- **Observation:** TODO
- **Proposed fix:** TODO
- **Milestone target:** TBD

### Slot G2 — TBD

- **Pass:** TODO
- **Avg duration (µs):** TODO
- **Observation:** TODO
- **Proposed fix:** TODO
- **Milestone target:** TBD

### Slot G3 — TBD

- **Pass:** TODO
- **Avg duration (µs):** TODO
- **Observation:** TODO
- **Proposed fix:** TODO
- **Milestone target:** TBD

## Static candidate list (pre-profiler hypothesis)

Informed by reading the code, not yet measured. Mark confirmed/ruled-out
after real measurements.

- `lights_from_camera` called from three sites (scene ctor, `state/render.rs`,
  `state/update.rs`) — check whether redundant recomputation dominates.
- `create_light_bind_group_selective` — every rebuild allocates a fresh
  bind group; a lock-lights frame may still thrash it.
- `compute_panes` — runs every frame even when layout is unchanged.
- `GuiSnapshot::diff` — O(fields) compare each frame; likely cheap but
  worth confirming under a hot turntable.
- `UV overlap GPU readback` — asynchronous, but stalls on the readback
  side when polled each frame.
- `egui tessellation` — egui's own cost; look for redundant repaint requests.

## Action after this file is complete

Each populated slot becomes a GitHub issue tagged `perf` and milestoned
`0.6.0` (or later). Closing an issue updates this file with a ✓ next to
the slot and links the PR.
