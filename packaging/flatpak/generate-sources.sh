#!/usr/bin/env bash
# Regenerate packaging/flatpak/cargo-sources.json from the workspace
# Cargo.lock. Run this before submitting a new release to Flathub or
# whenever Cargo.lock changes.
#
# Requires Python 3 and the flatpak-cargo-generator.py script from the
# flatpak-builder-tools repo. The script is fetched on-demand into a
# cache dir to avoid vendoring it into our tree.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
OUT="$REPO_ROOT/packaging/flatpak/cargo-sources.json"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/solarxy-flatpak"
GENERATOR="$CACHE_DIR/flatpak-cargo-generator.py"

mkdir -p "$CACHE_DIR"

if [ ! -f "$GENERATOR" ]; then
    echo "Fetching flatpak-cargo-generator.py..."
    curl -fsSL \
        "https://raw.githubusercontent.com/flatpak/flatpak-builder-tools/master/cargo/flatpak-cargo-generator.py" \
        -o "$GENERATOR"
fi

echo "Generating $OUT from $REPO_ROOT/Cargo.lock..."
python3 "$GENERATOR" "$REPO_ROOT/Cargo.lock" -o "$OUT"

echo "Done. Commit packaging/flatpak/cargo-sources.json before submitting to Flathub."
