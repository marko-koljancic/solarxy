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
- `console.rs` — in-app log console: `ConsoleState` (buffer, filter, docked/floating flags), `LogBuffer`, and a `tracing::Layer` that feeds `tracing` events into the egui console view
- `aabb.rs` — axis-aligned bounding box helpers
- `calc/` — non-GPU model math (only compiled with the `analyzer` feature):
  - `analyze.rs` — `ModelAnalyzer`, the analyze-mode entry point that loads via solarxy-formats and produces a `Report`
  - `geometry.rs` — geometry-derived stats used in reports and validation

**Rendering subsystem (`cgi/`):**
- `gui/` — egui integration, decomposed into single-responsibility files:
  - `mod.rs` — re-exports (`EguiRenderer`, `SidebarChanges`, `ToastSeverity`)
  - `renderer.rs` — `EguiRenderer` frame orchestration
  - `sidebar.rs` — collapsible panels (View, Inspect, Material, Debug, Rendering, Advanced)
  - `menu.rs` — native-style menu bar (File / View / Analyze) with shortcut labels
  - `snapshot.rs` — `GuiSnapshot` bidirectional sidebar↔state mirror; `SidebarChanges` flags; `HudInfo`
  - `actions.rs` — `MenuActions` event flags; `MenuBarVisibility` panel toggles
  - `overlays.rs` — toast notifications (`Toast`, `ToastSeverity`), FPS/frame-time HUD, loading indicator
  - `stats.rs` — `ModelInfo` + `draw_stats_window()` (file + geometry details modal)
  - `console_view.rs` — docked + floating log viewer with level filter
  - `theme.rs` — dark theme, accent colors, font sizing
  - `about.rs` — About modal
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
2. GBuffer pass (`gbuffer.wgsl`, if SSAO) — position + normal data
3. Background pass (`background.wgsl`) — skybox / solid background
4. Main pass (`shader.wgsl`) — PBR + inspection mode switch (Material ID, Texel Density, Depth)
5. Floor pass (`floor.wgsl`) — shadow-catching transparent floor
6. Wireframe/ghosted overlays (`ghosted.wgsl`) and edge wireframe (`edge_wire.wgsl`, distinct pipeline)
7. Grid (`grid.wgsl`), normals (`normals.wgsl`), and axis gizmo (`gizmo.wgsl`) visualization
8. Validation overlay (`validation.wgsl`) — color-coded issue highlights
9. SSAO (`ssao.wgsl` + `ssao_blur.wgsl`) + Bloom (`bloom.wgsl`) post-processing
10. Composite pass (`composite.wgsl`) — tone mapping, viewport/scissor rect
11. UV Map passes (UV panes) — checker/texture/wire variants (`uv_map.wgsl`), debug overlay (`uv_debug.wgsl`), and overlap detection (`uv_overlap.wgsl`)
12. egui overlay — sidebar, HUD, model stats, toast notifications

**Split viewport:** F1 (single), F2 (vertical), F3 (horizontal). Per-pane cameras, inspection modes, display settings. Active pane by cursor position.

**Inspection modes** (number keys 1-5): Shaded, Material ID, UV Map, Texel Density, Depth

**Material overrides** (`Shift+M` / sidebar): `MaterialOverride::{None, Clay, ClayDark, Chrome, Silhouette}` → `camera.material_override` (0-4). These are stylized, not physical, and short-circuit parts of `fs_main` in `shader.wgsl`:
- Silhouette (4u) early-returns solid black
- Chrome (3u) skips all three direct lights so it only samples the prefiltered environment
- Clay Light/Dark (1u/2u) use a directionless ambient — the L0 spherical-harmonic coefficient of the active IBL's irradiance map (`IblState::irradiance_average`, computed CPU-side in all three constructors and pushed to the GPU via `LightsUniform.ibl_avg_{r,g,b}`) — and route direct lights through `lambert_direct` to suppress the cook_torrance specular lobe

## Key Patterns

### GPU uniform buffers are hand-laid-out
CPU structs (`CameraUniform` in `cgi/camera.rs`, `LightsUniform` in `cgi/light.rs`, and most `*Uniform` structs under `cgi/`) are `#[repr(C)]` with explicit `_pad` fields chosen to hit WGSL's 16-byte struct-size alignment. Several have `const _: () = assert!(std::mem::size_of::<T>() == N);` at the crate root — when extending a uniform, preserve the assert (repack the padding) or update it in lockstep with the shader side. Corresponding WGSL `struct` declarations in `cgi/shaders/*.wgsl` must match the Rust layout, but can declare a **prefix** of the CPU struct and simply omit trailing fields they don't read (wgpu enforces size at the binding, not shape). Practical consequence: you can add a field to `CameraUniform` and only update `shader.wgsl` — the other 13+ shaders that only read `material_override` continue to work unchanged.

Bind group layouts use `min_binding_size: None` (`bind_groups.rs`), so growing a uniform buffer is a no-op for layouts — but the Rust struct size must still match the WGSL side of the consuming shader.

### IBL update flows through one chokepoint
`IblState` has three constructors (`fallback`, `from_sky_colors`, `from_hdri`) — any IBL-derived CPU data (e.g. the L0 ambient) must be computed in **all three**. `rebuild_light_bind_group` in `state/update.rs` is the single chokepoint triggered on HDRI drop, IblMode toggle (`I` / `Shift+I`), and background change. Scene-wide IBL-derived uniforms are pushed to the GPU with a partial `queue.write_buffer` there so Clay modes etc. update instantly without waiting for the next camera-driven frame (which may not fire at all under Lock Lights).

### State plumbing shape
- `lights_from_camera` in `state/mod.rs` is called from 3 sites (init, `setup_split_secondary`, per-frame in `update.rs`); adding a parameter means updating all three.
- Sidebar ↔ state sync goes through `GuiSnapshot::{from_state, write_back_pane/display/post}` in `cgi/gui/snapshot.rs` — adding a sidebar control means adding a field to `GuiSnapshot` **and** wiring it through both `from_state` and the matching `write_back_*`. `SidebarChanges` (same file) is the flag struct the sidebar returns to signal which groups the caller needs to react to.
- `PaneDisplaySettings` is per-pane, `DisplaySettings` is global. Choose deliberately when adding new knobs — per-pane lets split-view compare modes, global is simpler and avoids per-pane write fanout.

### Other
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

## Release & packaging

Version is single-sourced in `[workspace.package]` in the root `Cargo.toml` and inherited by all four crates via `version.workspace = true`. Bumping the release version is a one-line edit.

**Binary installers** (shell / PowerShell / MSI) are produced by `cargo-dist` 0.31.0. Config lives in `dist-workspace.toml`; the CI workflow is the generated `.github/workflows/release.yml`. `dist` regenerates `wix/main.wxs` on every run, but the product-icon edit is preserved via `allow-dirty = ["msi"]` — do not remove that.

**Prerelease version format matters for MSI**: use dot-separated semver prereleases (e.g. `0.5.0-rc.1`, not `0.5.0-rc1`). WiX requires an `A.B.C.D` integer form and cargo-dist can only map the dotted form (`rc.1` → trailing `.1`). The single-identifier form fails the build at the MSI stage.

**Native bundles** (macOS `.dmg`, Linux `.deb` + `.rpm` + `.AppImage`) are produced by a separate reusable workflow, `.github/workflows/native-bundle.yml`, invoked from cargo-dist's generated `release.yml` via the `post-announce-jobs` hook in `dist-workspace.toml`. Running in-graph (not on `release: published`) is deliberate — the `release` event is not fired for releases created by `GITHUB_TOKEN`-authenticated actions, so a standalone workflow would never trigger. The heavy lifting is in the composite action at `.github/actions/native-bundle/action.yml`:
- macOS: hand-rolled `.app` + `Info.plist` + ad-hoc `codesign --sign -` + `create-dmg` (see comment in `dist-workspace.toml` for why this is *not* inside cargo-dist).
- Linux: `cargo-deb` for `.deb`; `cargo-generate-rpm` for `.rpm` (reads `[package.metadata.generate-rpm]` in Cargo.toml, mirroring the deb asset layout); `appimagetool` for x86_64 AppImage. aarch64 AppImage is deferred (0.6.0).

**Local dev smoke**:
- `scripts/build_local_dmg.sh` — mirrors the CI macOS bundle path end-to-end.
- `scripts/gen_placeholder_icons.sh` — regenerates every icon in `res/bundle/` (256/512/1024 PNG, `.icns`, multi-size `.ico`) from a Python-generated master PNG. Rerun after swapping in real icon art.

**Bundle assets** live in `res/bundle/`:
- Icons (`solarxy-{256,512,1024}.png`, `solarxy.png`, `solarxy.icns`, `solarxy.ico`)
- `linux/solarxy.desktop`, `linux/appimage/AppRun`
- `macos/Install CLI.command` (clears Gatekeeper quarantine on `/Applications/Solarxy.app` + sudo symlink into `/usr/local/bin`), `macos/READ ME FIRST.txt` (Gatekeeper walkthrough; filename chosen for top-of-DMG sort)

**Changelog**: `docs/changelog/CHANGELOG.md` (Keep a Changelog format). Not at the repo root.
