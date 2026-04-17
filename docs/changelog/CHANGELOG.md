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
- **Windows CLI MSI** — added `[package.metadata.wix]` with stable
  `upgrade-guid` and `path-guid` to `crates/solarxy-cli/Cargo.toml`.
  Without explicit GUIDs, cargo-dist would regenerate them on every
  build and Windows would not recognise subsequent installers as
  upgrades of prior installs.
- **Windows CLI MSI (WXS template)** — added
  `crates/solarxy-cli/wix/main.wxs` (plus the matching
  `install-source-msi.txt` marker). `[package.metadata.wix]` alone
  told cargo-dist to build a CLI MSI, but `dist-workspace.toml` has
  `allow-dirty = ["msi"]` (to protect the hand-edited GUI WXS), which
  also suppresses auto-generation of the CLI template — so the CLI
  MSI step failed with "There are no WXS files to create an
  installer". The template ships `solarxy-cli.exe`, adds
  `%ProgramFiles%\solarxy-cli\bin` to PATH, and drops an
  `install-source` marker under `%ProgramData%\Solarxy-cli\` so
  `solarxy-cli --update` can detect the MSI channel and route to
  `winget upgrade` instead of running an axoupdater self-update that
  UAC would block. `solarxy_core::install_source::marker_path()`
  picks the `Solarxy-cli` subfolder when the running exe is
  `solarxy-cli`; `classify_exe_path` also now matches
  `\Program Files\solarxy-cli\` as a fallback.
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
