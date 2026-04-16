# solarxy

![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)
![Rust](https://img.shields.io/badge/Rust-1.92%2B-orange.svg)
![Release](https://github.com/marko-koljancic/solarxy/actions/workflows/release.yml/badge.svg)
![GitHub Release](https://img.shields.io/github/v/release/marko-koljancic/solarxy)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)
[![Docs](https://img.shields.io/badge/Docs-Documentation-green)](docs/SolarxyDocumentation.md)

A lightweight, cross-platform 3D model viewer and visual debugger built with Rust and wgpu. Inspect 3D models in a real-time graphical viewer with split viewports and inspection overlays, or analyze them from the terminal with built-in validation checks.

<p align="center">
  <img src="docs/gif/solarxy.gif" width="100%">
</p>

## Features

- **Multi-format support** -- OBJ, STL, PLY, and glTF/GLB
- **PBR rendering** -- Cook-Torrance BRDF, normal mapping, shadow mapping, IBL (diffuse + specular), SSAO, bloom, selectable tone mapping (Reinhard, ACES Filmic, Linear, None), alpha blending, 3-light system, 4x MSAA
- **Split viewport** -- side-by-side or stacked panes with independent cameras and display settings per pane
- **Inspection modes** -- Material ID, Texel Density heat map, Depth visualization, UV Map with overlap detection
- **Material overrides** -- Clay Light, Clay Dark, Chrome (IBL-only reflective black), and Silhouette (flat black) for surface inspection
- **Validation overlay** -- color-coded 3D visualization of validation issues (degenerate triangles, missing UVs, bad material refs)
- **egui sidebar** -- interactive control panel with bidirectional keyboard sync
- **Interactive analysis** -- TUI with per-mesh and per-material breakdowns, validation checks
- **Report export** -- save analysis reports to file in text or JSON format
- **Persistent preferences** -- configure display, rendering, and lighting settings via TUI or in-viewer shortcuts
- **Drag-and-drop** -- drop model files or HDR/EXR environment maps directly into the viewer window

## Supported Formats

| Format | Extensions | Notes |
|---|---|---|
| Wavefront OBJ | `.obj` | Meshes, materials (`.mtl`), textures, UVs |
| STL | `.stl` | Geometry only, no materials |
| PLY | `.ply` | Flexible vertex attributes, optional normals and UVs |
| glTF 2.0 | `.gltf`, `.glb` | PBR materials, normal maps, embedded textures |

## Installation

Pre-built native installers are published on each tagged release at
[github.com/marko-koljancic/solarxy/releases](https://github.com/marko-koljancic/solarxy/releases).

| Platform | Download | Install |
|---|---|---|
| macOS (Apple Silicon) | `Solarxy-0.5.0-aarch64.dmg` | Open, drag to **Applications**. See *First launch on macOS* below. |
| macOS (Intel) | `Solarxy-0.5.0-x86_64.dmg` | Same as above. |
| Windows x64 | `solarxy-x86_64-pc-windows-msvc.msi` | Double-click, follow the wizard. Click through SmartScreen (see below). |
| Ubuntu / Debian (x64) | `solarxy_0.5.0-1_amd64.deb` | `sudo apt install ./solarxy_0.5.0-1_amd64.deb` |
| Ubuntu / Debian (ARM64) | `solarxy_0.5.0-1_arm64.deb` | Same as above. |
| Linux distro-agnostic | `Solarxy-0.5.0-x86_64.AppImage` | `chmod +x` then run. Requires glibc 2.28+. |

### First launch on macOS

Solarxy 0.5.0 is shipped **unsigned** — macOS Gatekeeper blocks the first launch.

**Recommended:** double-click **Install CLI.command** inside the mounted DMG. It clears the quarantine attribute on `/Applications/Solarxy.app` (so Gatekeeper won't trigger on first launch) and symlinks `solarxy` into `/usr/local/bin/`. After the sudo prompt, open **Solarxy.app** in Applications — no further prompts.

**Manual bypass** (if you skip the CLI installer): macOS 15 no longer shows an inline "Open Anyway" button in the first-launch dialog.

1. Double-click **Solarxy.app**. macOS says "Solarxy cannot be opened because it cannot be verified." Click **Done**.
2. Open **System Settings → Privacy & Security → Security**. Click **Open Anyway** next to the Solarxy line.
3. Confirm with your password / Touch ID.
4. Launch Solarxy.app again and click **Open** in the final confirmation dialog.

macOS remembers the choice — subsequent launches do not prompt.

### First launch on Windows

The MSI is unsigned — Windows SmartScreen shows **"Windows protected your PC"** on first run. Click **More info** → **Run anyway**. Verify the SHA-256 checksum published alongside the MSI on the release page before proceeding if in doubt.

Code signing (Apple Developer certificate + Azure Trusted Signing) is on the roadmap.

### One-line installers (developers)

For CI or scripted installs, the shell and PowerShell installers from cargo-dist place `solarxy` in `$CARGO_HOME/bin`:

```bash
# macOS / Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/marko-koljancic/solarxy/releases/latest/download/solarxy-installer.sh | sh
```

```powershell
# Windows
powershell -c "irm https://github.com/marko-koljancic/solarxy/releases/latest/download/solarxy-installer.ps1 | iex"
```

### Updating

If installed via any of the installers above (native, shell, PowerShell, or MSI), update in-place with:

```bash
solarxy --update
```

## Build from source

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- MSRV: see `Cargo.toml`

### Build

```bash
cargo build --release
```

### Usage

View a model (default mode):

```bash
cargo r --release -- --model path/to/model.obj
```

Or launch the viewer and drag a file onto the window:

```bash
cargo r --release
```

Analyze a model in the terminal:

```bash
cargo r --release -- --model path/to/model.glb --mode analyze
```

Analyze and save the report to a file:

```bash
cargo r --release -- --model path/to/model.glb --mode analyze --output report.txt
```

## CLI Reference

| Flag | Description | Default |
|---|---|---|
| `-m, --model <PATH>` | Path to model file (optional in view mode -- supports drag-and-drop) | -- |
| `-M, --mode <MODE>` | `view`, `analyze`, `preferences`, or `docs` | `view` |
| `-f, --format <FORMAT>` | Output format: `text` or `json` (analyze mode only) | `text` |
| `-o, --output <PATH>` | Save report to file (analyze mode only) | -- |
| `--about` | Show version and application info | -- |

## View Mode

The viewer renders models with physically-based shading (Cook-Torrance BRDF), normal mapping, real-time shadow mapping, image-based lighting (diffuse irradiance + specular reflections), screen-space ambient occlusion (SSAO), HDR bloom, selectable tone mapping (ACES Filmic, Reinhard, Linear, None), alpha blending, and 4x MSAA anti-aliasing. A 3-light system (key, fill, rim) follows the camera to provide consistent illumination. The scene includes a shadow-catching floor, an infinite grid, an axis gizmo, and optional bounding-box overlays.

A top menu bar (**File**, **Edit**, **View**, **Help**) is visible by default — every viewport setting is reachable from the **View** menu in addition to its keyboard shortcut. Press `F10` to hide or show the menu bar, `F11` to toggle borderless fullscreen.

<p align="center">
  <img src="docs/img/solarxy-view.png" width="100%">
</p>

### Sidebar Panel

Press `Tab` to toggle an interactive sidebar with collapsible sections for view mode, inspection, display toggles, validation, post-processing, and lighting controls. All controls are bidirectionally synced with keyboard shortcuts.

<p align="center">
  <img src="docs/img/solarxy-view-split-3d.png" width="100%">
</p>


### Split Viewport

| Key | Layout |
|---|---|
| `F1` | Single viewport (default) |
| `F2` | Vertical split (left: UV Map, right: 3D) |
| `F3` | Horizontal split (top: UV Map, bottom: 3D) |
| `Ctrl/⌘+L` | Toggle camera linking between panes |

Each pane has independent camera, view mode, inspection mode, and display settings. The active pane is determined by cursor position.

<p align="center">
  <img src="docs/img/solarxy-view-split-UV.png" width="100%">
</p>


### Inspection Modes

| Key | Mode | Description |
|---|---|---|
| `1` | Shaded | Full PBR rendering (default) |
| `2` | Material ID | Flat color per material slot |
| `3` | UV Map | 2D UV-space view with overlap detection |
| `4` | Texel Density | Blue/green/red heat map of UV density |
| `5` | Depth | Linearized depth (white = near, black = far) |

Inspection modes apply per pane in split view and compose independently with view modes (W/X).

<p align="center">
  <img src="docs/img/solarxy-view-inspect.png" width="100%">
</p>

### Camera Controls

| Input | Action |
|---|---|
| Left mouse drag | Orbit |
| Middle mouse drag | Pan |
| Scroll wheel | Zoom |

### Keyboard Shortcuts

#### File

| Key | Action |
|---|---|
| `Ctrl/⌘+O` | Open model (native dialog) |
| `Ctrl/⌘+Shift+O` | Import HDRI (native dialog) |
| `C` | Save screenshot (PNG) |
| `Shift+S` | Save preferences to disk |

#### Display

| Key | Action |
|---|---|
| `W` | Cycle view mode (Shaded / Shaded+Wire / Wireframe) |
| `S` | Shaded mode |
| `X` | Toggle ghosted view |
| `N` | Cycle normals (Off / Face / Vertex / Face+Vertex) |
| `U` | Cycle UV overlay (Off / Gradient / Checker) |
| `B` | Cycle background (White / Gradient / Dark Gray / Black) |
| `G` | Toggle grid |
| `A` | Toggle axis gizmo |
| `V` | Toggle turntable rotation |
| `Shift+W` | Cycle wireframe line weight (Light / Medium / Bold) |
| `Shift+B` | Cycle bounds display (Off / Whole Model / Per Mesh) |
| `I` | Toggle IBL (image-based lighting) |
| `Shift+I` | Cycle IBL mode (Diffuse / Full) |
| `M` | Toggle clay material override (Textured / Clay Light) |
| `Shift+M` | Cycle material override (Textured / Clay Light / Clay Dark / Chrome / Silhouette) |
| `Shift+D` | Toggle bloom effect |
| `Shift+A` | Toggle local axes (model/mesh centers) |
| `Shift+O` | Toggle SSAO (screen-space ambient occlusion) |
| `Shift+T` | Cycle tone mapping (None / Linear / Reinhard / ACES Filmic) |
| `E` / `Shift+E` | Increase / decrease exposure |
| `Shift+V` | Toggle validation overlay |
| `Shift+L` | Toggle lights lock |

#### Camera & Navigation

| Key | Action |
|---|---|
| `H` | Frame model (reset view) |
| `T` `F` `L` `R` | Top / Front / Left / Right view |
| `P` | Perspective projection |
| `O` | Orthographic projection |
| `Arrow keys` | Camera movement |

#### Interface

| Key | Action |
|---|---|
| `F10` | Toggle menu bar |
| `F11` | Toggle borderless fullscreen |
| `Tab` | Toggle sidebar panel |
| `` ` `` | Toggle console panel |
| `?` | Toggle keyboard shortcuts overlay |

> `Esc` no longer quits the app — use **File → Quit** from the menu bar or close the window. Open modals (About, settings dialogs) implement their own Esc-to-dismiss.

## Analyze Mode

The analyzer opens a terminal UI with four tabs: **Overview**, **Meshes**, **Materials**, and **Validation**. Overview shows aggregate counts and bounding box dimensions. Meshes and Materials provide per-element breakdowns. Validation lists errors and warnings found in the model.

<p align="center">
  <img src="docs/img/solarxy-analyze.png" width="100%">
</p>

### Navigation

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Next / previous tab |
| `1` `2` `3` `4` | Jump to tab |
| `j` / `k`, arrows | Scroll up / down |
| `g` / `G` | Jump to top / bottom |
| `PgUp` / `PgDn` | Page scroll |
| `e` | Export text report (prompts for filename) |
| `J` | Export JSON report (prompts for filename) |
| `q` / `Esc` | Quit |

## Preferences

Solarxy persists display, rendering, and lighting settings in a TOML configuration file at a platform-specific location: `~/.config/solarxy/config.toml` on Linux, `~/Library/Application Support/solarxy/config.toml` on macOS, and `%APPDATA%\solarxy\config.toml` on Windows. Preferences are loaded automatically on startup and can be managed in three ways: through the dedicated preferences editor, with keyboard shortcuts in the viewer, or by editing the config file directly.

Launch the preferences editor:

```bash
cargo r --release -- --mode preferences
```

<p align="center">
  <img src="docs/img/solarxy-preferences.png" width="100%">
</p>

### Configurable Settings

| Category | Setting | Values |
|---|---|---|
| Display | Background | White / Gradient / Dark Gray / Black |
| Display | View Mode | Shaded / Shaded+Wire / Wireframe / Ghosted |
| Display | Normals Mode | Off / Face / Vertex / Face+Vertex |
| Display | Grid Visible | on / off |
| Display | Axis Gizmo Visible | on / off |
| Display | Local Axes Visible | on / off |
| Display | Bloom Enabled | on / off |
| Display | SSAO Enabled | on / off |
| Display | UV Mode | Off / Gradient / Checker |
| Display | Projection Mode | Perspective / Orthographic |
| Display | Turntable Active | on / off |
| Display | Turntable RPM | 1.0 -- 60.0 (default 5.0) |
| Display | IBL Mode | Off / Diffuse / Full |
| Display | Tone Mode | None (clip) / Linear / Reinhard / ACES Filmic |
| Display | Exposure | 0.1 -- 10.0 (default 1.0) |
| Display | Inspection Mode | Shaded / Material ID / Texel Density / Depth |
| Display | Texel Density Target | 0.01 -- 10.0 (default 1.0) |
| Rendering | Wireframe Line Weight | Light / Medium / Bold |
| Rendering | MSAA Sample Count | 1 / 2 / 4 |
| Lighting | Lighting Lock | on / off |

### Navigation

| Key | Action |
|---|---|
| `↑` / `↓`, `k` / `j` | Navigate settings |
| `Enter` / `Space` / `→` | Cycle value forward |
| `←` / `h` | Cycle value backward |
| `s` | Save preferences |
| `r` | Reset to defaults |
| `q` / `Esc` | Quit |

Settings can also be changed on the fly in the viewer using keyboard shortcuts and saved with `Shift+S`.

## Docs Mode

The built-in documentation viewer provides an interactive, five-tab reference covering all modes, keyboard shortcuts, CLI options, supported formats, and preferences -- accessible offline without leaving the terminal.

```bash
cargo r --release -- --mode docs
```

<p align="center">
  <img src="docs/img/solarxy-docs.png" width="100%">
</p>

### Navigation

| Key | Action |
|---|---|
| `Tab` / `Shift+Tab` | Next / previous tab |
| `1` `2` `3` `4` `5` | Jump to tab |
| `j` / `k`, arrows | Scroll up / down |
| `g` / `G` | Jump to top / bottom |
| `PgUp` / `PgDn` | Page scroll |
| `q` / `Esc` | Quit |

## Validation Checks

The analyzer runs the following checks and reports errors or warnings:

- Normal count does not match vertex count
- UV count does not match vertex count
- Missing UVs (severity depends on format)
- Non-triangulated meshes (index count not divisible by 3)
- Empty index buffers
- Invalid material references
- Missing texture files
- Degenerate triangles (near-zero-area faces)

## Workspace Structure

SolarXY is built as a Rust workspace with three library crates:

| Crate | Description |
|---|---|
| [`solarxy-core`](crates/solarxy-core/) | Core data types, geometry algorithms, validation, preferences |
| [`solarxy-formats`](crates/solarxy-formats/) | 3D model format loaders (OBJ, STL, PLY, glTF/GLB) |
| [`solarxy-cli`](crates/solarxy-cli/) | CLI parsing (clap) and TUI interfaces (ratatui) |

## Tech Stack

**Core:** Rust 2024 Edition, wgpu 27, winit, WGSL shaders

**UI:** egui 0.33, ratatui, crossterm, clap

**Libraries:** tobj, stl_io, ply-rs-bw, gltf, cgmath, image

## Contributing

Contributions are welcome! Feel free to open an issue or submit a pull request.

## License

Licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Contact

[Marko Koljancic](https://koljam.com/)
