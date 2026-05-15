# Solarxy JSON Schemas

Versioned JSON Schemas for Solarxy's configuration and report formats. IDEs
that honor `$schema` URLs (VS Code, JetBrains) pick these up for autocomplete
and inline error highlighting when editing `solarxy.toml`.

## Available schemas

| Schema | Type | Stable URL |
|---|---|---|
| `solarxy-config.v1.json` | `solarxy.toml` (ProjectConfig) | `https://raw.githubusercontent.com/marko-koljancic/solarxy/main/schemas/solarxy-config.v1.json` |

## Editor setup

Add this header to the top of your `solarxy.toml`:

```toml
#:schema https://raw.githubusercontent.com/marko-koljancic/solarxy/main/schemas/solarxy-config.v1.json
```

…or configure your IDE's TOML language server to associate the schema with
files named `solarxy.toml`.

## Regenerating

Schemas are generated from the canonical Rust types via `schemars`:

```bash
cargo run -p solarxy-core --features schemars-gen --example gen_schemas \
    > schemas/solarxy-config.v1.json
```

The `cargo test -p solarxy-core --features schemars-gen --test schema_drift`
integration test asserts that the file on disk matches the generated schema
byte-for-byte — so any type change that wasn't regenerated fails CI.

## Versioning

Each schema is pinned to its `format_version` field. A breaking schema change
ships as a new file (`solarxy-config.v2.json`); old files keep their old URL
indefinitely.

The `main` branch is currently the source of truth. Schemastore.org
submission is planned for a future release once the schema has stabilized
against real-world `solarxy.toml` files.
