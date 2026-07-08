#!/usr/bin/env bash
# Builds solarxy-web to wasm and runs wasm-bindgen (+ wasm-opt when present).
# Usage: build-wasm.sh [OUT_DIR] [--dev]
#   OUT_DIR  where the wasm-bindgen `--target web` output lands
#            (default: crates/solarxy-web/smoke/pkg).
#   --dev    debug build (faster compile, larger wasm); default is release.
set -euo pipefail
cd "$(dirname "$0")/../.."   # workspace root (solarxy/)

OUT_DIR="${1:-crates/solarxy-web/smoke/pkg}"
PROFILE="release"
PROFILE_FLAG="--release"
if [[ "${1:-}" == "--dev" || "${2:-}" == "--dev" ]]; then
    PROFILE="debug"
    PROFILE_FLAG=""
    [[ "${1:-}" == "--dev" ]] && OUT_DIR="crates/solarxy-web/smoke/pkg"
fi

echo "==> cargo build ($PROFILE) for wasm32"
cargo build -p solarxy-web --target wasm32-unknown-unknown $PROFILE_FLAG

RAW="target/wasm32-unknown-unknown/$PROFILE/solarxy_web.wasm"
echo "==> wasm-bindgen --target web -> $OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen --target web --out-dir "$OUT_DIR" "$RAW"

BG="$OUT_DIR/solarxy_web_bg.wasm"
if command -v wasm-opt >/dev/null 2>&1 && [[ "$PROFILE" == "release" ]]; then
    echo "==> wasm-opt -Oz"
    wasm-opt -Oz -all -o "$OUT_DIR/opt.wasm" "$BG"
    mv "$OUT_DIR/opt.wasm" "$BG"
fi

SIZE=$(stat -f%z "$BG" 2>/dev/null || stat -c%s "$BG")
printf '==> done: %s (%.2f MB)\n' "$BG" "$(echo "scale=2; $SIZE/1048576" | bc)"
