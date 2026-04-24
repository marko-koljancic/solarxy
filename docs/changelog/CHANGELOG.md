# Changelog

All notable changes to Solarxy are documented here. The format is based on
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), and this
project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Historical entries for versions prior to 0.5.0 live on the
[GitHub Releases page](https://github.com/marko-koljancic/solarxy/releases).

---

## [Unreleased]

Nothing yet.

---

## [0.5.0-rc.11] — unreleased

Final polish RC before 0.5.0 stable. No new features; every change closes a
"this feels unpolished" gap surfaced during rc.10 dogfooding. Menu bar,
modal behaviour, toast notifications, console capture, and the CLI install
path all get their last round of cleanup.

### Added

- **Keyboard Shortcuts modal** (`?` or View menu → Keyboard Shortcuts).
  Draggable reference window listing every binding grouped by category
  (File / Window & Layout / Navigation / Shading & Inspection / Show /
  Lighting). Replaces the old bottom-of-viewport hints overlay. Read-only
  for this release; user-remappable shortcuts land in 0.6.0.
- **Preferences → Interface → "Open Model Stats on model load"** checkbox
  (default on). Governs whether loading a model auto-opens the Model
  Stats panel. Backed by `UiPrefs::open_stats_on_model_load`; new
  `#[serde(default)]` keeps older `config.toml` files compatible.
- **Preferences → Startup → Config File** section: shows the absolute
  config path and provides an **Open config file** button (opens the
  TOML file in the system-default text editor). Replaces the removed
  Edit-menu entries.
- **Toast tracing bridge**: every `push_toast` now emits a matching
  `tracing` event on `target: "solarxy::toast"`. Users can review toast
  history after the fact in the docked console.
- `docs/perf/rc11-baseline.md` + `docs/perf/rc11-profiling-notes.md` —
  skeletal perf-spike artifacts (reproduction commands, metrics grid,
  hot-path templates). Measurements fill in on maintainer hardware; hot
  paths surface as 0.6.0 issues.

### Changed

- **Menu bar simplified**. File menu drops "Save Preferences" (the
  modal's OK button saves atomically; `Shift+S` shortcut still saves
  for power users). Edit menu trims down to a single "Preferences…"
  entry — the config-path label and Open Config File button move into
  the Preferences modal's Startup tab. View menu drops the six panel-
  visibility checkboxes (Sidebar / Menu Bar / Console / Model Stats /
  FPS HUD / Keyboard Shortcuts) and keeps only functional view
  controls + a Keyboard Shortcuts button at the end. Window menu is now
  the single source of truth for panel toggles (Menu Bar joins
  Sidebar / Console / Model Stats / FPS HUD there).
- **About / Check for Updates / Preferences modals are now draggable**.
  They still open centered, but users can reposition them without the
  anchor pinning them back.
- **Toast notifications redesigned** to stack at bottom-center (Discord
  / Slack style). Newest sits at the bottom; older entries stack
  upward. Auto-dismiss after 5 s (was 3 s); click-to-dismiss retained.
  Cap of 5 queued toasts preserved.
- **Console buffer captures `solarxy=trace` by default**
  (was `solarxy=debug`). The UI filter dropdown still lets users pick
  ERROR / WARN / INFO / DEBUG; TRACE events are captured but require
  `SOLARXY_CONSOLE_LOG=solarxy=trace` to surface in the UI filter.
- **Model-load log message format** updated to
  `"Loaded model: {path} ({verts} verts, {tris} tris, {meshes} meshes)"`.
- **Prerelease-channel explanation** in Preferences → Updater is now
  shown only when the Prerelease radio is active (previously always
  visible, which made the Stable branch look over-explained).
- **CLI docs (`solarxy-cli --mode docs` About tab)**: CLI invocation
  examples now correctly use `solarxy-cli` (was `solarxy`, which is the
  GUI binary). GUI-launch examples keep the `solarxy` name.

### Removed

- The "Drop a 3D model to view" centered placeholder. An empty viewport
  stays visually clean; File → Open Model and drag-and-drop both still
  work without the prompt.
- The bottom-of-viewport hints overlay (`HINTS_MODEL` / `HINTS_NO_MODEL`
  strings) and the `hints_visible` flag that drove it. Replaced by the
  Keyboard Shortcuts modal.

### Breaking / ⚠ Migration

- **CLI install path moved from `~/.cargo/bin` to `~/.local/bin`.** The
  shell and PowerShell installers now place `solarxy-cli` in the XDG
  Base Dir location. Users upgrading from rc.10:
  - Ensure `~/.local/bin` is on your `PATH` (most Linux distros set this
    in `~/.profile`; macOS users may need to add it manually).
  - The old binary at `~/.cargo/bin/solarxy-cli` is NOT removed
    automatically — delete it with `rm ~/.cargo/bin/solarxy-cli` for a
    clean layout. `install_source::classify_exe_path` keeps recognising
    the legacy location so `--update` still routes correctly for
    users who haven't migrated.

### Internal

- Toast caller audit: ~8 duplicate `tracing::…!` calls alongside
  `set_toast` were removed. With `push_toast` now emitting the matching
  event, those callers were double-logging.
- New unit tests in `solarxy-core::install_source` cover both the
  legacy `.cargo/bin` and the new `.local/bin` classifications.

---

## [0.5.0-rc.10] — 2026-04-23

UI and code-quality polish. Closes the remaining user-visible gaps before
tagging 0.5.0 stable. Preferences move from a standalone TUI to a GUI dialog;
the CLI `--mode preferences` variant stays parseable but prints a migration
hint. Several sidebar-unreachable fields (window size, MSAA, recent-files
capacity, updater behaviour, default UI visibility) become editable for the
first time.

### Added

- **GUI preferences dialog** (`Edit → Preferences…`, `Ctrl/⌘+,`). Tabbed
  egui modal with three tabs: **Startup** (window size, MSAA sample count),
  **Interface** (default sidebar / FPS HUD / console visibility,
  recent-files capacity), **Updater** (check-for-updates on launch,
  stable / prerelease channel). OK saves to `config.toml` + closes;
  Cancel or Esc reverts; Reset-to-defaults acts on the visible tab only.
  The modal deliberately does **not** duplicate sidebar controls —
  scope is strictly fields the sidebar cannot reach at runtime.
- **`Preferences::ui`** and **`Preferences::updater`** sections, backed by
  new `UiPrefs` and `UpdaterPrefs` structs + the `UpdaterChannel` enum.
  Loaded with `#[serde(default)]` so rc.8-era `config.toml` files upgrade
  in place. `MAX_RECENT_FILES_CAP = 50` upper bound.
- **Console copy-to-clipboard** — right-click a log entry for "Copy
  message" or "Copy full line" (timestamp + level + message).
- **Console search** — substring filter beside the level filter, combining
  with it via AND. Case-insensitive; × clears.
- **Toast queue** — rapid-fire notifications (e.g. load + HDRI + material
  in quick succession) now queue instead of replacing. Cap 5; oldest dropped
  on overflow.
- **Window menu** — DCC-style top-level menu with visibility toggles for
  Sidebar / Console / Model Stats / FPS HUD, mirroring the corresponding
  View-menu entries.
- **GPU uniform size asserts** — `GradientUniform` (48 bytes) and
  `WireframeParams` (32 bytes) in `solarxy-renderer::frame` now have
  `const _: () = assert!(size_of::<T>() == N);` guards, matching the
  existing pattern on `CameraUniform`, `LightsUniform`,
  `MaterialUniform`, and `LightEntry`.
- **Workspace-wide `Debug` derives** on pure-data types in
  `solarxy-core` (`AABB`, `ViewLayout`, `DisplaySettings`, `BoundsMode`,
  `PaneDisplaySettings`, `ValidationResult`, all `Json*` types) and on
  non-GPU types in `solarxy-app` (`GuiSnapshot`, `HudInfo`,
  `SidebarChanges`, `MenuActions`, `MenuBarVisibility`, `Toast`,
  `HudResult`, `LogEntry`, `ConsoleLayer`, `ConsoleState`). Types that
  transitively own wgpu resources (`State`, `Renderer`, `ModelScene`,
  etc.) intentionally remain without `Debug`.

### Changed

- **Stats window** — removed the visible "N/A" placeholders for UV
  Coverage and Validation Status. These fields implied functionality that
  didn't exist. Real implementations (UV overlap GPU readback + validation
  summary) land in 0.6.0; the UV Data section still shows UV Mapping
  (Yes/No), which is real.
- **`solarxy-core` feature `config` renamed to `serialization`**. The old
  name was ambiguous (compile-time config vs runtime blob vs config-file
  I/O); the new name unambiguously covers what's gated (serde + toml +
  dirs + tracing, used by `preferences`, `json`, `report`,
  `install_source`, `view_config`). All workspace-internal consumers
  updated; no external consumers known.
- **`Edit → Preferences` menu entry** — was a stub RichText label above
  the config-file button; now a proper button that opens the GUI dialog
  (with keyboard shortcut label).
- **Recent-files capacity** is now user-configurable via
  `Preferences::ui::max_recent_files` (clamped to `1..=50`); the old
  hard-coded `MAX_RECENT_FILES = 20` is the default.
- **`README.md` preferences section** rewritten to describe the GUI
  dialog + direct TOML editing; old TUI screenshots and per-setting
  table dropped.
- **`solarxy-cli` docs-mode tabs** shrunk from 5 to 3 (**About**,
  **Analyze Mode**, **Formats**). View Mode documentation was a
  scope-mismatched 147-line import of GUI-viewer content; preferences
  documentation covered the removed TUI. Both moved out.
- **Menu recent-files string truncation** simplified — replaced a
  double-`collect::<Vec<_>>()` reversal with a single-pass `skip()`.
  Cleaner read, half the allocations.

### Removed

- **`crates/solarxy-cli/src/tui_preferences.rs`** — the interactive
  preferences TUI. `solarxy-cli --mode preferences` now prints a
  migration hint (GUI dialog path + config-file location) and exits
  with code 1. The clap variant stays parseable so scripted invocations
  don't blow up with "unknown value".
- **`crates/solarxy-cli/content/preferences.txt`** and
  **`view_mode.txt`** — docs-mode content for the two removed tabs.

### Internal

- Documented the `cycle_enum!` macro's safety invariant
  (`unwrap_or(0)` fallback) with a rustdoc comment.
- Documented `min_binding_size: None` intent on the three sites in
  `bind_groups.rs` (the uniform-binding helper plus the two storage-buffer
  entries for variable-length per-model data).

---

## [0.5.0-rc.9] — 2026-04-23

CI / packaging / documentation cleanup ahead of 0.5.0 stable. No user-facing
code changes — every diff is release-surface, docs, or CI triggers.

### Removed

- Deleted `.github/workflows/flathub-bump.yml`, `homebrew-bump.yml`, and
  `winget-release.yml`. The job-level `if:` on each used
  `secrets.X != ''` at expression scope, which GitHub Actions silently
  errors on — the workflows had been failing on every release since
  rc.7. Tap / manifest / winget-pkgs fork don't exist yet; reinstate
  in 0.5.1 alongside `{{PRODUCT_CODE}}` extraction and a regex-based
  Homebrew formula version substitution.
- Deleted `packaging/winget/` entirely. The manifest's `ProductCode`
  was a copy of the `UpgradeCode` GUID from `wix/main.wxs`, which
  violates WiX semantics: `ProductCode` must rotate per build so that
  `winget upgrade` correctly detects version transitions. Fix requires
  per-build extraction from the built MSI — wired up at reinstatement.
- Removed the `aarch64-unknown-linux-gnu` leg from the native-bundle
  matrix. `action.yml`'s AppImage step is gated to x86_64 only, so the
  aarch64 leg was compiling binaries nothing consumed. Re-add when
  upstream `appimagetool` ships a stable aarch64 binary.

### Fixed

- **CI branch gating** — `.github/workflows/ci.yml` now runs on pushes
  to `ui-revamp` in addition to `main`. The 0.5.0 RC chain had been
  merging to `ui-revamp` without `cargo fmt --check`,
  `clippy -D warnings`, or `cargo test` enforcement.
- **Flatpak manifest** — `packaging/flatpak/dev.koljam.solarxy.yaml`
  no longer passes `--no-default-features --features viewer,analyzer`
  to `cargo build`. The root `solarxy` crate has no `[features]` block
  (always-GUI by design), so these flags would error with "unknown
  feature" during Flathub's sandboxed offline build. Replaced with a
  bare `cargo --offline build --release --bin solarxy`.
- **Flatpak `cargo-sources.json`** — regenerated from `Cargo.lock`
  via `packaging/flatpak/generate-sources.sh`. The committed file
  was a 3-byte `[]` placeholder; Flathub's sandboxed builder requires
  the full 557-crate manifest (~300 KB) to materialize sources
  offline.
- **`dist-workspace.toml` stale comment** — dropped the trailing
  comment block that described a `release: published` trigger model
  replaced in rc.2 by the `post-announce-jobs` in-graph hook (which
  is documented correctly at the top of the file).
- **`CLAUDE.md:165-170`** — removed `cargo-deb` / `cargo-generate-rpm`
  / `[package.metadata.generate-rpm]` references (dropped in rc.7) and
  the `packaging/winget/` pointer (deleted above). The bundle-pipeline
  description now matches actual CI surface.

### Added

- **`README.md` — System requirements section** — documents the AVX2+FMA
  x86_64 baseline (Intel Haswell 2013+, AMD Excavator 2015+) imposed by
  `.cargo/config.toml`'s `target-feature=+avx2,+fma`. Apple Silicon and
  Linux aarch64 retain no additional requirements.
- **`docs/SolarxyDocumentation.md` — out-of-date banner** — the
  0.3.x-era document is not refreshed for 0.5.0; the banner points
  readers at README, CHANGELOG, and `solarxy-cli --mode docs` for
  current state. Full rewrite deferred to 0.6.0.

---

## [0.5.0-rc.8] — 2026-04-17

Final pre-stable RC. Validates the rc.7 packaging rearchitecture and the
post-rc.7 crate-extraction refactor against the complete installer matrix.
No user-visible behavior changes since rc.7.

### Changed (internal)

- Extracted `solarxy-renderer` and `solarxy-app` as dedicated crates from
  the monolithic root (commit `3c4527b`). The root `solarxy` crate is now
  a thin GUI entrypoint; all wgpu state lives in `solarxy-renderer` and
  the winit `ApplicationHandler` + egui UI lives in `solarxy-app`. See
  [CLAUDE.md](../../CLAUDE.md) for the updated 6-crate layout.

### Fixed (release-pipeline)

- **Windows GUI MSI** — removed the `solarxy-cli.exe` binary reference
  from `wix/main.wxs`. cargo-dist 0.31.0 stages binaries per-package
  when building each workspace MSI, so the GUI staging directory only
  contained `solarxy.exe`; the cross-crate reference caused `light.exe`
  to fail with LGHT0103 ("file not found"). The CLI ships as its own
  MSI — consistent with the rc.7 "two separate distributions" model on
  every other platform.
- **Windows CLI distribution** — CLI on Windows ships as cargo-dist's
  shell / PowerShell installers (and a portable `.zip`) rather than an
  MSI. `[package.metadata.wix]` on `crates/solarxy-cli/Cargo.toml` has
  been removed along with the CLI WXS template: CLI MSIs are not
  idiomatic on Windows (no well-known Rust CLI — ripgrep, fd, zoxide,
  eza, bat, delta, cargo-dist itself — ships one), and the cargo-dist
  + cargo-wix path was failing with a `candle` schema error with no
  diagnostic output to shorten the guess loop. The GUI continues to
  ship as an MSI where Start Menu + Add/Remove Programs integration
  actually earns its keep. A winget `portable` manifest for the CLI
  is deferred to 0.5.1.
- **Auto-bump workflows** — added a prerelease guard to
  `flathub-bump.yml`, `homebrew-bump.yml`, and `winget-release.yml`.
  The three workflows previously fired on every `release: published`
  event, which would have published any successful RC to Flathub,
  Homebrew, and winget as though it were stable. The guard skips the
  jobs when `github.event.release.prerelease == true`;
  `workflow_dispatch` runs are still allowed for manual retries.
- **Rust 1.94 clippy** — added `#[must_use]` to `PipelineBuilder` and
  `CameraState::clone_with_new_resources`; allow-listed
  `clippy::pub_underscore_fields` in `solarxy-renderer` for the GPU
  `_pad` uniform alignment fields. The lints were promoted under
  `#[warn(clippy::pedantic)]` in recent toolchains.
- **Local DMG smoke script** — `scripts/build_local_dmg.sh` now embeds
  both `solarxy` and `solarxy-cli` into the `.app`, matching the CI
  action (`.github/actions/native-bundle/action.yml`). Previously the
  local script produced a malformed `.app` (no CLI binary), causing
  `Install CLI.command` to hard-fail during local smoke.

---

## [0.5.0-rc.7] — 2026-04-17

Packaging rearchitecture release. Closes three platform-specific install
bugs (Windows console flicker, Fedora 42 Vulkan-driver gap, macOS
Gatekeeper friction) by splitting the binary and replacing the .deb / .rpm
output with Flathub + Homebrew distribution.

### Changed (breaking, prerelease scope)

- **Two binaries** instead of one: `solarxy` is the GUI, `solarxy-cli` is
  the terminal companion (analyze / preferences / docs / self-update).
  Existing `solarxy --mode analyze` invocations move to
  `solarxy-cli --mode analyze`. The GUI no longer accepts `--mode`,
  `--about`, `--update`, `--format`, or `--output` — only `--model`,
  `--verbose`, `--log-level`.
- **Windows GUI uses the Windows subsystem** in release builds — no more
  console window appearing alongside the GUI when launched from Start
  Menu. Debug builds keep the console for stderr / panic visibility.
- **Linux GUI** moves to Flatpak (Flathub) as the primary distribution
  channel. AppImage stays as a fallback. `.deb` and `.rpm` are no longer
  produced by the release pipeline; community packagers can still build
  them from source.
- **macOS**: `.app` bundle now embeds both `solarxy` and `solarxy-cli`.
  `Install CLI.command` symlinks `solarxy-cli` into `/usr/local/bin`
  (was `solarxy`).

### Added

- **Homebrew tap** at `koljam/homebrew-solarxy` (separate repo). Cask for
  the GUI (`brew install --cask koljam/solarxy/solarxy`) auto-strips
  Gatekeeper quarantine via postflight. Formula for the CLI
  (`brew install koljam/solarxy/solarxy-cli`) — cross-platform macOS +
  Linux.
- **Winget manifest** submitted on each release. Users on Windows can
  install with `winget install Koljam.Solarxy` and update with
  `winget upgrade Koljam.Solarxy`.
- **Flathub manifest** under `packaging/flatpak/`. App ID
  `dev.koljam.solarxy` matches the macOS bundle identifier. Auto-bump on
  every release via `.github/workflows/flathub-bump.yml`.
- **"Check for Updates" menu item** (Help → Check for Updates...). Reads
  the install source (Flatpak / Cask / MSI / etc.) and shows the right
  upgrade command — `brew upgrade --cask`, `winget upgrade`, or a link
  to the Flathub page or GitHub releases. Replaces the silent
  `axoupdater` self-update on the GUI.
- **`solarxy-cli --update`** still self-updates via `axoupdater` for
  shell-installer installs, but refuses to run on Homebrew-formula or
  Flatpak installs (which it would corrupt) and prints the correct
  package-manager command instead.
- **Install-source detection** (`solarxy_core::install_source::detect`)
  using `FLATPAK_ID` / `APPIMAGE` env vars, an `install-source` marker
  file written by each installer, and exe-path heuristics.

### Fixed

- **Windows**: GUI no longer opens an extra terminal window when
  launched from Start Menu (root cause: missing `windows_subsystem`
  attribute on the GUI binary).
- **Fedora 42**: GUI launches reliably via Flatpak, which ships its own
  Vulkan driver in the Freedesktop runtime instead of relying on host
  packages. The old `.rpm` only declared `vulkan-loader` and silently
  failed when no GPU driver package was installed.
- **macOS**: Homebrew Cask path bypasses the manual Gatekeeper dance
  for new users. The DMG / Install CLI.command path remains for users
  without Homebrew.

### Removed

- `[package.metadata.deb]` and `[package.metadata.generate-rpm]`
  sections from the root `Cargo.toml`. The `cargo deb` and
  `cargo generate-rpm` steps in the native-bundle action are gone too.

---

## [0.5.0] — 2026-04-XX

UI-revamp milestone: top menu bar, native file dialogs, in-app console, and
native installers on every major platform. No user-visible rendering changes.

### Added

- **Menu bar** (File / Edit / View / Help) visible by default. Every viewport
  setting is reachable through the **View** menu using DCC-style grouping
  (Shading / Inspection / Material Override / Show / Background / Lighting /
  Post-Processing / Layout / Projection / Turntable / Panel Toggles), inspired
  by Blender and Unreal.
- **Native OS file dialogs** for model (`Ctrl/⌘+O`) and HDRI
  (`Ctrl/⌘+Shift+O`) import.
- **In-app console panel** with docked and detached (floating) modes,
  per-level filter (ERROR / WARN / INFO / DEBUG), auto-scroll, clear, and
  color-coded output. Toggle with `` ` ``.
- **Independent console log filter** via the `SOLARXY_CONSOLE_LOG` env var.
  Stdout continues to honour `RUST_LOG`.
- **Recent Files submenu** under File. Stores the 20 most recent loads,
  displays up to 10 with 50-char leading-ellipsis truncation and full paths on
  hover.
- **About modal** (Help → About Solarxy) with version, license, repository
  link. Dismisses on `Esc` or the window X.
- **Wiki link** (Help → Solarxy Wiki) opens the repository wiki in the
  default browser.
- **Edit → Open Config File** opens the preferences TOML in the OS default
  editor.
- **FPS HUD** (View → Panel Toggles → FPS HUD). Off by default; draggable
  `egui::Window`; shows FPS, frame time, backend, active pane label, cameras-
  linked indicator, and validation counts. Session-only position (persistence
  across sessions deferred to 0.6.0).
- **Toast banner** redesigned as a wide top-aligned banner with severity
  icons (`✓` / `⚠` / `✕` / `ℹ`). Click to dismiss early; auto-dismisses after
  2–3 s.
- **Model Stats auto-open** on successful model load. Sticky: if you close
  the panel manually, subsequent loads respect that until you re-enable it via
  the View menu.
- **Window title** now reflects the loaded model — `Solarxy — foo.glb`.
- **`F10`** toggles the menu bar; **`F11`** toggles borderless fullscreen.
- **Native installers on every major platform** (new in 0.5.0):
  - macOS: `.dmg` — drag to Applications with Install CLI.command helper
  - Windows: `.msi` — Start Menu entry, PATH registration
  - Ubuntu / Debian (x64, ARM64): `.deb` — desktop menu integration
  - Fedora / RHEL 9+ / openSUSE (x64, ARM64): `.rpm` — same assets as the
    `.deb`, pulls `vulkan-loader` at install time
  - Distro-agnostic Linux (x64): `.AppImage`

### Changed

- **Keyboard hints overlay** (`?`) shortened and **off by default** — the menu
  bar now handles discoverability.
- **Internal — `src/cgi/gui.rs` decomposed** from a 1740-line god-file into
  an 11-file `src/cgi/gui/` module (menu, sidebar, renderer, snapshot, stats,
  console_view, theme, overlays, about, actions, mod).
- **Internal — layout switching** (`F1`/`F2`/`F3`) shares a single
  `set_view_layout` helper between keyboard and menu paths.
- **Internal — `PaneMode` / `ProjectionMode`** migrated to the shared
  `cycle_enum!` macro.
- **Version scheme** — workspace crates now inherit `version`, `edition`,
  `rust-version`, `license`, `repository`, `authors` via
  `[workspace.package]`; future bumps touch a single field.

### Removed

- **Global `Esc` handler** — `Esc` no longer quits the viewer. Use
  **File → Quit** or close the window. Modals implement their own local
  Esc-to-dismiss (About modal, future settings dialogs).
- **Model Stats checkbox** from the sidebar — moved to
  **View → Panel Toggles → Model Stats**.

### Fixed

- **Ghosted viewport after model close** — the composite pass no longer
  samples stale bloom and SSAO textures when no model is loaded. Empty panes
  render a clean background gradient.
- **Keyboard shortcut gating** — all viewport shortcuts correctly gated on
  `!gui.wants_keyboard_input()`, eliminating interference with GUI text
  fields (Recent Files search, etc.).

### Notes on first launch

Installers are **unsigned** in 0.5.0:

- **macOS** — Gatekeeper blocks first launch. Easiest bypass: double-click
  **Install CLI.command** inside the DMG; it clears the quarantine attribute
  on Solarxy.app. Manual route: **System Settings → Privacy & Security →
  Open Anyway**. Walkthrough in
  [README.md](../../README.md#first-launch-on-macos) and `READ ME FIRST.txt`
  inside the DMG.
- **Windows** — SmartScreen shows "Windows protected your PC". Click
  **More info → Run anyway**.

Code signing (Apple Developer certificate + Azure Trusted Signing) is on the
roadmap.

[Unreleased]: https://github.com/marko-koljancic/solarxy/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/marko-koljancic/solarxy/releases/tag/v0.5.0
