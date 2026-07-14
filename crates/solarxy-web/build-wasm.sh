#!/usr/bin/env bash
# Builds solarxy-web to wasm, runs wasm-bindgen, then wasm-opt.
#
# Usage: build-wasm.sh [OUT_DIR] [--dev | --dist]
#   OUT_DIR  where the wasm-bindgen `--target web` output lands
#            (default: crates/solarxy-web/smoke/pkg).
#   --dev    debug build (fast compile, large wasm, no wasm-opt).
#   --dist   the profile used for shipping: fat LTO, one codegen unit, symbols
#            stripped (see [profile.dist] in the workspace Cargo.toml).
#   default  the `release` profile (thin LTO), for local dev of the real app.
#
# On --dist and payload size, measured rather than assumed: fat LTO buys NOTHING
# here. release vs dist, after wasm-opt -Oz, is 954,982 vs 955,247 bytes brotli --
# the dist artifact is 265 bytes LARGER. wasm-opt already performs whole-module
# optimisation and subsumes what LTO would contribute, and `release` already
# carries strip+panic=abort, so LTO is the only difference between the profiles.
# Ship --dist if you want (releases are infrequent and it may help codegen
# quality, which we do not measure), but do not reach for it as a size lever.
# The size lever was the JS bundle: elkjs was ~1.6 MB of a 2.45 MB entry chunk
# and is now dynamically imported.
#
# NOTE the profile word is load-bearing TWICE: it selects the cargo profile AND
# names the directory cargo emits into (target/wasm32-unknown-unknown/<profile>/).
# Setting only the cargo flag would leave the path pointing at a stale artifact
# from a previous build -- wasm-bindgen would happily consume it and ship an old
# wasm with no error at all. Both come from $PROFILE, so they cannot drift.
set -euo pipefail
cd "$(dirname "$0")/../.."   # workspace root (solarxy/)

OUT_DIR=""
PROFILE="release"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dev)  PROFILE="debug" ;;
        --dist) PROFILE="dist" ;;
        --*)    echo "error: unknown flag $1" >&2; exit 1 ;;
        *)      OUT_DIR="$1" ;;
    esac
    shift
done
OUT_DIR="${OUT_DIR:-crates/solarxy-web/smoke/pkg}"

case "$PROFILE" in
    debug)   PROFILE_FLAG="" ;;
    release) PROFILE_FLAG="--release" ;;
    *)       PROFILE_FLAG="--profile $PROFILE" ;;
esac

# wasm-bindgen must match the Cargo pin exactly or the generated glue and the
# wasm disagree at runtime, which surfaces as an inscrutable boot failure.
WANT_BINDGEN=$(grep -oE 'wasm-bindgen = "=[0-9.]+"' crates/solarxy-web/Cargo.toml \
    | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' || true)
command -v wasm-bindgen >/dev/null 2>&1 \
    || { echo "error: wasm-bindgen not on PATH (cargo install wasm-bindgen-cli --version ${WANT_BINDGEN})" >&2; exit 1; }
HAVE_BINDGEN=$(wasm-bindgen --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1)
if [[ -n "$WANT_BINDGEN" && "$HAVE_BINDGEN" != "$WANT_BINDGEN" ]]; then
    echo "error: wasm-bindgen CLI is $HAVE_BINDGEN but Cargo pins =$WANT_BINDGEN" >&2
    echo "       cargo install wasm-bindgen-cli --version $WANT_BINDGEN --force" >&2
    exit 1
fi

# wasm-opt used to be silently skipped when absent, so a machine without binaryen
# produced a much larger wasm and said nothing at all. Now:
#   dist    -- hard requirement. This is the profile we ship; a missing optimiser
#              must never be discovered from a bloated artifact after the fact.
#   release -- loud warning, not a failure. `npm run dev` runs this through
#              `predev`, and a contributor without binaryen should still be able
#              to start the dev server.
HAVE_WASM_OPT=1
command -v wasm-opt >/dev/null 2>&1 || HAVE_WASM_OPT=0
if [[ "$HAVE_WASM_OPT" == "0" ]]; then
    if [[ "$PROFILE" == "dist" ]]; then
        echo "error: wasm-opt (binaryen) not on PATH; it is required for a dist build" >&2
        exit 1
    fi
    if [[ "$PROFILE" != "debug" ]]; then
        echo "WARNING: wasm-opt not found -- the wasm will be MUCH larger than a shipped build." >&2
        echo "         Install binaryen. (Only --dist treats this as fatal.)" >&2
    fi
fi

echo "==> cargo build ($PROFILE) for wasm32"
cargo build -p solarxy-web --target wasm32-unknown-unknown $PROFILE_FLAG

RAW="target/wasm32-unknown-unknown/$PROFILE/solarxy_web.wasm"
[[ -f "$RAW" ]] || { echo "error: expected artifact missing: $RAW" >&2; exit 1; }

echo "==> wasm-bindgen --target web -> $OUT_DIR"
mkdir -p "$OUT_DIR"
wasm-bindgen --target web --out-dir "$OUT_DIR" "$RAW"

BG="$OUT_DIR/solarxy_web_bg.wasm"
if [[ "$PROFILE" != "debug" && "$HAVE_WASM_OPT" == "1" ]]; then
    echo "==> wasm-opt -Oz"
    wasm-opt -Oz -all -o "$OUT_DIR/opt.wasm" "$BG"
    mv "$OUT_DIR/opt.wasm" "$BG"
fi

SIZE=$(stat -f%z "$BG" 2>/dev/null || stat -c%s "$BG")
printf '==> done (%s): %s (%.2f MB)\n' "$PROFILE" "$BG" "$(echo "scale=2; $SIZE/1048576" | bc)"
