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
integration test asserts that the file on disk parses to the same JSON value
as the generated schema. Whitespace and inline-array formatting are tolerated
(some editors re-flow JSON on save); only *content* drift fails the test. If
the test fails, regenerate with the command above.

## Versioning

Each schema is pinned to its `format_version` field. A breaking schema change
ships as a new file (`solarxy-config.v2.json`); old files keep their old URL
indefinitely.

## Schemastore submission (0.6.0)

A PR against [`github.com/SchemaStore/schemastore`][ss] is queued for after
the 0.6.0 tag lands. Once accepted, the schema becomes auto-discovered for
files named `solarxy.toml` in any editor with a JSON Schema-aware TOML
language server (VS Code Even Better TOML, JetBrains, taplo, etc.), with no
`#:schema` header required in the user's config file.

Submission body draft:

```
Adds: src/schemas/json/solarxy-config.json (mirrored from
https://raw.githubusercontent.com/marko-koljancic/solarxy/main/schemas/solarxy-config.v1.json)

File-matching: solarxy.toml

The schema is generated from the canonical Rust types (schemars derive) and
covered by an in-repo drift test. format_version: 1 for the 0.6.0 cycle.
```

Tracking issue / PR link: _(filled in after 0.6.0 tag, before submitting)_

[ss]: https://github.com/SchemaStore/schemastore
