# solarxy-formats

3D model format loaders for [Solarxy](https://github.com/marko-koljancic/solarxy). Reads OBJ, STL, PLY, and glTF/GLB files into a common `RawModelData` representation.

## Supported Formats

| Format | Extensions | Loader | Notes |
|--------|-----------|--------|-------|
| Wavefront OBJ | `.obj` | [tobj](https://crates.io/crates/tobj) | Meshes, materials (.mtl), textures, UVs, PBR extensions (Pr/Pm) |
| STL | `.stl` | [stl_io](https://crates.io/crates/stl_io) | Binary and ASCII, geometry only |
| PLY | `.ply` | [ply-rs-bw](https://crates.io/crates/ply-rs-bw) | Flexible attributes, companion texture detection |
| glTF 2.0 | `.gltf`, `.glb` | [gltf](https://crates.io/crates/gltf) | Full PBR, normal maps, embedded textures, scene hierarchy |

## Usage

```toml
[dependencies]
solarxy-formats = "0.4"
```

```rust
use solarxy_formats::load_model;

let model = load_model("path/to/model.glb")?;
println!("Loaded {} mesh(es), {} material(s)",
    model.meshes.len(), model.materials.len());
```

The `load_model` function auto-detects the format from the file extension and returns `anyhow::Result<RawModelData>`.

### Per-Format Modules

Each format also has a dedicated module for direct access:

```rust
use solarxy_formats::obj::load_obj;
use solarxy_formats::stl::load_stl;
use solarxy_formats::ply::load_ply;
use solarxy_formats::gltf::load_gltf;
```

## Re-exports

This crate re-exports the core geometry types from `solarxy-core`:

`RawMeshData`, `RawMaterialData`, `RawImageData`, `RawModelData`

## Part of the Solarxy Workspace

| Crate | Description |
|-------|-------------|
| [solarxy-core](../solarxy-core/) | Core types, geometry, validation, preferences |
| **solarxy-formats** | 3D model format loaders |
| [solarxy-cli](../solarxy-cli/) | CLI parsing and TUI interfaces |

## License

MIT
