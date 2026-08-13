# solarxy

![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)
![Rust](https://img.shields.io/badge/Rust-1.92%2B-orange.svg)
![Release](https://github.com/marko-koljancic/solarxy/actions/workflows/release.yml/badge.svg)
![GitHub Release](https://img.shields.io/github/v/release/marko-koljancic/solarxy)
![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux%20%7C%20Web-informational)
[![Solarxy Web](https://img.shields.io/badge/Solarxy%20Web-WebGPU%20beta-8A2BE2)](https://solarxy.koljam.com)
[![Wiki](https://img.shields.io/badge/Wiki-User%20Docs-blue)](https://github.com/marko-koljancic/solarxy/wiki)

Solarxy is a cross-platform 3D model viewer, validator, and browser-based node modeler built with Rust and wgpu (WebGPU). Inspect and validate models in a native real-time viewer, analyze them from the terminal for CI, or build geometry parametrically in the browser with a node graph. All three surfaces run on one shared Rust core.

<p align="center">
  <img src="res/solarxy.gif" width="100%">
</p>

## Three ways to run it

As of v0.7.0 the same Rust core powers three surfaces:

- **`solarxy`** - the native GUI viewer (window, PBR rendering, split viewports, sidebar, inspection overlays, review system).
- **`solarxy-cli`** - the terminal companion: `analyze` (model report / interactive TUI / JSON export) and `view` (launches the GUI), plus `--update`.
- **[Solarxy Web](https://solarxy.koljam.com)** - the viewer, validator, and review toolset in the browser on WebGPU, plus **node-based parametric modeling** on the same core. It is a public beta: nothing to install, nothing uploaded to a server.

## Solarxy Web

Solarxy Web runs entirely in your browser at **[solarxy.koljam.com](https://solarxy.koljam.com)**. It renders with WebGPU and adds a node graph for building and modifying geometry non-destructively, on the same Rust engine and wgpu renderer as the desktop app.

- **Parametric node graph** - primitives, transforms, arrays, mirrors, materials, UV projection, and more flow through a typed node graph (77 node types across 15 categories). Change a parameter upstream and everything downstream recooks.
- **Typed connections** - ports are typed and color-coded; illegal connections are rejected with feedback and lossy ones are flagged.
- **Imports as nodes** - OBJ, STL, PLY, and glTF/GLB load through import nodes (off the main thread) and flow into the graph like any other geometry; textures and materials come along.
- **Full desktop toolset** - PBR rendering, the inspection modes, UV overlap, validation overlays, HDRI/IBL, split viewports, transform gizmos, dockable panes, and the review system, all in the browser.
- **`.slxy` scene files** - `File -> Save` writes a self-contained archive holding the document and every asset it references, so it reopens later on any machine with nothing missing. The format is frozen at version 1.
- **Autosave** - your document is continuously saved to the browser's private storage (OPFS); reopening after a crash offers to recover it.

Solarxy Web needs a WebGPU-capable browser (Chrome or Edge 113+, Safari 26+ on macOS; Firefox support is arriving). If your browser cannot run it, the desktop app does everything the viewer and validator do, natively. See [Wiki / Solarxy Web](https://github.com/marko-koljancic/solarxy/wiki/Solarxy-Web) and the generated [Node Reference](https://github.com/marko-koljancic/solarxy/wiki/Node-Reference).

## Documentation

Full user documentation lives in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki):

- [Solarxy Web](https://github.com/marko-koljancic/solarxy/wiki/Solarxy-Web) - the browser app, browser requirements, node modeling, saving your work
- [Node Reference](https://github.com/marko-koljancic/solarxy/wiki/Node-Reference) - every node type, generated from the registry
- [User Guide](https://github.com/marko-koljancic/solarxy/wiki/User-Guide) - viewer, analyze TUI, preferences, validation
- [Installation](https://github.com/marko-koljancic/solarxy/wiki/Installation) - install paths per platform, first-launch caveats, system requirements
- [CLI Reference](https://github.com/marko-koljancic/solarxy/wiki/CLI-Reference) - every flag for `solarxy` and `solarxy-cli`
- [Keyboard Shortcuts](https://github.com/marko-koljancic/solarxy/wiki/Keyboard-Shortcuts) - full reference (or press `?` in the app)
- [Troubleshooting](https://github.com/marko-koljancic/solarxy/wiki/Troubleshooting) - common errors, performance tips, config reset
- [Release Notes](https://github.com/marko-koljancic/solarxy/wiki/Release-Notes) - version history and breaking changes

## Features

### Modeling and geometry (Solarxy Web)

- **Node-based parametric modeling** - a non-destructive node graph with 77 node types across four typed contexts: object-level containers, cameras, lights and the scene environment; geometry primitives (box, sphere, cylinder, cone, plane, torus, torus knot, line, circle) and modifiers (transform, displace, array, scatter, copy to points, mirror, merge, subdivide, delete, compute normals, UV project, points from geo, edges to geo, attribute create, attribute randomize, attribute wrangle, attribute promote, attribute copy, attribute from image); texture networks (constant, ramp, noise, voronoi, gradient, checker, brick, levels, blur, pack ORM, height-to-normal, and more); material networks (principled, matcap, toon, unlit, mix); plus imports, utility nodes and export taps.
- **Materials and textures** - an inline material node with optional image map ports, planar/box/cylindrical/spherical UV projection, and a content-addressed texture cache; the `Image` data type carries decoded bitmaps through the graph.
- **Transform gizmos** - interactive translate, rotate, and scale gizmos with Ctrl snapping, world/local orientation, and a live delta readout.
- **Subflows** - a `geo` container opens its own canvas, so a complex object collapses to one tidy node in the scene.

### Rendering and inspection

- **PBR rendering** - Cook-Torrance BRDF, normal mapping, shadow mapping, IBL (diffuse + specular), SSAO, bloom, selectable tone mapping (Reinhard, ACES Filmic, Linear, None), alpha blending, multi-light direct lighting, 4x MSAA.
- **Split viewport** - side-by-side, stacked, quad, or three-left-big panes with independent cameras and display settings per pane.
- **Inspection modes** - Shaded, Material ID, UV Map with overlap detection, Texel Density heat map, Depth, Overdraw heat map, AO Preview.
- **Material overrides** - Clay Light, Clay Dark, Chrome (IBL-only reflective black), and Silhouette (flat black) for surface inspection.
- **Material Inspector** - view-only per-material panel with base-color swatch, scalar PBR (metallic/roughness), alpha mode, and 128x128 texture thumbnails for albedo / normal / metallic-roughness / occlusion / emissive.
- **Validation overlay** - color-coded 3D visualization of validation issues (flipped normals, non-manifold edges, triangle-budget overruns, degenerate triangles, missing UVs, bad material refs).
- **Review System** - place spatially-anchored annotations on a model's surface (desktop saves a `<model>.solarxy-review.json` sidecar; web keeps them in the `.slxy` document), categories (Info / Warning / Question / Change), threaded replies, re-anchoring, cascade-delete confirm. Toggle via `Shift+R`.

### Validation and tooling

- **Configurable validation** - per-project `solarxy.toml` overrides budgets, severities, and filename-classifier rules; JSON Schema published at [`schemas/solarxy-config.v1.json`](schemas/solarxy-config.v1.json) for editor autocomplete.
- **CI-friendly CLI** - `solarxy-cli --mode analyze --paths "assets/**/*.glb" --adapter github-actions --adapter-format sarif` emits SARIF / JUnit-style / TAP / workflow-commands output via the `solarxy-validate` adapter crate.
- **Dockable panels** - the viewport and every panel are rearrangeable tabs; layouts persist across launches (`egui_dock` on desktop, `dockview` on web).
- **Ayu Mirage theme** - flat dark UI with amber accent (`#FFC44C`), bundled Lilex font.
- **Interactive analysis** - a terminal TUI with per-mesh and per-material breakdowns and validation checks.
- **Persistent preferences** - configure defaults via the GUI **Edit -> Preferences...** dialog (`Ctrl/Cmd+,`) or edit the TOML directly; live viewer changes persist via **Edit -> Save View Settings as Default**.
- **Drag-and-drop** - drop model files or HDR/EXR environment maps directly into the viewer.

## Supported Formats

| Format | Extensions | Notes |
|---|---|---|
| Wavefront OBJ | `.obj` | Meshes, materials (`.mtl`), textures, UVs |
| STL | `.stl` | Geometry only, no materials |
| PLY | `.ply` | Flexible vertex attributes, optional normals and UVs |
| glTF 2.0 | `.gltf`, `.glb` | PBR materials, normal maps, embedded or external textures |

On the desktop these open directly; in Solarxy Web they load through import nodes. Draco-compressed glTF is not supported (re-export uncompressed).

## Installation

The fastest way to try Solarxy is the browser app: open **[solarxy.koljam.com](https://solarxy.koljam.com)**, nothing to install.

For the native desktop app and CLI:

```bash
# macOS - Homebrew. The cask installs the GUI app (Gatekeeper cleared
# automatically); the formula installs the CLI.
brew install --cask marko-koljancic/solarxy/solarxy
brew install marko-koljancic/solarxy/solarxy-cli

# Windows - winget
winget install Koljam.Solarxy
```

**Linux** and direct downloads (DMG / MSI / AppImage), CLI-only installs, first-launch caveats, system requirements, and the update flow: see [Wiki / Installation](https://github.com/marko-koljancic/solarxy/wiki/Installation).

## Usage

```bash
solarxy -m path/to/model.obj                                    # GUI viewer
solarxy-cli --mode analyze -m model.glb                         # Terminal report (single file)
solarxy-cli --mode analyze -m model.glb -f json -o report.json  # JSON to file

# Batch validation for CI:
solarxy-cli --mode analyze \
    --paths "assets/**/*.glb" \
    --config solarxy.toml \
    --adapter github-actions \
    --adapter-format sarif \
    --output report.sarif
```

Every flag for both binaries, the validation error reference, and analyze TUI shortcuts: see [Wiki / CLI Reference](https://github.com/marko-koljancic/solarxy/wiki/CLI-Reference).

## Build from source

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs)). MSRV: Rust 1.92, edition 2024.
- For the web shell: the `wasm32-unknown-unknown` target, plus `wasm-bindgen-cli` and `wasm-opt` (binaryen), and Node.js.

### Desktop (GUI + CLI)

```bash
cargo build --release
cargo r --release --bin solarxy -- --model path/to/model.obj                 # GUI
cargo r --release --bin solarxy-cli -- --mode analyze --model path/to/m.glb  # CLI
```

### Web shell

The frontend needs the wasm host built first (`web/src/wasm/pkg/` is a gitignored build output), then a served secure context (localhost qualifies) for WebGPU and OPFS.

```bash
bash crates/solarxy-web/build-wasm.sh web/src/wasm/pkg          # build wasm -> wasm-bindgen -> wasm-opt into web/
cd web && npm install && npm run dev                           # Vite dev server (predev rebuilds the wasm)
cd web && npm run typecheck && npm test && npm run build       # tsc + vitest + production bundle
```

The `getrandom_backend="wasm_js"` rustflag for `wasm32` lives in the workspace `.cargo/config.toml`.

## Workspace Structure

Solarxy is a Rust workspace of 15 members: the root GUI binary plus 14 crates spanning the desktop GUI, the CLI, headless rendering, and the web shell, alongside the `web/` React frontend.

| Crate | Description |
|---|---|
| [`solarxy`](.) | GUI binary entrypoint (`src/main.rs`): parses GUI args, sets up tracing, loads preferences, launches `solarxy-app`. |
| [`solarxy-core`](crates/solarxy-core/) | Pure data types: geometry, validation, preferences, view config, raycast, and the GPU-free `SceneDelta` engine/renderer contract. No GPU / winit / egui; feature-gated serde and fs. |
| [`solarxy-formats`](crates/solarxy-formats/) | OBJ / STL / PLY / glTF loaders to `RawModelData`. Byte-first API (wasm-clean); path wrappers behind `std-fs`. |
| [`solarxy-imaging`](crates/solarxy-imaging/) | Pure-CPU image operators for the texture context: adjust, composite, generate, filter, and ORM packing over `RawImageData`. Deterministic, single-threaded, wasm-clean. |
| [`solarxy-kernel`](crates/solarxy-kernel/) | Pure-CPU parametric geometry: `GeometrySet`, the primitive generators, transform bake, and merge. wasm-clean, no wgpu/fs. |
| [`solarxy-bvh`](crates/solarxy-bvh/) | GPU-free bounding volume hierarchy for ray queries: the 32-byte node, a binned-SAH BVH2 builder, the two-level structure, and the CPU traversal the shader kernel is a twin of. Depends on `solarxy-core` alone, so the import worker can build one without a GPU. |
| [`solarxy-graph`](crates/solarxy-graph/) | The headless studio core: node-graph document, topology, cook engine, node registry (77 node types + typed-port coercion), undo, review, and the `Engine` facade (Command in, EventBatch out). No wgpu, no winit. |
| [`solarxy-scenefile`](crates/solarxy-scenefile/) | The `.slxy` self-contained scene file: schema-owned scene/manifest types, the ZIP container with content-addressed asset blobs, SHA-256 integrity, and the schema-version migration gate. |
| [`solarxy-renderer`](crates/solarxy-renderer/) | All wgpu state: pipelines, shaders, IBL / SSAO / bloom / shadow / composite, and the multi-object `SceneObjects` path. winit/egui-decoupled; compiles to wasm32. |
| [`solarxy-host`](crates/solarxy-host/) | Shared host orchestration both shells drive: the per-pane pass chain and composite, the uniform writes, the lighting chokepoint, the per-pane camera lifecycle, the view-state model, and the gizmo drag solver. Also the raster implementation of the renderer's backend contract, which owns the multi-object scene. No engine dependency. |
| [`solarxy-app`](crates/solarxy-app/) | winit `ApplicationHandler` + egui: the desktop shell (sidebar, menu, console, dialogs, dock, review UI). |
| [`solarxy-web`](crates/solarxy-web/) | The `wasm-bindgen` boundary + WebGPU host: hosts the canvas, drives the frame loop, and serializes Commands and Events between the Rust core and the React frontend. |
| [`solarxy-validate`](crates/solarxy-validate/) | Validation orchestration + pipeline adapters (GitHub Actions / generic-JSON). Library API for integrators; consumed by `solarxy-cli`. |
| [`solarxy-render`](crates/solarxy-render/) | Headless render orchestration: loads a scene file or a bare model into one document, cooks it, renders it through the same backends the shells drive, and writes the image. Library API for integrators, so rendering needs no subprocess. |
| [`solarxy-cli`](crates/solarxy-cli/) | clap parser, analyze TUI, terminal companion binary (`solarxy-cli`). |

`web/` is the frontend: a Vite + React 19 display mirror of the Rust-owned document. The palette, typed handles, and parameter panel are pure interpreters of the node registry, so a node added in Rust needs zero frontend changes.

## Tech Stack

**Core:** Rust 2024 edition, wgpu (WebGPU / native), WGSL shaders.

**Desktop UI:** egui, ratatui, crossterm, clap, winit.

**Web:** wasm-bindgen, React 19, Vite 6, `@xyflow/react` (node canvas), zustand (state), dockview (docking).

**Format and geometry libraries:** tobj, stl_io, ply-rs-bw, gltf, cgmath, image.

## Contributing

Contributions are welcome. Feel free to open an issue or submit a pull request. See the [Contributing](https://github.com/marko-koljancic/solarxy/wiki/Contributing) page for details.

## License

Licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

The path-traced renderer ports code from MIT-licensed work by other authors, and the
published algorithms it implements are credited alongside it. Both are recorded in
[THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES.md), which ships inside every distribution.

## Contact

[Marko Koljancic](https://koljam.com/)
