# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Solarxy is a cross-platform 3D model viewer, visual debugger, and validator built in Rust with wgpu (WebGPU). It ships as **two separate binaries**:

- `solarxy` — GUI viewer (winit + egui + wgpu, PBR rendering).
- `solarxy-cli` — CLI + TUI: `analyze` (model report / TUI), `preferences` (TUI editor), `docs` (embedded docs TUI), and `view` which shells out to the GUI binary.

The two are distributed separately (Flathub / Homebrew Cask / winget MSI for GUI; shell / PowerShell installers + Homebrew formula for CLI — no CLI MSI, matching the Rust-CLI convention). The CLI's `--update` flow detects the install channel and either self-updates via `axoupdater` or prints the package-manager command.

## Build & Run Commands

```bash
cargo build                                                    # Debug build (whole workspace)
cargo build --release                                          # Release build
cargo r --release -- --model res/models/xyzrgb_dragon.obj      # GUI viewer (root bin is always-GUI)
cargo r -p solarxy-cli --release -- --mode analyze -m X.obj    # Analyze mode (TUI or stdout)
cargo r -p solarxy-cli --release -- --mode preferences         # Preferences editor (TUI)
cargo r -p solarxy-cli --release -- --mode docs                # Built-in docs viewer (TUI)
cargo fmt                                                      # Format (see rustfmt.toml)
cargo clippy --all-targets                                     # Lint (pedantic + curated allows)
cargo test                                                     # All tests
cargo test -p solarxy-core                                     # One crate
cargo test -p solarxy-core validation::tests::                 # Filter by path
cargo test -p solarxy-formats --test loaders                   # Integration tests (tests/fixtures/)
RUST_LOG=solarxy=debug cargo r --release -- ...                # Verbose logging
```

**MSRV:** Rust 1.92. **Edition:** 2024.

**Feature flags** live on the inner crates, not the root:
- `solarxy-core`: `config` (default) gates `preferences`, `json`, `report`, `install_source`, `view_config` (the serde/toml/dirs-dependent modules).
- `solarxy-cli`: `tui` (default), `analyzer` (default), `updater` (default).
- `solarxy-app` and `solarxy-renderer`: no features — always link wgpu/winit/egui.
- Root `solarxy` binary: **no features**. GUI is always linked; there is no headless build of this crate.

## Architecture

**6-crate workspace:**

| Crate | Role |
|-------|------|
| `solarxy` (root) | Thin GUI entrypoint. `src/main.rs` parses its own small `GuiArgs`, sets up tracing, loads preferences, calls `solarxy_app::run_viewer`. |
| `solarxy-core` | Pure data types: `AABB`, `geometry`, `validation`, `preferences`, `report`, `view_config`, `json`, `install_source`. No GPU, no winit, no egui. |
| `solarxy-formats` | Format loaders (OBJ, STL, PLY, glTF/GLB) → `RawModelData`. Integration tests under `crates/solarxy-formats/tests/loaders.rs` + `tests/fixtures/`. |
| `solarxy-renderer` | All wgpu state: pipelines, bind groups, shaders, IBL, SSAO, bloom, shadow, composite, camera, per-frame draw (`frame.rs`), per-model GPU scene (`scene.rs`). No winit, no egui. |
| `solarxy-app` | winit `ApplicationHandler` + egui + `State`. Owns input, sidebar, menu, HUD, toasts, console, dialogs. Depends on `solarxy-renderer`. |
| `solarxy-cli` | clap `Args`, TUI apps (`tui_analysis`, `tui_docs`, `tui_preferences`), analyzer (`calc/analyze.rs`), its own `[[bin]]` at `src/bin/solarxy-cli.rs`. View mode spawns the `solarxy` GUI binary as a subprocess. |

Version is single-sourced in `[workspace.package]` and inherited via `version.workspace = true`. The `dist` profile inherits from `release` with `lto = "fat"`.

### `solarxy-app` internals (the interesting half)

- `app.rs` — `ApplicationHandler`, event loop, Tab toggles sidebar.
- `state/` — the app's central `State`:
  - `mod.rs` — struct definition, `Pane`, `PendingLoad`, `InputState`, wiring to `solarxy_renderer::{frame, scene}`.
  - `init.rs` — startup.
  - `update.rs` — per-frame updates, plus `rebuild_light_bind_group` (the **single IBL/lights chokepoint**; see Key Patterns).
  - `render.rs` — `State::render`, surface handling, per-pane orchestration (delegates draws into `solarxy-renderer`).
  - `panes.rs` — split-viewport geometry (`compute_panes`, layout math for F1/F2/F3).
  - `overlap.rs` — UV overlap GPU readback polling.
  - `capture.rs` — screenshot capture.
  - `input/` — `mod.rs` for keyboard/mouse, `dialogs.rs` (native file pickers via `rfd`), `menu_actions.rs` (menu bar → state).
  - `view_state.rs` — `ViewState` (the app-side bundle), re-exporting `ViewLayout`, `DisplaySettings`, `PaneDisplaySettings`, `BoundsMode` **from** `solarxy-core::view_config`.
- `gui/` — egui integration, one responsibility per file:
  - `renderer.rs` — `EguiRenderer` frame orchestration.
  - `sidebar.rs` — collapsible panels (View / Inspect / Material / Debug / Rendering / Advanced).
  - `menu.rs` — native-style menu bar (File / View / Analyze) with shortcut labels.
  - `snapshot.rs` — **`GuiSnapshot` (the sidebar ↔ state mirror)** and `SidebarChanges` flags, `HudInfo`.
  - `actions.rs` — `MenuActions` event flags.
  - `overlays.rs` — toasts (`ToastSeverity`), FPS/frame-time HUD, loading indicator.
  - `stats.rs` — `ModelInfo` + `draw_stats_window()`.
  - `console_view.rs` — docked/floating log viewer with level filter.
  - `update_modal.rs` — in-app update dialog.
  - `theme.rs`, `about.rs` — dark theme / About modal.
- `console.rs` — `LogBuffer` + `ConsoleLayer` (a `tracing::Layer` feeding the egui console).

### `solarxy-renderer` internals

- `frame.rs` — `Renderer`, `RenderTargets`, `PostProcessing`, `GradientUniform`, `WireframeResources`, `UvOverlapResources`, `ValidationColorResources`, `IblResources`. The thing `State` calls each frame.
- `scene.rs` — `ModelScene` (per-loaded-model GPU state: buffers, bind groups, shadow, validation map), `BackgroundModeExt`, `lights_from_camera`, `create_light_bind_group(_selective)`.
- `pipelines.rs` — every `wgpu::RenderPipeline`; built at startup, reused.
- `pipeline_builder.rs` — fluent `PipelineBuilder` to cut boilerplate in `pipelines.rs`.
- `bind_groups.rs` — `BindGroupLayouts`: the **single source of truth** for every layout used by pipelines (`min_binding_size: None`, so uniform growth is a no-op for layouts).
- `camera.rs` / `camera_state.rs` — orbit camera, per-pane camera bundle, `CameraUniform`.
- `light.rs` — `LightEntry`, `LightsUniform` (CPU side of lights + IBL ambient L0).
- `ibl.rs` — `IblState` with three constructors: `fallback`, `from_sky_colors`, `from_hdri`. `BrdfLut`.
- `ssao.rs`, `bloom.rs`, `shadow.rs`, `composite.rs` — post-FX + per-pane compositing (viewport/scissor + tone mapping).
- `visualization.rs` — grid, axes gizmo, bounds, normals.
- `model.rs`, `material.rs`, `texture.rs`, `uv_camera.rs`, `validation.rs`, `resources.rs`, `geometry.rs` — GPU resources + loaders.
- `shaders/` — 19 WGSL files (listed in the render pipeline below).

### Render pipeline (multi-pass, per pane in split mode)

1. Shadow pass (`shadow.wgsl`) — depth from key light.
2. GBuffer pass (`gbuffer.wgsl`, if SSAO) — position + normal.
3. Background pass (`background.wgsl`) — skybox / solid / gradient.
4. Main pass (`shader.wgsl`) — PBR + inspection-mode switch (Material ID, Texel Density, Depth).
5. Floor pass (`floor.wgsl`) — shadow-catching transparent floor.
6. Wireframe/ghosted overlays (`ghosted.wgsl`) and edge wireframe (`edge_wire.wgsl`, distinct pipeline).
7. Grid (`grid.wgsl`), normals (`normals.wgsl`), axis gizmo (`gizmo.wgsl`).
8. Validation overlay (`validation.wgsl`) — color-coded issue highlights.
9. SSAO (`ssao.wgsl` + `ssao_blur.wgsl`) + Bloom (`bloom.wgsl`) post-processing.
10. Composite pass (`composite.wgsl`) — tone mapping, viewport/scissor rect.
11. UV Map passes (UV panes): `uv_map.wgsl` (checker/texture/wire), `uv_debug.wgsl`, `uv_overlap.wgsl`.
12. egui overlay (sidebar, HUD, stats, toasts, update modal).

**Split viewport:** F1 (single), F2 (vertical), F3 (horizontal). Per-pane cameras, inspection modes, display settings; active pane by cursor position.

**Inspection modes** (number keys 1–5): Shaded, Material ID, UV Map, Texel Density, Depth.

**Material overrides** (`Shift+M` / sidebar) → `MaterialOverride::{None, Clay, ClayDark, Chrome, Silhouette}` → `camera.material_override` (0–4). Stylized, not physical; short-circuit paths in `fs_main` of `shader.wgsl`:
- Silhouette (4u): solid black early-return.
- Chrome (3u): skips all three direct lights; only samples the prefiltered env.
- Clay Light/Dark (1u/2u): directionless ambient from the L0 SH coefficient of the active IBL's irradiance map (`IblState::irradiance_average`, computed CPU-side in all three constructors, pushed to GPU via `LightsUniform.ibl_avg_{r,g,b}`); direct lights routed through `lambert_direct` to suppress the Cook-Torrance specular lobe.

## Key Patterns

### GPU uniform buffers are hand-laid-out
CPU structs (`CameraUniform` in `solarxy-renderer/src/camera.rs`, `LightsUniform` in `solarxy-renderer/src/light.rs`, and most `*Uniform` structs across `solarxy-renderer/src/`) are `#[repr(C)]` with explicit `_pad` fields picked to hit WGSL's 16-byte struct-size alignment. Several have a `const _: () = assert!(std::mem::size_of::<T>() == N);` — when extending a uniform, preserve the assert (repack padding) or update it in lockstep with the shader. WGSL `struct` declarations in `crates/solarxy-renderer/src/shaders/*.wgsl` must match the Rust layout but may declare a **prefix** of the CPU struct and omit trailing fields they don't read (wgpu enforces size at the binding, not shape). Practical consequence: you can add a field to `CameraUniform` and only update `shader.wgsl` — the other shaders that only read `material_override` keep working. Bind-group layouts use `min_binding_size: None` (`bind_groups.rs`), so growing a uniform is layout-invisible — but the Rust size still has to match the consuming shader's side.

### IBL update flows through one chokepoint
`IblState` has three constructors (`fallback`, `from_sky_colors`, `from_hdri`) — any IBL-derived CPU data (e.g. the L0 ambient) must be computed in **all three**. `rebuild_light_bind_group` in `solarxy-app/src/state/update.rs` is the single chokepoint triggered on HDRI drop, IblMode toggle (`I` / `Shift+I`), and background change. Scene-wide IBL-derived uniforms are pushed to the GPU with a partial `queue.write_buffer` there, so Clay modes etc. update instantly without waiting for the next camera-driven frame (which may not fire at all under Lock Lights).

### State plumbing shape
- `lights_from_camera` (now in `solarxy-renderer/src/scene.rs`) is called from three sites: `ModelScene` construction (scene.rs), `state/render.rs`, and `state/update.rs`. Adding a parameter means updating all three.
- Sidebar ↔ state sync goes through `GuiSnapshot::{from_state, write_back_pane/display/post}` in `solarxy-app/src/gui/snapshot.rs` — adding a sidebar control means adding a field to `GuiSnapshot` **and** wiring both `from_state` and the matching `write_back_*`. `SidebarChanges` (same file) is the flag struct the sidebar returns so the caller knows which groups to react to.
- `PaneDisplaySettings` (per-pane) vs `DisplaySettings` (global) — both live in `solarxy-core::view_config`. Per-pane lets split-view compare modes; global avoids per-pane write fanout. Pick deliberately.

### Cross-crate type ownership
Types used on **both** sides of the CPU/GPU boundary live in `solarxy-core` so both `solarxy-renderer` and `solarxy-app` can reach them without a cycle:
- `solarxy_core::view_config` — `ViewLayout`, `DisplaySettings`, `PaneDisplaySettings`, `BoundsMode`.
- `solarxy_core::preferences` — every enum shared by sidebar + shader (`MaterialOverride`, `InspectionMode`, `PaneMode`, `UvMapBackground`, `BackgroundMode`, `ToneMode`, `NormalsMode`, `UvMode`, `IblMode`, `ViewMode`).
- `solarxy_core::validation` — `ValidationReport`, `IssueKind`, `Severity`, etc.

The renderer re-exports a few things it owns (`frame::*`, `scene::*`) to the app via `solarxy_app::state::mod.rs` `pub(super) use` blocks — grep those imports when you need to know what the app is allowed to touch.

### Other
- wgpu bind groups for GPU resource access; pipelines created at init and reused.
- `Vertex` trait defines buffer layouts for different vertex types.
- Camera auto-frames model on load using AABB bounds.
- Resources loaded async with `pollster` blocking.
- Per-pane rendering with independent command encoders, viewport rects, scissor rects.
- egui sidebar bidirectionally synced with keyboard shortcuts.
- `help.rs` uses `include_str!` to embed content from `crates/solarxy-cli/content/*.txt`.
- Preferences live at `~/.config/solarxy/config.toml` (`dirs::config_dir()` + `solarxy/config.toml`); loaded via `solarxy_core::preferences::load()` on startup and surfaced in the viewer, the preferences TUI, and as keyboard-driven mutations saved with `Shift+S`.

## Formatting

`rustfmt.toml`: max width 100, 4-space indentation, Unix line endings, Rust 2024 edition, imports grouped by std/external/crate.

Each crate lints as `#![warn(clippy::pedantic)]` with a curated allow list at the top of its `lib.rs` (or `src/main.rs` for the root bin) — keep the allow lists consistent when moving code between crates, otherwise clippy will fire in the new home.

## Release & packaging

Version is single-sourced in `[workspace.package]` in the root `Cargo.toml`. Bumping release is a one-line edit.

**Prerelease version format matters for MSI**: use dot-separated semver prereleases (e.g. `0.5.0-rc.1`, not `0.5.0-rc1`). WiX requires an `A.B.C.D` integer form and cargo-dist can only map the dotted form (`rc.1` → trailing `.1`).

**Binary installers (CLI: `solarxy-cli`)** — shell / PowerShell / portable `.zip` — produced by `cargo-dist` 0.31.0. No CLI MSI: CLI MSIs aren't idiomatic on Windows (ripgrep, fd, zoxide, eza, bat, delta, cargo-dist itself don't ship one), so `[package.metadata.wix]` on `crates/solarxy-cli/Cargo.toml` is intentionally absent. Config in `dist-workspace.toml`; CI in the generated `.github/workflows/release.yml`. `dist` regenerates the root `wix/main.wxs` (GUI MSI) on every run; the product-icon edit is preserved via `allow-dirty = ["msi"]`.

**Native GUI bundles (`solarxy`)** — macOS `.dmg`, Linux `.deb` + `.rpm` + `.AppImage` — produced by `.github/workflows/native-bundle.yml`, invoked from cargo-dist's generated `release.yml` via the `post-announce-jobs` hook in `dist-workspace.toml`. In-graph (not `release: published`) is deliberate: `release` events don't fire for `GITHUB_TOKEN`-created releases. Heavy lifting is in the composite action `.github/actions/native-bundle/action.yml`:
- macOS: hand-rolled `.app` + `Info.plist` + ad-hoc `codesign --sign -` + `create-dmg` (see the comment in `dist-workspace.toml` for why this is *not* inside cargo-dist).
- Linux: `cargo-deb` (`.deb`), `cargo-generate-rpm` (`.rpm`, reads `[package.metadata.generate-rpm]`), `appimagetool` (x86_64 AppImage; aarch64 deferred to 0.6.0).

**Distribution channels:**
- GUI: **Flathub** (`dev.koljam.solarxy`, manifest in `packaging/flatpak/`), **Homebrew Cask** (`koljam/solarxy/solarxy`, `packaging/homebrew/`), **winget** (`Koljam.Solarxy`, `packaging/winget/`). Plus raw bundles from GitHub Releases.
- CLI: `cargo-dist` installers (shell / PowerShell + portable `.zip`), Homebrew formula (`solarxy-cli`). No MSI — winget CLI manifest (portable type) deferred to 0.5.1.
- `solarxy-cli --update` detects the install source via `solarxy_core::install_source::detect()`: Homebrew → `brew upgrade solarxy-cli`, Flatpak → `flatpak update dev.koljam.solarxy`, otherwise `axoupdater` self-update.

**Local dev smoke:**
- `scripts/build_local_dmg.sh` — mirrors the CI macOS bundle path end-to-end.
- `scripts/gen_placeholder_icons.sh` — regenerates every icon in `res/bundle/` (256/512/1024 PNG, `.icns`, multi-size `.ico`) from a Python-generated master PNG. Rerun after swapping in real icon art.

**Bundle assets** live in `res/bundle/`:
- Icons (`solarxy-{256,512,1024}.png`, `solarxy.png`, `solarxy.icns`, `solarxy.ico`).
- `linux/solarxy.desktop`, `linux/appimage/AppRun`.
- `macos/Install CLI.command` (clears Gatekeeper quarantine on `/Applications/Solarxy.app` + sudo symlink into `/usr/local/bin`), `macos/READ ME FIRST.txt` (Gatekeeper walkthrough; filename chosen for top-of-DMG sort).

**Changelog**: `docs/changelog/CHANGELOG.md` (Keep a Changelog format). Not at the repo root.
