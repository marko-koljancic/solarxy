# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SolarXY is a cross-platform 3D model viewer and validator built in Rust with wgpu (WebGPU). It has two modes: a real-time graphical viewer with PBR rendering, and a CLI/TUI model analyzer.

## Build & Run Commands

```bash
cargo build                  # Debug build
cargo build --release        # Release build
cargo r --release -- --model res/models/xyzrgb_dragon.obj              # View mode (default)
cargo r --release -- --model res/models/xyzrgb_dragon.obj --mode analyze  # Analyze mode
cargo fmt                    # Format code (see rustfmt.toml for config)
cargo clippy                 # Lint
cargo test                   # Run tests
```

## Architecture

**Dual-mode binary:**
- `main.rs` — CLI entry point, dispatches to viewer or analyzer based on `--mode`
- `lib.rs` — Viewer entry (`run_viewer()`) using winit's `ApplicationHandler`

**Core modules:**
- `state.rs` — Central rendering state. Owns the wgpu device/queue/surface, all render pipelines, camera, lights, loaded model, and view mode state. This is the largest file and the heart of the renderer.
- `cgi/` — Rendering subsystem:
  - `camera.rs` — Orbit camera with mouse controls, auto-framing from AABB
  - `light.rs` — 3-light system (key/fill/rim) that follows the camera
  - `model.rs` — OBJ model loading, vertex structures (`ModelVertex`, `LineVertex`), AABB computation, normals geometry
  - `material.rs` / `texture.rs` — Material and texture GPU resource management
  - `resources.rs` — File I/O and resource loading (models, textures)
  - `shaders/` — WGSL shader files (7 shaders, see below)
- `cli/` — Command-line parsing (clap) and TUI (ratatui)
- `calc/` — Model analysis and reporting

**Render pipeline (multi-pass):**
1. Shadow pass (`shadow.wgsl`) — depth-only from key light's perspective
2. Main pass (`shader.wgsl`) — Cook-Torrance PBR with normal mapping, 3 dynamic lights, shadow sampling, Reinhard tone mapping
3. Floor pass (`floor.wgsl`) — shadow-catching transparent floor
4. Wireframe/ghosted overlays (`ghosted.wgsl`)
5. Grid (`grid.wgsl`) and normals (`normals.wgsl`) visualization
6. Light spheres (`light.wgsl`) — instance-rendered light position indicators

**View modes** (cycle with W): Shaded, ShadedWireframe, WireframeOnly, Ghosted
**Normals modes** (cycle with N): Off, Face, Vertex, FaceAndVertex

## Key Patterns

- wgpu bind groups for GPU resource access; pipelines are created at init and reused
- `Vertex` trait defines buffer layouts for different vertex types
- Camera auto-frames model on load using AABB bounds
- Resources are loaded async with pollster blocking
- `build.rs` copies `res/` to the build output directory

## Formatting

Uses `rustfmt.toml`: max width 100, 4-space indentation, Unix line endings, Rust 2024 edition, imports grouped by std/external/crate.
