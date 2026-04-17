# Flatpak / Flathub packaging

This directory holds the [Flathub](https://flathub.org) submission for the
Solarxy GUI. Flatpak is the primary Linux distribution channel as of
v0.5.0-rc.7 — it sandboxes the GUI, ships its own GL/Vulkan stack via the
Freedesktop runtime (which fixes the Fedora 42 missing-driver bug that
broke the old `.rpm`), and integrates with GNOME Software / KDE Discover
for one-click install and auto-update.

## Files

- `dev.koljam.solarxy.yaml` — flatpak-builder manifest. Build instructions,
  finish-args (sandbox permissions), runtime + SDK pinning.
- `dev.koljam.solarxy.desktop` — `.desktop` entry. Filename matches app-id
  per Flathub rules.
- `dev.koljam.solarxy.metainfo.xml` — AppStream metadata. Required by
  Flathub; drives the listing shown in software stores.
- `cargo-sources.json` — generated vendored crate sources, committed to
  the repo. Refreshed per release with `generate-sources.sh`.
- `generate-sources.sh` — wrapper around
  [flatpak-cargo-generator.py](https://github.com/flatpak/flatpak-builder-tools/tree/master/cargo).

## Build locally

```bash
# 1. Generate the cargo sources manifest from Cargo.lock
./packaging/flatpak/generate-sources.sh

# 2. Build the Flatpak (requires flatpak-builder + the freedesktop runtime)
flatpak install -y flathub \
    org.freedesktop.Platform//24.08 \
    org.freedesktop.Sdk//24.08 \
    org.freedesktop.Sdk.Extension.rust-stable//24.08
flatpak-builder --user --install --force-clean build-dir \
    packaging/flatpak/dev.koljam.solarxy.yaml

# 3. Run
flatpak run dev.koljam.solarxy
```

## Submitting to Flathub (one-time)

1. Fork [github.com/flathub/flathub](https://github.com/flathub/flathub).
2. Add `dev.koljam.solarxy.yaml` (and the `.desktop` + `.metainfo.xml`) to
   a new branch under `submissions/`.
3. Open a PR; reviewers will request changes (commonly: stricter
   sandboxing, fixed metainfo screenshots).
4. Once merged, Flathub creates a dedicated repo
   `github.com/flathub/dev.koljam.solarxy`. All future updates land via
   PRs to that repo (handled by `.github/workflows/flathub-bump.yml`).

## Per-release maintenance

The `flathub-bump.yml` workflow runs on every GitHub release. It checks
out the Flathub repo, regenerates `cargo-sources.json` against the new
`Cargo.lock`, bumps the `archive` source URL + SHA256, and opens a PR on
the Flathub repo for the maintainers to merge.
