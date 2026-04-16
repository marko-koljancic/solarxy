# `native-bundle` composite action

Builds macOS `.dmg`, Debian/Ubuntu `.deb`, and x86_64 Linux `.AppImage`
artifacts for Solarxy out of a pre-built release binary.

## When it runs

Invoked from `.github/workflows/native-bundle.yml`, which fires on
`release: published` after cargo-dist has created the GitHub release and
uploaded the shell / powershell / tarball / MSI artifacts. The native
bundle workflow runs a per-target matrix, builds a fresh binary, invokes
this action, and uploads the bundles to the live release via
`gh release upload`.

| Runner | Output |
|---|---|
| `macos-14` (aarch64) | `Solarxy-<ver>-aarch64.dmg` with `Install CLI.command` + README.txt inside |
| `macos-13` (x86_64) | `Solarxy-<ver>-x86_64.dmg` — same layout |
| `ubuntu-22.04` (x86_64) | `solarxy_<ver>-1_amd64.deb` + `Solarxy-<ver>-x86_64.AppImage` |
| `ubuntu-22.04-arm` (aarch64) | `solarxy_<ver>-1_arm64.deb` — AppImage skipped (0.6.0-07) |
| Windows | Not invoked — cargo-dist produces MSI natively via `installers = ["msi"]` |

## Inputs

- `target` — Rust target triple (drives platform branching)
- `version` — release tag, `v` prefix stripped for artifact filenames
- `binary-path` — path to the pre-built `solarxy` binary
  (e.g. `target/aarch64-apple-darwin/release/solarxy`)

## Why this architecture (separate workflow, not cargo-dist-native)

Three cargo-dist extension points are available, none of them a good fit:

1. **`github-build-setup`** — injects steps BEFORE `dist build`. The binary
   doesn't exist at that point, so it can only install tools. Useless for us.
2. **`local-artifacts-jobs`** — runs a user workflow in parallel to
   `build-local-artifacts`. Its artifacts upload correctly as workflow
   artifacts, but `dist host` only uploads what's in its own
   `dist-manifest.json` — our bundles get silently dropped.
3. **Separate workflow on `release: published`** (chosen) — survives
   `dist generate` untouched, lets cargo-dist do its thing, uploads via
   `gh release upload` after the release is public. Trade-off: ~3 min delay
   between release publish and bundle availability.

## Fallback path

`create-dmg` (Homebrew formula) has had sparse maintenance. If it breaks, the
action's tail contains a commented-out block that rebuilds the DMG using
macOS's built-in `hdiutil` — plain, unstyled, but rock-solid. Recovery is:

1. Uncomment the `macOS — fallback DMG via hdiutil` step near the bottom of
   `action.yml`.
2. Comment out the `create-dmg` call inside the `macOS — build .app and DMG`
   step.

## Testing locally (macOS only)

The macOS `run:` block runs verbatim in bash. With a local release binary:

```bash
cargo build --release
brew install create-dmg
V=0.5.0 TARGET=aarch64-apple-darwin BINARY=target/release/solarxy \
  bash -c '<paste the macOS run: block from action.yml>'
ls bundle-out/
```

For Linux, use Docker:

```bash
docker run --rm -v "$PWD":/w -w /w rust:1.92 bash -c '
  cargo install cargo-deb --version 2.12.0 --locked
  cargo build --release
  cargo deb --no-build
'
```

## Known brittleness

- **`create-dmg`** is a Homebrew-only tool with a small maintainer pool.
  Failing fast via the fallback block (above) is our insurance.
- **`appimagetool` continuous** — upstream does not tag releases. The
  `continuous` download URL can break if the AppImage project changes their
  naming scheme. Monitor in X4 RC smoke.
- **`cargo-deb` 2.12.0** pinned. Newer releases tend to be backwards
  compatible but we pin to avoid silent drift. Bump deliberately.

## Why no cargo-bundle

Upstream `cargo-bundle` on crates.io has been unmaintained since 2020 and
active forks (Zed, others) target their own use cases. Building the
macOS `.app` layout by hand is 25 lines of shell — less code than pinning and
maintaining a fork, and eliminates a whole dependency.
