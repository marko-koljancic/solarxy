# solarxy-cli

CLI argument parsing and terminal UI interfaces for [Solarxy](https://github.com/marko-koljancic/solarxy).

This crate provides the command-line interface layer: argument parsing via [clap](https://crates.io/crates/clap), and an interactive analysis TUI via [ratatui](https://crates.io/crates/ratatui). It also hosts the standalone `solarxy-cli` binary that exec's the GUI viewer when invoked with `--mode view`.

## Components

| Module | Description |
|--------|-------------|
| `parser` | clap-derived `Args` struct with `OperationMode` and `OutputFormat` enums |
| `calc::analyze` | Model analysis (counts, AABB, validation, per-mesh / per-material breakdowns) |
| `tui` | The terminal substrate and the analyze surface: a tiled workspace of ten panel types, three presets and free arrangement, over a capability model, a file-based theme system, a split tree generic over its panels, one keymap table and a braille rasteriser |
| `render_sink` | The plain progress line: one row on standard error, rewritten in place on a terminal and written once per step anywhere else |
| `render_tui` | The render dashboard: six readouts over one render, on the same substrate the analyze surface uses |
| `render_watch` | The live window: the picture as it converges, behind the `watch` feature |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `tui` | Yes | Enables the ratatui/crossterm terminal surfaces. The `solarxy-cli` binary requires it |
| `analyzer` | Yes | Enables `calc::analyze` (depends on `solarxy-formats`) |
| `updater` | Yes | Enables `--update` self-update via `axoupdater` |
| `render` | Yes | Enables `solarxy-cli render`, which brings the GPU stack into an otherwise pure-CPU binary |
| `watch` | No | Enables `--watch`, the live window. Off by default so a build carries no window code unless it is asked for; the distribution build turns it on |

Without the `tui` feature, only the `parser` module is available — useful for embedding Solarxy's CLI parsing in headless tools.

## The render command

`solarxy-cli render` takes a `.slxy` scene or a bare model file and writes an
image. Every flag that has a counterpart on the scene's render node is an
override, so the node stays authoritative.

Progress has three surfaces, all reading one stream, and all of them report on
**standard error**: standard output carries data, which is the image when
`--out -` is given and the result when `--json` is.

| Surface | How | What it shows |
|---|---|---|
| Plain line | the default | One row, rewritten in place on a terminal and written once per step in a log |
| Dashboard | `--tui` | Tile grid, sample gauge with an estimate, per-stage timings, what was asked for, throughput, and the picture as it converges |
| Window | `--watch` | The picture, on screen, with pan, zoom and pass selection, holding the finished frame until dismissed |

Two things worth knowing about them. The dashboard **falls back to the plain
line** when standard error is not a terminal or is smaller than it needs, and
says so; because it paints on standard error rather than taking standard
output, `--tui` and `--json` compose. And a run with either of the two picture
surfaces asks the renderer for a **finer tiling**, because pixels reach a
surface only when a tile finishes and an ordinary image is a single tile. The
image written is unchanged either way.

The window's controls: drag pans, the wheel zooms about the cursor, and `F`
or the Fit button returns to the letterbox fit, which is the default;
resizing refits. The canvas around the picture is a checker that never moves
with it. A pass selector defaults to the beauty and lists the passes `--aov`
requested, with an unrequested pass disabled and naming the flag that would
produce it; a raster run, which writes no passes, shows the beauty alone.
Switching passes never restarts the render. Escape or `q` cancels it, as
does closing the window; a float render previews clamped and encoded, and
the chrome says so, because the file is where the real values are.

Exit codes are the taxonomy a build system branches on: 0 rendered, 1 usage,
2 input, 3 cook, 4 no adapter, 5 device lost, 6 cancelled, 7 output.

## Usage

```toml
[dependencies]
solarxy-cli = "0.5"
```

### Parsing arguments

```rust
use clap::Parser;
use solarxy_cli::parser::{Args, OperationMode};

let args = Args::parse();
match args.mode {
    OperationMode::View => { /* exec the GUI binary */ }
    OperationMode::Analyze => { /* run analysis */ }
}
```

### Key types

- `Args` — top-level CLI arguments (model path, mode, format, output, `--about`, `--update`)
- `OperationMode` — `View`, `Analyze`
- `OutputFormat` — `Text`, `Json`

> Documentation lives in the [Solarxy Wiki](https://github.com/marko-koljancic/solarxy/wiki). The in-terminal `--mode docs` viewer was retired in v0.5.x; preferences moved to the GUI **Edit → Preferences…** dialog.

## Part of the Solarxy workspace

| Crate | Description |
|-------|-------------|
| [solarxy-core](../solarxy-core/) | Core types, geometry, validation, preferences |
| [solarxy-formats](../solarxy-formats/) | 3D model format loaders |
| [solarxy-renderer](../solarxy-renderer/) | wgpu rendering pipelines |
| [solarxy-app](../solarxy-app/) | winit + egui GUI app |
| **solarxy-cli** | CLI parsing, analysis, terminal companion binary |

## License

GPL-3.0-or-later. See the workspace [LICENSE](../../LICENSE).
