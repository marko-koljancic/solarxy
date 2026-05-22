# solarxy

![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)
![Rust](https://img.shields.io/badge/Rust-1.92%2B-orange.svg)
![Release](https://github.com/marko-koljancic/solarxy/actions/workflows/release.yml/badge.svg)
![GitHub Release](https://img.shields.io/github/v/release/marko-koljancic/solarxy)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-informational)
[![Wiki](https://img.shields.io/badge/Wiki-User%20Docs-blue)](https://github.com/marko-koljancic/solarxy/wiki)

A lightweight, cross-platform 3D model viewer and visual debugger built with Rust and wgpu. Inspect 3D models in a real-time graphical viewer with split viewports and inspection overlays, or analyze them from the terminal with built-in validation checks.

<p align="center">
  <img src="res/solarxy.gif" width="100%">
</p>

## Documentation

Full user documentation lives in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki):

- [User Guide](https://github.com/marko-koljancic/solarxy/wiki/User-Guide) — viewer, analyze TUI, preferences, validation
- [Installation](https://github.com/marko-koljancic/solarxy/wiki/Installation) — install paths per platform, first-launch caveats, system requirements
- [CLI Reference](https://github.com/marko-koljancic/solarxy/wiki/CLI-Reference) — every flag for `solarxy` and `solarxy-cli`
- [Keyboard Shortcuts](https://github.com/marko-koljancic/solarxy/wiki/Keyboard-Shortcuts) — full reference (or press `?` in the GUI)
- [Troubleshooting](https://github.com/marko-koljancic/solarxy/wiki/Troubleshooting) — common errors, performance tips, config reset
- [Release Notes](https://github.com/marko-koljancic/solarxy/wiki/Release-Notes) — version history and breaking changes

## Features

- **Multi-format support** -- OBJ, STL, PLY, and glTF/GLB
- **PBR rendering** -- Cook-Torrance BRDF, normal mapping, shadow mapping, IBL (diffuse + specular), SSAO, bloom, selectable tone mapping (Reinhard, ACES Filmic, Linear, None), alpha blending, 3-light system, 4x MSAA
- **Split viewport** -- side-by-side or stacked panes with independent cameras and display settings per pane; draggable divider (double-click to reset 50/50)
- **Inspection modes** -- Material ID, UV Map with overlap detection, Texel Density heat map, Depth, **Overdraw** heat map (`6`), **AO Preview** (`7`)
- **Material overrides** -- Clay Light, Clay Dark, Chrome (IBL-only reflective black), and Silhouette (flat black) for surface inspection
- **Review System** -- place spatially-anchored annotations on a model's surface, save to `<model>.solarxy-review.json` sidecar, re-open later to see annotations re-anchored. Categories (Info / Warning / Question / Change), threaded replies, cascade-delete confirm. Toggle via `Shift+R`.
- **Material Inspector** -- view-only per-material panel (`Window → Material Inspector`) with base-color swatch, scalar PBR (metallic/roughness), alpha mode, and 128×128 texture thumbnails for albedo / normal / metallic-roughness / occlusion / emissive
- **Validation overlay** -- color-coded 3D visualization of validation issues (flipped normals, non-manifold edges, triangle-budget overruns, degenerate triangles, missing UVs, bad material refs)
- **Configurable validation** -- per-project `solarxy.toml` overrides budgets, severities, and filename-classifier rules; JSON Schema published at [`schemas/solarxy-config.v1.json`](schemas/solarxy-config.v1.json) for editor autocomplete
- **CI-friendly CLI** -- `solarxy-cli analyze --paths "assets/**/*.glb" --adapter github-actions --adapter-format sarif` emits SARIF / JUnit-style / TAP / workflow-commands output via the `solarxy-validate` adapter crate
- **Dockable panels** -- Viewport / Sidebar / Review Panel / Console / Material Inspector / Stats tabs in an `egui_dock` layout; auto-save on quit, restore on launch, **Window → Save Layout / Restore / Reset**
- **Ayu Mirage theme** -- flat dark UI with amber accent (`#FFC44C`), bundled Lilex font, custom dock styling
- **egui sidebar** -- interactive control panel with bidirectional keyboard sync
- **Interactive analysis** -- TUI with per-mesh and per-material breakdowns, validation checks
- **Persistent preferences** -- configure defaults via the GUI **Edit → Preferences…** dialog (`Ctrl/⌘+,`) or tweak the TOML file directly; live viewer changes are persisted via **Edit → Save View Settings as Default**
- **Drag-and-drop** -- drop model files or HDR/EXR environment maps directly into the viewer window

## Supported Formats

| Format | Extensions | Notes |
|---|---|---|
| Wavefront OBJ | `.obj` | Meshes, materials (`.mtl`), textures, UVs |
| STL | `.stl` | Geometry only, no materials |
| PLY | `.ply` | Flexible vertex attributes, optional normals and UVs |
| glTF 2.0 | `.gltf`, `.glb` | PBR materials, normal maps, embedded textures |

## Installation

```bash
# macOS — installs both binaries, clears Gatekeeper automatically
brew install --cask koljam/solarxy/solarxy

# Linux — GUI via Flathub
flatpak install flathub dev.koljam.solarxy

# Windows — winget (auto-submitted on each stable release)
winget install Koljam.Solarxy
```

Direct downloads (DMG / MSI / AppImage), CLI-only installs, first-launch
caveats, system requirements, and the update flow: see
[Wiki / Installation](https://github.com/marko-koljancic/solarxy/wiki/Installation).

## Usage

```bash
solarxy -m path/to/model.obj                                    # GUI viewer
solarxy-cli --mode analyze -m model.glb                         # Terminal report (single file)
solarxy-cli --mode analyze -m model.glb -f json -o report.json  # JSON to file

# Batch validation for CI (GitHub Actions / Perforce / GitLab / Jenkins):
solarxy-cli --mode analyze \
    --paths "assets/**/*.glb" \
    --config solarxy.toml \
    --adapter github-actions \
    --adapter-format sarif \
    --output report.sarif
```

Every flag for both binaries, validation error reference, and analyze TUI
shortcuts: see
[Wiki / CLI Reference](https://github.com/marko-koljancic/solarxy/wiki/CLI-Reference).

## Build from source

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- MSRV: see `Cargo.toml`

### Build

```bash
cargo build --release
```

### Run

```bash
cargo r --release --bin solarxy -- --model path/to/model.obj                 # GUI
cargo r --release --bin solarxy-cli -- --mode analyze --model path/to/m.glb  # CLI
```

## Workspace Structure

Solarxy is built as a Rust workspace with one binary entrypoint plus six library crates:

| Crate | Description |
|---|---|
| [`solarxy`](.) | GUI binary entrypoint (`src/main.rs`) — parses GUI args, sets up tracing, launches `solarxy-app` |
| [`solarxy-core`](crates/solarxy-core/) | Pure data types, geometry, validation, preferences, view config — no GPU / winit / egui |
| [`solarxy-formats`](crates/solarxy-formats/) | OBJ / STL / PLY / glTF loaders → `RawModelData` |
| [`solarxy-renderer`](crates/solarxy-renderer/) | wgpu pipelines, shaders, IBL / SSAO / bloom / shadow / composite |
| [`solarxy-app`](crates/solarxy-app/) | winit `ApplicationHandler`, egui sidebar / menu / console / dialogs, state machine |
| [`solarxy-cli`](crates/solarxy-cli/) | clap parser, analyze TUI, terminal companion binary (`solarxy-cli`) |
| [`solarxy-validate`](crates/solarxy-validate/) | Validation orchestration + pipeline adapters (GitHub Actions / generic-JSON). Library API for vendor integrators; consumed by `solarxy-cli` |

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
