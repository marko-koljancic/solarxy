# solarxy

A lightweight, cross-platform tool to view and validate 3D models and scenes.
This initial version provides a simple UI, glTF/OBJ import, basic shading,
and a validation checker that catches common model issues.
The renderer uses WebGPU for cross-platform GPU acceleration and Rust for core logic.

## Features

- Open and inspect glTF, OBJ, and other common model formats
- Real-time PBR and wireframe rendering modes
- Validation and consistency checks
- Command-line and GUI modes for different workflows

## Getting started

- [::: REMOVE :::] Browser (WASM): build to WebAssembly and serve with a static file server;
  requires a WebGPU-capable browser
- Native: build and run with Cargo (Rust and wgpu backends)

## Development

- [::: REMOVE :::] Core in Rust, rendering via wgpu (WebGPU abstraction)
- [::: REMOVE :::] Minimal JS/HTML bootstrap for the WASM target
- Tests and validation suites included (cargo test)

## License

Licensed under the MIT License - see the included LICENSE file for details
or view the full text at <https://opensource.org/licenses/MIT>

## Contact

[Marko Koljancic](https://koljam.com/)
