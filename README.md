# solarxy

A lightweight, cross-platform 3D model viewer and validator. Features a simple UI, glTF/OBJ support, basic shading, and validation checks for common model issues. Built with Rust and WebGPU for cross-platform GPU acceleration.

## Features

- **Multi-format support**: Open and inspect OBJ, glTF, and other common 3D model formats
- **Real-time rendering**: Simple, PBR and wireframe rendering modes
- **Validation**: Built-in consistency checks to catch common model issues
- **Flexible workflow**: Both command-line and GUI modes available

## Getting Started

### Prerequisites

- Rust toolchain (install from [rustup.rs](https://rustup.rs))

### Installation

```bash
cargo build --release
cargo run --release
```

## Development

- **Core**: Written in Rust
- **Rendering**: wgpu (WebGPU abstraction layer)
- **Testing**: Run `cargo test` for validation suites

## License

Licensed under the MIT License - see the included LICENSE file for details
or view the full text at <https://opensource.org/licenses/MIT>

## Contact

[Marko Koljancic](https://koljam.com/)
