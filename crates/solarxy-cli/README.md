# solarxy-cli

CLI argument parsing and terminal UI interfaces for [Solarxy](https://github.com/marko-koljancic/solarxy).

This crate provides the command-line interface layer: argument parsing via [clap](https://crates.io/crates/clap), and an interactive analysis TUI via [ratatui](https://crates.io/crates/ratatui). It also hosts the standalone `solarxy-cli` binary that exec's the GUI viewer when invoked with `--mode view`.

## Components

| Module | Description |
|--------|-------------|
| `parser` | clap-derived `Args` struct with `OperationMode` and `OutputFormat` enums |
| `calc::analyze` | Model analysis (counts, AABB, validation, per-mesh / per-material breakdowns) |
| `tui_analysis` | Interactive analysis report TUI (4-tab layout: Overview, Meshes, Materials, Validation) |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `tui` | Yes | Enables the ratatui/crossterm `tui_analysis` TUI |
| `analyzer` | Yes | Enables `calc::analyze` (depends on `solarxy-formats`) |
| `updater` | Yes | Enables `--update` self-update via `axoupdater` |

Without the `tui` feature, only the `parser` module is available — useful for embedding Solarxy's CLI parsing in headless tools.

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

MIT
