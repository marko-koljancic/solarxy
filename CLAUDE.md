# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Solarxy is a cross-platform 3D model viewer, visual debugger, and validator built in Rust with wgpu (WebGPU). It ships as **two separate binaries**:

- `solarxy` — GUI viewer (winit + egui + wgpu, PBR rendering). Preferences live inside the GUI via `Edit → Preferences…` (`Ctrl/⌘+,`).
- `solarxy-cli` — CLI + TUI: `analyze` (model report / TUI) and `view` which shells out to the GUI binary. The `--mode preferences` and `--mode docs` variants were dropped from `OperationMode` in v0.5.x — `clap` rejects them with "invalid value". Preferences live in the GUI **Edit → Preferences…** dialog (`Ctrl/⌘+,`); user docs live in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki).

The two are distributed separately (Flathub + Homebrew Cask + winget + DMG / MSI / AppImage for GUI; shell / PowerShell installers + Homebrew formula + portable `.zip` for CLI — no CLI MSI, matching the Rust-CLI convention). winget GUI manifest landed in 0.6.0 at `packaging/winget/manifests/k/Koljam/Solarxy/<version>/` with a `{{PRODUCT_CODE}}` placeholder; `.github/workflows/winget-release.yml` extracts the per-build ProductCode from the signed MSI on each stable tag and submits to `microsoft/winget-pkgs`. The CLI's `--update` flow detects the install channel and either self-updates via `axoupdater` or prints the package-manager command.

## Build & Run Commands

```bash
cargo build                                                    # Debug build (whole workspace)
cargo build --release                                          # Release build
cargo r --release -- --model res/models/xyzrgb_dragon.obj      # GUI viewer (root bin is always-GUI)
cargo r -p solarxy-cli --release -- --mode analyze -m X.obj    # Analyze mode (TUI or stdout)
cargo r -p solarxy-cli --release -- --about                    # Print version / repo / license
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
- `solarxy-core`: `serialization` (default) gates `preferences`, `json`, `report`, `install_source`, `view_config` (the serde/toml/dirs-dependent modules). Renamed from `config` in rc.10 — the old name was ambiguous (compile-time config vs runtime config blob vs config-file I/O); `serialization` unambiguously names what's gated.
- `solarxy-cli`: `tui` (default), `analyzer` (default), `updater` (default).
- `solarxy-app` and `solarxy-renderer`: no features — always link wgpu/winit/egui.
- Root `solarxy` binary: **no features**. GUI is always linked; there is no headless build of this crate.

## Architecture

**7-crate workspace:**

| Crate | Role |
|-------|------|
| `solarxy` (root) | Thin GUI entrypoint. `src/main.rs` parses its own small `GuiArgs`, sets up tracing, loads preferences, calls `solarxy_app::run_viewer`. |
| `solarxy-core` | Pure data types: `AABB`, `geometry`, `validation`, `preferences`, `report`, `view_config`, `json`, `install_source`, `project_config`, `review`. No GPU, no winit, no egui. |
| `solarxy-formats` | Format loaders (OBJ, STL, PLY, glTF/GLB) → `RawModelData`. Integration tests under `crates/solarxy-formats/tests/loaders.rs` + `tests/fixtures/`. |
| `solarxy-renderer` | All wgpu state: pipelines, bind groups, shaders, IBL, SSAO, bloom, shadow, composite, camera, per-frame draw (`frame.rs`), per-model GPU scene (`scene.rs`). No winit, no egui. |
| `solarxy-app` | winit `ApplicationHandler` + egui + `State`. Owns input, sidebar, menu, HUD, toasts, console, dialogs. Depends on `solarxy-renderer`. |
| `solarxy-validate` | Validation orchestration + pipeline adapters (GitHub Actions / generic-JSON). Library API consumed by `solarxy-cli` and by external vendors who want structured validation results without a subprocess. `thiserror` per library convention; feature-gated `clap` derive (`features = ["clap"]`). |
| `solarxy-cli` | clap `Args`, analyze TUI (`tui_analysis`), analyzer (`calc/analyze.rs`), its own `[[bin]]` at `src/bin/solarxy-cli.rs`. `--mode view` spawns the `solarxy` GUI binary as a subprocess. |

Version is single-sourced in `[workspace.package]` and inherited via `version.workspace = true`. The `dist` profile inherits from `release` with `lto = "fat"`.

### `solarxy-app` internals (the interesting half)

- `app.rs` — `ApplicationHandler`, event loop. `Tab` toggles the Sidebar tab via `gui::dock::toggle_tab` — the same canonical add/remove helper every Window-menu and shortcut-driven panel toggle routes through. Also fires `flush_dock_layout_on_exit` on `WindowEvent::CloseRequested` to auto-save the current dock layout into preferences.
- `state/` — the app's central `State`:
  - `mod.rs` — struct definition, `Pane`, `PendingLoad`, `InputState`, wiring to `solarxy_renderer::{frame, scene}`.
  - `init.rs` — startup.
  - `update.rs` — per-frame updates, plus `rebuild_light_bind_group` (the **single IBL/lights chokepoint**; see Key Patterns).
  - `render.rs` — `State::render`, surface handling, per-pane orchestration (delegates draws into `solarxy-renderer`).
  - `panes.rs` — split-viewport geometry (`compute_panes`, layout math for F1–F5, including Quad and Three-Left-Big; each pane's content rect excludes the per-pane toolbar strip).
  - `overlap.rs` — UV overlap GPU readback polling.
  - `capture.rs` — screenshot capture.
  - `input/` — `mod.rs` for keyboard/mouse, `dialogs.rs` (native file pickers via `rfd`), `menu_actions.rs` (menu bar → state).
  - `view_state.rs` — `ViewState` (the app-side bundle), re-exporting `ViewLayout`, `DisplaySettings`, `PaneDisplaySettings`, `BoundsMode` **from** `solarxy-core::view_config`.
  - `review.rs` — `ReviewState` + `EditDraft` (in-memory mirror of the open `.solarxy-review.json`). `load_review_for_model` / `save_review_sidecar` handle disk I/O via `solarxy-core::review::ReviewFile`. Owns the **marker hit-test** (`marker_at_screen_pos`, 20 px screen-space threshold) and the **re-anchor sub-mode** (`begin_reanchor` / `cancel_reanchor` / `complete_reanchor` keyed by `reanchor_target: Option<String>`). Author defaults to anonymous; sidecar location honors `ProjectConfig.review.sidecar_dir` from `solarxy.toml`.
  - `raycast.rs` — CPU Möller-Trumbore + AABB slab-method. Single primitive feeding Review System anchoring, mesh-under-cursor hit-testing (Outliner hide / isolate, viewport context menu), and any future 3D ↔ UV selection sync.
  - `hdri_info.rs` — `HdriInfo` (filename / path / resolution / file size), populated on HDRI load for the Properties panel's HDRI section.
- `gui/` — egui integration, one responsibility per file:
  - `dock.rs` — `egui_dock` integration. Defines `SolarxyTab` (Viewport / Sidebar / ReviewPanel / Console / MaterialInspector / Properties / Outliner), `SolarxyTabViewer`, `default_dock_state`, and the `tab_present` / `toggle_tab` helpers every Window-menu toggle and panel shortcut routes through. The **Viewport tab is special**: non-floatable (`allowed_in_windows = false`), transparent (`clear_background = false` so the wgpu surface shows through), and closeable-but-restorable via `Window → Viewport`. The wgpu `compute_panes` math reads the Viewport tab's rect from the **previous** frame (`EguiRenderer::last_viewport_rect`) — a one-frame latency that's invisible at steady state but means the rect is briefly stale during resize / dock-rearrangement transients.
  - `renderer.rs` — `EguiRenderer` frame orchestration. Owns the active `Theme`, the toast queue (`VecDeque<Toast>`, cap 5 — cf. `TOAST_QUEUE_CAP`), the modals (preferences / update / screenshot / keyboard-shortcuts), console + material-inspector state, the persistent `DockState<SolarxyTab>`, and `last_viewport_rect`. Mirrors `dock::tab_present` into `MenuBarVisibility.*_visible` before drawing the menu bar each frame.
  - `sidebar.rs` — collapsible panels (Display / Post-Processing / Material). **Canonical surface for live display/rendering/material settings** — the preferences modal deliberately does not duplicate these; per-pane view controls live on the pane toolbar, validation/HDRI in the Properties panel.
  - `menu.rs` — native-style menu bar: **File / Edit / Render / Review / View / Layout / Window / Help**. `Review` (`draw_review_menu` — amber `● Review` when review mode is active) sits between `Render` and `View` because it is a viewport mode, not a utility; it carries the Review Mode / Show Markers toggles plus **Save Review Notes** (enabled when there are unsaved annotations). `Edit` carries `Preferences…` (`Ctrl/⌘+,`, `MenuActions::open_preferences`) and **`Save View Settings as Default`** (`save_view_defaults` — persists display/rendering/lighting prefs; replaces the removed `Shift+S` keybind). `Render` holds `Save Screenshot…` plus Inspection / Lighting submenus; `Layout` switches viewport layouts (F1–F5) and exposes `Save Layout` / `Restore Saved Layout` / `Reset Layout` (`save_dock_layout` / `reset_dock_layout` in `gui/actions.rs`; Restore disabled when no manual layout is saved). Per-pane menu items (Inspection / Projection / Show / Background) act on the **active pane**. The Window menu's panel-visibility checkmarks (Viewport / Sidebar / Outliner / Properties / Review Panel / Console / Material Inspector / Menu Bar / Status Bar) are **projected each frame from `dock::tab_present`** — `renderer.rs` mirrors them into `MenuBarVisibility.*_visible` before the menu draws and toggle clicks route through `dock::toggle_tab`, so the dock tree (not the menu flags) is the authoritative panel-visibility state. The Keyboard Shortcuts modal opens with `?`.
  - `snapshot.rs` — **`GuiSnapshot` (the sidebar ↔ state mirror)** and `SidebarChanges` flags, `HudInfo`.
  - `actions.rs` — `MenuActions` event flags (`open_model`, `open_hdri`, `open_preferences`, `open_shortcuts_modal`, `set_layout`, …).
  - `overlays.rs` — toast **queue** (bottom-center stacked, drop-oldest on overflow — see `EguiRenderer::push_toast` / `draw_toast_queue`), loading indicator, overdraw legend, `ToastSeverity`. (Frame-time / fps now live in the status bar, not a floating HUD.) Each `push_toast` emits a matching `tracing` event on `target: "solarxy::toast"` — callers must NOT also emit their own log for the same message, or the console records it twice.
  - `preferences_modal.rs` — tabbed GUI preferences dialog (**Startup / Appearance / View / Interface / Updater**). OK / Cancel / Reset semantics; Esc = Cancel. Scope is **fields the sidebar can't reach at runtime** (window size, MSAA), the theme choice (Appearance tab), custom-background CRUD (View tab), plus `UiPrefs` + `UpdaterPrefs` sections. Startup tab shows the config file path and an **Open config file** button. Commits via `take_committed_prefs()` drained by `state/render.rs` after `render_ui`. Draggable (not pinned).
  - `keyboard_shortcuts_modal.rs` — read-only reference window listing every binding, grouped by category (File / Window & Layout / Navigation / Shading & Inspection / Show & Overlays / Mesh Visibility / Review / Lighting & Post-Processing). Opens via `?`. Dismiss with Esc or the window X. Draggable. User-remappable shortcuts land in a future release.
  - `pane_toolbar.rs` — 3ds Max-style **viewport label menus**: a few frameless bracketed text labels (`label_menu` — `[ Scene 3D ]` / `[ Shaded ]` / `[ Perspective ]`; a UV pane shows `[ UV Map ]` / `[ Display ]`) that **float directly on the 3D scene** — no strip fill — and open an Ayu dropdown with nested submenus on click. The `Shaded` label = the effective display mode via `display_label` (Inspection / Override are submenus under it); projection / Overlays / Background are submenus under `Perspective`. `style_frameless_labels` scopes the `menu_button`s to bare text (no fill any state), with idle `theme.fg` shifting to the amber accent on hover / open. The 3D scene renders the **full pane** (`render_pane` / `target_dimensions` / `build_review_panes` no longer reserve the 22 px `PANE_TOOLBAR_HEIGHT` strip — that const now only feeds `pointer_in_pane_content` + screenshot capture, keeping the top 22 px a camera-dead-zone where the labels sit). The active pane writes through `GuiSnapshot`, other panes write `&mut [PaneDisplaySettings; 4]` directly. `background_combo` (a `ComboBox`) is retained here only for the Preferences modal's custom-background editor.
  - `status_bar.rs` — single-line bottom status bar (replaced the floating FPS HUD): filename·format, validation counts, review badge, active-pane label, ms/fps, backend; responsive collapse at 600 / 900 / 1200 px. Registered as a `TopBottomPanel::bottom` **before** `DockArea::show` (it's a stock panel, styled explicitly — `make_dock_style` doesn't cover it).
  - `properties.rs` — the Properties dock tab (replaced `stats.rs`): Model / HDRI / Validation sections. `ModelInfo` lives here; a validation row click flies the active camera to the affected mesh.
  - `outliner.rs` — the Outliner dock tab: Meshes + Materials lists with per-row hide toggles, right-click context actions, and click-to-frame. Mesh visibility is the per-`Mesh` `visible` bool.
  - `screenshot_modal.rs` — the `C`-key screenshot review modal: downscaled preview, optional expand-all-review-notes recapture, `Save As…` PNG via `rfd`. Nothing is written until the user picks a path.
  - `viewport_context_menu.rs` — right-click-in-viewport mesh menu (Hide / Hide Others / Show All / Frame this).
  - `console_view.rs` — docked/floating log viewer with level filter, message-content substring search, and right-click Copy message / Copy full line. Buffer captures `solarxy=trace` by default; UI dropdown shows ERROR/WARN/INFO/DEBUG.
  - `update_modal.rs` — in-app update dialog. Draggable (not pinned).
  - `review_popup.rs` — floating new/edit annotation popup anchored at the click position. `Cmd/Ctrl+Enter` saves, `Esc` cancels (egui-consumed before reaching `state/input`).
  - `review_overlay.rs` — screen-space marker overlay (pins + expand-on-hover sprite cards with leader lines). Replaced the previous WGSL `review_marker` pipeline with an egui-painted layer: free text rendering, one-call pane clipping, no z-fight against bloom/SSAO. Driven by `ReviewPaneOverlay { egui_rect, view_proj }` slices built per-active-3D-pane in `state/render.rs` (UV panes are filtered out upstream). Markers no longer participate in post-processing — a deliberate readability win over the old SDF-shape-vs-bloom interaction.
  - `review_visuals.rs` — the per-category letter glyphs (`i` / `!` / `?` / `✎`) and labels, plus `category_color(theme, …)` which reads the four category colors from `Theme.review` (`ReviewColors`) so they re-contrast on the light theme. Consumed by `review_panel` (chips) and `review_overlay` (pins + cards) — drift between marker color and panel chip color is the user's #1 visual-correlation cue, keep them in sync.
  - `review_panel.rs` — docked-or-floating side panel (Console pattern). Category filter chips, text search, three collapsible sections (Open / Needs re-anchor / **Complete**), a header **Markers** toggle (`markers_hidden`), inline selected-annotation editor with Reply/Delete/Re-place buttons, cascade-delete confirmation modal. "Complete" is a UI-string-only rename — the on-disk field stays `resolved: bool`. `MenuBarVisibility.review_panel_visible` is the canonical visibility write target.
  - `material_inspector.rs` — view-only **master/detail** panel (`Window → Material Inspector`, a dock tab). A compact material picker list (base-color swatch + name + 5-slot texture-presence indicator) beside a detail pane (scalar PBR + a 128 px thumbnail per texture role: Albedo / Normal / Metallic-Roughness / Occlusion / Emissive); the split flips side-by-side ↔ stacked with the dock shape. Thumbnails decode lazily from `Model::material_thumbnails` into a per-model `(material_idx, role)` cache, dropped on model swap via `EguiRenderer::reset_material_inspector`. "Open externally" via `open::that()`; disabled for embedded textures. **No 3D-viewport mutation.**
  - `theme.rs` — the `Theme` struct: two Ayu Mirage-inspired flat presets (`ayu_mirage_dark` default, `ayu_mirage_light`), chosen by `UiPrefs::theme` (`ThemeChoice`) and **hot-swapped** from the preferences modal with no restart. Carries every UI token (bg / fg / accent / selection / widget colors plus `bg_elevated`, `border`, `severity_*`, and a `review` sub-struct holding the four annotation-category colors — the single source of truth migrated out of `review_visuals`). Entry points: `apply_theme(ctx, &Theme)` (egui `Visuals` + widget palette), `make_dock_style(ctx, &Theme)` (the paired `egui_dock::Style`), `configure_fonts` (bundles Lilex), and the `PANE_TOOLBAR_HEIGHT` const shared with `compute_panes`. Zero corner radii throughout; the amber accent doubles as the review-mode stripe.
  - `about.rs` — About modal (reference pattern for Esc-dismissable non-modal egui windows). Draggable.
- `console.rs` — `LogBuffer` + `ConsoleLayer` (a `tracing::Layer` feeding the egui console). `ConsoleState` carries the UI-side level filter, search string, docked/floating flag.

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
- `skybox.rs` — HDRI sky pass; keeps the source equirectangular texture after IBL convolution so `BackgroundMode::HdriSky` can render it as a backdrop.
- `overdraw.rs` — overdraw inspection (count + visualize passes).
- `shaders/` — 22 WGSL files (listed in the render pipeline below).

### Render pipeline (multi-pass, per pane in split mode)

1. Shadow pass (`shadow.wgsl`) — depth from key light.
2. GBuffer pass (`gbuffer.wgsl`, if SSAO) — position + normal.
3. Background pass — `background.wgsl` (solid / gradient) or `skybox.wgsl` (HDRI sky, when the pane's `background_mode` is `HdriSky`).
4. Main pass (`shader.wgsl`) — PBR + inspection-mode switch (Material ID, Texel Density, Depth).
5. Floor pass (`floor.wgsl`) — shadow-catching transparent floor.
6. Wireframe/ghosted overlays (`ghosted.wgsl`) and edge wireframe (`edge_wire.wgsl`, distinct pipeline).
7. Grid (`grid.wgsl`), normals (`normals.wgsl`), axis gizmo (`gizmo.wgsl`).
8. Validation overlay (`validation.wgsl`) — color-coded issue highlights.
9. SSAO (`ssao.wgsl` + `ssao_blur.wgsl`) + Bloom (`bloom.wgsl`) post-processing.
10. Composite pass (`composite.wgsl`) — tone mapping, viewport/scissor rect.
11. UV Map passes (UV panes): `uv_map.wgsl` (checker/texture/wire), `uv_debug.wgsl`, `uv_overlap.wgsl`.
12. egui overlay (menu bar, per-pane toolbars, docked panels, status bar, toasts, modals).

**Split viewport:** F1 (single), F2 (vertical split), F3 (horizontal split), F4 (quad — 2×2), F5 (three-left-big). Per-pane cameras, inspection modes, display settings, and background; active pane by cursor position. The HDR render target is pane-sized and reused per pane.

**Inspection modes** (number keys 1–7): Shaded, Material ID, UV Map, Texel Density, Depth, Overdraw (`overdraw_count.wgsl` / `overdraw_show.wgsl`), AO Preview.

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
- `solarxy_core::preferences` — every enum shared by sidebar + shader (`MaterialOverride`, `InspectionMode`, `PaneMode`, `UvMapBackground`, `ToneMode`, `NormalsMode`, `UvMode`, `IblMode`, `ViewMode`). `BackgroundMode` is a tagged sum (`Builtin(BuiltinBg)` | `Custom(u32)`), resolved to colors against `Preferences::view::custom_backgrounds`.
- `solarxy_core::validation` — `ValidationReport`, `IssueKind`, `Severity`, etc.

The renderer re-exports a few things it owns (`frame::*`, `scene::*`) to the app via `solarxy_app::state::mod.rs` `pub(super) use` blocks — grep those imports when you need to know what the app is allowed to touch.

### Dock layout persistence
All six panels (Sidebar / Review Panel / Console / Material Inspector / Properties / Outliner) plus the Viewport live as tabs in a single `egui_dock::DockState<SolarxyTab>` owned by `EguiRenderer`. `solarxy_core::preferences::DockPrefs` holds two `Option<String>` JSON blobs that serialize that state via `egui_dock`'s `serde` feature (workspace dep `egui_dock = "0.18"` with `features = ["serde"]`):

- `last_layout_json` — auto-saved on app quit (`State::flush_dock_layout_on_exit` in `state/input/mod.rs`, called from `app.rs` on `WindowEvent::CloseRequested`). Restored on startup in `state/init.rs` so the window comes back exactly how you left it. Write is short-circuited if the JSON hasn't changed.
- `saved_layout_json` — only ever written by `Window → Save Layout`; never overwritten automatically. `Window → Restore Saved Layout` reads it back; the menu entry stays disabled when it's `None` (driven by `EguiRenderer::has_saved_layout`, mirrored from `Preferences.dock.saved_layout_json.is_some()` at startup).

If you add a `SolarxyTab` variant, **handle the serde compatibility carefully** — old `last_layout_json` blobs in users' `config.toml` will fail to deserialize otherwise, and the silent fallback is the default layout (data not lost, but the user loses their arrangement). `Window → Reset Layout` calls `EguiRenderer::reset_dock_layout` to rebuild from `default_dock_state` in `gui/dock.rs` without touching either persisted blob — that's the user-facing escape hatch when a layout gets wedged.

### Review System click routing
Left-click in review mode (`Shift+R`) walks a three-step ladder in `state/input/mod.rs::try_review_pick`:
1. **Re-anchor pending** (`review.reanchor_target.is_some()`) → raycast → `complete_reanchor`; consumes the click unconditionally so a miss doesn't fall through to creation.
2. **Marker hit-test** — project visible markers to pane-relative pixels; within ~20 px of cursor wins. Sets `selected` + flips `scroll_to_selected` so the panel jumps to that row. Resolved markers stay hit-testable (they render dimmed, not hidden).
3. **Geometry raycast** — Möller-Trumbore → open `EditDraft` popup at the click position.

`Esc` runs an analogous priority chain inside `gui/renderer.rs::render_ui` (after the popup / delete-confirm modal have had their own chance to consume): re-anchor cancel → review-mode exit. Each consumes the key and emits a toast via `MenuActions.cancel_reanchor` / `exit_review_mode`. `Cmd/Ctrl+S` while review-mode-active saves the sidecar.

### Other
- wgpu bind groups for GPU resource access; pipelines created at init and reused.
- `Vertex` trait defines buffer layouts for different vertex types.
- Camera auto-frames model on load using AABB bounds.
- Resources loaded async with `pollster` blocking.
- Per-pane rendering with independent command encoders, viewport rects, scissor rects.
- egui sidebar bidirectionally synced with keyboard shortcuts.
- Preferences live at `~/.config/solarxy/config.toml` (`dirs::config_dir()` + `solarxy/config.toml`); loaded via `solarxy_core::preferences::load()` on startup. Three edit surfaces, each authoritative for a different slice: **the sidebar** (plus `Edit → Save View Settings as Default`) for live per-session display/rendering/material settings; **`Edit → Preferences…` modal (`Ctrl/⌘+,`)** for startup-only fields (window size, MSAA), the theme choice (`UiPrefs::theme`), custom backgrounds, UI visibility defaults, recent-files capacity, and updater behaviour; **direct TOML editing** via the Preferences modal's **Open config file** button (Startup tab). `Preferences::ui` (`UiPrefs`), `Preferences::updater` (`UpdaterPrefs` + `UpdaterChannel`), and `Preferences::view` (`ViewPrefs` — custom backgrounds + default background) all default via `#[serde(default)]` so older `config.toml` files upgrade cleanly. Use `config_path()` to resolve the platform-specific path.

## Performance

Performance baseline + profiling notes from rc.11 are deferred to 0.6.0; measurements are filled in on maintainer hardware as hot paths are profiled. The previous `docs/perf/` skeleton was removed alongside the v0.5.0 documentation overhaul.

## Formatting

`rustfmt.toml`: max width 100, 4-space indentation, Unix line endings, Rust 2024 edition, imports grouped by std/external/crate.

Each crate lints as `#![warn(clippy::pedantic)]` with a curated allow list at the top of its `lib.rs` (or `src/main.rs` for the root bin) — keep the allow lists consistent when moving code between crates, otherwise clippy will fire in the new home.

## Release & packaging

Version is single-sourced in `[workspace.package]` in the root `Cargo.toml`. Bumping release is a one-line edit.

**Prerelease version format matters for MSI**: use dot-separated semver prereleases (e.g. `0.5.0-rc.1`, not `0.5.0-rc1`). WiX requires an `A.B.C.D` integer form and cargo-dist can only map the dotted form (`rc.1` → trailing `.1`).

**Binary installers (CLI: `solarxy-cli`)** — shell / PowerShell / portable `.zip` — produced by `cargo-dist` 0.31.0. No CLI MSI: CLI MSIs aren't idiomatic on Windows (ripgrep, fd, zoxide, eza, bat, delta, cargo-dist itself don't ship one), so `[package.metadata.wix]` on `crates/solarxy-cli/Cargo.toml` is intentionally absent. Config in `dist-workspace.toml`; CI in the generated `.github/workflows/release.yml`. `dist` regenerates the root `wix/main.wxs` (GUI MSI) on every run; the product-icon edit is preserved via `allow-dirty = ["msi"]`.

**Native GUI bundles (`solarxy`)** — macOS `.dmg` + Linux `.AppImage` — produced by `.github/workflows/native-bundle.yml`, invoked from cargo-dist's generated `release.yml` via the `post-announce-jobs` hook in `dist-workspace.toml`. In-graph (not `release: published`) is deliberate: `release` events don't fire for `GITHUB_TOKEN`-created releases. Heavy lifting is in the composite action `.github/actions/native-bundle/action.yml`:
- macOS: hand-rolled `.app` + `Info.plist` + ad-hoc `codesign --sign -` + `create-dmg`.
- Linux: `appimagetool` (x86_64 AppImage only; aarch64 deferred to 0.7.0+ pending upstream arm64 stable binary).
- `.deb` + `.rpm` were dropped in rc.7 in favour of Flathub for distro-agnostic coverage; community packagers can still build native packages from source.

**Distribution channels:**
- GUI: **Flathub** (`dev.koljam.solarxy`, manifest in `packaging/flatpak/`), **Homebrew Cask** (`koljam/solarxy/solarxy`, `packaging/homebrew/`), **winget** (`Koljam.Solarxy`, manifests in `packaging/winget/manifests/k/Koljam/Solarxy/<version>/` with `{{PRODUCT_CODE}}`/`{{INSTALLER_SHA256}}` placeholders filled by `.github/workflows/winget-release.yml` on each stable tag). Plus raw DMG / MSI / AppImage bundles from GitHub Releases.
- CLI: `cargo-dist` installers (shell / PowerShell + portable `.zip`), Homebrew formula (`solarxy-cli`). No MSI — winget CLI manifest (portable type) still deferred (Rust-CLI convention: ripgrep, fd, zoxide, eza, bat, delta, cargo-dist itself don't ship one either).
- `solarxy-cli --update` detects the install source via `solarxy_core::install_source::detect()`: Homebrew → `brew upgrade solarxy-cli`, Flatpak → `flatpak update dev.koljam.solarxy`, otherwise `axoupdater` self-update.

**Local dev smoke:**
- `packaging/scripts/build_local_dmg.sh` — mirrors the CI macOS bundle path end-to-end.
- `packaging/scripts/gen_bundle_icons.sh` — regenerates every icon in `res/bundle/` (256/512/1024 PNG, `.icns`, multi-size `.ico`) from a Python-generated master PNG. Rerun after swapping in real icon art.

**Bundle assets** live in `res/bundle/`:
- Icons (`solarxy-{256,512,1024}.png`, `solarxy.png`, `solarxy.icns`, `solarxy.ico`).
- `linux/solarxy.desktop`, `linux/appimage/AppRun`.
- `macos/Install CLI.command` (clears Gatekeeper quarantine on `/Applications/Solarxy.app` + sudo symlink into `/usr/local/bin`), `macos/READ ME FIRST.txt` (Gatekeeper walkthrough; filename chosen for top-of-DMG sort).

**Release notes**: maintained in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki/Release-Notes). No in-repo `CHANGELOG.md`; cargo-dist sources GitHub Release bodies from its own manifest, not a repo file.


## Working Agreement

- **Implementation according to best practices**, as a senior software engineer and computer graphics domain expert in Rust. For UI work, also as a UX/UI expert.
- **Ask for clarifications until at least 90% sure** of intent before implementing. The user can clarify or provide decision guidance. Craft a detailed, objective plan before writing code.
- **No `unwrap` / `expect` outside of tests.** Use `?` with `anyhow` (app/CLI) or `thiserror` (library crates) and add context via `.context(...)` where it helps debugging.
- **Library crates use `thiserror`; binary crates use `anyhow`.** Do not pull `anyhow` into `solarxy-core` (except behind the `serialization` feature), `solarxy-formats`, or `solarxy-renderer` as a public dependency.
- **Distinguish current state from planned refactors.** The 0.6.0 milestone plan describes future work (pipeline sub-struct grouping, `GuiSnapshot::apply_to_state` consolidation, doc-comment coverage). Do not refactor toward planned-but-unscheduled work without surfacing it first.
- **Surface findings before unilaterally refactoring.** Multi-file refactors get a plan first.

## Audit

Run `/solarxy-audit` for a full code-quality sweep covering Rust idioms, wgpu + WGSL correctness, egui patterns, ratatui (analyze TUI only), architecture, performance & safety, workspace hygiene, and (when relevant) cross-platform readiness. The audit runs in an isolated subagent so the report does not pollute the main session. See `.claude/skills/solarxy-audit/SKILL.md`.
