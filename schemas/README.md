# Solarxy JSON Schemas

Versioned JSON Schemas for Solarxy's configuration and report formats. IDEs
that honor `$schema` URLs (VS Code, JetBrains) pick these up for autocomplete
and inline error highlighting when editing `solarxy.toml`.

## Available schemas

| Schema | Type | Stable URL |
|---|---|---|
| `solarxy-config.v1.json` | `solarxy.toml` (ProjectConfig) | `https://raw.githubusercontent.com/marko-koljancic/solarxy/main/schemas/solarxy-config.v1.json` |
| `slxy-scene.v2.json` | `scene.json` inside a `.slxy` scene archive (SceneJson), current | `https://raw.githubusercontent.com/marko-koljancic/solarxy/main/schemas/slxy-scene.v2.json` |
| `slxy-scene.v1.json` | The same, at schema version 1. Superseded, kept at its address | `https://raw.githubusercontent.com/marko-koljancic/solarxy/main/schemas/slxy-scene.v1.json` |

### `.slxy` scene format versioning

`schema_version` is **2** as of v0.10.0. Version 0 was a pre-beta format that
carried no compatibility guarantees, version 1 was the public-beta freeze at
v0.7.0, and version 2 renamed the context vocabulary: a geometry network is a
SOP network and an image network is a COP network, so the sub-graph kinds and
two of the three container type ids changed spelling. Files at either older
version still open and are migrated up on read, one step at a time.

The two version fields move together and are held so by a compile-time
assertion: `MIN_READER_CURRENT` went to 2 with the vocabulary because a
version-1 reader handed a version-2 file would not fail, it would meet
container type ids it has never heard of, load each as a parameterless
placeholder, and destroy the document on the next save.

From here on the contract is:

- **Adding an optional field** (`#[serde(default)]`) needs no version bump: older
  readers ignore it, newer readers default it.
- **Any change that an old reader could misread** bumps `SCHEMA_VERSION_CURRENT`
  and adds a step to `migrate_scene` in `crates/solarxy-scenefile/src/lib.rs`.
- **`MIN_READER_CURRENT` and `READER_VERSION` move together.** If they drift, a
  build writes files (`min_reader: N`) that its own reader rejects as "too new".
  A `const` assertion in `crates/solarxy-scenefile/src/lib.rs` now fails the
  build if they do, which the surrounding prose had claimed for two releases
  and nothing enforced.
- **The migration is a chain, not a switch.** Each version has a step and the
  driver walks them in order, so the oldest format reaches the newest. It
  applied one step and returned until v0.10.0, which was correct only while a
  single step existed.
- A new schema file is generated and checked in **beside** the old one, which
  keeps its address; `schema_drift.rs` derives the path it checks from the
  version constant, so a bump repoints it by construction.

An earlier `slxy-scene.v0.json` existed but was never published to a stable URL
(it was absent from this table), so the freeze renamed it rather than adding a
second file. `slxy-scene.v1.json` was published, so version 2 was added beside
it and it is left alone. Published schemas are add-only, as stated below.

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

## Schemastore

The schema is published in [SchemaStore][ss], so it is auto-discovered for
files named `solarxy.toml` in any editor with a JSON Schema-aware TOML
language server (VS Code Even Better TOML, JetBrains, taplo, and the rest).
No `#:schema` header is needed in the user's config file.

- Pull request [SchemaStore/schemastore#5721][pr], merged 2026-05-22.
- Upstream path `src/schemas/json/solarxy-config.json`, mirrored from
  `solarxy-config.v1.json` in this directory.
- Catalog entry matches `solarxy.toml` and serves the schema from
  <https://www.schemastore.org/solarxy-config.json>.

The upstream copy is a **mirror, not a fetch**: nothing upstream re-reads this
repository. A change to the canonical schema here therefore does not reach
editors until it is submitted upstream again. The drift test above keeps the
file honest against the Rust types, and keeps nothing honest against
SchemaStore.

[ss]: https://github.com/SchemaStore/schemastore
[pr]: https://github.com/SchemaStore/schemastore/pull/5721
