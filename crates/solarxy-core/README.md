# solarxy-core

Pure-Rust core data types, geometry algorithms, and validation for 3D model analysis.

This crate provides the foundational types and logic used by the [Solarxy](https://github.com/marko-koljancic/solarxy) 3D model viewer and validator. It has no GPU dependencies and can be used independently for model processing and validation pipelines.

## Key Types

### Geometry

- `AABB` -- axis-aligned bounding box with min/max corners, center, diagonal, and merge operations
- `RawMeshData` -- per-mesh vertex positions, normals, texture coordinates, indices, and material ID
- `RawMaterialData` -- material properties (ambient, diffuse, specular, roughness, metallic, textures)
- `RawImageData` -- texture image data (RGBA pixels, dimensions)
- `RawModelData` -- complete model: meshes, materials, images, and model name

### Validation

- `ValidationReport` -- collection of `ValidationIssue` items with error/warning counts
- `ValidationIssue` -- severity, scope (mesh or material), kind, and human-readable message
- `ValidationResult` -- report plus per-mesh degenerate triangle face lists

### Enums

Display and rendering mode enums with `Display`, serialization, and cycling support:

`ViewMode`, `InspectionMode`, `BackgroundMode`, `NormalsMode`, `UvMode`, `LineWeight`, `BoundsMode`, `ProjectionMode`, `IblMode`, `ToneMode`, `PaneMode`, `UvMapBackground`

### Preferences (feature-gated)

- `Preferences` -- serializable config struct with display, rendering, lighting, window, and history sections

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `config` | Yes | Enables `Preferences`, JSON report serialization, and config file I/O (adds serde, toml, dirs, tracing) |

## Usage

```toml
[dependencies]
solarxy-core = "0.4"
```

```rust
use solarxy_core::{RawMeshData, RawModelData, ValidationResult};
use solarxy_core::validation::validate_raw_model;

let model: RawModelData = /* load from your source */;
let result: ValidationResult = validate_raw_model(&model);

for issue in &result.report.issues {
    println!("[{}] {}: {}", issue.severity, issue.scope, issue.message);
}
```

## Constants

- `SUPPORTED_EXTENSIONS` -- `["obj", "stl", "ply", "gltf", "glb"]`

## Part of the Solarxy Workspace

This crate is part of the [Solarxy](https://github.com/marko-koljancic/solarxy) workspace:

| Crate | Description |
|-------|-------------|
| **solarxy-core** | Core types, geometry, validation, preferences |
| [solarxy-formats](../solarxy-formats/) | 3D model format loaders |
| [solarxy-cli](../solarxy-cli/) | CLI parsing and TUI interfaces |

## License

MIT
