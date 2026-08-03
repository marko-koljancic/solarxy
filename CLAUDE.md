# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Solarxy is a cross-platform 3D model viewer, visual debugger, and validator built in Rust with wgpu (WebGPU). It ships as **two separate binaries**:

- `solarxy` — GUI viewer (winit + egui + wgpu, PBR rendering). Preferences live inside the GUI via `Edit → Preferences…` (`Ctrl/⌘+,`).
- `solarxy-cli` — CLI + TUI: `analyze` (model report / TUI) and `view` which shells out to the GUI binary. The `--mode preferences` and `--mode docs` variants were dropped from `OperationMode` in v0.5.x — `clap` rejects them with "invalid value". Preferences live in the GUI **Edit → Preferences…** dialog (`Ctrl/⌘+,`); user docs live in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki).

The two are distributed separately (Flathub + Homebrew Cask + winget + DMG / MSI / AppImage for GUI; shell / PowerShell installers + Homebrew formula + portable `.zip` for CLI — no CLI MSI, matching the Rust-CLI convention). winget GUI manifest lives at `packaging/winget/manifests/k/Koljam/Solarxy/<version>/` (3 YAML files) with exactly one release-time placeholder, `{{INSTALLER_SHA256}}`, which `.github/workflows/winget-release.yml` fills from the real MSI on each stable tag before submitting to `microsoft/winget-pkgs`. ProductCode is deliberately omitted (it rotates every WiX build and winget reads it from the MSI itself). **The version directory must be committed BEFORE the tag is pushed** — the workflow hard-throws `"No winget manifest at ..."` otherwise. The CLI's `--update` flow detects the install channel and either self-updates via `axoupdater` or prints the package-manager command.

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

**Web shell (Phase 4 of the web-expansion milestone):**

```bash
bash crates/solarxy-web/build-wasm.sh web/src/wasm/pkg          # Build wasm -> wasm-bindgen -> wasm-opt into web/
cargo build -p solarxy-web --target wasm32-unknown-unknown      # Compile the wasm host (native build is near-empty)
cargo clippy -p solarxy-web --target wasm32-unknown-unknown -- -D warnings
cd web && npm install && npm run dev                            # Vite dev server (predev rebuilds the wasm)
cd web && npm run typecheck && npm test && npm run build        # tsc + vitest + production bundle
```

The web frontend needs a served secure context (localhost qualifies) for WebGPU + OPFS. `web/src/wasm/pkg/` is the build output (gitignored); regenerate it with `build-wasm.sh`. The `getrandom_backend="wasm_js"` rustflag for `wasm32` lives in the workspace `.cargo/config.toml`.

**MSRV:** Rust 1.92. **Edition:** 2024.

**Feature flags** live on the inner crates, not the root:
- `solarxy-core`: `serialization` (default) gates `preferences`, `json`, `report`, `install_source`, `view_config` (the serde/toml/dirs-dependent modules). Renamed from `config` in rc.10 — the old name was ambiguous (compile-time config vs runtime config blob vs config-file I/O); `serialization` unambiguously names what's gated.
- `solarxy-cli`: `tui` (default), `analyzer` (default), `updater` (default).
- `solarxy-app` and `solarxy-renderer`: no features — always link wgpu/winit/egui.
- Root `solarxy` binary: **no features**. GUI is always linked; there is no headless build of this crate.
- `solarxy-graph`: no default features; pure serde. `solarxy-web`: no default features.

### Web shell internals (`solarxy-web` + `web/`)

- **One core, two shells.** `solarxy-graph` (engine) and `solarxy-renderer` never depend on each other; they meet only at `solarxy_core::scene::SceneDelta`. On web both compile into one wasm instance, so cooked geometry is an in-memory `Arc` handoff and **never crosses into JavaScript** — only `Command`s, `EventBatch`es, snapshots, and asset bytes do.
- **Mirror-and-command.** Rust owns all document state. `web/` is a display mirror: React Flow gestures dispatch `Command`s; the returned `EventBatch` (and each frame's cook batch) is applied to a zustand mirror store (`web/src/store/mirror.ts`); a monotonic `revision` detects desync and recovers by calling `snapshot()`. Boundary types are hand-authored in `web/src/engine/types.ts` and pinned to the Rust serde shapes (all camelCase) by `command_boundary_json_shape_is_camelcase`.
- **Registry-driven UI (zero-frontend-change contract).** The palette, typed handles, and parameter panel are pure interpreters of the `RegistrySnapshot` (`web/src/registry/datatypes.ts`, `flow/FlowNode.tsx`, `components/ParameterPanel.tsx`, `components/NodePalette.tsx`). A node added in Rust needs zero `web/` changes; a new `ParamType`/`DataType` variant is a deliberate frontend change. Guarded by `web/src/registry/extensibility.test.ts`.
- **The web host drives the real renderer (Phase 6).** `crates/solarxy-web/src/app.rs` runs `Renderer` + `SceneObjects` + `SceneEnvironment` with per-pane `CameraState` (F1-F5 layouts, per-pane encoders into a shared HDR target, composite per viewport rect, sRGB via `view_formats`). View state is **host-owned, not engine-owned**: mutators return a full `ViewStateDto`, async happenings ride `HostEvent`s (`paneRects`, `activePane`, `uvOverlap`, `viewChanged`) drained once per frame into `web/src/store/viewState.ts`. Validation flows as engine events (`ValidationSummary`/`ValidationReport`) plus `SceneOp::SetValidation` lowering; the UV pane sources the selected node's cooked geometry (`Engine::selected_geometry` against the mirrored canvas context) with the overlap statistic read back asynchronously; the HDRI environment prepares CPU-side in the worker (`prepare_hdri_job` -> `PreparedHdri`) and finishes on the GPU (`set_environment_prepared` -> the ported `rebuild_light_bind_group` chokepoint). The tsify `.d.ts` layer remains deferred in favor of the hand-authored `types.ts`.
- **Persistence.** OPFS autosave (ring of 3, debounced 2s / max 15s) via `web/src/persistence/opfs.ts`; recovery prompt on load; `beforeunload` guard; explicit save/open via the File System Access API (download / file-input fallback). Phase 5 stores the full `.slxy` archive bytes (document + camera + embedded assets), so recovery is one `load_slxy`.
- **Imports (Phase 5).** Model files are staged content-addressed (`stage_asset`); an import node's cook yields a `ParseModel` job that the session pump (`web/src/engine/session.ts`) hands to the **import Web Worker** (`web/src/engine/importWorker.ts`, a second headless `solarxy_web.wasm` instance running the GPU-free `parse_model_job` export). The worker returns a `GeometrySet` transfer blob (`solarxy-kernel::transfer`, geometry-only) committed under the generation guard via `submit_parsed_model`. Draco-compressed glTF is rejected in the worker with a clear message (decision-18 cut-line), surfaced as a toast like every worker parse error. Folder drops traverse `webkitGetAsEntry` (`web/src/persistence/dropEntries.ts`); the `assetRef` widget is multi-select (stages every companion, the param points at the primary).
- **Review + polish (Phase 7).** In-scene review lives in `solarxy-graph::review`: anchors (ctx/node/mesh/face/barycentric + world fallback) with runtime staleness via `geometry_hash` (**53-bit masked** so it survives the JS number boundary), flat reply threading (replies inherit the parent anchor engine-side), cascade delete, re-anchor. Markers are DOM pins positioned per frame in pane-relative CSS px through `web/src/engine/markers.ts` (imperative registry, no React re-render). Screenshots: `solarxy-renderer/src/capture.rs` (padded readback + non-blocking poll) rendered offscreen by the host at a **4.0 MP budget** (larger captures can lose the WebGPU device; no device-loss recovery on web yet). The keymap table (`web/src/input/keymap.ts`) is the single source of truth: the shortcuts modal is generated from it, and context resolution is viewport > canvas > global (pointer-over flags). The exclusive-shadow-caster rule is engine-side: granting `cast_shadow` on a root light clears all others in one undo step.
- **UX chrome (Phase 7b).** Node fills are pastel category tokens (values generated from the Rust palette into `tokens.generated.css`, imported by `tokens.css`); edges color by the source port's `DataType`; the hover radial (`web/src/flow/RadialMenu.tsx`, ~400 ms dwell) and the modeless draggable `NodeInfoModal` are pure mirror consumers; the **note node is the single non-registry component** (`NoteNode.tsx`, keyed on `typeId === "note"`, writes the existing engine params). Auto-layout (dagre + ELK, `web/src/flow/layout.ts`) dispatches ONE `moveNodes` command. The parameter panel renders param groups as underline tabs (Validation is a gated tab); viewport panes use ghost bracketed label menus (`PaneToolbar.tsx`), with UV Layout enterable from the Display dropdown. Desks (`web/src/store/desks.ts` + ui-store arrangement) snapshot app chrome only; `SplitPane` children are **keyed** so a viewport side swap moves the canvas DOM node instead of remounting it (a remount would lose the WebGPU surface). React Flow's double-click zoom is disabled (dblclick = container dive / note edit).

## Architecture

**13-crate workspace + the `web/` frontend** (the web-expansion milestone added `solarxy-kernel`, `solarxy-graph`, `solarxy-scenefile`, `solarxy-web`, and `web/`):

| Crate | Role |
|-------|------|
| `solarxy` (root) | Thin GUI entrypoint. `src/main.rs` parses its own small `GuiArgs`, sets up tracing, loads preferences, calls `solarxy_app::run_viewer`. |
| `solarxy-core` | Pure data types: `AABB`, `geometry`, `validation`, `preferences`, `report`, `view_config`, `json`, `install_source`, `project_config`, `review`, `scene` (the GPU-free `SceneDelta`/`CookedGeometry`/`LightDef` engine-renderer contract), `raycast` (Moller-Trumbore, used by web picking), and `theme` (the ungated shared interface palette: two-tier primitives + roles driving the egui GUI, the analyze TUI, and — via `examples/gen_tokens.rs` — the web's `tokens.generated.css`; drift-guarded by `tests/tokens_drift.rs`). Feature-gated: `serde` (wasm-facing), `fs`, `serialization`. No GPU, no winit, no egui. |
| `solarxy-formats` | Format loaders (OBJ, STL, PLY, glTF/GLB) → `RawModelData`, plus `hdr` (Radiance/OpenEXR → `RawImageHdr`) and `lut` (Adobe `.cube` → `LutCube`). Byte-first `load_*_bytes` API always available; `std-fs` (default) adds the path wrappers, off for wasm. |
| `solarxy-imaging` | Pure-CPU image operators for the texture context (phase 19): adjust/composite/generate/filter/ORM ops over `RawImageData`, deterministic, single-threaded, wasm-clean. |
| `solarxy-kernel` | Pure-CPU parametric geometry: `GeometrySet`/`KernelMesh` (per-buffer `Arc`, converged with `core::scene`), the 7 primitive generators, transform bake, merge. wasm-clean, no wgpu/fs. |
| `solarxy-graph` | The headless studio core: document/topology/cook engine/registry (77 registered node types, test-asserted in `nodes/mod.rs`: the MVP set plus the texture/material/output contexts and the scene environment, all behind the typed-port coercion matrix + declarative param schemas)/undo/review, and the `Engine` facade (`Command` in, `EventBatch` out, budgeted resumable cook, `take_scene_delta`, `pick`, `invoke_action`, JSON `save_document`/`load_document`). Typed contexts (`ContextKind { Obj, Geo, Mat, Tex }` on every graph; containers declare `opens`; placement judged by graph kind; cross-context data travels by path reference with cycle refusal at set time and reference-ordered cooking). Mirror-and-command model. No wgpu, no winit. |
| `solarxy-scenefile` | The `.slxy` self-contained scene file: the schema-owned `SceneJson`/`manifest.json` serde+schemars types (section 6.6), the ZIP container with content-addressed `assets/<sha256>` blobs, SHA-256 integrity, and the `schema_version`/`min_reader` migration gate. `zip` is pulled `default-features = false` (Stored entries) so it compiles to `wasm32`. `solarxy-graph` maps a live document to/from these types (`engine/scenefile.rs`); this crate never depends on the engine. |
| `solarxy-renderer` | All wgpu state: pipelines, bind groups, shaders, IBL, SSAO, bloom, shadow, composite, camera, per-frame draw (`frame.rs`), per-model GPU scene (`scene.rs`), the multi-object `SceneObjects` path (`scene_objects.rs`, consumes `SceneDelta`; dedupes upserts by attribute-`Arc` identity, builds per-object validation overlay resources from `SceneOp::SetValidation`, and constructs `uv_edge_data` for UV panes). Winit/egui-decoupled (`Renderer::new` takes a shell-owned `SurfaceConfiguration`); compiles to `wasm32`. |
| `solarxy-host` | The orchestration both GPU shells drive, extracted in 0.8.2 so it exists once: the per-pane pass chain and composite, the per-pane uniform writes, `rebuild_light_bind_group` (the lighting chokepoint), the per-pane camera lifecycle, `HostViewState`, and the gizmo drag solver. Depends on `solarxy-core`, `solarxy-renderer` and `solarxy-kernel` only. **No `solarxy-graph` dependency** (the desktop shell has no engine yet and a crate beneath both shells must not hand it one) and **no renderer trait** (one implementor is premature abstraction; 0.9.0 adds it when a second exists). The golden-capture harness lives here as an example and renders through the shared path, so the pixel gate covers the extracted code rather than sitting beside it. |
| `solarxy-app` | winit `ApplicationHandler` + egui + `State`. Owns input, sidebar, menu, HUD, toasts, console, dialogs. Depends on `solarxy-renderer`. Not yet wired to `solarxy-graph` (next milestone). |
| `solarxy-web` | The `wasm-bindgen` boundary + WebGPU host (cdylib): the `SolarxyApp` class (dispatch/frame/pick/snapshot, per-pane cameras + view state, `stage_asset`, `save_slxy`/`load_slxy`, the worker pumps `take_import_jobs`/`take_validate_jobs`/`submit_*`, `fly_to_issue`, `set_environment_prepared`), the GPU-free worker exports `parse_model_job`/`validate_geometry_job`/`prepare_hdri_job`, serde-wasm-bindgen at the boundary. Drives the full `solarxy-renderer` (Phase 6). All wasm-only code is `cfg(target_arch = "wasm32")`, so native `cargo build --workspace` stays green; the real host builds for `wasm32`. |
| `solarxy-validate` | Validation orchestration + pipeline adapters (GitHub Actions / generic-JSON). Library API consumed by `solarxy-cli` and by external vendors who want structured validation results without a subprocess. `thiserror` per library convention; feature-gated `clap` derive (`features = ["clap"]`). |
| `solarxy-cli` | clap `Args`, analyze TUI (`tui_analysis`), analyzer (`calc/analyze.rs`), its own `[[bin]]` at `src/bin/solarxy-cli.rs`. `--mode view` spawns the `solarxy` GUI binary as a subprocess. |

`web/` is the Vite + React 19 + `@xyflow/react` + zustand frontend: a display mirror of the Rust-owned document (mirror-and-command), with the registry-driven palette/parameter-panel/typed-handles (a node added in Rust needs zero frontend changes). See the "Web shell" section below and the workspace-root `SOLARXY-WEB-*` docs.

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
  - `pane_toolbar.rs` — 3ds Max-style **viewport label menus**: a few frameless bracketed text labels (`label_menu` — `[ Scene 3D ]` / `[ Shaded ]` / `[ Perspective ]`; a UV pane shows `[ UV Map ]` / `[ Display ]`) that **float directly on the 3D scene** — no strip fill — and open a theme-styled dropdown with nested submenus on click. The `Shaded` label = the effective display mode via `display_label` (Inspection / Override are submenus under it); projection / Overlays / Background are submenus under `Perspective`. `style_frameless_labels` scopes the `menu_button`s to bare text (no fill any state), with idle `theme.fg` shifting to the amber accent on hover / open. The 3D scene renders the **full pane** (`render_pane` / `target_dimensions` / `build_review_panes` no longer reserve the 22 px `PANE_TOOLBAR_HEIGHT` strip — that const now only feeds `pointer_in_pane_content` + screenshot capture, keeping the top 22 px a camera-dead-zone where the labels sit). The active pane writes through `GuiSnapshot`, other panes write `&mut [PaneDisplaySettings; 4]` directly. `background_combo` (a `ComboBox`) is retained here only for the Preferences modal's custom-background editor.
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
  - `theme.rs` — the egui adapter over the shared interface palette. **This file authors no colors**: every value comes from `solarxy_core::theme::Palette` (the single source that also feeds the web frontend via generated CSS and the analyze TUI), and this file only maps semantic roles onto egui's widget vocabulary via `Theme::from_palette`. Two presets, selected by `UiPrefs::theme` (`ThemeChoice::Dark` / `ThemeChoice::Light`, with serde aliases for the pre-0.7.1 `AyuMirage*` names so old config files load) and **hot-swapped** from the preferences modal with no restart. `Theme` remains a flat `Copy` bundle of `Color32` tokens (bg / fg / accent / selection / widget colors plus `bg_elevated`, `border`, `severity_*`, and the `review` sub-struct holding the four annotation-category colors, which must match the web's `--cat-*` tokens — the drift tests in `solarxy-core/tests/tokens_drift.rs` guard the CSS side). Entry points: `apply_theme(ctx, &Theme)` (egui `Visuals` + widget palette), `make_dock_style(ctx, &Theme)` (the paired `egui_dock::Style`), and `configure_fonts` (bundles Lilex); the `PANE_TOOLBAR_HEIGHT` const lives in `solarxy_core::view_config`, not here. Zero corner radii throughout; the accent doubles as the review-mode stripe. Changing a color means editing the palette in `solarxy-core` and regenerating the web tokens (`cargo run -p solarxy-core --example gen_tokens > web/src/styles/tokens.generated.css`), never editing this file.
  - `about.rs` — About modal (reference pattern for Esc-dismissable non-modal egui windows). Draggable.
- `console.rs` — `LogBuffer` + `ConsoleLayer` (a `tracing::Layer` feeding the egui console). `ConsoleState` carries the UI-side level filter, search string, docked/floating flag.

### `solarxy-renderer` internals

- `frame.rs` — `Renderer`, `RenderTargets`, `PostProcessing`, `GradientUniform`, `WireframeResources`, `UvOverlapResources` (owns the async overlap readback: `request_readback` + non-blocking `poll_readback`, shared by desktop and web), `ValidationColorResources`, `IblResources`, and `DrawObject { model, instance_buffer, validation: Option<&ObjectValidationGpu>, selected }` (the selection tint draws inside the main pass). The thing `State` calls each frame.
- `scene.rs` — `ModelScene` (per-loaded-model GPU state), recomposed since Phase 6 as `{ model, env: SceneEnvironment, stats, validation* }`; `BackgroundModeExt`, `lights_from_camera`, `create_light_bind_group(_selective)`.
- `environment.rs` — `SceneEnvironment`: the model-independent scene half (lights uniform/buffer/bind group, identity instance buffer, `ShadowState`, `VisualizationState`). Extracted from `ModelScene` in Phase 6 with construction order preserved verbatim (golden-verified); shared by desktop `ModelScene` and the web host.
- `panes.rs` — the shared split-viewport math (`PaneRect`, `compute_panes` for F1-F5, `compute_target_dimensions`, `hit_test_pane`, divider rects). Desktop `state/panes.rs` is now a thin egui adapter; the web host uses it for rendering, pointer routing, and DOM toolbar positioning.
- `pipelines.rs` — every `wgpu::RenderPipeline`; built at startup, reused.
- `pipeline_builder.rs` — fluent `PipelineBuilder` to cut boilerplate in `pipelines.rs`.
- `bind_groups.rs` — `BindGroupLayouts`: the **single source of truth** for every layout used by pipelines (`min_binding_size: None`, so uniform growth is a no-op for layouts).
- `camera.rs` / `camera_state.rs` — orbit camera, per-pane camera bundle, `CameraUniform`.
- `light.rs` — `LightEntry`, `LightsUniform` (CPU side of lights + IBL ambient L0).
- `ibl.rs` — `IblState` with constructors `fallback`, `from_sky_colors`, `from_hdri`, `from_hdr_bytes`/`from_exr_bytes`, and `from_prepared`. `PreparedHdri` holds the GPU-free stages (decode + sanitize + irradiance convolution + average) with a pack/unpack codec for the web worker boundary; a bitwise determinism test pins the worker path to the inline path. `BrdfLut`.
- `ssao.rs`, `bloom.rs`, `shadow.rs`, `composite.rs` — post-FX + per-pane compositing (viewport/scissor). `composite.rs` owns the whole finishing chain: exposure, the pre-tone-map LUT slot, the tone-map switch, the display-referred LUT slot, then lift/gamma/gain. `CompositeLook` is the resolved per-pane look and `resolve_look` is the one place the camera-versus-pane precedence is written.
- `lut.rs` — colour-grading tables on the GPU: `LutCube` uploaded as an `Rgba16Float` 3D texture, deduped on content hash, with an identity table bound to any empty slot so a disabled slot costs no pipeline permutation. **Parsing is not here**: it lives in `solarxy-formats::lut`, because the camera node has to carry a table so the look saves with the document and the engine cannot depend on the renderer. `Renderer::set_lut` is the single chokepoint that pairs upload with the bind-group rebuild.
- `visualization.rs` — grid, axes gizmo, bounds, normals.
- `model.rs`, `material.rs`, `texture.rs`, `uv_camera.rs`, `validation.rs`, `resources.rs`, `geometry.rs` — GPU resources + loaders.
- `skybox.rs` — HDRI sky pass; keeps the source equirectangular texture after IBL convolution so `BackgroundMode::HdriSky` can render it as a backdrop.
- `overdraw.rs` — overdraw inspection (count + visualize passes).
- `shaders/` — 24 WGSL files (the render pipeline below lists the ones a pass is named for).

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
10. Composite pass (`composite.wgsl`) — the finishing chain in order: bloom add, AO multiply, exposure, the pre-tone-map LUT slot (log-encoded input), the tone-map switch, the display-referred LUT slot, then lift/gamma/gain. Plus the viewport/scissor rect.
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

**`tests/uniform_layout.rs` is the guard, and it is opt-in.** It computes the naga span of a named WGSL struct and compares it to `std::mem::size_of` on the Rust side — the comparison nothing else in the build makes. Only structs a shader declares *whole* belong in its table (a prefix is legitimately smaller); `LightsUniform`, `LabelParams` and `MaterialUniform` are in it. Add a case whenever a uniform grows, because the failure mode it catches is silent: WGSL aligns `vec3<f32>` to 16 bytes in the uniform address space while Rust aligns `[f32; 3]` to 4, so a mispaired colour leaves the Rust `const _` assert passing, the shader compiling, and the viewport black at draw time. The 0.8.2 growth of `MaterialUniform` from 64 to 160 bytes (the principled surface properties, appended as six vec4-shaped blocks precisely so each `vec3` lands on a 16-byte boundary) is the worked example.

**`MaterialUniform::from_material` is the single CPU→GPU material conversion** (`material.rs`). Both upload paths call it: `resources.rs::upload_model` (the file importer) and `upload_cooked_materials` (the node graph). They previously carried identical struct literals with nothing keeping them in step, so a field added to one and forgotten in the other showed up only as the same material shading differently depending on where it came from.

### IBL update flows through one chokepoint
`IblState` construction funnels HDRI paths through `from_hdr_pixels`/`from_prepared`; any IBL-derived CPU data (e.g. the L0 ambient) must be computed in **every** constructor (`fallback`, `from_sky_colors`, and the shared HDRI core). `rebuild_light_bind_group` in `solarxy-app/src/state/update.rs` is the single chokepoint triggered on HDRI drop, IblMode toggle (`I` / `Shift+I`), and background change. Scene-wide IBL-derived uniforms are pushed to the GPU with a partial `queue.write_buffer` there, so Clay modes etc. update instantly without waiting for the next camera-driven frame (which may not fire at all under Lock Lights).

### State plumbing shape
- `lights_from_camera` (now in `solarxy-renderer/src/scene.rs`) is called from **five** sites: `SceneEnvironment::new` (`environment.rs`), `solarxy-app`'s `state/update.rs` and `state/render.rs`, and `solarxy-web`'s `app.rs` twice (the unlocked-lights arm and its `setup_pane_lighting`). Adding a parameter means updating all five. It also synthesizes the viewer rig's key/fill/rim intensities, which are stated in the same units light nodes use, so a change to one is a change to the other.
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

Performance baseline + profiling notes from rc.11 are deferred to a later milestone; measurements are filled in on maintainer hardware as hot paths are profiled. The previous `docs/perf/` skeleton was removed alongside the v0.5.0 documentation overhaul.

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
- GUI: **Flathub** (`dev.koljam.solarxy`, manifest in `packaging/flatpak/`), **Homebrew Cask** (`marko-koljancic/solarxy/solarxy`, `packaging/homebrew/`), **winget** (`Koljam.Solarxy`, manifests in `packaging/winget/manifests/k/Koljam/Solarxy/<version>/` with the `{{INSTALLER_SHA256}}` placeholder filled by `.github/workflows/winget-release.yml` on each stable tag). Plus raw DMG / MSI / AppImage bundles from GitHub Releases.
- CLI: `cargo-dist` installers (shell / PowerShell + portable `.zip`), Homebrew formula (`solarxy-cli`). No MSI — winget CLI manifest (portable type) still deferred (Rust-CLI convention: ripgrep, fd, zoxide, eza, bat, delta, cargo-dist itself don't ship one either).
- `solarxy-cli --update` detects the install source via `solarxy_core::install_source::detect()`: Homebrew → `brew upgrade solarxy-cli`, Flatpak → `flatpak update dev.koljam.solarxy`, otherwise `axoupdater` self-update.

**Local dev smoke:**
- `scripts/build_local_dmg.sh` — mirrors the CI macOS bundle path end-to-end.
- `scripts/gen_bundle_icons.sh` — regenerates every icon in `res/bundle/` (256/512/1024 PNG, `.icns`, multi-size `.ico`) from a Python-generated master PNG. Rerun after swapping in real icon art.

**Bundle assets** live in `res/bundle/`:
- Icons (`solarxy-{256,512,1024}.png`, `solarxy.png`, `solarxy.icns`, `solarxy.ico`).
- `linux/solarxy.desktop`, `linux/appimage/AppRun`.
- `macos/Install CLI.command` (clears Gatekeeper quarantine on `/Applications/Solarxy.app` + sudo symlink into `/usr/local/bin`), `macos/READ ME FIRST.txt` (Gatekeeper walkthrough; filename chosen for top-of-DMG sort).

**Release notes**: maintained in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki/Release-Notes). No in-repo `CHANGELOG.md`; cargo-dist sources GitHub Release bodies from its own manifest, not a repo file.


## Working Agreement

- **Implementation according to best practices**, as a senior software engineer and computer graphics domain expert in Rust. For UI work, also as a UX/UI expert.
- **Clarify before planning, tiered by stakes.** Before proposing a plan, identify the ambiguities, missing context, and unstated assumptions in the request. Ask clarifying questions in a **single batch**, not drip-fed, covering scope, success criteria, constraints, dependencies, personas, and acceptance criteria. Continue rounds until at least 90% confident in **what** and **how**. State remaining assumptions explicitly, and name context that only the maintainer holds rather than inventing it. Then produce a plan with objectives and scope (in and out), approach and key decisions, work breakdown and sequencing, risks/assumptions/dependencies, acceptance criteria, and an objective confidence percentage. Do not begin execution until the maintainer explicitly confirms; on pushback, revise and re-request rather than partially executing. **Run this in full** when the work changes scope, touches a public surface, reopens a ratified decision, spans more than a couple of files, or is hard to reverse. **Skip it** for a narrow unambiguous ask (a named bug, a single file, a direct question), where the right move is to state assumptions inline and proceed. Resolve what is knowable yourself first: read the code, check the amendment history, run the search. Asking three questions about a one-line fix is its own failure.
- **Ask before adding anything to the repository.** This repo is public and its dependencies are a supply chain, not a convenience. **Gated, ask first:** any new Cargo or npm dependency (including dev-dependencies, and including a version bump that pulls a new transitive tree); any new top-level directory; any new workspace crate; any committed binary asset. **Not gated:** source files inside an existing crate or module, tests, fixtures, and documents under `Docs/`. When a design or plan implies a gated addition, surface it at plan time with the reason and the alternative you considered, not at implementation time. A stray runbook was committed to the repo root during 0.8.1 and had to be deleted before merge; the root carries `CLAUDE.md` and `README.md` only.
- **No `unwrap` / `expect` outside of tests.** Use `?` with `anyhow` (app/CLI) or `thiserror` (library crates) and add context via `.context(...)` where it helps debugging.
- **Library crates use `thiserror`; binary crates use `anyhow`.** Do not pull `anyhow` into `solarxy-core` (except behind the `serialization` feature), `solarxy-formats`, or `solarxy-renderer` as a public dependency.
- **Distinguish current state from planned refactors.** The 0.6.0 milestone plan describes future work (pipeline sub-struct grouping, `GuiSnapshot::apply_to_state` consolidation, doc-comment coverage). Do not refactor toward planned-but-unscheduled work without surfacing it first.
- **Surface findings before unilaterally refactoring.** Multi-file refactors get a plan first.
- **No milestone planning codes in comments, test names, or log output.** Work-item codes (`W0a`, `W-C1`), numbered stages and phases (`Stage 8`, `pre-Phase-10`, `phase-17`) and decision codes (`M-3`, `D-24`, `R-5`, `C-4`, `P-9`) are ephemeral artifacts of a planning document. A comment carrying one is unreadable without that document open beside the file, and the decision codes are per-milestone, so `decision M-3` names a different decision in `expr/mod.rs` than it does in `bounds_node.rs`. Say what the code stood for instead: `pre-Phase-10 desk shape` becomes `the pre-dockview desk shape`, `(decision M-11)` becomes the decision's substance. **Version references are fine and often load-bearing** - "a blob persisted before 0.8.1" is *why* a migration fallback exists. Enforced by `no_planning_codes_in_comments` in `crates/solarxy-core/tests/tokens_drift.rs`, which also covers `describe`/`it`/`test` titles because those are read in CI output. Planning codes stay correct and expected in `Docs/`, which the rule does not scan.

## Agents and skills

All agents and skills live in `.claude/` **in this repo** and are versioned with it. They are loaded only when a session starts here, which is why nothing lives at the workspace root any more. Because this repo is public, every file under `.claude/` is a public artifact: it carries no secrets or host detail, and it obeys the redaction rule.

**Design sources live in `design/`**, one Pencil file per shell (`web/`, `desktop/`, `tui/`). They are encrypted: open them only through the Pencil MCP tools, never with `Read` or `Grep`. Colour is owned by `solarxy_core::theme::Palette` and drift-tested, not by the design file; the design file owns composition. See `design/README.md` before any visual work.

**Every agent reads `.claude/skills/solarxy-domain/SKILL.md` first.** That briefing carries the architecture invariants, the shared vocabulary, the five surfaces of record, the engagement protocol above, the public-surface rules, and the canonical reading list. Change a shared rule there, not in eleven agent files.

| Agent | Reach for it when |
|---|---|
| `architect` | Design before code: crate boundaries, the engine-renderer contract, the wasm boundary, data-model and migration decisions, anything spanning surfaces. |
| `rust-engineer` | Implementing in the Rust workspace outside the renderer: engine, kernel, scenefile, formats, imaging, validate, CLI, desktop shell. |
| `frontend-engineer` | Implementing under `web/`: React and zustand, the node canvas, the boundary mirrors, workers and persistence, the public pages. |
| `graphics-engineer` | Implementing in `solarxy-renderer` and WGSL: passes, pipelines, uniforms, IBL and shadows, capture, goldens. |
| `product-manager` | Strategy and lifecycle: vision, positioning, release themes, the program to 1.0, go-to-market, milestone-level board ownership. |
| `product-owner` | SDLC: decomposing a milestone into epics, tasks and sub-tasks, writing acceptance criteria, exit gates, scope-pressure calls. |
| `product-designer` | Interaction model, UX-spec guardianship, keymap policy, and the design of the public surfaces. |
| `technical-writer` | Any authored prose and its consistency: amendments, the log, the wiki, release notes, reference docs, public item text. |
| `qa-engineer` | Test strategy, coverage audits, exit-criteria verification, regression gates, and cross-surface release verification. |
| `devops-engineer` | Build and release pipeline, size budgets, the release train and its fan-out, edge routes and deploy. |
| `security-engineer` | Headers and CSP, supply chain, untrusted-input review at the wasm and parser boundaries, public-surface disclosure. |
| `milestone-planner` | Researching and drafting a code-grounded milestone specification. Driven by the skill of the same name. |

| Skill | What it is for |
|---|---|
| `solarxy-domain` | The shared briefing every agent reads first. |
| `solarxy-sdlc` | The nine delivery stages from idea to verified release, who owns each, and which file is authoritative. Use to answer where a piece of work sits and what comes next. |
| `solarxy-git` | Commit message and branch naming conventions: Conventional Commits with a closed type and scope set, the branch model, pull request titles, and the golden-accept footer. |
| `solarxy-tracker` | Operating the public GitHub board: the four-level hierarchy, the golden item shapes, the pastel label taxonomy, the cleanup policy. |
| `solarxy-sync` | Keeping the five surfaces in step. Run before declaring any task, milestone, or release done. |
| `solarxy-brand` | The design language and voice for the public pages, plus the three platform constraints a new page must satisfy. |
| `milestone-planner` | Planning or revising a release milestone, and fanning it out to the board. |
| `solarxy-audit` | `/solarxy-audit` runs a full code-quality sweep in an isolated subagent so the report does not pollute the main session. |
