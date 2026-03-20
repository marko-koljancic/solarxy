# solarxy

A lightweight, cross-platform 3D model viewer and validator, built with Rust and `wgpu`. It provides a simple interface for inspecting 3D models, with support for common formats and validation checks.

## Roadmap

- **Multi-format Support**: Load and inspect `.obj` models. (glTF support is on the roadmap).
- **Real-time Rendering**: Basic real-time rendering with texturing and lighting.
- **Validation**: Built-in checks to identify common issues in supported 3D models.
- **Flexible Workflow**: Use either the graphical interface or the command-line for validation.

## Technical Details

- **Core**: Written in Rust (2024 Edition).
- **Rendering**: `wgpu` for cross-platform graphics, using shaders written in WGSL.
- **Windowing**: `winit` for window creation and management.
- **CLI**: `clap` for command-line argument parsing and `ratatui` for the terminal user interface.
- **Model Loading**: `tobj` for `.obj` file parsing.
- **Math**: `cgmath` for 3D mathematics.

## Getting Started

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))
- MSRV (Minimum Supported Rust Version): See `Cargo.toml`.

### Installation & Running

To build and run the application in release mode:

```bash
cargo build --release
cargo run --release -- --model [path-to-your-model.obj]
```

To run in debug mode:

```bash
cargo run -- --model [path-to-your-model.obj]
```

## Contributing

Contributions are welcome! Feel free to open an issue or submit a pull request.

## License

Licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Contact

[Marko Koljancic](https://koljam.com/)
