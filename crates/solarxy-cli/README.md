# solarxy-cli

CLI argument parsing and terminal UI interfaces for [Solarxy](https://github.com/marko-koljancic/solarxy).

This crate provides the command-line interface layer: argument parsing via [clap](https://crates.io/crates/clap), and interactive TUI applications via [ratatui](https://crates.io/crates/ratatui) for model analysis, preferences editing, and built-in documentation.

## Components

| Module | Description |
|--------|-------------|
| `parser` | clap-derived `Args` struct with `OperationMode` and `OutputFormat` enums |
| `help` | Styled documentation content for the built-in docs viewer |
| `tui_analysis` | Interactive analysis report TUI (4-tab layout: Overview, Meshes, Materials, Validation) |
| `tui_preferences` | Preferences editor TUI (navigate settings, cycle values, save/reset) |
| `tui_docs` | Documentation viewer TUI (5-tab layout with scrolling and tab navigation) |

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `tui` | Yes | Enables ratatui/crossterm TUI applications (`tui_analysis`, `tui_preferences`, `tui_docs`, `help`) |

Without the `tui` feature, only the `parser` module is available -- useful for embedding Solarxy's CLI parsing in headless tools.

## Usage

```toml
[dependencies]
solarxy-cli = "0.4"
```

### Parsing Arguments

```rust
use clap::Parser;
use solarxy_cli::parser::{Args, OperationMode};

let args = Args::parse();
match args.mode {
    OperationMode::View => { /* launch viewer */ }
    OperationMode::Analyze => { /* run analysis */ }
    OperationMode::Preferences => { /* open preferences editor */ }
    OperationMode::Docs => { /* open docs viewer */ }
}
```

### Key Types

- `Args` -- top-level CLI arguments (model path, mode, format, output, about flag)
- `OperationMode` -- `View`, `Analyze`, `Preferences`, `Docs`
- `OutputFormat` -- `Text`, `Json`
- `AppInfo` -- version/description/repository/license for the About tab

## Part of the Solarxy Workspace

| Crate | Description |
|-------|-------------|
| [solarxy-core](../solarxy-core/) | Core types, geometry, validation, preferences |
| [solarxy-formats](../solarxy-formats/) | 3D model format loaders |
| **solarxy-cli** | CLI parsing and TUI interfaces |

## License

MIT
