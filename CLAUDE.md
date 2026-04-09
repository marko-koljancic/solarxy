# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SolarXY is a cross-platform 3D model viewer, visual debugger, and validator built in Rust with wgpu (WebGPU). It has four modes: a real-time graphical viewer with PBR rendering, a CLI/TUI model analyzer, a preferences editor, and a built-in documentation viewer.

## Build & Run Commands

```bash
cargo build                                       # Debug build (all default features)
cargo build --release                             # Release build
cargo r --release -- --model res/models/xyzrgb_dragon.obj                   # View mode (default)
cargo r --release -- --model res/models/xyzrgb_dragon.obj --mode analyze    # Analyze mode (TUI)
cargo r --release -- --mode preferences           # Preferences editor (TUI)
cargo r --release -- --mode docs                  # Built-in docs viewer (TUI)
cargo fmt                                         # Format (see rustfmt.toml)
cargo clippy --all-targets                        # Lint
cargo test                                        # Run all tests
cargo test -p solarxy-core                        # Run tests for one crate
cargo test -p solarxy-core validation::tests::    # Run a single test (filter by path)
RUST_LOG=solarxy=debug cargo r --release -- ...   # Verbose logging (default level: warn)
```

**MSRV:** Rust 1.92 (see `Cargo.toml`).

**Feature flags** (all on by default): `viewer` (wgpu/winit/egui), `analyzer` (format loaders), `tui` (ratatui sub-apps), `updater` (axoupdater). `main.rs` is heavily `#[cfg]`-gated on these — disabling `viewer` builds a TUI-only binary, etc.

## Architecture

**4-crate workspace:**
- `solarxy` (root) — main binary with viewer, GPU rendering, egui UI
- `solarxy-core` — pure data types, geometry, validation, preferences (no GPU deps)
- `solarxy-formats` — format loaders (OBJ, STL, PLY, glTF/GLB) → `RawModelData`
- `solarxy-cli` — CLI parsing (clap) and TUI interfaces (ratatui).
  Public modules: `parser`, `help`, `tui_analysis`, `tui_docs`, `tui_preferences`
  (TUI modules gated behind the `tui` feature; private `tui/` module holds shared
  ratatui widgets). Lints as `#![warn(clippy::pedantic)]` with a curated allow list.

**Main binary structure:**
- `main.rs` — CLI entry point, dispatches to viewer or analyzer based on `--mode`
- `app.rs` — winit `ApplicationHandler`, event loop, egui sidebar toggle (Tab key)
- `state/` — application state:
  - `mod.rs` — main State struct, pane computation, render orchestration
  - `renderer.rs` — per-pane GPU rendering (3D passes + UV Map + validation overlay)
  - `view_state.rs` — `PaneDisplaySettings`, `ViewLayout`, `DisplaySettings`
  - `input.rs` — all keyboard and mouse input handling (ground truth for key bindings)
  - `update.rs` — state updates per frame
  - `init.rs` — initialization
  - `capture.rs` — screenshot capture
- `validation.rs` — validation rule definitions used by the analyzer and viewer overlay
- `preferences.rs` — viewer-side preferences glue (loads/saves via solarxy-core)
- `aabb.rs` — axis-aligned bounding box helpers
- `calc/` — non-GPU model math (only compiled with the `analyzer` feature):
  - `analyze.rs` — `ModelAnalyzer`, the analyze-mode entry point that loads via solarxy-formats and produces a `Report`
  - `geometry.rs` — geometry-derived stats used in reports and validation

**Rendering subsystem (`cgi/`):**
- `gui.rs` — egui integration (sidebar, divider, model stats, theme)
- `camera.rs` / `camera_state.rs` — orbit camera, per-pane camera management
- `pipelines.rs` — all wgpu render pipelines
- `composite.rs` — per-pane compositing with viewport/scissor rects and tone mapping
- `ibl.rs` — image-based lighting (diffuse + specular)
- `ssao.rs` — screen-space ambient occlusion
- `bloom.rs` — HDR bloom post-process
- `shadow.rs` — shadow mapping
- `uv_camera.rs` — 2D UV-space orthographic camera
- `visualization.rs` — grid, axes, bounds rendering
- `model.rs` — GPU model, vertex structures, AABB, normals geometry
- `material.rs` / `texture.rs` — material and texture GPU resources
- `resources.rs` — file I/O and resource loading
- `shaders/` — WGSL shader files

**Render pipeline (multi-pass, per pane in split mode):**
1. Shadow pass (`shadow.wgsl`) — depth-only from key light's perspective
2. GBuffer pass (if SSAO) — position + normal data
3. Main pass (`shader.wgsl`) — PBR + inspection mode switch (Material ID, Texel Density, Depth)
4. Floor pass (`floor.wgsl`) — shadow-catching transparent floor
5. Wireframe/ghosted overlays (`ghosted.wgsl`)
6. Grid (`grid.wgsl`) and normals (`normals.wgsl`) visualization
7. Validation overlay (`validation.wgsl`) — color-coded issue highlights
8. SSAO + Bloom post-processing
9. Composite pass — tone mapping, viewport/scissor rect
10. UV Map passes (UV panes) — UV-space rendering + overlap detection
11. egui overlay — sidebar, HUD, model stats, toast notifications

**Split viewport:** F1 (single), F2 (vertical), F3 (horizontal). Per-pane cameras, inspection modes, display settings. Active pane by cursor position.

**Inspection modes** (number keys 1-5): Shaded, Material ID, UV Map, Texel Density, Depth

## Key Patterns

- wgpu bind groups for GPU resource access; pipelines created at init and reused
- `Vertex` trait defines buffer layouts for different vertex types
- Camera auto-frames model on load using AABB bounds
- Resources loaded async with pollster blocking
- Per-pane rendering with independent command encoders, viewport rects, and scissor rects
- egui sidebar bidirectionally synced with keyboard shortcuts
- help.rs uses `include_str!` to embed content from `crates/solarxy-cli/content/*.txt`
- Preferences live at `~/.config/solarxy/config.toml`; loaded via `solarxy_core::preferences::load()` on every startup and surfaced in the viewer, the preferences TUI, and as keyboard-driven mutations saved with `Shift+S`

## Formatting

Uses `rustfmt.toml`: max width 100, 4-space indentation, Unix line endings, Rust 2024 edition, imports grouped by std/external/crate.
