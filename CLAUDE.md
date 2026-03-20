# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

SolarXY is a cross-platform 3D model viewer and validator written in Rust. It supports `.obj` files and offers two modes: a real-time graphical viewer and a command-line analysis/validation tool.

## Commands

```bash
# Build
cargo build
cargo build --release

# Run (viewer mode, default)
cargo run -- --model res/models/cube/cube.obj

# Run (analyze mode)
cargo run -- --model res/models/cube/cube.obj --mode analyze

# Lint & format
cargo clippy
cargo fmt

# Tests
cargo test
```

## Architecture

The codebase is split into three top-level modules:

- **`cgi/`** — Computer Graphics Interface: GPU pipeline setup, camera, lighting, materials, mesh/model structs, texture loading, and WGSL shaders. Rendering is done via `wgpu` with a dual-pipeline setup (model + light source). The `DrawModel` and `DrawLight` traits extend `wgpu::RenderPass`.
- **`cli/`** — Argument parsing (`clap`), OBJ file validation, and the `ratatui`-based terminal UI for analysis output (`tui.rs`).
- **`calc/`** — Model analysis logic: inspects meshes, materials, and textures, producing a structured report consumed by the TUI.

**Entry points:**
- `src/main.rs` — CLI binary: parses args, selects mode (View → `lib.rs`, Analyze → `calc/`)
- `src/lib.rs` — Graphics viewer: `winit` event loop, `App` struct, delegates to `state.rs`
- `src/state.rs` — Core rendering state: GPU device/queue setup, bind groups, render loop

**Data flow (View mode):** `main` → `lib::run()` → `App` (winit loop) → `State` (wgpu render)

**Data flow (Analyze mode):** `main` → `calc::analyze()` → `cli::tui` (ratatui display)

## Code Conventions

- `rustfmt.toml`: 120-char line width, 4-space indent, trailing commas on multiline, Unix line endings
- Error handling: `anyhow::Result<T>` throughout; `color_eyre` for enhanced top-level display
- GPU buffer types derive `bytemuck::Pod + Zeroable`
- Resource loading is async; `pollster::block_on` used at the sync/async boundary

## Key Files

| File | Role |
|------|------|
| `src/cgi/shaders/shader.wgsl` | Main shader: normal mapping, Phong lighting |
| `src/cgi/shaders/light.wgsl` | Light source visualization shader |
| `src/cgi/resources.rs` | Loads OBJ models and textures; computes tangents/bitangents |
| `src/calc/analyize.rs` | Model analysis and report generation (note: filename has typo) |
| `build.rs` | Copies `res/` to the build output directory |

## Resources

Test models live in `res/models/` and textures in `res/textures/`. The build script copies these to the Cargo output dir so they're accessible at runtime relative to the binary.
